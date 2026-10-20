// Versions and playback DTOs from the core, mirroring
// apps/web/src/core/generated/{VersionsSnapshot,VersionRow,PlaybackSnapshot,...}.ts.
// Enumerations the UI only displays (group, reason, hide reason) stay strings
// so a new value from the core never breaks decoding.

import AVFoundation
import Foundation

struct VersionsSnapshot: Decodable, Hashable, Sendable {
    let recommended: VersionRow?
    let rows: [VersionRow]
    let hidden: HiddenSummary
    let pendingAddons: [String]
    let slowAddons: [String]
    let failedAddons: [String]
    let done: Bool
    let frozen: Bool
    /// What the ranking read about this device and title (optional for older cores).
    let conditions: VersionsConditions?
    /// Every addon answered, nothing is listed, and the hidden rows say why.
    let blocked: BlockedVersions?
    /// The one-time learning note, shown with Undo.
    let note: LearnedNote?
    /// Every listed version is a swarm with one seeder or none: said over the
    /// list and on the recommended card before Play (optional for older cores).
    var nobodySharing: Bool?
}

/// What the ranking read (plans/m13-stream-choice.md 4.2): "This TV: 4K HDR,
/// about 42 Mbit/s on Wi-Fi, 61 GB free".
struct VersionsConditions: Decodable, Hashable, Sendable {
    /// 2160, 1080, 720 or 480.
    let screenClass: Int
    let hdr: Bool
    /// "wifi", "ethernet", "cellular" or "other".
    let network: String
    let http: Speed
    let p2p: Speed
    let freeBytes: Int64?
    let runtimeMin: Int?
    let sizeLimitBytes: Int64?
    let minSeeders: Int
    /// The built-in torrent engine runs here.
    let engine: Bool
    /// A streaming service account is connected.
    let account: Bool
}

/// A measured speed; 0 samples is the estimate.
struct Speed: Decodable, Hashable, Sendable {
    let mbps: Double
    let samples: Int
}

/// Why no version is listed, with the versions as they would rank once fixed.
struct BlockedVersions: Decodable, Hashable, Sendable {
    /// "needsAccount", "noSpace" or "nothingPlaysHere".
    let reason: String
    let count: Int
    let rows: [VersionRow]
}

/// A choice the picks taught, on one screen class.
struct LearnedNote: Decodable, Hashable, Sendable {
    let screenClass: Int
    /// "ready", "height", "hdr" or "size".
    let choice: String
    let up: Bool
    /// The preferences now in effect, for the note's numbers.
    let prefs: StreamPrefs
}

/// The one thing to say about a version (plans/m13-stream-choice.md 3.7),
/// with its numbers; the words live in apps/shared/stream-choice-words.json.
struct ReasonLine: Decodable, Hashable, Sendable {
    /// "copy", "fetch", "stalled", "noPicture", "didNotPlay", "veryFew",
    /// "otherYear", "mayPause", "washedOut", "large", "few", "ready", "quick"
    /// or "should".
    let kind: String
    var quality: String?
    var service: String?
    var seeders: Int?
    var year: Int?
    var needMbps: Double?
    var haveMbps: Double?
    var samples: Int?
    /// "mayPause": `haveMbps` is the small-swarm figure (a swarm under the wanted seeders).
    var smallSwarm: Bool?
    var sizeBytes: Int64?
    var limitBytes: Int64?
}

struct VersionRow: Decodable, Hashable, Sendable, Identifiable {
    /// What `playback` takes.
    let key: String
    /// "4K", "1080p", "720p" or "SD"; else the source ("WEB-DL") or "Quality not stated".
    let quality: String
    /// Words, never emoji: "Dolby Vision", "HDR10", "Atmos", "5.1", "Remux".
    let features: [String]
    let sizeBytes: Int64?
    let status: VersionStatus
    /// Service ("Real-Debrid") or addon name.
    let via: String
    let releaseName: String
    let languages: [String]
    /// "readyNow", "playsDirectly", "matchesScreen", "sameReleaseAsLast", "yourLanguage", "smallerFaster".
    let reasons: [String]
    /// "best", "smallerFaster", "otherLanguages" or "needsDownload".
    let group: String
    let failoverCount: Int
    /// What to tell the user about it (optional for older cores).
    let line: ReasonLine?
    /// "ready", "unknown", "swarm" or "fetch".
    let sourceClass: String?
    let bitrateMbps: Double?

