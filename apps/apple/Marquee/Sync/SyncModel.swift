import Foundation
#if !os(tvOS)
import LocalAuthentication
#endif
#if canImport(UIKit)
import UIKit
#endif
import Observation

/// The sync account on this device: one per app. The keystore record lives
/// in the Keychain; every sync op gets it and may hand back a new one, which
/// is stored before anything else happens.
///
/// Every op on the record runs in turn (one at a time, in the order asked),
/// in tasks owned by this model, never by a view: leaving a page or a scene
/// change cannot cancel an op between the core's answer and storing its
/// record. A record that comes back is kept only if it is not older than the
/// one stored (`revision`). Rounds run in one worker: asking while a round
/// runs keeps one more for after it.
@MainActor @Observable
final class SyncModel {
    /// A record is stored, or held for this session: this device is in a sync account.
    private(set) var joined: Bool
    /// A record the Keychain kept that this device does not use: after
    /// leaving, or left by a deleted install. The page says so with Try again
    /// (`deleteKeyAgain`).
    private(set) var keyLeft: KeyLeft?

    enum KeyLeft: Equatable {
        case afterLeave, earlierInstall
    }
    /// The Keychain did not answer when asked for the record: this device may
    /// be in an account, so the page says so with Try again, never the ways in.
    private(set) var keyUnreadable = false
    /// A round runs or is asked for.
    private(set) var syncing = false
    private(set) var lastReport: SyncReport?
    private(set) var lastSynced: Date?
    private(set) var devices: SyncDevices?
    /// The last read of the device list failed: the list, and the 12
    /// characters with it, may be out of date until a read works again.
    private(set) var listFailed = false
    /// Changes that failed since this device joined, until the user dismisses them.
    private(set) var failed = SyncFailed()
    /// What went wrong in the last round, in words, and what the user can do about it.
    private(set) var problem: SyncProblem?
    /// What the last leave did, for the page that follows it.
    private(set) var notice: String?
    /// The pairing attempt. It belongs here, not to the page, so its join is
    /// stored the moment it arrives, whether or not the page is on screen.
    private(set) var pairing: LiveRecord<PairingSnapshot>?
    /// The last join: in the Keychain (true), for this session only (false), or none yet.
    private(set) var joinStored: Bool?
    /// Joined with a recovery kit and no new kit made since: the page asks for one.
    private(set) var recovered: Bool {
        didSet { defaults.set(recovered, forKey: "syncRecovered") }
    }

    /// Kits made here and not yet said saved: the one whose letters are held,
    /// and the ids of the pending ones (kept across launches, never a kit).
    /// Only this model changes it (tests seed it).
    var kits: KitLedger {
        didSet { if kits.pending != oldValue.pending { defaults.set(kits.pending, forKey: "syncPendingKits") } }
    }
    /// A kit is being made, in this model's own task: a page left meanwhile still gets it held.
    private(set) var makingKit = false

    @ObservationIgnored private let core: CoreClient
    @ObservationIgnored private let records: SyncRecord
    @ObservationIgnored private let defaults: UserDefaults
    /// How long a made kit's letters stay once its page is left.
    @ObservationIgnored var kitHold = KitLedger.hold
    @ObservationIgnored private var holdTimer: Task<Void, Never>?
    /// The kit page is in view (`kitPage`).
    @ObservationIgnored private var kitPageOpen = false
    @ObservationIgnored private var scheduled: Task<Void, Never>?
    @ObservationIgnored private var worker: Task<Void, Never>?
    @ObservationIgnored private var pending = false
    /// The last op in line; the next one starts when it ends.
    @ObservationIgnored private var line: Task<Void, Never>?
    /// Bumped by every join and leave. An op from before it stores and shows nothing.
    /// The main actor is the lock: the bump, the store and each op's check
    /// after its await never interleave.
    @ObservationIgnored private var account = 0
    /// A record the Keychain refused, held in memory: nothing is sent until it is stored.
    @ObservationIgnored private var unstored: JSONValue?
    #if !os(tvOS)
    /// The OS prompt itself (passcode, Face ID or Touch ID). Tests answer it; the app never changes it.
    @ObservationIgnored var ownerPrompt: @MainActor (_ reason: String) async -> Bool = { reason in
        (try? await LAContext().evaluatePolicy(.deviceOwnerAuthentication, localizedReason: reason)) ?? false
    }
    #endif
    /// A sign-in held for linking that another account holds (`linkedElsewhere`): Account's
    /// Signed in with offers to use it for this account. In memory only: the ticket is a secret.
    var heldLink: HeldLink?

