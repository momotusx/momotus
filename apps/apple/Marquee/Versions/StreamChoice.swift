import Foundation

/// Smart stream choice (plans/m13-stream-choice.md 3.7, 3.9, 3.11, 4.4, 4.5).
/// The words are apps/shared/stream-choice-words.json, bundled as it is, so
/// the web, Android and Apple say the same; its "about" gives the rules each
/// app follows, and the functions below are those rules.
struct StreamChoiceWords: Decodable, Sendable {
    struct Line: Decodable, Sendable {
        let tone: String
        let text: String
        let textChosen: String?
        /// mayPause when only a few are sharing the file: the swarm, not the connection.
        let textSmallSwarm: String?
        let details: [String]
        /// buffer on mobile data, where it says no minutes.
        var detailsMobile: [String]? = nil
    }

    struct Choice<Value: Decodable & Sendable>: Decodable, Sendable {
        let label: String
        let value: Value?
    }

    struct UpDown: Decodable, Sendable {
        let up: String
        let down: String
    }

    /// A zero-playable state's line, its action's word and a dimmed row's status.
    struct Empty: Decodable, Sendable {
        let text: String
        var action: String?
        var playFromDevice: String?
        var row: String?
        var chooseAnother: String?
    }

    struct Page: Decodable, Sendable {
        let finding: String
        let stillLooking: String
        let showAll: String
        let showFewer: String
        let back: String
        let details: String
        let bestTv: String
        let bestPhone: String
        /// How a row starts, after its size: "ready", "direct", "download", "shared" ("shared by {people}").
        let source: [String: String]
        let smaller: String
        let sharper: String
        /// "uhd", "fhd", "hd", "other" to the section's heading.
        let sections: [String: String]
    }

    struct Player: Decodable, Sendable {
        let finding: String
        let findingChosen: String
        let chooseAnother: String
        let chooseAnotherSaid: String
        /// Every start subtitle the core tried could not be read (`startSubtitleFailed`); no source named.
        /// Optional: the key lands with the core's subtitle hand-over.
        let subtitlesFailed: String?
    }

    struct Learned: Decodable, Sendable {
        let size: UpDown
        let height: UpDown
        let ready: UpDown
        let hdr: UpDown
        let undo: String
        let settingsLines: [String: UpDown]
        let screens: [String: String]

        func note(_ choice: String) -> UpDown? {
            switch choice {
            case "size": size
            case "height": height
            case "ready": ready
            case "hdr": hdr
            default: nil
            }
        }
    }

    struct Question: Decodable, Sendable {
        struct Patch: Decodable, Sendable {
            let sizeLimitBytes: Int64?
            let minSeeders: Int
        }

        struct Answer: Decodable, Sendable {
            let label: String
            let text: String
            let patch: Patch
        }

        struct Skip: Decodable, Sendable {
            let label: String
            let means: String
        }

        let title: String
        let lead: String
        let fast: Answer
        let best: Answer
        let skip: Skip
    }

    struct Settings: Decodable, Sendable {
        struct Size: Decodable, Sendable {
            let label: String
            let custom: String
            let choices: [Choice<Int64>]
        }

        struct Seeders: Decodable, Sendable {
            let label: String
            let help: String
            let custom: String
            let choices: [Choice<Int>]
        }

        struct Options<Value: Decodable & Sendable>: Decodable, Sendable {
            let label: String
            let choices: [Choice<Value>]
        }

        struct Audio: Decodable, Sendable {
            let label: String
            let help: String
        }

        struct Learn: Decodable, Sendable {
            let label: String
            let choices: [Choice<Bool>]
            let forget: String
        }

        struct Device: Decodable, Sendable {
            let label: String
            let measureAgain: String
            let measuring: String
        }

        let title: String
        let lede: String
        let summary: String
        let size: Size
        let seeders: Seeders
        let resolution: Options<Int>
        let hdr: Options<Bool>
        let ready: Options<Bool>
        let audio: Audio
        let learn: Learn
        let device: Device
    }

    struct Conditions: Decodable, Sendable {
        let header: String
        let devices: [String: String]
        let screens: [String: String]
        let hdr: String
        let speed: String
        let free: String
        let networks: [String: String]
    }

    let plurals: [String: [String]]
    let lines: [String: Line]
    let speedSource: [String: String]
    /// By the core's state ("noSource", ... "notStarted").
    let empty: [String: Empty]
    let page: Page
    let player: Player
    let learned: Learned
    let question: Question
    let settings: Settings
    let conditions: Conditions

