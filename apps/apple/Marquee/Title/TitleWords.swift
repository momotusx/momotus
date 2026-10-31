import Foundation

/// "21:00" or "1:02:03".
func clock(ms: Int64) -> String {
    let total = Int(ms / 1000)
    let (h, m, s) = (total / 3600, total / 60 % 60, total % 60)
    return h > 0 ? String(format: "%d:%02d:%02d", h, m, s) : String(format: "%d:%02d", m, s)
}

/// Minutes from a runtime like "57 min", "2h 9min" or "1h".
func minutes(_ runtime: String?) -> Int? {
    guard let runtime else { return nil }
    let hours = runtime.firstMatch(of: /(\d+)\s*h/).flatMap { Int($0.1) } ?? 0
    let mins = runtime.firstMatch(of: /(\d+)\s*m/).flatMap { Int($0.1) } ?? 0
    let total = hours * 60 + mins
    return total > 0 ? total : nil
}

/// A core date ("2013-08-11") as a date at the start of that day, UTC.
func releaseDate(_ value: String?) -> Date? {
    guard let value else { return nil }
    return try? Date(value, strategy: .iso8601.year().month().day())
}

/// A calendar day's heading: "Today", "Tomorrow", "Yesterday", else
/// "Wednesday, October 21". Both are core days at UTC midnight, so they are
/// counted and named in UTC, never in the viewer's zone (`viewer`, there so tests can
/// be a viewer anywhere): read there, a day west of UTC was the day before,
/// and the night the clocks change counted as no day at all.
func dayHeading(_ day: String, today: String, viewer: TimeZone = .current, locale: Locale = .current) -> String {
    guard let date = releaseDate(day), let now = releaseDate(today) else { return day }
    let utc = TimeZone(identifier: "UTC") ?? TimeZone(secondsFromGMT: 0) ?? viewer
    var calendar = Calendar(identifier: .gregorian)
    calendar.timeZone = utc
    switch calendar.dateComponents([.day], from: now, to: date).day {
    case 0: return "Today"
    case 1: return "Tomorrow"
    case -1: return "Yesterday"
    default:
        var style = Date.FormatStyle.dateTime.weekday(.wide).day().month(.wide)
        style.timeZone = utc
        style.locale = locale
        return date.formatted(style)
    }
}

/// A long text in blocks of whole sentences of about `size` characters,
/// for a TV reader whose blocks take focus one after another (a remote
/// scrolls a page only by moving focus). A sentence longer than `size`
/// is a block of its own; nothing is cut or dropped.
func readingBlocks(_ text: String, size: Int = 240) -> [String] {
    var sentences: [String] = []
    text.enumerateSubstrings(in: text.startIndex..., options: .bySentences) { sentence, _, _, _ in
        if let sentence = sentence?.trimmingCharacters(in: .whitespacesAndNewlines), !sentence.isEmpty {
            sentences.append(sentence)
        }
    }
    // A sentence longer than a block goes in pieces of whole words, so no
    // block outgrows the screen.
    let pieces = sentences.flatMap { sentence in
        sentence.count <= size ? [sentence] : join(sentence.split(separator: " ").map(String.init), size: size)
    }
    return join(pieces, size: size)
}

/// Parts joined with spaces into runs of at most `size` characters; a part
/// longer than that stands alone.
private func join(_ parts: [String], size: Int) -> [String] {
    var runs: [String] = []
    for part in parts {
        if let last = runs.last, last.count + 1 + part.count <= size {
            runs[runs.count - 1] = last + " " + part
        } else {
            runs.append(part)
        }
    }
    return runs
}

/// Up Next's countdown: seconds up to 90, then whole minutes rounded up
/// ("Starts in 4 min", never "Starts in 214 s").
func startsIn(seconds: Int) -> String {
    seconds > 90 ? "Starts in \((seconds + 59) / 60) min" : "Starts in \(seconds) s"
}

/// What VoiceOver says for "S2:E3", which it would read as letters and a colon.
func spokenEpisode(season: Int, episode: Int) -> String {
    "Season \(season), Episode \(episode)"
}

/// Every "S2:E3" in a line the core wrote, said as words (a Continue Watching
/// caption, a Calendar row, "Resume S2:E3").
func spokenCodes(_ text: String) -> String {
    text.replacing(/\bS(\d+):E(\d+)/) { match in
        spokenEpisode(season: Int(match.1) ?? 0, episode: Int(match.2) ?? 0)
    }
}