    var id: String { key }
}

enum VersionStatus: Decodable, Hashable, Sendable {
    case ready(service: String?)
    case torrent(seeds: Int?)
    case needsDownload(service: String?)
    case direct

    private enum Key: String, CodingKey { case kind, service, seeds }

    init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: Key.self)
        switch try c.decode(String.self, forKey: .kind) {
        case "ready": self = .ready(service: try c.decodeIfPresent(String.self, forKey: .service))
        case "torrent": self = .torrent(seeds: try c.decodeIfPresent(Int.self, forKey: .seeds))
        case "needsDownload": self = .needsDownload(service: try c.decodeIfPresent(String.self, forKey: .service))
        case "direct": self = .direct
        case let kind:
            throw DecodingError.dataCorruptedError(forKey: .kind, in: c, debugDescription: "unknown status \(kind)")
        }
    }
}

struct HiddenSummary: Decodable, Hashable, Sendable {
    let count: Int
    /// Hide reason ("wrongTitle", "unplayable", ...) and how many it hid.
    let byReason: [ReasonCount]

    struct ReasonCount: Decodable, Hashable, Sendable {
        let reason: String
        let count: Int

        /// The core sends each pair as a two-element array.
        init(from decoder: any Decoder) throws {
            var c = try decoder.unkeyedContainer()
            reason = try c.decode(String.self)
            count = try c.decode(Int.self)
        }
    }
}

struct PlaybackSnapshot: Decodable, Hashable, Sendable {
    /// "resolving", "ready" or "failed".
    let phase: String
    /// Loopback URL for the player, once `ready`.
    let url: String?
    /// The version that is playing: the picked key or one of its failovers.
    let key: String?
    /// The source adopted last, one more per new source; named in `failOver`
    /// (optional for older cores).
    let attempt: Int64?
    let filename: String?
    let sizeBytes: Int64?
    /// Where to start, from saved progress.
    let resumeMs: Int64?
    /// Sources that failed, in order, each with a plain reason.
    let attempts: [Attempt]
    /// External subtitles as loopback WebVTT URLs, growing while pending.
    let subtitles: [SubtitleTrack]
    let subtitlesPending: Bool
    /// The next released episode, for an Up Next card.
    let next: NextEpisode?
    /// Pass as `bingeGroup` when opening Versions for the next episode.
    let bingeGroup: String?
    /// Skip segments of the playing version, after `duration` (optional for older cores).
    let segments: [Segment]?
    /// When to offer Up Next (the credits start); nil keeps the app's own timing.
    let nextAtMs: Int64?
    /// Trakt's side of this play (optional for older cores).
    let trakt: TraktPlayback?
    /// Why subtitle files did not load, one per source and problem; empty
    /// again after a failover. An older core sends none: read `notes`.
    let subtitleNotes: [SubtitleNote]?
    /// The recommendation failed for a reason of its own and the view moved
    /// on by itself to this version: named while `resolving` (optional for older cores).
    let trying: OtherVersion?
    /// On `failed`, after a version picked by hand: the ranking's next pick,
    /// opened with its key from the same Versions view (optional for older cores).
    let offer: OtherVersion?
    /// The track the core turns on at start (plans/m13-subtitle-languages.md 6),
    /// once the list has settled; nil for none (optional for older cores).
    let autoSubtitle: String?
    /// Try again taken in this view, one more each time it is accepted: show
    /// "Trying again" from it, so a retry that fails again at once still reads
    /// as a new try (optional for older cores).
    let retries: Int?

    var notes: [SubtitleNote] { subtitleNotes ?? [] }
}

/// A version the playback moved on to, or offers instead: its key for
/// `switchVersion`, its label ("1080p WEB") for the viewer.
struct OtherVersion: Decodable, Hashable, Sendable {
    let key: String
    let label: String
}

/// What the player does about a subtitle note: the words to say for a few
/// seconds, and whether the picked file goes off.
struct SubtitleTrouble: Equatable {
    let say: String
    let off: Bool
}

