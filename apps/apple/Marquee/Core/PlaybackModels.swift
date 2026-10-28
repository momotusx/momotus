// Versions and playback DTOs from the core, mirroring
// apps/web/src/core/generated/{VersionsSnapshot,VersionRow,PlaybackSnapshot,...}.ts.
// Enumerations the UI only displays (group, reason, hide reason) stay strings
// so a new value from the core never breaks decoding.

import AVFoundation
import Foundation

struct VersionsSnapshot: Decodable, Hashable, Sendable {
    let recommended: VersionRow?
    let rows: [VersionRow]
    let pendingAddons: [String]
    let slowAddons: [String]
    let failedAddons: [String]
    let done: Bool
    let frozen: Bool
    /// What the ranking read about this device and title (optional for older cores).
    let conditions: VersionsConditions?
    /// The one-time learning note, shown with Undo.
    let note: LearnedNote?
    /// Nothing can play, and why (plans/play-every-version.md 2a); nil while
    /// anything plays or the view still asks (optional for older cores).
    var empty: EmptyVersions?
    /// The "Smaller file" and "Sharper" shortcuts (2f), never the pick itself.
    var smaller: VersionRow?
    var sharper: VersionRow?
    /// `rows`' keys by quality, in the core's order (2f); empty from an older core.
    var sections: [VersionSection]?
    /// What was hidden and why, for the versions metric line only (plans/apple-second-engine.md section 4).
    var hidden: HiddenSummary?
    /// "From plugins" (plans/plugins-and-marketplace.md 4.4, 7.8): the Nuvio providers' rows, a pass of their
    /// own after the addons', never the pick (optional for older cores).
    var plugins: SourceRows?
}

/// The rows of a source ranked in a pass of its own, and whether it is still working.
struct SourceRows: Decodable, Hashable, Sendable {
    let rows: [VersionRow]
    let pending: Bool
}

struct HiddenSummary: Decodable, Hashable, Sendable {
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

/// Why nothing can play (plans/play-every-version.md 2a), with what the
/// shared table offers for it.
struct EmptyVersions: Decodable, Hashable, Sendable {
    /// "noSource", "unreachable", "needsAccount", "kidsNothing",
    /// "noResults" or "nothingPlaysHere"; a string, so a newer state still decodes.
    let kind: String
    /// The versions found, for the states that have some.
    let count: Int
    /// Those versions, listed dimmed and each still startable.
    let rows: [VersionRow]
    /// The source kept on this device: Play from this device starts it.
    let key: String?
}

/// One quality section of the Versions page: "uhd", "fhd", "hd" or "other".
struct VersionSection: Decodable, Hashable, Sendable {
    let quality: String
    let keys: [String]
}

/// What the ranking read (plans/m13-stream-choice.md 4.2): "This TV: 4K HDR,
/// about 42 Mbit/s on Wi-Fi".
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
    /// "otherYear", "mayPause", "washedOut", "large", "few", "ready", "quick",
    /// "should" or "buffer".
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
    /// "buffer": the minutes ahead its window holds (at most 10); none on mobile data.
    var aheadMin: Int?
    /// "buffer": the window's size.
    var budgetBytes: Int64?
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
    /// "On this device": the version last played here starts from what this
    /// device keeps, with no peer (optional for older cores).
    var onDevice: String?
    /// "addon", "archive" or "plugin" (optional for older cores).
    var origin: String?
    /// The host a plugin or Archive row plays from, and its source's name.
    var host: String?
    var sourceName: String?

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
    /// Every track the start subtitle could turn on failed to read, each handing over to the next as
    /// `autoSubtitle` changed, and none is left: say so with no source (optional for older cores).
    var startSubtitleFailed: Bool?
    /// The recommendation failed for a reason of its own and the view moved
    /// on by itself to this version: named while `resolving` (optional for older cores).
    let trying: OtherVersion?
    /// On `failed`, after a version picked by hand: the ranking's next pick,
    /// opened with its key from the same Versions view (optional for older cores).
    let offer: OtherVersion?
    /// The start has taken 20 s, or a stall the core will not move on from
    /// lasts: "Choose another version" shows (plans/play-every-version.md 2e).
    var chooseAnother: Bool?
    /// Why nothing played, once `failed`: an `empty` kind ("notStarted",
    /// "nothingPlaysHere", "unreachable"); nil from an older core.
    var empty: String?
    /// The track the core turns on at start (plans/m13-subtitle-languages.md 6),
    /// once the list has settled; nil for none (optional for older cores).
    let autoSubtitle: String?
    /// The second subtitle line's track (plans/dual-subtitles.md 3.2), followed until the viewer picks one;
    /// drawn only while a first line is on; nil for none and from an older core.
    let autoSecondSubtitle: String?
    /// The tag the core reads the audio in, for the Spoken language row; nil when not known.
    let spokenLanguage: String?
    /// Try again taken in this view, one more each time it is accepted: show
    /// "Trying again" from it, so a retry that fails again at once still reads
    /// as a new try (optional for older cores).
    let retries: Int?
    /// How far the start is while `resolving`: the loading screen's fill and
    /// its words (plans/player-polish.md 1.3); nil before the first source
    /// starts, from `ready` on, and from an older core.
    var start: StartProgress?
    /// The file's container ("mkv"), once `ready`: the opened file's, else what an earlier play of the
    /// version opened, else the release's (optional for older cores).
    let container: String?
    /// The release's audio formats in the core's names ("dts_hd_ma") (optional for older cores).
    let audio: [String]?
    /// The second engine this version played on here before ("aether") (optional for older cores).
    let engine: String?
    /// When the start budget ends, epoch ms, while a first play is still opening; read only then
    /// (plans/apple-second-engine.md 2.3). Nil otherwise and from an older core.
    let startBudgetEndsAt: Int64?