    /// The bundled file: the app's own bundle, or the test bundle that compiles this file.
    static let shared: StreamChoiceWords = {
        guard let url = Bundle(for: BundleToken.self).url(forResource: "stream-choice-words", withExtension: "json"),
              let data = try? Data(contentsOf: url),
              let words = try? JSONDecoder().decode(StreamChoiceWords.self, from: data)
        else { fatalError("stream-choice-words.json is bundled with the app (project.yml)") }
        return words
    }()
}

private final class BundleToken {}

/// One version's line in words: its tone, the default text, and the details
/// shown on focus (TV), hover (Mac) or tap (phone).
struct LineWords: Equatable {
    let tone: VersionWords.Tone
    let text: String
    let details: String
}

/// A choice the picks taught, as Settings lists it.
struct LearnedRow: Equatable {
    let screenClass: Int
    let choice: String
    let up: Bool
    let text: String
}

enum StreamChoice {
    static var words: StreamChoiceWords { .shared }

    /// Versions pins its pick while the core says it is not pinned: the core
    /// lets a failed pick go, and the page pins the new one (review r2 P3-4).
    static func shouldFreeze(open: Bool, frozen: Bool?) -> Bool {
        open && frozen != true
    }

    /// 1024-based, as addons print sizes: one decimal under 100 GB with a
    /// trailing .0 dropped, whole numbers from 100 GB, MB under 1 GB.
    static func bytes(_ n: Int64) -> String {
        let gb = Double(n) / 1_073_741_824
        if gb >= 100 { return "\(Int(gb.rounded())) GB" }
        if gb >= 1 {
            let tenths = (gb * 10).rounded() / 10
            return tenths == tenths.rounded() ? "\(Int(tenths)) GB" : String(format: "%.1f GB", tenths)
        }
        if n >= 1_048_576 { return "\(Int((Double(n) / 1_048_576).rounded())) MB" }
        return "\(max(1, Int((Double(n) / 1024).rounded()))) KB"
    }

    /// "1 person", "1,204 people".
    static func counted(_ what: String, _ n: Int) -> String {
        let forms = words.plurals[what] ?? ["{n}", "{n}"]
        return forms[n == 1 ? 0 : 1].replacingOccurrences(of: "{n}", with: grouped(n))
    }

    private static func grouped(_ n: Int) -> String {
        let formatter = NumberFormatter()
        formatter.locale = Locale(identifier: "en_US")
        formatter.numberStyle = .decimal
        return formatter.string(from: NSNumber(value: n)) ?? String(n)
    }

    /// Fills `{name}` fields; nil when one it names is unknown, so the part is left out.
    static func fill(_ template: String, _ fields: [String: String?]) -> String? {
        var known = true
        var out = ""
        var rest = Substring(template)
        while let open = rest.firstIndex(of: "{"), let close = rest[open...].firstIndex(of: "}") {
            out += rest[..<open]
            let name = String(rest[rest.index(after: open)..<close])
            if let value = fields[name] ?? nil { out += value } else { known = false }
            rest = rest[rest.index(after: close)...]
        }
        out += rest
        return known ? out : nil
    }

    private static func capital(_ s: String) -> String {
        s.prefix(1).uppercased() + s.dropFirst()
    }

    /// A line's tone, text and details. `limitChosen`: the user set the size
    /// limit, so a large file is "larger than the size you chose".
    static func lineWords(_ line: ReasonLine, limitChosen: Bool = false) -> LineWords? {
        guard let w = words.lines[line.kind] else { return nil }
        let details = lineParts(line).joined(separator: ", ")
        return LineWords(tone: tone(w.tone), text: lineText(w, line, limitChosen: limitChosen), details: capital(details))
    }