    /// `leftover`: a record left by a deleted install could not be removed yet
    /// (`SyncRecord.forgetLeftover`): it is never loaded, and the page offers
    /// to delete it.
    init(core: CoreClient, records: SyncRecord = .keychain, defaults: UserDefaults = .standard, leftover: Bool = false) {
        self.core = core
        self.records = records
        self.defaults = defaults
        recovered = defaults.bool(forKey: "syncRecovered")
        kits = KitLedger(pending: defaults.stringArray(forKey: "syncPendingKits") ?? [])
        if defaults.bool(forKey: Self.goneKey) { problem = SyncProblem(code: "accountGone", message: "") }
        if leftover {
            joined = false
            keyLeft = .earlierInstall
            return
        }
        do {
            joined = try records.load() != nil
        } catch {
            // Fails closed: a Keychain that did not answer is not "no account".
            joined = false
            keyUnreadable = true
            coreLog.error("sync: the Keychain did not give the record: \(error, privacy: .public)")
        }
    }

    private func refuseUnreadableKey() throws {
        if keyUnreadable { throw CoreError("the Keychain did not give the record", code: "keyUnreadable") }
    }

    /// Asks the Keychain for the record again (the page's Try again).
    func readKeyAgain() {
        do {
            joined = try records.load() != nil
            keyUnreadable = false
            if joined { request() }
        } catch {
            keyUnreadable = true
        }
    }

    // MARK: Rounds

    /// Asks for a round: now, or once the running one ends.
    func request() {
        pending = true
        guard worker == nil else { return }
        syncing = true
        worker = Task {
            while pending {
                pending = false
                await rounds()
            }
            syncing = false
            worker = nil
        }
    }

    /// Asks for a round a few seconds after an edit, so a burst of edits goes
    /// in one round. Cancelling cancels the wait only, never a round.
    func schedule() {
        guard joined else { return }
        scheduled?.cancel()
        scheduled = Task {
            try? await Task.sleep(for: .seconds(5))
            guard !Task.isCancelled else { return }
            request()
        }
    }

    /// Syncs, going again while the core asks to (a round that changed the
    /// record publishes nothing until it is stored).
    private func rounds() async {
        for _ in 0..<5 {
            guard joined else { return }
            let mine = account
            do {
                let report: SyncReport = try await withRecord("syncNow") { $0.record }
                // A join or a leave replaced the account while this round ran: its answer is not ours.
                guard account == mine else { return }
                lastReport = report
                lastSynced = .now
                problem = nil
                failed = failed.adding(report)
                // The 12 characters to compare come from the device list: keep it as fresh as the round.
                if report.fingerprint != devices?.fingerprint { try? await loadDevices() }
                if !report.again { return }
            } catch {
                guard account == mine else { return }
                problem = SyncProblem(error, action: .round)
                return
            }
        }
    }

    // MARK: Joining

    /// Starts pairing with a device that already syncs; a new attempt replaces the last.
    func startPairing(server: String, name: String) {
        // A Keychain that did not answer may hold an account: nothing replaces it unseen.
        guard !keyUnreadable else { return }
        let attempt = LiveRecord<PairingSnapshot>("pairing", params: ["server": server, "name": name, "form": DeviceForm.current], core: core)
        attempt.onSnapshot = { [weak self, weak attempt] snapshot in
            guard case .joined(let record) = snapshot, let self, let attempt, self.pairing === attempt, self.joinStored == nil else { return }
            self.join(record)
        }
        joinStored = nil
        pairing = attempt
        attempt.start()
    }