    var notes: [SubtitleNote] { subtitleNotes ?? [] }
}

/// One start of the playback, from the first source it tries to `ready`.
struct StartProgress: Decodable, Hashable, Sendable {
    /// "details", "connecting", "buffering" or "opening"; a string, so a newer core's stage still decodes.
    let stage: String
    /// Peers connected to a torrent.
    let peers: Int
    /// What a torrent's swarm sent of the start the player needs, and all of it.
    let haveBytes: Int64
    let needBytes: Int64
    let bytesPerS: Int64
    /// The source of the version it is on, from 1, of `sources`.
    let source: Int
    let sources: Int
    /// 0 to 1, never back within one start, never 1 before `ready`.
    let progress: Double
    /// The core's words once a stage takes long ("Finding peers, 3 so far",
    /// "Trying source 2 of 3"); nil otherwise.
    let caption: String?
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
        case "blocked": "\(source) is turning requests away. Momotus will try again in a few minutes."
        case "changed": "\(source) changed its pages, so Momotus cannot read them. An update will fix this."
        case "trackFailed": "This subtitle could not be read."
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
    /// What tells it from the other tracks of its language: the group, source
    /// and resolution its release text names ("CAKES, WEB 1080p"), else the
    /// text, else the source, numbered when two would read the same.
    var name: String?
    /// The language's own name ("Türkçe", "Português (Brasil)"); older cores send none.
    var nativeName: String?
    /// The track names the playing release's group: "Same release group";
    /// older cores send none.
    var fits: Bool?
    /// The release text the addon names, for a detail line; older cores send none.
    var release: String?
    /// OpenSubtitles says it was made for this very file (its hash): "Matches this file".
    var hashMatch: Bool?
    /// The frame rate it was made for, when one of it and the playing file's is PAL (25) and the
    /// other is not: "Made for 25 fps".
    var madeForFps: Double?
    /// The delay the viewer last left this track at on this version and video, in ms (positive: later).
    var delayMs: Int?

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

    /// A track's line in its group; with no name or label its language, never the source (as Android's TrackNames).
    static func title(_ track: SubtitleTrack) -> String {
        nonEmpty(track.name) ?? nonEmpty(track.label) ?? heading(track)
    }