    /// A line's details as its parts, each filled, before they are joined: "shared by 89 people", "4 GB".
    static func lineParts(_ line: ReasonLine) -> [String] {
        guard let w = words.lines[line.kind] else { return [] }
        var fields: [String: String?] = [
            "quality": line.quality,
            // Never the service's name: "your account" when the line names one, as for addons.
            "account": line.service.map { _ in "your account" },
            "year": line.year.map(String.init),
            // The people sharing it, in plain words: "shared by 21 people".
            "people": line.seeders.map { counted("people", $0) },
            "size": line.sizeBytes.map(bytes),
            "limit": line.limitBytes.map(bytes),
            "need": line.needMbps.map { String(Int($0.rounded())) },
            "have": line.haveMbps.map { String(Int($0.rounded())) },
            "minutes": line.aheadMin.map(String.init),
            "budget": line.budgetBytes.map(bytes),
        ]
        // Only a speed a playback measured is said: an unmeasured one leaves its part out (plan 2g).
        if let samples = line.samples, samples > 0 {
            fields["speedSource"] = words.speedSource["measured"]?.replacingOccurrences(of: "{playbacks}", with: counted("playbacks", samples))
        }
        // A window on mobile data keeps only a short stretch ahead: no minutes.
        let parts = line.kind == "buffer" && line.aheadMin == nil ? w.detailsMobile ?? w.details : w.details
        return parts.compactMap { fill($0, fields) }
    }

    /// The line's words: the size limit's own when the user set it, the small
    /// swarm's when a version may pause because few are sharing it.
    static func lineText(_ w: StreamChoiceWords.Line, _ line: ReasonLine, limitChosen: Bool) -> String {
        if limitChosen, let chosen = w.textChosen { return chosen }
        if line.kind == "mayPause", line.smallSwarm == true, let swarm = w.textSmallSwarm { return swarm }
        return w.text
    }

    static func tone(_ name: String) -> VersionWords.Tone {
        switch name {
        case "ok": .ok
        case "warn": .warn
        case "bad": .bad
        default: .idle
        }
    }

    /// The speed that decides here: HTTP with a streaming service account or
    /// without the engine, else the torrent engine's.
    static func speed(http: Speed, p2p: Speed, engine: Bool, account: Bool) -> Speed {
        account || !engine ? http : p2p
    }

    /// "4K", "1080p", ...
    static func screenWord(_ screenClass: Int) -> String {
        words.conditions.screens[String(screenClass)] ?? "\(screenClass)p"
    }

    /// "4K HDR, about 42 Mbit/s on Wi-Fi, 61 GB free"; an unmeasured speed names the network alone.
    static func conditionsLine(screenClass: Int, hdr: Bool, network: String, speed: Speed, freeBytes: Int64?) -> String {
        let c = words.conditions
        let screen = screenWord(screenClass)
        let net = c.networks[network] ?? c.networks["other"] ?? network
        let speedPart = speed.samples > 0 ? fill(c.speed, ["mbps": String(Int(speed.mbps.rounded())), "network": net]) : net
        let free = freeBytes.flatMap { fill(c.free, ["free": bytes($0)]) }
        return [hdr ? fill(c.hdr, ["screen": screen]) : screen, speedPart, free].compactMap { $0 }.joined(separator: ", ")
    }

    /// "tv", "desktop" or "phone": how the words name this device.
    static var device: String {
        #if os(tvOS)
        "tv"
        #elseif os(macOS)
        "desktop"
        #else
        // ponytail: an iPad reads as "this phone" too; the words have no tablet yet.
        "phone"
        #endif
    }

    /// The one-time note (3.9) with the preferences now in effect; nil when
    /// the change names no number (no limit, the screen's resolution).
    static func learnedNote(_ note: LearnedNote, device: String) -> String? {
        guard let w = words.learned.note(note.choice) else { return nil }
        return fill(note.up ? w.up : w.down, [
            "device": words.conditions.devices[device] ?? device,
            "limit": note.prefs.sizeLimitBytes.map(bytes),
            "height": note.prefs.maxHeight.map(screenWord),
        ])
    }

    /// Largest screen first, as Settings lists them.
    private static func classes(_ learned: LearnedByClass) -> [(Int, Learned)] {
        [(2160, learned.uhd), (1080, learned.fhd), (720, learned.hd), (480, learned.sd)]
    }

    private static let choices = ["size", "height", "ready", "hdr"]

    private static func counter(_ l: Learned, _ choice: String) -> Int {
        switch choice {
        case "size": l.size
        case "height": l.height
        case "ready": l.ready
        default: l.hdr
        }
    }