    /// Drops the attempt ("Try again", "New code").
    func endPairing() {
        pairing = nil
    }

    /// The first device of a new account.
    /// With a sign-in's `ticket` the identity is linked to the new account;
    /// `move` takes it from another account (Start over).
    func createAccount(server: String, invite: String, name: String, ticket: String? = nil, move: Bool = false) async throws {
        try refuseUnreadableKey()
        var fields: [String: any Sendable] = ["server": server, "name": name, "form": DeviceForm.current]
        if !invite.isEmpty { fields["invite"] = invite }
        if let ticket { fields["ticket"] = ticket }
        if move { fields["move"] = true }
        let record: JSONValue = try await core.call("createSyncAccount", fields, as: JSONValue.self)
        join(record)
    }

    /// A new device joining with a recovery kit; the page then asks for a new kit.
    func recover(server: String, kit: String, name: String) async throws {
        try await recover(server: server, secret: ["kit": kit], name: name)
    }

    /// A new device joining with a passkey's PRF output (`recover {prf}`):
    /// no new kit is asked for after it. `kitUnknown` when it opens no account here.
    func recover(server: String, prf: Data, name: String) async throws {
        try await recover(server: server, secret: ["prf": PasskeyContract.base64url(prf)], name: name)
    }

    private func recover(server: String, secret: [String: String], name: String) async throws {
        try refuseUnreadableKey()
        var fields: [String: any Sendable] = ["server": server, "name": name, "form": DeviceForm.current]
        for (key, value) in secret { fields[key] = value }
        let recovered = try await core.call("recover", fields, as: Recovered.self)
        // Only a recovery kit asks for a new kit after it.
        join(recovered.record, recovered: recovered.via != "passkey")
    }

    /// `recover`'s answer: the record to store, and the way in (`kit` or `passkey`).
    private struct Recovered: Decodable {
        let record: JSONValue
        let via: String?
    }

    /// A join replaces any account before it (the pages ask first), then its
    /// first round runs. When the Keychain refuses the record, the join lasts
    /// for this session and sync waits until the record is stored.
    private func join(_ record: JSONValue, recovered: Bool = false) {
        account += 1
        dropKits()
        // A new account's record is stored as it is: revisions compare only within an account.
        let stored = records.store(record)
        // A key left by a deleted install is gone once the new one took its place.
        if keyLeft == .earlierInstall, stored { SyncRecord.cleaned(defaults) }
        keyLeft = nil
        unstored = stored ? nil : record
        joinStored = stored
        joined = true
        notice = nil
        defaults.set(false, forKey: Self.goneKey)
        self.recovered = recovered
        lastReport = nil
        lastSynced = nil
        devices = nil
        listFailed = false
        failed = SyncFailed()
        problem = stored ? nil : SyncProblem(code: "storage", message: "")
        request()
    }

    // MARK: The account's devices

    /// The device list, fresh from the sync server.
    @discardableResult
    func loadDevices() async throws -> SyncDevices {
        let mine = account
        let list: SyncDevices
        do {
            list = try await withRecord("syncDevices") { $0.record }
        } catch {
            if account == mine, !(error is CancellationError) { listFailed = true }
            throw error
        }
        if account == mine {
            devices = list
            listFailed = false
            kits.follow(list.members)
        }
        return list
    }

    /// Vouches for the device that shows `qr` and hands it the account.
    /// Returns the six digits the new device must show too.
    /// Behind the OS prompt: whoever confirms the digits next joins the account.
    func approvePairing(_ qr: String) async throws -> String {
        try await confirmOwner(SyncWords.authApprove)
        struct Approved: Decodable, Sendable { let code: String; let record: JSONValue }
        let approved: Approved = try await withRecord("approvePairing", ["qr": qr]) { $0.record }
        // The new device joins once it confirms; the list catches up on the next look.
        try? await loadDevices()
        return approved.code
    }