/// A subtitle source's trouble (plans/m13-turkcealtyazi.md 4.2). The route
/// answers every failed file with an empty WebVTT, so the player never sees
/// an error: this note is how the viewer learns why.
struct SubtitleNote: Decodable, Hashable, Sendable {
    /// The addon's name, as on `SubtitleTrack.source`.
    let source: String
    /// "unreachable", "blocked", "changed", "trackFailed"; a string, so a newer core's value still decodes.
    let problem: String

    /// The words the player shows, the same on every app.
    var words: String {
        switch problem {
        case "unreachable": "\(source) did not answer. Try its subtitles again in a moment."
        case "blocked": "\(source) is turning requests away. Marquee will try again in a few minutes."
        case "changed": "\(source) changed its pages, so Marquee cannot read them. An update will fix this."
        case "trackFailed": "A subtitle file from \(source) could not be read. Pick another one."
        default: "Subtitles from \(source) did not load."
        }
    }

    /// The notes a snapshot adds to the last one, in its order. A failover
    /// empties the list, so the count starts again from nothing.
    static func added(from old: [SubtitleNote], to new: [SubtitleNote]) -> [SubtitleNote] {
        new.filter { !old.contains($0) }
    }
}

/// Where a source starts: where the last one stopped when a failover
/// replaced it, else the saved position, or the beginning for Start over.
func sourceStart(stoppedAt: Double?, resumeMs: Int64?, fromStart: Bool = false) -> Double {
    stoppedAt ?? (fromStart ? 0 : Double(resumeMs ?? 0) / 1000)
}

/// Where a failed source stopped: its start while the seek to it has not
/// landed (the player still says 0 there), else the playhead.
/// The start seek's completion: the position it landed at when it finished
/// for the item still loaded and the start is still pending (no viewer seek
/// took over), else nil and the start stays pending.
func startLanded(finished: Bool, sameItem: Bool, pending: Double?) -> Double? {
    guard finished, sameItem else { return nil }
    return pending
}

/// The position to keep from a periodic time: none while a seek is in flight,
/// since the player can still report where it was. Only the seek's own
/// completion ends the wait (`startLanded`).
func ticked(time: Double, pending: Double?) -> Double? {
    pending == nil ? time : nil
}

func stoppedPosition(pendingStart: Double?, position: Double) -> Double {
    pendingStart ?? position
}

/// What failed, as `failOver` reports it: the core words `kind` for the
/// attempt list, and keeps `error` (the player's own words) only for "other".
struct PlayerFailure: Equatable, Sendable {
    let kind: String
    let error: String
}

/// AVFoundation's decode and format errors: the device cannot play the file.
/// A decoder that is only busy now (decoderTemporarilyUnavailable) is other:
/// it says nothing against the version.
private let decodeErrors: Set<Int> = [
    AVError.Code.decodeFailed.rawValue,
    AVError.Code.decoderNotFound.rawValue,
    AVError.Code.fileFormatNotRecognized.rawValue,
]

/// Sorts a player item's error into what failed: a URL loading error is a
/// timeout or a network failure, an AVFoundation decode or format error is
/// decode, and anything else is other with its own description.
func playerFailure(_ error: Error?) -> PlayerFailure {
    guard let error else { return PlayerFailure(kind: "other", error: "the player could not open this source") }
    let ns = error as NSError
    let said = ns.localizedDescription
    switch ns.domain {
    case NSURLErrorDomain:
        return PlayerFailure(kind: ns.code == NSURLErrorTimedOut ? "timeout" : "network", error: said)
    case AVFoundationErrorDomain where decodeErrors.contains(ns.code):
        return PlayerFailure(kind: "decode", error: said)
    default:
        return PlayerFailure(kind: "other", error: said)
    }
}

/// The `failOver` command for `failure` of the source the player loaded as
/// `attempt` (the snapshot's when it was loaded). An unknown attempt is left
/// out, and the core then acts on the current source.
func failOverCommand(_ failure: PlayerFailure, attempt: Int64?) -> [String: any Sendable] {
    var command: [String: any Sendable] = ["type": "failOver", "error": failure.error, "kind": failure.kind]
    if let attempt { command["attempt"] = attempt }
    return command
}

