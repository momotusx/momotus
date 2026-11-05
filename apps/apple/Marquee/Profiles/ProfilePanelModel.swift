import Foundation

/// One profile's last title (`profilesLastWatched`), mirroring apps/web/src/core/generated/LastWatched.ts. The core
/// formats `label` and `spoken`; the app shows them as they come.
struct LastWatched: Decodable, Hashable, Sendable {
    let profile: String
    let itemId: String
    /// "Slow Horses, S4:E2" for an episode of a series, else the name.
    let label: String
    /// The label as a screen reader says it: "Slow Horses, season 4, episode 2".
    let spoken: String
    /// The title's backdrop, else the episode's still; never a poster.
    let art: String?
}

/// The Mac's profile panel (plans/profile-panel.md, the owner's pick 7): its words, and what its card and rows show,
/// as values. The words equal the web's `panelWords` (apps/web/src/lib/profiles.ts).
enum ProfilePanelModel {
    static let title = "Profiles"
    static let close = "Close"
    static let watchingNow = "Watching now"
    static let syncing = "Syncing"
    static let lastWatched = "Last watched"
    static let switchTo = "Switch to"
    static let kids = "Kids"
    static let incognitoLine = "Watch without keeping anything"
    static let manage = "Manage profiles"
    static let settings = "Settings"

    /// The current profile's card: Incognito's while it runs, with Leave as its action.
    struct Card: Equatable {
        let profile: ProfileInfo
        let sub: String
        let last: LastWatched?
        let leave: Bool
    }

    /// A "Switch to" row: another profile, or Incognito (`profile` nil).
    struct Row: Equatable, Identifiable {
        let id: String
        let profile: ProfileInfo?
        let name: String
        /// The last-watched label, "Syncing", or Incognito's line; nil leaves the name alone.
        let line: String?
        let art: String?
        let lock: Bool
        let kids: Bool
        let disabled: Bool
        let spoken: String
    }

    /// Titles and art show only outside a kids session and Incognito; the core refuses the call there anyway.
    static func asks(_ snapshot: ProfilesSnapshot) -> Bool {
        !snapshot.incognito && snapshot.current?.kids != true
    }

    static func card(_ snapshot: ProfilesSnapshot, last: [String: LastWatched]) -> Card? {
        guard let me = snapshot.current else { return nil }
        if snapshot.incognito { return Card(profile: me, sub: Incognito.keptNothing, last: nil, leave: true) }
        let shows = asks(snapshot) && !me.syncing
        return Card(profile: me, sub: me.syncing ? syncing : watchingNow, last: shows ? last[me.id] : nil, leave: false)
    }

    /// Every other profile in picker order, then Incognito when it is offered and not running.
    static func rows(_ snapshot: ProfilesSnapshot, last: [String: LastWatched]) -> [Row] {
        let kidsSession = snapshot.current?.kids == true
        let titles = asks(snapshot)
        var rows = snapshot.profiles.filter { $0.id != snapshot.active }.map { p in
            let watched = titles && !p.syncing ? last[p.id] : nil
            return Row(
                id: p.id, profile: p, name: p.name, line: p.syncing ? syncing : watched?.label, art: watched?.art,
                lock: p.hasPin && !p.syncing, kids: p.kids, disabled: p.syncing,
                spoken: spoken(p, watched: watched?.spoken, kidsSession: kidsSession))
        }
        if Incognito.offered(snapshot) && !snapshot.incognito {
            rows.append(Row(
                id: Incognito.id, profile: nil, name: Incognito.name, line: incognitoLine, art: nil,
                lock: Incognito.pinProfile(snapshot) != nil, kids: false, disabled: false,
                spoken: "\(Incognito.spoken(snapshot)), \(incognitoLine)"))
        }
        return rows
    }

    /// Manage profiles: the owner's, unlocked, outside a kids session and Incognito.
    static func manages(_ snapshot: ProfilesSnapshot) -> Bool {
        !snapshot.locked && !snapshot.incognito && snapshot.current?.isOwner == true && snapshot.current?.kids != true
    }

    /// What a screen reader hears for a row, as the web's `panelRowLabel`: the name, then what the row shows in
    /// words; a kids session hears that a grown-up's PIN comes next.
    static func spoken(_ p: ProfileInfo, watched: String?, kidsSession: Bool) -> String {
        [
            p.name,
            p.kids ? "kids profile" : "",
            p.syncing ? "still syncing" : p.hasPin ? "locked with a PIN" : "",
            !p.syncing ? watched.map { "last watched \($0)" } ?? "" : "",
            kidsSession && !p.syncing ? "needs a grown-up's PIN" : "",
        ].filter { !$0.isEmpty }.joined(separator: ", ")
    }
}