    /// The digits on the new device differ: the approval of `qr` ends now, with
    /// a key rotation that drops the device if it joined meanwhile (and every
    /// device it let in). Behind the OS prompt; a cancelled prompt changes nothing.
    func cancelPairing(_ qr: String) async throws {
        try await confirmOwner(SyncWords.authDiffer)
        let _: JSONValue = try await withRecord("cancelPairing", ["qr": qr]) { $0 }
        try? await loadDevices()
    }

    /// Both sides of a different device list, read only.
    func forkPreview() async throws -> ForkPreview {
        try await withRecord("forkPreview") { (_: ForkPreview) in nil }
    }

    /// Makes a recovery kit and holds its letters here until "I saved it"
    /// (`kits`). Behind the OS prompt, as the protocol asks; never on a TV.
    /// The work runs in this model's own task: Cancel or Back while it runs
    /// still gets the kit held, and the hub says it waits. Unseen kits go
    /// first when the list shows them; one the list could not show stays.
    func createRecoveryKit() async throws {
        #if os(tvOS)
        throw CoreError("A TV does not make recovery kits.", code: "unconfirmed")
        #else
        guard !makingKit else { return }
        try await confirmOwner(SyncWords.authKit)
        makingKit = true
        try await Task { @MainActor in
            defer { makingKit = false }
            let mine = account
            let before = try? await loadDevices()
            if let kitsBefore = before.map({ Set($0.members.filter(\.kit).map(\.id)) }) {
                for id in kits.unseen {
                    if kitsBefore.contains(id) {
                        let _: JSONValue = try await withRecord("removeDevice", ["device": id]) { $0 }
                    }
                    kits.settle(id)
                }
            }
            let made: MadeKit = try await withRecord("createRecoveryKit") { $0.record }
            guard mine == account else { return }
            recovered = false
            hold(made.held)
            try? await loadDevices()
        }.value
        #endif
    }

    /// Shows the held kit again: only after the OS prompt.
    func confirmShowKit() async throws {
        try await confirmOwner(SyncWords.authShowKit)
    }

    /// The kit page is open, or was left: the letters never go while the
    /// page can show them, and go `kitHold` after it is left.
    func kitPage(open: Bool) {
        kitPageOpen = open
        holdTimer?.cancel()
        guard !open, let kit = kits.held else { return }
        // ponytail: a fixed hold after the page; the letters never outlive it, and the kit then counts as unseen.
        holdTimer = Task { [weak self, kitHold] in
            try? await Task.sleep(for: kitHold)
            guard !Task.isCancelled else { return }
            self?.kits.expire(kit)
        }
    }

    /// A kit just made: held, and when its page is not in view (left while
    /// the kit was made, or another tab over it) its hold starts now.
    func hold(_ kit: HeldKit) {
        kits.hold(kit)
        if !kitPageOpen { kitPage(open: false) }
    }

    /// "I saved it": the letters leave memory and the kit is no longer pending.
    func kitSaved() {
        holdTimer?.cancel()
        kits.saved()
    }

    /// The user checked the list for a kit nobody could name.
    func checkedUnknownKit() {
        kits.checkedUnknown()
    }

    private func dropKits() {
        holdTimer?.cancel()
        kits.drop()
    }

    /// What reporting `lost` would drop: its devices (ticked) and its kits (always).
    func lostPreview(_ lost: String) async throws -> LostPreview {
        try await withRecord("lostPreview", ["lost": lost]) { (_: LostPreview) in nil }
    }

