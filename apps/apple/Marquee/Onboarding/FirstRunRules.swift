import Foundation
import SwiftUI

/// Columns side by side whose widths keep the given shares of the room left after the gaps (round2.html setup's
/// 1.15 / 1 / 1 on wide forms), each as tall as it needs, aligned at the top.
struct ShareColumns: Layout {
    let shares: [CGFloat]
    let spacing: CGFloat

    /// Setup's three columns, 36 apart.
    static let setup = ShareColumns(shares: [1.15, 1, 1], spacing: 36)

    func widths(_ total: CGFloat, count: Int) -> [CGFloat] {
        let used = Array(shares.prefix(count)) + Array(repeating: 1, count: max(count - shares.count, 0))
        let room = max(total - spacing * CGFloat(max(count - 1, 0)), 0)
        let sum = used.reduce(0, +)
        return used.map { sum > 0 ? room * $0 / sum : 0 }
    }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width ?? 1060
        let heights = zip(subviews, widths(width, count: subviews.count)).map { $0.sizeThatFits(ProposedViewSize(width: $1, height: nil)).height }
        return CGSize(width: width, height: heights.max() ?? 0)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var x = bounds.minX
        for (subview, width) in zip(subviews, widths(bounds.width, count: subviews.count)) {
            subview.place(at: CGPoint(x: x, y: bounds.minY), proposal: ProposedViewSize(width: width, height: nil))
            x += width + spacing
        }
    }
}

/// First run's words (apps/shared/first-run-words.json), bundled as they are, shared with the web and Android:
/// a key the file adds or drops fails the reader test (FirstScreensTests).
struct FirstRunWords: Decodable, Sendable {
    let welcomeTitle: String
    let welcomeLead: String
    let getStarted: String
    let alreadyUse: String
    let pauseMotion: String
    let playMotion: String
    let loginLine: String
    let loginReturning: String
    let useAnotherDevice: String
    let tvLoginTitle: String
    let qrTitle: String
    let qrHow: String
    let qrWaiting: String
    let tvSteps: [String]
    let tvNoQr: String
    let tryAgain: String
    let gettingProfiles: String
    let whoTitle: String
    let whoProvider: String
    let whoDevice: String
    let whoSignedIn: String
    let whoKids: String
    let addProfile: String
    let profileTitle: String
    let profileLineAccount: String
    let profileLineLocal: String
    let nameLabel: String
    let namePlaceholder: String
    let colourLabel: String
    let `continue`: String
    let setupTitle: String
    let setupLine: String
    let skip: String
    let bringTitle: String
    let bringLibrary: String
    let bringLibraryLine: String
    let startFresh: String
    let startFreshLine: String
    let subtitlesTitle: String
    let noSubtitles: String
    let moreLanguages: String
    let sourcesOn: String
    let review: String
    let startWatching: String
    let setupPrefsFailed: String
    let setupSourcesFailed: String
    let skipImport: String

    /// Every key the file holds but its "about".
    static let keys: Set<String> = [
        "welcomeTitle", "welcomeLead", "getStarted", "alreadyUse", "pauseMotion", "playMotion", "loginLine", "loginReturning",
        "useAnotherDevice", "tvLoginTitle", "qrTitle", "qrHow", "qrWaiting", "tvSteps", "tvNoQr", "tryAgain", "gettingProfiles",
        "whoTitle", "whoProvider", "whoDevice", "whoSignedIn", "whoKids", "addProfile", "profileTitle", "profileLineAccount",
        "profileLineLocal", "nameLabel", "namePlaceholder", "colourLabel", "continue", "setupTitle", "setupLine", "skip",
        "bringTitle", "bringLibrary", "bringLibraryLine", "startFresh", "startFreshLine", "subtitlesTitle", "noSubtitles",
        "moreLanguages", "sourcesOn", "review", "startWatching", "setupPrefsFailed", "setupSourcesFailed", "skipImport",
    ]

    /// The bundled file: the app's own bundle, or the test bundle that compiles this file.
    static let shared: FirstRunWords = {
        guard let url = Bundle(for: FirstRunWordsToken.self).url(forResource: "first-run-words", withExtension: "json"),
              let data = try? Data(contentsOf: url),
              let words = try? JSONDecoder().decode(FirstRunWords.self, from: data)
        else { fatalError("first-run-words.json is bundled with the app (project.yml)") }
        return words
    }()
}