/// What Resume says after its label: "36 min left" from what the file has
/// left (Continue Watching's figure), else from the runtime when it is known,
/// else the position ("from 21:00").
func resumeText(resumeMs: Int64, leftMs: Int64? = nil, runtime: String?) -> String {
    if let leftMs, leftMs > 0 { return minutesLeft(ms: leftMs) }
    guard let total = minutes(runtime) else { return "from \(clock(ms: resumeMs))" }
    let left = Int((Double(total) - Double(resumeMs) / 60_000).rounded())
    return "\(max(1, left)) min left"
}

/// Whole minutes rounded up, as the core words Continue Watching.
func minutesLeft(ms: Int64) -> String {
    "\(max(1, (ms + 59_999) / 60_000)) min left"
}

/// An episode whose release day is still to come: it shows "Airs Oct 21" and
/// cannot be played yet.
func airsLater(_ released: String?, now: Date = .now) -> Bool {
    releaseDate(released).map { $0 > now } ?? false
}

/// The title page's line in place of Play while the next episode is not out:
/// "Next episode airs Oct 21", the day read in UTC as the episode list reads it.
func nextAirsWords(_ day: String, locale: Locale = .current) -> String? {
    guard let date = releaseDate(day) else { return nil }
    let style = Date.FormatStyle(locale: locale, timeZone: TimeZone(identifier: "UTC") ?? .current).month(.abbreviated).day()
    return "Next episode airs \(date.formatted(style))"
}

/// The title page's line when the core names neither an episode to play nor
/// the day the next one airs: why there is no Play.
func noPlayWords(hasEpisodes: Bool) -> String {
    hasEpisodes ? "The next episode has no air date yet" : "This series lists no episodes to play"
}

/// Under Continue Watching when Remove did not go through.
func removeFailedWords(name: String, reason: String) -> String {
    "\(name) could not be removed from Continue Watching: \(reason)"
}

/// An episode's caption, the same words on every form: "Airs Oct 21" for one still to come, else
/// "36 min left · Feb 18, 2022" (the core's time left, or the progress against `minutes`) or
/// "57 min · Feb 18, 2022". The day is kept in UTC so it never slips (plans/design-v3-phase3-apple.md).
func episodeCaption(_ episode: Episode, minutes: Int?, now: Date = .now, locale: Locale = .current) -> String {
    let day = Date.FormatStyle(locale: locale, timeZone: TimeZone(identifier: "UTC") ?? .current)
    let released = releaseDate(episode.released)
    if let released, released > now {
        return whole("Airs \(released.formatted(day.month(.abbreviated).day()))")
    }
    var parts: [String] = []
    if let left = episode.leftMs, left > 0, !episode.watched {
        parts.append(minutesLeft(ms: left))
    } else if let minutes {
        if let progress = episode.progress, !episode.watched {
            parts.append("\(max(1, Int((Double(minutes) * (1 - progress)).rounded()))) min left")
        } else {
            parts.append("\(minutes) min")
        }
    }
    if let released { parts.append(released.formatted(day.month(.abbreviated).day().year())) }
    return parts.map(whole).joined(separator: "\u{A0}\u{B7} ")
}

/// A caption part a line never breaks inside ("52 min", "Oct 7, 2026"): on the iPhone's narrow episode list a
/// line breaks only after a dot (the iphone-a walk, 2026-10-23).
private func whole(_ part: String) -> String { part.replacing(/\s/, with: "\u{A0}") }

/// The v3 episode card's caption: "Watched" for a watched episode, as the frames draw it; the shared
/// words otherwise. Card only, so the TV page's list keeps its words.
func cardCaption(_ episode: Episode, minutes: Int?, now: Date = .now, locale: Locale = .current) -> String {
    episode.watched ? "Watched" : episodeCaption(episode, minutes: minutes, now: now, locale: locale)
}

/// The panel's episode search: the loaded season by number ("3") or by name, without case. The core
/// has no episode search (plans/design-v3-phase3-apple.md G3).
func filterEpisodes(_ episodes: [Episode], query: String) -> [Episode] {
    let text = query.trimmingCharacters(in: .whitespaces)
    guard !text.isEmpty else { return episodes }
    return episodes.filter { "\($0.episode)" == text || $0.title.range(of: text, options: .caseInsensitive) != nil }
}

/// The episode search found none of the loaded episodes: a line that says so with the query, never a blank list.
/// Nil while it finds some, with no query, and before the season has episodes.
func noEpisodeMatch(_ episodes: [Episode], query: String) -> String? {
    let text = query.trimmingCharacters(in: .whitespaces)
    guard !text.isEmpty, !episodes.isEmpty, filterEpisodes(episodes, query: text).isEmpty else { return nil }
    return "No episodes in this season match \"\(text)\"."
}