    /// Removes a device or a kit. It has landed once this returns; a list that
    /// cannot be fetched right after does not undo it.
    func removeDevice(_ device: String) async throws {
        try await confirmOwner(SyncWords.authRemove)
        let _: JSONValue = try await withRecord("removeDevice", ["device": device]) { $0 }
        // A pending kit just removed needs no saving and no removing, list or no list.
        kits.settle(device)
        try? await loadDevices()
    }

    /// Drops `lost` and the devices it added except `keep`; their kits go too.
    func reportLost(_ lost: String, keep: [String]) async throws {
        try await confirmOwner(SyncWords.authLost)
        let _: JSONValue = try await withRecord("reportLost", ["lost": lost, "keep": keep]) { $0 }
        try? await loadDevices()
    }

    /// Leaves the account. Without `force` the core refuses while edits are unsent.
    /// `removeAfter`: the first step of a sign-out that also removes the library
    /// (D13), with or without the debrid accounts: the core records the removal
    /// before the leave goes out, and no notice is said, it goes on.
    func signOut(force: Bool, askedAgain: Bool = false, removeAfter debrid: Bool? = nil) async throws {
        try await confirmOwner(debrid != nil ? SyncWords.authSignOut : SyncWords.authLeave, endsAccess: true, askedAgain: askedAgain)
        var fields: [String: any Sendable] = ["force": force]
        if let debrid { fields["removeAfter"] = ["debrid": debrid] }
        let _: JSONValue = try await withRecord("signOut", fields) { _ in nil }
        forget()
        problem = nil
        notice = debrid != nil ? nil : SyncWords.left
    }

    /// Stops syncing on this device only: nothing is sent and the sync server
    /// still lists it; the core takes only this device's own record
    /// (`wrongRecord` otherwise). The way out when the server will not let it leave.
    func leaveHere(askedAgain: Bool = false) async throws {
        try await confirmOwner(SyncWords.authLeave, endsAccess: true, askedAgain: askedAgain)
        let _: JSONValue = try await withRecord("signOut", ["force": true, "local": true]) { _ in nil }
        forget()
        problem = nil
        notice = SyncWords.leftHere
    }

    /// Deletes the account on the sync server for every device, after the
    /// OS prompt (the page asked once first, naming what goes). With no
    /// passcode that ask was the step, as for leaving (D15): deleting ends
    /// access and every library stays. Once the server confirmed, this
    /// device forgets it as after a sign-out; the library stays.
    func deleteAccount(receipt: String? = nil) async throws {
        if receipt == nil { try await confirmOwner(SyncWords.authDelete, endsAccess: true, askedAgain: true) }
        var fields: [String: any Sendable] = [:]
        if let receipt { fields["receipt"] = receipt }
        let _: JSONValue = try await withRecord("deleteAccount", fields) { _ in nil }
        forget()
        problem = nil
        notice = SyncWords.accountGone
    }

    /// The OS prompt before D13's Remove from this device (its steps 2 and 3,
    /// with no account left to leave), as for a sign-out.
    func confirmRemoval(askedAgain: Bool = false) async throws {
        try await confirmOwner(SyncWords.authRemoveHere, endsAccess: true, askedAgain: askedAgain)
    }

    /// The sync server this device's account is on, as its record names it: the
    /// one field the app reads from it, so a code for another server is said so.
    func joinedServer() -> String? {
        guard case .object(let fields)? = try? storedRecord(), case .string(let server)? = fields["server"] else { return nil }
        return server
    }

    /// OK on "Your Marquee account was deleted", heard from the sync server:
    /// the line goes, also from the next launch.
    func dismissGone() {
        if problem?.code == "accountGone" { problem = nil }
        defaults.set(false, forKey: Self.goneKey)
    }

    /// Set while "Your Marquee account was deleted" waits for its OK.
    static let goneKey = "syncGone"

    /// After `fork`: the sync server's device list, as the compare page showed
    /// it (`head`), without the devices the user ticked. A list that moved since
    /// is refused with `rosterChanged`.
    func useServerRoster(drop: [String], head: String) async throws {
        try await confirmOwner(SyncWords.authServerList)
        let _: JSONValue = try await withRecord("useServerRoster", ["drop": drop, "head": head]) { $0 }
        problem = nil
        request()
        try? await loadDevices()
    }