    /// Beneath the line, as the web and Android say it: the source unless the line starts with it,
    /// Forced, SDH, the core's match ("Matches this file", else "Same release group"), "Made for 25
    /// fps", then the release text cut in the middle to the line's `max` characters.
    static func detail(_ track: SubtitleTrack, max: Int = 46) -> String? {
        // Never the source: no addon is named where a viewer reads it.
        var parts = [
            track.forced ? "Forced" : nil,
            track.hearingImpaired ? "For the hard of hearing" : nil,
            track.hashMatch == true ? "Matches this file" : track.fits == true ? "Same release group" : nil,
            track.madeForFps.map(madeFor),
        ].compactMap { $0 }
        if let release = nonEmpty(track.release), release != title(track) {
            let room = max - parts.joined(separator: ", ").count - 2
            parts.append(middle(release, max: room >= 24 ? room : max))
        }
        return parts.isEmpty ? nil : parts.joined(separator: ", ")
    }

    /// "Made for 25 fps", "Made for 23.976 fps": the rate as written, not the core's float.
    static func madeFor(_ fps: Double) -> String {
        var rate = String(format: "%.3f", fps)
        while rate.hasSuffix("0") { rate.removeLast() }
        if rate.hasSuffix(".") { rate.removeLast() }
        return "Made for \(rate) fps"
    }