/// The Information band's rows: Genres, Runtime, and for a series Seasons and the selected season's
/// episodes, "8 episodes in Season 2" (the core sends one season's episodes at a time).
func informationRows(
    genres: [String], directors: [String] = [], runtime: String?, seasons: [Int], selectedSeason: Int?, episodes: Int
) -> [(key: String, value: String)] {
    var rows: [(key: String, value: String)] = []
    if !genres.isEmpty { rows.append(("Genres", genres.prefix(3).joined(separator: ", "))) }
    // Every director, also those the hero's one chip line dropped (review r2 P3-2).
    if !directors.isEmpty { rows.append((directors.count == 1 ? "Director" : "Directors", directors.joined(separator: ", "))) }
    if let runtime { rows.append(("Runtime", runtime)) }
    let regular = seasons.filter { $0 > 0 }.count
    if regular > 0 { rows.append(("Seasons", "\(regular)")) }
    if let season = selectedSeason, episodes > 0 {
        let name = season == 0 ? "Specials" : "Season \(season)"
        rows.append(("Episodes", "\(episodes) \(episodes == 1 ? "episode" : "episodes") in \(name)"))
    }
    return rows
}

/// Start Over in the Mac's Playback menu is live only for the title in front: it has a resume
/// point, its place is shown and no player covers it (plans/design-v3-phase3-apple.md 2.1).
func startOverAvailable(resume: Int64?, shown: Bool, covered: Bool) -> Bool {
    (resume ?? 0) > 0 && shown && !covered
}

/// A title-level mark the page can toggle.
enum TitleMark: Equatable {
    case library(on: Bool)
    case watched(on: Bool)
}

/// What opened the episodes panel: keyboard focus goes back to it on close.
enum PanelOpener: Hashable {
    case season
    case card(String)
    /// A card's "...": an unaired card's play button is off, so focus goes back to its menu.
    case more(String)

    static func card(_ videoId: String, unaired: Bool) -> PanelOpener {
        unaired ? .more(videoId) : .card(videoId)
    }
}

/// The title's marks in the order they show: Library where the profile can set it (not a kids profile,
/// whose Library is the owner's list), Watched only for a movie. The core keeps no watched mark for a whole
/// series (C14, C41), so a series shows Library alone.
func titleMarks(kind: String, inLibrary: Bool, watched: Bool, librarySettable: Bool = true) -> [TitleMark] {
    (librarySettable ? [.library(on: inLibrary)] : []) + (kind == "movie" ? [.watched(on: watched)] : [])
}

/// The title's reserved warning-only line under Play: the core's line for why nothing plays, else
/// nothing (no badges, no size, never "Finding versions").
func titleWarning(_ versions: VersionsSnapshot?) -> String? {
    versions?.empty.flatMap { VersionWords.emptyLine($0.kind) }
}

/// What Play does as the versions stand (plans/play-every-version.md, the shared table's title row).
/// Play is always the title's primary, never Settings or an account step: still searching it opens the
/// player at once, which starts the pick once it settles; with no source in reach a source kept on this
/// device plays; the states that list rows or an action open Versions, which holds them; the states
/// with Back alone leave Play in place but inert, since a page holding only Back is a dead end.
enum TitlePlay: Equatable {
    /// The player, with this key, or with none: it starts the pick once it settles.
    case player(key: String?)
    case versions
    case inert

    /// The states whose only action is Back.
    static let backOnly: Set<String> = ["kidsNothing", "noResults", "needsAccount"]

    static func of(_ versions: VersionsSnapshot?) -> TitlePlay {
        guard let empty = versions?.empty else { return .player(key: versions?.recommended?.key) }
        if empty.kind == "unreachable" { return empty.key.map { .player(key: $0) } ?? .versions }
        return backOnly.contains(empty.kind) ? .inert : .versions
    }
}

/// What the title page shows while it has no details: Loading, or the reason it failed
/// (the header's own failure, else the failed open, which leaves no header at all).
enum TitleFallback: Equatable {
    case loading
    case failed(String)
}

func titleFallback(header: TitleState?, failure: String?) -> TitleFallback {
    if case .failed(let reason) = header { return .failed(reason) }
    if header == nil, let failure { return .failed(failure) }
    return .loading
}

/// The header while Try again reopens the page: a header that failed (no details) goes, so the page
/// shows Loading and then the new answer, even when it is the same words; one with details stays.
func headerAfterRetry(_ header: TitleHeader?) -> TitleHeader? {
    header?.details == nil ? nil : header
}

/// Where the title page's failure line starts: below the screen's top safe area (the status bar and the
/// navigation bar with its floating Back, which the full-bleed page runs under) and a gap, and never
/// higher than the usual top gap (a window with no bar above it).
func failureTop(safeTop: CGFloat) -> CGFloat {
    max(safeTop + Theme.Space.m, Theme.Space.xl)
}