    /// Forgets the failures listed since joining.
    func dismissFailed() {
        failed = SyncFailed()
    }

    /// Deletes a record the Keychain kept (`keyLeft`).
    func deleteKeyAgain() {
        guard let left = keyLeft, records.delete() else { return }
        if left == .earlierInstall { SyncRecord.cleaned(defaults) }
        keyLeft = nil
    }

    /// Drops the account record here only, when the relay cannot take this
    /// device back (wrong relay, entries lost). The library stays.
    func forgetHere() {
        forget()
        problem = nil
    }

    // MARK: Signing in (plans/accounts.md 3.4)

    /// The sign-in buttons a relay offers; none on a relay without sign-in.
    func relayInfo(server: String) async throws -> [String] {
        struct Info: Decodable { let providers: [String] }
        return try await core.call("relayInfo", ["server": server], as: Info.self).providers
    }

    /// A native sheet's flow: its nonce (bound to the relay host) goes to the sheet.
    func nativeSignInStart(server: String, provider: String, revoke: Bool = false) async throws -> NativeStart {
        var fields: [String: any Sendable] = ["server": server, "provider": provider]
        if revoke { fields["revoke"] = true }
        return try await core.call("nativeSignInStart", fields, as: NativeStart.self)
    }

    /// The sheet's ID token for that flow: `found`, `new` or (a revoke) `receipt`.
    func nativeSignIn(flow: String, idToken: String, authorizationCode: String? = nil) async throws -> SignInSnapshot {
        var fields: [String: any Sendable] = ["flow": flow, "idToken": idToken]
        if let authorizationCode { fields["authorizationCode"] = authorizationCode }
        return try await core.call("nativeSignIn", fields, as: SignInSnapshot.self)
    }

    /// An emailed code for the address: its flow (the verifier stays in the core,
    /// in memory). `badAddress` is answered before any call (accounts-email.md 4).
    func emailSignInStart(server: String, email: String) async throws -> EmailStart {
        try await core.call("emailSignInStart", ["server": server, "email": email], as: EmailStart.self)
    }

    /// The typed code for that flow: `found` or `new`, as a provider sign-in ends.
    func emailSignIn(flow: String, code: String) async throws -> SignInSnapshot {
        try await core.call("emailSignIn", ["flow": flow, "code": code], as: SignInSnapshot.self)
    }

    /// A relay's brokered sign-in (Google, Facebook, any other): the view to
    /// open in an auth session, ending in `found`, `new` or `failed`.
    func signInView(server: String, provider: String) -> LiveRecord<SignInSnapshot> {
        LiveRecord("signIn", params: ["server": server, "provider": provider, "return": "momotus://auth"], core: core)
    }

    /// Apple's revoke on a surface without the native sheet: the same view, ending in `receipt`.
    func appleRevokeView(server: String) -> LiveRecord<SignInSnapshot> {
        LiveRecord("appleRevoke", params: ["server": server, "return": "momotus://auth"], core: core)
    }

    /// Hands the auth session's own return to its view; the core takes only
    /// its own flow's return, from its relay.
    func returned(_ view: LiveRecord<SignInSnapshot>, fragment: String) async throws {
        guard let sub = view.sub else { throw CoreError("the sign-in ended", code: "wrongFlow") }
        try await core.command(sub, ["type": "returned", "fragment": fragment])
    }

    // MARK: Passkeys and links

    /// Saves a passkey as a way in (the owner only, D17): its PRF output, the
    /// label every platform writes. The passkey sheet's user verification is
    /// the re-authentication, so no OS prompt on top.
    @discardableResult
    func savePasskey(prf: Data, label: String) async throws -> String {
        struct Saved: Decodable, Sendable { let id: String; let record: JSONValue }
        let saved: Saved = try await withRecord("savePasskey", ["prf": PasskeyContract.base64url(prf), "label": label]) { $0.record }
        try? await loadDevices()
        return saved.id
    }