extension PlaybackSnapshot {
    /// Up Next shows from the release-matched credits start when the core
    /// knows it, else in the last 20 s; at once after Skip credits, and at the end.
    func offersNext(positionMs: Int64, durationMs: Int64, ended: Bool, skippedAt: Int64?) -> Bool {
        guard next != nil, durationMs > 60_000 else { return false }
        if ended { return true }
        if let skippedAt, positionMs >= skippedAt { return true }
        if let nextAtMs { return positionMs >= nextAtMs }
        return durationMs - positionMs <= 20_000
    }

    /// The segment under the playhead.
    func segment(at positionMs: Int64) -> Segment? {
        segments?.first { positionMs >= $0.startMs && positionMs < $0.endMs }
    }
}

/// A stretch a Skip button jumps over, active on `[startMs, endMs)`.
struct Segment: Decodable, Hashable, Sendable {
    /// "intro", "recap", "credits" or "preview".
    let kind: String
    let startMs: Int64
    let endMs: Int64
    /// The database it came from, for attribution.
    let source: String

    /// "Skip intro", "Skip recap", "Skip credits", "Skip preview".
    var label: String { "Skip \(kind)" }

    /// Credits that run to the end of the file lead into Up Next, not a seek.
    func endsFile(durationMs: Int64) -> Bool {
        kind == "credits" && durationMs > 0 && endMs >= durationMs - 1000
    }
}

struct Attempt: Decodable, Hashable, Sendable {
    let key: String
    let via: String
    let error: String
}

/// Sources that failed the same way, from one addon, said once.
struct FailedSources: Hashable, Sendable {
    let via: String
    let error: String
    let count: Int

    /// "The link no longer exists (404) · 8 sources".
    var words: String { count > 1 ? "\(error) · \(count) sources" : error }

    /// One row per addon and reason, whatever its count.
    var key: String { "\(via)\u{0}\(error)" }

    /// The failure history with each addon and reason once, in the order they first failed.
    static func group(_ attempts: [Attempt]) -> [FailedSources] {
        var order: [FailedSources] = []
        for attempt in attempts {
            if let i = order.firstIndex(where: { $0.via == attempt.via && $0.error == attempt.error }) {
                order[i] = FailedSources(via: attempt.via, error: attempt.error, count: order[i].count + 1)
            } else {
                order.append(FailedSources(via: attempt.via, error: attempt.error, count: 1))
            }
        }
        return order
    }
}

struct SubtitleTrack: Decodable, Hashable, Sendable, Identifiable {
    let url: String
    /// As the addon gives it, usually ISO 639-2 ("eng").
    let lang: String
    let label: String?
    /// Addon name.
    let source: String
    let forced: Bool
    let hearingImpaired: Bool
    /// One tag for every spelling of the language ("pt-BR", "zh-Hant"); older cores send none.
    var language: String?
    /// Its English name, the heading of its group ("Portuguese (Brazil)").
    var languageName: String?
    /// What tells it from the other tracks of its language: the release, else
    /// the source, numbered when two would read the same.
    var name: String?

    var id: String { url }
}

/// The Subtitles menu (plans/m13-subtitle-languages.md 2): the core's tracks by
/// language in the core's order, which puts the profile's languages first.
/// Those stay on top; the rest go under "Other languages".
enum SubtitleMenu {
    struct Group: Hashable {
        let language: String
        let heading: String
        let tracks: [SubtitleTrack]
    }

    /// The same track in a new list after the view opened again on another
    /// address: everything but its URL matches.
    static func same(_ track: SubtitleTrack, in tracks: [SubtitleTrack]) -> SubtitleTrack? {
        tracks.first {
            $0.lang == track.lang && $0.label == track.label && $0.source == track.source && $0.forced == track.forced
                && $0.hearingImpaired == track.hearingImpaired && $0.language == track.language && $0.name == track.name
        }
    }