    /// `text` cut to `max` characters in the middle, at a dot or a space, so a release's start and
    /// its group at the end both stay in view (the web's `middle`).
    static func middle(_ text: String, max: Int) -> String {
        let c = Array(text)
        guard c.count > max else { return text }
        let at = { (j: Int) in j >= 0 && j < c.count && (c[j] == "." || c[j] == " ") }
        var from = c.count - Swift.max(1, max - 1 - (max - 1) * 45 / 100)
        while from > 0 && !at(from - 1) { from -= 1 }
        // A tail with no boundary near enough keeps its share and starts at the next boundary instead.
        if c.count - from > max - 5 {
            from = c.count - (max - 5)
            while from < c.count && !at(from - 1) { from += 1 }
        }
        let tail = String(c[from...]).drop { $0 == "." || $0 == " " }
        var cut = max - 1 - tail.count
        while cut > 0 && !at(cut) { cut -= 1 }
        var head = String(c[..<(cut > 0 ? cut : Swift.max(0, max - 1 - tail.count))])
        while head.hasSuffix(".") || head.hasSuffix(" ") { head.removeLast() }
        return "\(head)\u{2026}\(tail)"
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
    /// The core's start subtitle (`autoSubtitle`) turned on for the current source. The core moves it on
    /// when its file cannot be read, so a new one is turned on in its place while the viewer has not chosen.
    private var appliedAuto: String?
    /// The view opened again on a new address: the next `autoSubtitle` is the same track renamed.
    private var adoptNextAuto = false
    /// The source the play is on: the snapshot's `attempt`.
    private var attempt: Int64?

    /// The playback view `sub` opened: commands go to it, never to an older view.
    func opened(sub: UInt64, deliver: @escaping (_ sub: UInt64, _ command: [String: any Sendable]) -> Void) {
        send = { deliver(sub, $0) }
    }

    /// The start subtitle to turn on now, as if picked, sending nothing: once per `autoSubtitle` the core
    /// names (a new one when the last could not be read), never over the viewer's own choice.
    func startSubtitle(_ auto: String?, tracks: [SubtitleTrack]) -> SubtitleTrack? {
        guard !viewerChose, let auto, auto != appliedAuto, let track = tracks.first(where: { $0.url == auto }) else { return nil }
        if adoptNextAuto {
            // The same track on the new address: it stays on as it was (its cues and timing kept).
            (appliedAuto, adoptNextAuto) = (auto, false)
            return nil
        }
        appliedAuto = auto
        return track
    }

    /// The start subtitle the core named has gone with none after it (`startSubtitleFailed`): the one
    /// shown goes off. Never the viewer's own choice.
    func startSubtitleGone(_ auto: String?) -> Bool {
        guard !viewerChose, appliedAuto != nil, auto == nil else { return false }
        appliedAuto = nil
        return true
    }

    /// The viewer's pick from the menu, or Off (nil): it wins for the rest of
    /// the play, and the core remembers it for the title.
    func chose(_ track: SubtitleTrack?) {
        viewerChose = true
        send?(["type": "subtitleChosen", "url": track?.url ?? NSNull()])
    }

    /// One of the file's own tracks: the viewer's pick. With its language tag the core remembers it as
    /// a track of that language (the next plays start with it); with none it is this play's alone.
    func choseInFile(language: String?) {
        viewerChose = true
        if let language { send?(["type": "subtitleChosen", "url": NSNull(), "language": language]) }
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
        adoptNextAuto = appliedAuto != nil
    }

    /// Another episode or version: its own start subtitle applies.
    func newPlay() {
        (viewerChose, appliedAuto, adoptNextAuto, attempt) = (false, nil, false, nil)
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
        (viewerChose, appliedAuto) = (false, nil)
        return true
    }

    /// One snapshot's effect on the shown subtitle: a new source drops it,
    /// then the start subtitle turns on, once per source. The player makes
    /// this one call per snapshot.
    func subtitleStep(attempt: Int64?, auto: String?, tracks: [SubtitleTrack]) -> (drop: Bool, turnOn: SubtitleTrack?) {
        let drop = source(attempt)
        let gone = startSubtitleGone(auto)
        return (drop || gone, startSubtitle(auto, tracks: tracks))
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

    /// Play pressed while versions are still found: the player opens at once
    /// and starts the pick once it settles (plans/play-every-version.md Step 3).
    var waitsForPick: Bool { key.isEmpty }
}

/// The subtitle and audio pickers (plans/player-polish.md 2.4, design/DECISION.md "Subtitle picker"): the
/// languages first, in the core's order with the profile's on top, each with its flag, English name, own name
/// and count; then one language's options, the file's own tracks first. The web's `lib/subtitles.ts`.
enum TrackPicker {
    /// One of the file's own tracks, by its place in the player's list.
    struct FileTrack: Hashable {
        let index: Int
        let name: String
        /// Its language tag as the file gives it ("en", "eng", "pt-BR"); nil or "und" for none.
        let language: String?
    }

    struct Language: Hashable, Identifiable {
        let language: String
        let name: String
        /// Its own name ("Türkçe") when it differs from the English one.
        let own: String?
        /// What a search matches as a code: the tag and every spelling the tracks carry.
        let codes: [String]
        var file: [FileTrack]
        let tracks: [SubtitleTrack]

        var id: String { language }
        /// Every option the language opens.
        var count: Int { file.count + tracks.count }
    }

    enum Option: Hashable {
        case file(Int)
        case track(SubtitleTrack)
    }

    /// The core's languages in its order (one per run of a language), each with the file's tracks of that
    /// language (joined by tag, else by its base language); a language only the file has comes after them, and
    /// the file's tracks that name none last, under "Unknown language" (as Android's picker).
    static func languages(_ tracks: [SubtitleTrack], file: [FileTrack]) -> [Language] {
        var out: [Language] = []
        for track in tracks {
            let tag = track.language ?? track.lang
            if let last = out.last, last.language == tag {
                out[out.count - 1] = Language(
                    language: last.language, name: last.name, own: last.own,
                    codes: Array(Set(last.codes + [track.lang.lowercased()])).sorted(), file: [], tracks: last.tracks + [track])
            } else {
                let name = SubtitleMenu.heading(track)
                let own = track.nativeName.flatMap { $0.isEmpty || $0 == name ? nil : $0 }
                out.append(Language(language: tag, name: name, own: own, codes: Array(Set([tag.lowercased(), track.lang.lowercased()])).sorted(), file: [], tracks: [track]))
            }
        }
        var unknown: [FileTrack] = []
        for f in file {
            guard let raw = TrackPicker.tag(f.language) else {
                unknown.append(f)
                continue
            }
            let tag = fileTag(raw)
            if let i = out.firstIndex(where: { $0.language.lowercased() == tag.lowercased() })
                ?? out.firstIndex(where: { SubtitleMenu.base($0.language) == SubtitleMenu.base(tag) }) {
                out[i].file.append(f)
            } else {
                out.append(Language(language: tag, name: englishName(tag), own: ownName(tag), codes: Array(Set([tag.lowercased(), raw.lowercased()])).sorted(), file: [f], tracks: []))
            }
        }
        if !unknown.isEmpty {
            out.append(Language(language: "", name: "Unknown language", own: nil, codes: [], file: unknown, tracks: []))
        }
        return out
    }

    /// A language tag as a file gives it ("en", "eng", "pt-BR"); nil for none or "und" (undetermined).
    static func tag(_ language: String?) -> String? {
        guard let language, !language.isEmpty, !language.hasPrefix("und") else { return nil }
        return language
    }

    /// A file's code as a tag: "eng" is "en", "pt-BR" stays.
    static func fileTag(_ code: String) -> String {
        let parts = code.split(separator: "-", maxSplits: 1).map(String.init)
        let base = SubtitleMenu.base(parts[0])
        return parts.count > 1 ? "\(base)-\(parts[1])" : base
    }

    static func englishName(_ tag: String) -> String {
        Locale(identifier: "en").localizedString(forIdentifier: tag) ?? tag.uppercased()
    }

    /// A file's language in its own words, capitalized as a name ("Deutsch", "Français"); nil when it reads as
    /// the English name or the system has no name for it.
    static func ownName(_ tag: String) -> String? {
        let locale = Locale(identifier: tag)
        guard let own = locale.localizedString(forIdentifier: tag), own != englishName(tag) else { return nil }
        return own.prefix(1).uppercased(with: locale) + own.dropFirst()
    }

    /// Lower case with the accents gone, so "turkce" finds "Türkçe".
    private static func fold(_ s: String) -> String {
        s.folding(options: [.diacriticInsensitive, .caseInsensitive], locale: Locale(identifier: "en")).lowercased()
    }

    private static func words(_ s: String) -> [String] {
        fold(s).split(whereSeparator: { !$0.isLetter && !$0.isNumber }).map(String.init)
    }

    /// The search's rule: a word of the English name or of the own name starts with what was typed, or a code
    /// does ("pt", "pob", "deu", "pt-br").
    static func matches(_ l: Language, _ typed: String) -> Bool {
        let code = fold(typed.trimmingCharacters(in: .whitespaces))
        guard !code.isEmpty else { return true }
        let q = words(code).joined(separator: " ")
        let starts = { (text: String) in (" " + words(text).joined(separator: " ")).contains(" " + q) }
        return starts(l.name) || l.own.map(starts) == true || l.codes.contains { $0.hasPrefix(code) || $0.hasPrefix(q) }
    }

    /// What picking a language turns on at once: the file's own track (it always fits the file), else the
    /// core's best match, its first.
    static func best(_ l: Language) -> Option? {
        if let f = l.file.first { return .file(f.index) }
        return l.tracks.first.map(Option.track)
    }

    /// The core's start pick inside the file: the first of the file's tracks in its language (by tag, else by base
    /// language), which wins over the core's own track (design/DECISION.md, Subtitle picker).
    static func startFile(_ auto: SubtitleTrack, in file: [FileTrack]) -> Int? {
        let tag = auto.language ?? auto.lang
        let tagged = file.compactMap { f in f.language.flatMap { $0.isEmpty || $0.hasPrefix("und") ? nil : (f, fileTag($0)) } }
        return (tagged.first { $0.1.lowercased() == tag.lowercased() } ?? tagged.first { SubtitleMenu.base($0.1) == SubtitleMenu.base(tag) })?.0.index
    }

    /// The language that is on: the one holding the core's track playing, or the file's track playing.
    static func chosen(_ languages: [Language], subtitle: SubtitleTrack?, fileIndex: Int?) -> String? {
        languages.first { l in
            subtitle.map(l.tracks.contains) == true || fileIndex.map { i in l.file.contains { $0.index == i } } == true
        }?.language
    }

    /// The circle-flags image for a tag: the tag itself ("pt-br"), else its language ("zh" for "zh-Hant"); nil
    /// gives the neutral round mark with two letters.
    static func flag(_ tag: String) -> String? {
        let lower = tag.lowercased()
        let base = SubtitleMenu.base(lower)
        if FlagSet.tags.contains(lower) { return "flag-\(lower)" }
        return FlagSet.tags.contains(base) ? "flag-\(base)" : nil
    }

    /// Whether a language with no flag shows the neutral language glyph (design/flag-overrides.json): the Chinese
    /// variants, contested and constructed ones; any other tag keeps its two letters.
    static func glyph(_ tag: String) -> Bool {
        let lower = tag.lowercased()
        return FlagSet.glyphs.contains(lower) || FlagSet.glyphs.contains(SubtitleMenu.base(lower))
    }

    /// The neutral mark's letters: "EN"; "?" for a track that names no language.
    static func letters(_ tag: String) -> String {
        tag.isEmpty ? "?" : String(SubtitleMenu.base(tag).prefix(2)).uppercased()
    }

    /// A language's row read out: "English, Auto, 4 options, selected".
    static func spoken(_ l: Language, auto: Bool, selected: Bool) -> String {
        ([l.name, l.own, auto ? "Auto" : nil, "\(l.count) \(l.count == 1 ? "option" : "options")", selected ? "selected" : nil]
            .compactMap { $0 }).joined(separator: ", ")
    }

    /// Audio: the file's default track reads Default, once there is more than one to choose from.
    static func marksDefault(index: Int, defaultIndex: Int?, count: Int) -> Bool { count > 1 && index == defaultIndex }

    /// An audio track's own-language name beside its English one ("German" with "Deutsch"); nil for a track that
    /// names no language, or when the two read the same.
    /// A regional tag ("de-DE") gives its base language's own name ("Deutsch") unless the English label says the
    /// region too ("German (Germany)" with "Deutsch (Deutschland)").
    static func audioOwnName(_ t: FileTrack) -> String? {
        guard let raw = tag(t.language) else { return nil }
        let full = fileTag(raw)
        let said = englishName(full) == t.name ? full : SubtitleMenu.base(full)
        return ownName(said).flatMap { $0 == t.name ? nil : $0 }
    }

    /// An audio track as the search reads it: its name, its own name ("deutsch" finds German) and its code.
    static func audioLanguage(_ t: FileTrack) -> Language {
        let tag = t.language.map(fileTag) ?? ""
        let codes = Set([tag.lowercased(), t.language?.lowercased()].compactMap { $0 }.filter { !$0.isEmpty })
        return Language(language: tag, name: t.name, own: audioOwnName(t), codes: codes.sorted(), file: [t], tracks: [])
    }

    /// A file track that names no language, by its place: an only audio track is "Original audio", several
    /// read "Audio 1", "Audio 2"; subtitles read "Subtitles" alone or numbered. Apple's own words for now: a
    /// shared key in stream-choice-words.json replaces them when the other apps add it in the same landing.
    static func placeName(index: Int, count: Int, audio: Bool) -> String {
        if audio { return count == 1 ? "Original audio" : "Audio \(index + 1)" }
        return count == 1 ? "Subtitles" : "Subtitles \(index + 1)"
    }

    /// The audio list has a flag column only when a track names its language; an untagged track then keeps the
    /// column's space blank, never a "?" mark.
    static func audioFlags(_ tracks: [FileTrack]) -> Bool { tracks.contains { tag($0.language) != nil } }

    /// Audio: a search only past five tracks.
    static func audioSearch(_ count: Int) -> Bool { count > 5 }
}

/// The picker's rows, keys and what a press does, apart from the view, so a Mac key walk is a test.
extension TrackPicker {
    enum Row: Hashable {
        case search, off, back, language(String), opener(String), option(Int), audio(Int)
    }

    /// A file track's option row is its index past this, so it never meets a core track's place in the list.
    static let fileRow = 10_000

    enum Key { case up, down, left, right, escape, enter }

    /// What a press or a key on a row does.
    enum Act: Equatable {
        case focus(Row)
        case open(String)
        case back
        case close
        case off
        case turnOn(Option)
        case audio(Int)
        case none
    }

    /// The focusable rows of the level showing, in order: the languages (search, Off, each language), a
    /// language's options (back, the file's tracks, the core's), or the audio tracks (search past five).
    static func order(audio: [FileTrack]?, open: Language?, shown: [Language]) -> [Row] {
        if let audio { return (audioSearch(audio.count) ? [Row.search] : []) + audio.map { .audio($0.index) } }
        if let open { return [.back] + open.file.map { .option(fileRow + $0.index) } + open.tracks.indices.map { .option($0) } }
        return [.search, .off] + shown.map { .language($0.language) }
    }

    /// A row's press (a click, a tap, Return): a language turns its best option on at once; its chevron opens it.
    static func press(_ row: Row, languages: [Language], open: Language?) -> Act {
        switch row {
        case .off: return .off
        case .language(let tag): return languages.first { $0.language == tag }.flatMap(best).map(Act.turnOn) ?? .none
        case .opener(let tag): return .open(tag)
        case .option(let id):
            guard let open else { return .none }
            if id >= fileRow { return .turnOn(.file(id - fileRow)) }
            return open.tracks.indices.contains(id) ? .turnOn(.track(open.tracks[id])) : .none
        case .audio(let index): return .audio(index)
        case .back: return .back
        case .search: return .none
        }
    }

    /// The Mac's keys over the rows (the timing stepper keeps its own while it has the focus on purpose): Up and
    /// Down move, Right opens the focused language, Left goes back, Esc goes back or closes, Return presses.
    static func key(_ key: Key, focus: Row?, order: [Row], languages: [Language], open: Language?) -> Act {
        // A language's chevron moves and opens as its row.
        let current = focus.map { row -> Row in if case .opener(let tag) = row { .language(tag) } else { row } }
        switch key {
        case .up, .down:
            guard !order.isEmpty else { return .none }
            let at = current.flatMap(order.firstIndex(of:)) ?? (key == .down ? -1 : order.count)
            return .focus(order[max(0, min(order.count - 1, at + (key == .down ? 1 : -1)))])
        case .right:
            if case .language(let tag)? = current { return .open(tag) }
            return .none
        case .left: return open == nil ? .none : .back
        case .escape: return open == nil ? .close : .back
        case .enter: return focus.map { press($0, languages: languages, open: open) } ?? .none
        }
    }

    /// The option a language's level lands on: the one playing when the language is the chosen one, else its best.
    static func landing(_ l: Language, subtitle: SubtitleTrack?, fileIndex: Int?) -> Row {
        if let i = fileIndex, l.file.contains(where: { $0.index == i }) { return .option(fileRow + i) }
        if let subtitle, let i = l.tracks.firstIndex(of: subtitle) { return .option(i) }
        switch best(l) {
        case .file(let i): return .option(fileRow + i)
        default: return .option(0)
        }
    }

    /// The Subtitles button's value: the language on, "English, Auto" for the core's start pick, its file track
    /// included (the language, not the file's own name for its track); else the track the viewer chose.
    static func buttonValue(core: String?, file: String?, auto: Bool, autoLanguage: String?) -> String? {
        guard auto, let language = core ?? autoLanguage else { return core ?? file }
        return "\(language), Auto"
    }

    /// The Mac and iPad panel's height: up to the area under the title bar (its row and padding, about 110 pt),
    /// at most 70 percent of the window, never under 320; `below` is the controls under it.
    static func panelHeight(window: CGFloat, below: CGFloat) -> CGFloat {
        max(320, min(window * 0.7, window - below - 110))
    }

    /// The track on is the core's start pick, not the viewer's own: the core's track itself, or the file's track
    /// that won in its language. Its language reads Auto.
    static func isAuto(subtitleURL: String?, autoURL: String?, textTrack: Int?, startFile: Int?, viewerChose: Bool) -> Bool {
        guard !viewerChose else { return false }
        if let textTrack { return textTrack == startFile }
        return subtitleURL != nil && subtitleURL == autoURL
    }
}