    /// The account's sign-in identities: provider and date, nothing more.
    func identities() async throws -> [IdentityLink] {
        struct Links: Decodable, Sendable { let links: [IdentityLink]; let record: JSONValue? }
        return try await withRecord("identities") { (links: Links) in links.record }.links
    }

    /// Links a sign-in's identity to this account (the owner only, D16).
    /// `linkedElsewhere` keeps the ticket: `move` takes it over.
    func linkIdentity(ticket: String, move: Bool = false) async throws {
        var fields: [String: any Sendable] = ["ticket": ticket]
        if move { fields["move"] = true }
        let _: JSONValue = try await withRecord("linkIdentity", fields) { Self.recordIn($0) }
    }

    /// Unlinks one identity. An Apple link sends Apple's revoke receipt, and
    /// that sheet was the re-authentication; otherwise the OS prompt first.
    func unlinkIdentity(link: Int64, receipt: String? = nil) async throws {
        if receipt == nil { try await confirmOwner(SyncWords.authUnlink) }
        var fields: [String: any Sendable] = ["link": link]
        if let receipt { fields["receipt"] = receipt }
        let _: JSONValue = try await withRecord("unlinkIdentity", fields) { Self.recordIn($0) }
    }

    /// Cancels a scheduled deletion; `unlink` also removes the identity that asked. Any profile.
    func cancelDeletion(unlink: Bool) async throws {
        let _: JSONValue = try await withRecord("cancelDeletion", ["unlink": unlink]) { Self.recordIn($0) }
        request()
    }

    /// The user saw these announcements: no round reports them again.
    func ackAnnouncements(_ ids: [String]) async throws {
        try await core.perform("ackAnnouncements", ["ids": ids])
        if let report = lastReport {
            lastReport = report.without(ids)
        }
    }

    /// An answer's `record`, when it carries one.
    nonisolated private static func recordIn(_ value: JSONValue) -> JSONValue? {
        guard case .object(let fields) = value, let record = fields["record"], record != .null else { return nil }
        return record
    }

    // MARK: The record

    /// Runs one op on the record, in turn. A record in the answer is stored
    /// before the next op runs; `dropped` (this device was removed) and
    /// `accountGone` (the account was deleted) forget the account here.
    private func withRecord<R: Decodable & Sendable>(
        _ op: String, _ fields: [String: any Sendable] = [:], record recordIn: @escaping @Sendable (R) -> JSONValue?
    ) async throws -> R {
        try await inTurn { [self] in
            let mine = account
            guard let record = try storedRecord() else { throw CoreError("this device has not joined the account", code: "notJoined") }
            var all = fields
            all["record"] = record
            do {
                let result: R = try await core.call(op, all, as: R.self)
                if let next = recordIn(result), mine == account, !keep(next) {
                    throw CoreError("the record was not stored", code: "storage")
                }
                return result
            } catch let error as CoreError where mine == account && (SyncRules.forgets(error.code) || error.code == "fork") {
                if SyncRules.forgets(error.code) { forget() }
                problem = SyncProblem(error, action: .round)
                // Said again at the next launch until OK: nobody may have seen it yet.
                if error.code == "accountGone" { defaults.set(true, forKey: Self.goneKey) }
                throw error
            }
        }
    }

    /// One op after another, in the order asked. The work runs in its own
    /// task, so a caller that goes away does not stop it half way.
    private func inTurn<T: Sendable>(_ work: @escaping @MainActor () async throws -> T) async throws -> T {
        let before = line
        let run = Task { @MainActor in
            await before?.value
            return try await work()
        }
        line = Task { _ = await run.result }
        return try await run.value
    }