    static func groups(_ tracks: [SubtitleTrack], preferred: [String]) -> (top: [Group], other: [Group]) {
        var order: [String] = []
        var byLanguage: [String: [SubtitleTrack]] = [:]
        for track in tracks {
            let key = nonEmpty(track.language) ?? track.lang
            if byLanguage[key] == nil { order.append(key) }
            byLanguage[key, default: []].append(track)
        }
        let all = order.compactMap { key in
            byLanguage[key].map { Group(language: key, heading: heading($0[0]), tracks: $0) }
        }
        let wanted = Set(preferred.map(base))
        let top = all.filter { wanted.contains(base($0.language)) }
        // With none of the profile's languages found, every language is on top.
        return top.isEmpty ? (all, []) : (top, all.filter { !top.contains($0) })
    }

    /// The track to turn on at start: the core's `autoSubtitle`, once, while
    /// the viewer has not picked anything in this play (a track, one in the
    /// file, or Off), and only once the track is in the list.
    static func autoPick(_ auto: String?, settled: Bool, tracks: [SubtitleTrack]) -> SubtitleTrack? {
        guard !settled, let auto else { return nil }
        return tracks.first { $0.url == auto }
    }

    /// The languages the menu puts on top, as the core orders the list: the
    /// profile's subtitle languages, then its audio languages, else the
    /// device's language and English (the languages the pick was ranked with).
    static func preferred(subtitle: [String], audio: [String], device: String?) -> [String] {
        subtitle + (audio.isEmpty ? [device, "en"].compactMap { $0 } : audio)
    }

    /// The group's heading and the Subtitles button's value.
    static func heading(_ track: SubtitleTrack) -> String {
        nonEmpty(track.languageName) ?? Locale(identifier: "en").localizedString(forLanguageCode: track.lang) ?? track.lang
    }

    /// A track's line in its group.
    static func title(_ track: SubtitleTrack) -> String {
        nonEmpty(track.name) ?? nonEmpty(track.label) ?? track.source
    }

    /// Beneath the line: the source unless the line starts with it, then Forced and SDH.
    static func detail(_ track: SubtitleTrack) -> String? {
        let parts = [
            title(track).hasPrefix(track.source) ? nil : track.source,
            track.forced ? "Forced" : nil,
            track.hearingImpaired ? "For the hard of hearing" : nil,
        ].compactMap { $0 }
        return parts.isEmpty ? nil : parts.joined(separator: ", ")
    }

    /// The language without its region or script, any ISO 639 spelling: "ger",
    /// "deu" and "de-AT" are all "de". A preference counts for its regions.
    static func base(_ code: String) -> String {
        Locale(identifier: code).language.languageCode.map { $0.identifier(.alpha2) ?? $0.identifier } ?? code.lowercased()
    }

    private static func nonEmpty(_ text: String?) -> String? {
        text.flatMap { $0.isEmpty ? nil : $0 }
    }
}

struct NextEpisode: Decodable, Hashable, Sendable {
    let videoId: String
    let title: String
    let season: Int
    let episode: Int
    let thumbnail: String?
}

/// What one play tells the core of the viewer, and whether the core's start
/// subtitle still applies (plans/m13-subtitle-languages.md 6). The player
/// owns one and starts it over for each episode and version.
@MainActor final class PlayChoices {
    /// Sends a playback command to the view opened last (`opened`).
    private(set) var send: ((_ command: [String: any Sendable]) -> Void)?
    /// The viewer picked a track, one in the file, or Off: it wins for the play.
    private(set) var viewerChose = false
    /// The start subtitle turned on for the current source.
    private var startApplied = false
    /// The source the play is on: the snapshot's `attempt`.
    private var attempt: Int64?

    var subtitleSettled: Bool { viewerChose || startApplied }

    /// The playback view `sub` opened: commands go to it, never to an older view.
    func opened(sub: UInt64, deliver: @escaping (_ sub: UInt64, _ command: [String: any Sendable]) -> Void) {
        send = { deliver(sub, $0) }
    }

    /// The start subtitle to turn on now, once per source, as if picked,
    /// sending nothing; never over the viewer's own choice.
    func startSubtitle(_ auto: String?, tracks: [SubtitleTrack]) -> SubtitleTrack? {
        guard let track = SubtitleMenu.autoPick(auto, settled: subtitleSettled, tracks: tracks) else { return nil }
        startApplied = true
        return track
    }