    /// What the picks taught, for Settings: each choice at 3 or more either
    /// way, by screen class, unless undone or forgotten (a class stamped with
    /// an older epoch reads as zero).
    static func learnedRows(_ prefs: Prefs) -> [LearnedRow] {
        classes(prefs.learned).flatMap { cls, l -> [LearnedRow] in
            guard l.epoch >= prefs.learnedEpoch else { return [] }
            return choices.compactMap { choice in
                let n = counter(l, choice)
                guard abs(n) >= 3, !l.muted.contains(choice), let w = words.learned.settingsLines[choice] else { return nil }
                let screens = words.learned.screens[String(cls)] ?? "\(cls)p screens"
                return LearnedRow(
                    screenClass: cls, choice: choice, up: n > 0,
                    text: (n > 0 ? w.up : w.down).replacingOccurrences(of: "{screens}", with: screens))
            }
        }
    }

    /// Anything learned that Forget would clear.
    static func anyLearned(_ prefs: Prefs) -> Bool {
        classes(prefs.learned).contains { _, l in
            l.epoch >= prefs.learnedEpoch && choices.contains { counter(l, $0) != 0 }
        }
    }

    /// "Up to 5 GB, shared by at least 50 people": the Settings row's line.
    static func summary(_ p: StreamPrefs) -> String {
        let s = words.settings
        let size = s.size.choices.first { $0.value == p.sizeLimitBytes }?.label
            ?? s.size.custom.replacingOccurrences(of: "{size}", with: bytes(p.sizeLimitBytes ?? 0))
        let people = s.seeders.choices.first { $0.value == p.minSeeders }?.label
            ?? s.seeders.custom.replacingOccurrences(of: "{n}", with: grouped(p.minSeeders))
        return s.summary.replacingOccurrences(of: "{size}", with: size).replacingOccurrences(of: "{people}", with: people)
    }

    /// The display's lines to the core's screen class (3.3): an iPhone (1179
    /// lines) and a 1440 phone are 1080.
    static func screenClass(lines: Int) -> Int {
        lines >= 2000 ? 2160 : lines >= 1000 ? 1080 : lines >= 700 ? 720 : 480
    }

    /// The Resolution choice that reads as picked: the screen's for no cap,
    /// and for a 4K cap, which is the screen's on every screen the apps run on.
    static func resolutionPicked(_ value: Int?, maxHeight: Int?) -> Bool {
        value == nil ? maxHeight == nil || (maxHeight ?? 0) >= 2160 : maxHeight == value
    }

    /// "Measure again" has something to clear: either speed was measured.
    static func measured(_ c: StreamConditions) -> Bool {
        c.http.samples > 0 || c.p2p.samples > 0
    }

    /// The `network` the core measures speed for, from the path's interface.
    static func network(wifi: Bool, ethernet: Bool, cellular: Bool) -> String {
        if ethernet { return "ethernet" }
        if wifi { return "wifi" }
        if cellular { return "cellular" }
        return "other"
    }

    /// Words that follow a size after a comma start in lower case: "12 GB, may start slowly", as on the
    /// web and Android; with no size before them they keep their capital.
    static func lowerFirst(_ words: String) -> String {
        words.prefix(1).lowercased() + words.dropFirst()
    }

    /// The HDR formats the core ranks for: none on a screen without HDR;
    /// Dolby Vision only where the player offers it.
    static func hdrNames(eligible: Bool, hdr10: Bool, hlg: Bool, dolbyVision: Bool) -> [String] {
        guard eligible else { return [] }
        return [hdr10 ? "hdr10" : nil, hlg ? "hlg" : nil, dolbyVision ? "dolby_vision" : nil].compactMap { $0 }
    }

    /// What the title and the episode say, for the ranking's bitrate, year and
    /// pack checks (4.3); the core reads the meta for whatever is left out.
    static func versionsFacts(runtime: String?, releaseInfo: String?, released: String?, episodesInSeason: Int?) -> [String: any Sendable] {
        var facts: [String: any Sendable] = [:]
        if let runtime { facts["runtime"] = runtime }
        if let releaseInfo { facts["releaseInfo"] = releaseInfo }
        if let released { facts["released"] = released }
        if let episodesInSeason, episodesInSeason > 0 { facts["episodesInSeason"] = episodesInSeason }
        return facts
    }

    /// The question's answer as a `setPrefs` patch of the stream preferences.
    static func patch(_ answer: StreamChoiceWords.Question.Answer) -> [String: any Sendable] {
        let size: any Sendable = answer.patch.sizeLimitBytes.map { $0 as any Sendable } ?? NSNull()
        return ["streams": ["sizeLimitBytes": size, "minSeeders": answer.patch.minSeeders] as [String: any Sendable]]
    }
}