    /// The account's record from the Keychain. One held in memory is stored
    /// first, and while the Keychain refuses it nothing is sent.
    private func storedRecord() throws -> JSONValue? {
        if let held = unstored {
            guard records.store(held) else {
                problem = SyncProblem(code: "storage", message: "")
                throw CoreError("the record was not stored", code: "storage")
            }
            unstored = nil
        }
        do {
            return try records.load()
        } catch {
            throw CoreError("the Keychain did not give the record", code: "keyUnreadable")
        }
    }

    /// Stores a record the core handed back, unless the stored one is newer.
    /// When the Keychain refuses it, it is held here and sync pauses until it is stored.
    private func keep(_ next: JSONValue) -> Bool {
        // A stored record that does not read is never overwritten blind: the new one waits here.
        let stored: JSONValue?
        do {
            stored = try records.load()
        } catch {
            unstored = next
            problem = SyncProblem(code: "storage", message: "")
            return false
        }
        let record = SyncRules.newer(stored, next)
        guard records.store(record) else {
            unstored = record
            problem = SyncProblem(code: "storage", message: "")
            return false
        }
        unstored = nil
        return true
    }

    /// Forgets the account here. When the Keychain refuses to delete the
    /// record, `keyLeft` says so: the next launch would otherwise find it.
    private func forget() {
        account += 1
        keyLeft = records.delete() ? nil : .afterLeave
        if keyLeft != nil { coreLog.error("sync: the Keychain kept the record after leaving") }
        unstored = nil
        joined = false
        joinStored = nil
        pairing = nil
        devices = nil
        listFailed = false
        failed = SyncFailed()
        lastReport = nil
        lastSynced = nil
        recovered = false
        dropKits()
    }

    /// The OS prompt (passcode, Face ID or Touch ID) before changing the
    /// account's devices or making a kit, as the protocol asks. A TV has no
    /// way to check who is there: its pages ask a second time instead.
    /// `endsAccess`: the op only ends this device's access (stopping syncing):
    /// with no lock it is not refused, the page asks a second time
    /// (`noLockAsk`) and calls again with `askedAgain`.
    private func confirmOwner(_ reason: String, endsAccess: Bool = false, askedAgain: Bool = false) async throws {
        #if os(tvOS)
        // Other TV pages ask a second time on their own.
        let hasLock = !endsAccess
        #else
        let context = LAContext()
        var error: NSError?
        let hasLock = context.canEvaluatePolicy(.deviceOwnerAuthentication, error: &error)
        #endif
        switch SyncRules.ownerStep(hasLock: hasLock, endsAccess: endsAccess, askedAgain: askedAgain) {
        case .prompt:
            #if !os(tvOS)
            let ok = await ownerPrompt(reason)
            guard ok else { throw CoreError("not confirmed", code: "unconfirmed") }
            #endif
        case .run: return
        case .askAgain: throw CoreError("no lock", code: "noLockAsk")
        case .refuse: throw CoreError("no passcode", code: "noPasscode")
        }
    }
}

/// What this device is, sealed in the roster (`form`): only phones and
/// tablets approve, and count as a second way in.
enum DeviceForm {
    @MainActor static var current: String {
        #if os(tvOS)
        return "tv"
        #elseif os(macOS)
        return "computer"
        #else
        return UIDevice.current.userInterfaceIdiom == .pad ? "tablet" : "phone"
        #endif
    }
}

/// How other devices in the sync account will see this one.
enum DeviceName {
    @MainActor static var current: String {
        #if os(macOS)
        return Host.current().localizedName ?? "Mac"
        #else
        return UIDevice.current.name
        #endif
    }
}

/// A sign-in another account holds, kept for Account's "Use it for this account instead".
struct HeldLink: Equatable {
    let ticket: String
    let provider: String
}

extension SyncReport {
    /// The report without announcements the user saw.
    func without(_ ids: [String]) -> SyncReport {
        var copy = self
        copy.announcements.removeAll { ids.contains($0.id) }
        return copy
    }
}