    /// The viewer's pick from the menu, or Off (nil): it wins for the rest of
    /// the play, and the core remembers it for the title.
    func chose(_ track: SubtitleTrack?) {
        viewerChose = true
        send?(["type": "subtitleChosen", "url": track?.url ?? NSNull()])
    }

    /// One of the file's own tracks: the viewer's pick, which the core does not know of.
    func choseInFile() {
        viewerChose = true
    }

    /// Try again on the failure card: the same versions from the first.
    func retry() {
        send?(["type": "retry"])
    }

    /// The view opened again because the stream server moved (review
    /// apple-journey P3-5): the same source on a new address, so its next
    /// attempt number drops no subtitle and the start subtitle does not come
    /// back over the viewer's choice.
    func reopened() {
        attempt = nil
    }

    /// Another episode or version: its own start subtitle applies.
    func newPlay() {
        (viewerChose, startApplied, attempt) = (false, false, nil)
    }

    /// The snapshot's source. A move on to another source drops the start
    /// pick, whose file was the old release's, so the new source's start
    /// subtitle applies; the viewer's own choice stays. True when the shown
    /// subtitle has to go.
    func source(_ next: Int64?) -> Bool {
        guard let next else { return false }
        defer { attempt = next }
        guard let attempt, next != attempt else { return false }
        // The viewer's pick goes too, as on the web: its file was the old
        // release's. The core's start subtitle for the new list brings back
        // the viewer's language, and Off stays Off (review r4 P3-4).
        (viewerChose, startApplied) = (false, false)
        return true
    }

    /// One snapshot's effect on the shown subtitle: a new source drops it,
    /// then the start subtitle turns on, once per source. The player makes
    /// this one call per snapshot.
    func subtitleStep(attempt: Int64?, auto: String?, tracks: [SubtitleTrack]) -> (drop: Bool, turnOn: SubtitleTrack?) {
        let drop = source(attempt)
        return (drop, startSubtitle(auto, tracks: tracks))
    }

    /// Another version of the same video, on the same Versions view: the play
    /// starts over. Nil for the version already playing.
    func switchVersion(from route: PlayRoute, to key: String) -> PlayRoute? {
        guard key != route.key else { return nil }
        newPlay()
        return PlayRoute(versionsSub: route.versionsSub, key: key, target: route.target, tapped: .now)
    }

    /// The next episode's pick: the play starts over.
    func nextEpisode(_ pick: PlayRoute) -> PlayRoute {
        newPlay()
        return pick
    }
}

/// What to find versions for: a movie, or one episode of a series.
struct PlayTargetInfo: Hashable, Sendable {
    let kind: String
    /// The movie or series id.
    let itemId: String
    let videoId: String
    /// The movie or series name, never an episode title: ranking uses it to
    /// hide sources for other titles.
    let title: String
    let poster: String?
    let season: Int?
    let episode: Int?
    let resumeMs: Int64?
    /// What the title page knows, sent so the ranking need not wait for the meta.
    var runtime: String?
    var releaseInfo: String?
    var released: String?
    var episodesInSeason: Int?

    /// `season` and `episode` when the caller knows them (the core's next
    /// episode); else read from an id shaped "<series>:<season>:<episode>".
    init(
        kind: String, itemId: String, videoId: String, title: String, poster: String?, resumeMs: Int64?,
        season: Int? = nil, episode: Int? = nil
    ) {
        (self.kind, self.itemId, self.videoId, self.title, self.poster, self.resumeMs) =
            (kind, itemId, videoId, title, poster, resumeMs)
        let parts = videoId.split(separator: ":")
        let parsed = parts.count == 3 ? (Int(parts[1]), Int(parts[2])) : (nil, nil)
        (self.season, self.episode) = (season ?? parsed.0, episode ?? parsed.1)
    }
}

/// Opens a playback view for one version.
struct PlayRoute: Hashable, Identifiable {
    let versionsSub: UInt64
    let key: String
    let target: PlayTargetInfo
    /// When the user asked to play, for time to first frame.
    let tapped: ContinuousClock.Instant
    /// Start over: the first source starts at the beginning, not the saved position.
    var fromStart = false

    var id: String { "\(versionsSub):\(key)" }
}
