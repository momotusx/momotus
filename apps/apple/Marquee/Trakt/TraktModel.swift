import Foundation
import Observation

/// Each profile's Trakt record in the Keychain: this device only, usable
/// after the first unlock, never synchronized or backed up (as the sync
/// record). Tests pass their own.
struct TraktKeystore: Sendable {
    /// The record, or nil when there is none. Throws when the Keychain did not answer.
    var load: @Sendable (_ profile: String) throws -> JSONValue?
    /// False when the Keychain refused it.
    var store: @Sendable (_ profile: String, _ record: JSONValue) -> Bool
    /// False when the Keychain refused (an absent record is deleted already).
    var delete: @Sendable (_ profile: String) -> Bool

    static let keychain = TraktKeystore(
        load: { try Keychain.read("trakt.\($0)").map { try JSONDecoder().decode(JSONValue.self, from: $0) } },
        store: { profile, record in (try? JSONEncoder().encode(record)).map { Keychain.write("trakt.\(profile)", $0) } ?? false },
        delete: { Keychain.delete("trakt.\($0)") }
    )
}

/// The running profile's Trakt (plans/m7-trakt.md section 3), for the app's
/// life. `begin` runs each time a profile's content may show (a start, an
/// unlock, a switch, a new core): that profile's record goes to the core
/// first, when this device holds one (never `setTrakt(null)`: a device that
/// holds none must not end a connection), then the `trakt` view opens for as
/// long as the profile runs. Every record the view offers is stored in the
/// Keychain, then acknowledged with `setTrakt`, only while the profile it
/// came from still runs. The record is never shown, logged or kept anywhere
/// else, except in memory for this session when the Keychain refuses it.
@MainActor @Observable
final class TraktModel {
    /// One opening of the view for one profile; a switch, a lock or a new core starts another.
    final class Session {
        let profile: String
        /// The record last handed over, so a push that repeats it is not stored twice.
        var acked: JSONValue?
        init(profile: String) { self.profile = profile }
    }

    /// The running profile's status, without its record; nil before the first push.
    private(set) var status: TraktStatus?
    /// The view, while a profile runs: its `failure` says why it did not open.
    private(set) var view: LiveRecord<TraktStatus>?
    /// Signed out, but the Keychain kept the record: said, with Try again.
    private(set) var deleteFailed = false
    /// Records the Keychain refused, kept for this session only, by profile.
    private(set) var memory: [String: JSONValue] = [:]

    @ObservationIgnored private let core: CoreClient
    @ObservationIgnored private let keystore: TraktKeystore
    @ObservationIgnored private let defaults: UserDefaults
    @ObservationIgnored private(set) var session: Session?

    init(core: CoreClient, keystore: TraktKeystore = .keychain, defaults: UserDefaults = .standard) {
        self.core = core
        self.keystore = keystore
        self.defaults = defaults
    }

    /// Profiles signed out whose record the Keychain kept: kept across
    /// launches (ids only), never loaded or sent, and deleted again at every begin.
    private var signedOut: Set<String> {
        get { Set(defaults.stringArray(forKey: Self.signedOutKey) ?? []) }
        set { defaults.set(newValue.sorted(), forKey: Self.signedOutKey) }
    }

    static let signedOutKey = "traktSignedOut"

    /// Deletes `profile`'s record; when the Keychain refuses, it stays marked
    /// signed out (never sent) until a delete works. True when it is gone.
    @discardableResult
    private func forget(_ profile: String) -> Bool {
        memory[profile] = nil
        let gone = keystore.delete(profile)
        if gone { signedOut.remove(profile) } else { signedOut.insert(profile) }
        return gone
    }

    /// The running profile's sign-in lasts for this session only: the Keychain refused it.
    var memoryOnly: Bool { session.map { memory[$0.profile] != nil } ?? false }

    /// `profile` runs, unlocked: its record first (`setTrakt` is refused while
    /// the session is locked), then the view. Never for Incognito: the core refuses Trakt there and the record
    /// is no one's (plans/batch-10-incognito.md 5.6).
    func begin(profile: String) async {
        end()
        guard profile != Incognito.id else { return }
        // A sign-out whose delete was refused: tried again, and its record is never sent.
        deleteFailed = signedOut.contains(profile) && !forget(profile)
        let session = Session(profile: profile)
        self.session = session
        if let record = record(of: profile) {
            do {
                try await core.perform("setTrakt", ["record": record])
            } catch {
                // A build without Trakt answers notConfigured; the view says so.
                coreLog.notice("trakt: the record was not handed over: \((error as? CoreError)?.code ?? "error", privacy: .public)")
            }
        }
        guard self.session === session else { return }
        let view = LiveRecord<TraktStatus>("trakt", core: core)
        view.onSnapshot = { [weak self, weak session] snapshot in
            guard let self, let session else { return }
            self.received(snapshot, in: session)
        }
        self.view = view
        view.start()
    }

    /// The profile stops (a switch, a lock, a core that stopped): its view closes.
    func end() {
        session = nil
        view = nil
        status = nil
    }

    /// A push of the view opened in `session`. A record in it is stored for
    /// that profile whatever runs now (it may be a refreshed token: the old
    /// one stops working), then acknowledged, but only while that session
    /// runs: a switch or a lock in between drops the acknowledgement.
    func received(_ snapshot: TraktStatus, in session: Session) {
        if self.session === session { status = snapshot.withoutRecord() }
        guard let record = snapshot.record, record != session.acked else { return }
        session.acked = record
        if keystore.store(session.profile, record) {
            memory[session.profile] = nil
            // A new sign-in took the place of a record whose delete was refused.
            signedOut.remove(session.profile)
        } else {
            memory[session.profile] = record
            coreLog.error("trakt: the Keychain did not keep the sign-in; it lasts for this session")
        }
        Task { [weak self] in
            // Checked and sent in one turn: nothing else runs between, so it reaches the core
            // before a switch can.
            guard let self, self.session === session else { return }
            do {
                try await self.core.perform("setTrakt", ["record": record])
            } catch {
                // Offered again with the next push.
                session.acked = nil
            }
        }
    }

    /// One round now.
    func syncNow() async throws -> TraktReport {
        try await core.call("traktSync", as: TraktReport.self)
    }

    /// Signs this device out for the running profile, then deletes its
    /// record; a refusal by the core keeps it, and a record the Keychain kept
    /// is said (`deleteFailed`).
    func signOut() async throws {
        guard let session else { return }
        try await core.perform("traktSignOut")
        session.acked = nil
        deleteFailed = !forget(session.profile)
    }

    /// Deletes a record the Keychain kept after signing out.
    func deleteAgain() {
        guard let session else { return }
        deleteFailed = !forget(session.profile)
    }

    /// A profile removed (here or on another device): its record goes too.
    func profileGone(_ profile: String) {
        forget(profile)
    }

    /// The record this device holds for `profile`: this session's, else the
    /// Keychain's. A Keychain that does not answer holds none here.
    private func record(of profile: String) -> JSONValue? {
        guard !signedOut.contains(profile) else { return nil }
        if let held = memory[profile] { return held }
        return (try? keystore.load(profile)) ?? nil
    }
}