private final class FirstRunWordsToken {}

/// What first run shows (plans/first-screens.md 2, 4.3), held equal to the web's and Android's by
/// apps/shared/fixtures/first-run-vectors.json (FirstRunRulesTests).
enum FirstRunStep: String, Equatable, Sendable {
    case welcome, login, profile, who, setup, home, none
    /// The Stremio import's sheet, after Start watching with Bring my Stremio library.
    case importing = "import"
}

enum FirstRunRules {
    /// What just happened (the vectors' `after`).
    enum After: String, Sendable {
        case start, getStarted, alreadyUse, notNow, created, joined, profileDone
    }

    /// The inputs, all from the core and the app's account state (the vectors' entry rows).
    struct Entry: Equatable, Sendable {
        var fresh: Bool
        var kidsSession = false
        var server: Bool
        var inAccount: Bool
        var profileStepDone: Bool
        var profiles = 1
        var pin = false
        var locked = false
        var after: After
    }

    /// The step first run shows next.
    static func next(_ e: Entry) -> FirstRunStep {
        if e.kidsSession { return .none }
        // A locked session meets the start picker before anything of first run.
        if e.locked { return .who }
        switch e.after {
        case .joined:
            // D2: one profile without a PIN opens Home; a joined account's profile is set up already.
            return e.profiles > 1 || e.pin ? .who : .home
        case .getStarted, .alreadyUse:
            return e.server ? .login : .profile
        case .notNow, .created:
            return .profile
        case .profileDone:
            return .setup
        case .start:
            guard e.fresh else { return .none }
            if e.profileStepDone { return .setup }
            return e.inAccount ? .profile : .welcome
        }
    }

    /// Start watching: the Stremio import's sheet when it was asked for, else Home.
    static func afterSetup(bring: Bool) -> FirstRunStep { bring ? .importing : .home }

    /// Back, Escape and the grabber (5): nil where they leave the app (or do nothing). `made`: the account was made
    /// in this first run, so the profile form is where a restart resumes.
    static func back(from step: FirstRunStep, server: Bool, made: Bool) -> FirstRunStep? {
        switch step {
        case .login: .welcome
        case .profile: made ? nil : server ? .login : .welcome
        case .setup: .profile
        case .importing: .setup
        case .welcome, .who, .home, .none: nil
        }
    }

    // MARK: Setup's subtitle chips (D11, vectors part chips)

    static let none = "none"
    static let more = "more"

    /// "Subtitles start in": the profile's chosen language first, else the device's, then the device's language and
    /// English when not already shown, then No subtitles, then More (always last). The first is chosen. `device` is
    /// the device language as the core lists it, nil when unknown or unnamed (English is chosen then).
    static func chips(device: String?, chosen: [String]) -> (chips: [String], selected: String) {
        var seen = Set<String>()
        let languages = [chosen.first, device, "en"].compactMap { $0 }.filter { !$0.isEmpty && seen.insert($0).inserted }
        return (languages + [none, more], languages[0])
    }

    /// What Start watching writes: the shown chip always (none for No subtitles), the stream patch only for Best quality.
    static func setupPatch(chip: String, best: [String: any Sendable]?) -> [String: any Sendable] {
        var patch: [String: any Sendable] = ["subtitleLanguages": chip == none ? [String]() : [chip]]
        if let best { patch.merge(best) { _, new in new } }
        return patch
    }

    // MARK: The profile form (3.4)

    /// The five colours the form offers, from the avatar palette.
    static let swatches = ["ember", "sky", "teal", "moss", "plum"]

    /// What Continue sends to `finishProfileStep`: a name only when one was typed and a colour only when a swatch
    /// was pressed, else empty (the core keeps the stored value). The swatch the form opens on is never sent.
    static func profilePayload(typed: String, pressed: String?, stored: String) -> [String: String] {
        let name = String(typed.trimmingCharacters(in: .whitespacesAndNewlines).prefix(30))
        let glyph = ProfileRules.parseAvatar(stored).glyph
        return ["name": name, "avatar": pressed.map { ProfileRules.formatAvatar(glyph: glyph, color: $0) } ?? ""]
    }

    /// Add profile on first run's Who's watching? (parity with the web and Android): what `createProfile` gets, the typed
    /// name (trimmed, 30 at most) and the letter on the swatch shown; nil for an empty name, which creates nothing.
    static func newProfile(typed: String, swatch: String) -> [String: String]? {
        let name = String(typed.trimmingCharacters(in: .whitespacesAndNewlines).prefix(30))
        guard !name.isEmpty else { return nil }
        return ["name": name, "avatar": ProfileRules.formatAvatar(glyph: "letter", color: swatch)]
    }

    /// Where focus starts on the profile form: Continue on TV, nothing on the phone, iPad and Mac (no keyboard over
    /// Continue); never the Name field.
    enum ProfileFocus: Equatable { case none, continueButton }

    static func profileFocus(tv: Bool) -> ProfileFocus { tv ? .continueButton : .none }

    /// The line under "Make your first profile".
    static func profileLine(made: Bool, words: FirstRunWords = .shared) -> String {
        made ? words.profileLineAccount : words.profileLineLocal
    }

    /// "Who's watching?"'s line: the provider signed in with, else the device that approved a QR join, else "Signed in."
    static func whoLine(provider: String?, device: String?, words: FirstRunWords = .shared) -> String {
        if let provider { return AccountRules.fill(words.whoProvider, ["provider": AccountRules.providerName(provider)]) }
        if let device, !device.isEmpty { return AccountRules.fill(words.whoDevice, ["device": device]) }
        return words.whoSignedIn
    }

    /// A tile's second line on wide forms (round2.html `.pk small`). The design draws the profile's last title, but the
    /// profiles snapshot carries none (ProfileInfo has no such field, and another profile's library cannot be read
    /// without switching to it), so the line says what the picker knows: "Only titles for children" for a kids
    /// profile, else the Manage row's own detail ("Main profile", "PIN set"); nothing for a plain profile.
    static func whoDetail(_ profile: ProfileInfo, words: FirstRunWords = .shared) -> String? {
        if profile.kids { return words.whoKids }
        let detail = ProfileRules.detail(profile)
        return detail == "No PIN" ? nil : detail
    }

    /// How long the sheet says "Getting your profiles" after a join before it decides anyway.
    static let pullWait: Duration = .seconds(15)

    // MARK: Sheets (round2.html combo A)

    enum Form: Equatable { case phone, wide, tv }

    /// iPad at regular width and the Mac take the centered sheet; the phone (and iPad compact) the bottom sheet.
    static func form(wide: Bool, tv: Bool) -> Form { tv ? .tv : wide ? .wide : .phone }

    enum Sheet: Equatable { case login, who, profile, setup, importing }

    /// The centered sheet's width; nil is the phone's bottom sheet, the screen's width.
    static func sheetWidth(_ sheet: Sheet, form: Form) -> CGFloat? {
        switch (form, sheet) {
        case (.phone, _): nil
        case (.tv, .login): 980
        case (_, .login): 860
        case (_, .setup): 1140
        case (_, .who), (_, .profile), (_, .importing): 760
        }
    }

    /// The phone's grabber, only where a drag does something: the Login sheet's, back to Welcome (MOB-11: it is a
    /// button too).
    static func grabber(_ sheet: Sheet, form: Form) -> Bool { form == .phone && sheet == .login }

    /// Welcome's poster columns: 4 on the phone, 9 wide and on TV.
    static func columns(_ form: Form) -> Int { form == .phone ? 4 : 9 }

    /// Every first-run control's height (round2.html): each at least 44 pt, Apple's floor (TouchTargetTests' rule).
    enum Size {
        static let primary: CGFloat = 54
        static let provider: CGFloat = 52
        static let link: CGFloat = 48
        static let chip: CGFloat = 48
        static let swatch: CGFloat = 48
        static let toggle: CGFloat = 48
        static let option: CGFloat = 56
        static let segment: CGFloat = 48
        static let grabber: CGFloat = 44
        static let all = [primary, provider, link, chip, swatch, toggle, option, segment, grabber]
    }
}
