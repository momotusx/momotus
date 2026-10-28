import Foundation
import Testing

// The addon install sheet (plans/addon-install-sheet.md): its words and Adds
// line against apps/shared/install-sheet.json, and the rule that nothing
// installs without it, for a Find addons card and for a pasted link.

private typealias R = InstallSheetRules

private struct Shared: Decodable {
    struct Vector: Decodable {
        let provides: [String]
        let types: [String]
        let line: String
    }

    let words: [String: String]
    let resources: [String: String]
    let kinds: [String: String]
    let vectors: [Vector]
}

private func shared() throws -> Shared {
    let url = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../../shared/install-sheet.json")
    return try JSONDecoder().decode(Shared.self, from: Data(contentsOf: url))
}

/// What the fakes were asked, in order.
@MainActor private final class Calls {
    var previews: [(url: String, acknowledged: Bool)] = []
    var installs: [(url: String, acknowledged: Bool)] = []
    var runs = 0
}

private func addon(_ name: String = "Local") -> AddonInfo {
    AddonInfo(url: "https://addon.example/k3y/manifest.json", host: "addon.example", name: name, description: nil, logo: nil,
              version: "1.0.0", enabled: false, provides: ["stream", "subtitles"], types: ["movie", "series"],
              configurable: false, configureUrl: nil, builtIn: nil)
}

private let tokenRefusal = CoreError("this link carries a sign-in in its settings: install it only if it is yours", code: "tokenInLink")

/// A link field over fakes of `previewAddon` and `installAddon`; a link
/// with "token" in it is refused until acknowledged, as the core does.
@MainActor private func field(_ calls: Calls, previewFails: (any Error)? = nil) -> LinkInstall {
    LinkInstall(
        preview: { url, acknowledged in
            calls.previews.append((url, acknowledged))
            if let previewFails { throw previewFails }
            if url.contains("token"), !acknowledged { throw tokenRefusal }
            return addon()
        },
        install: { url, acknowledged in calls.installs.append((url, acknowledged)) })
}

@Suite @MainActor struct InstallSheetTests {
    @Test func theAddsLineFollowsEveryVector() throws {
        let vectors = try shared().vectors
        #expect(vectors.count == 10)
        for v in vectors {
            #expect(R.addsLine(provides: v.provides, types: v.types) == (v.line.isEmpty ? nil : v.line), "\(v.provides) \(v.types)")
        }
    }

    @Test func theWordsAreTheSharedOnes() throws {
        let file = try shared()
        #expect(file.words.count == 8)
        #expect(R.table == file.words)
        #expect(R.resources == file.resources)
        for (type, word) in file.kinds { #expect(ListedAddon.kindWord(type) == word, "\(type)") }
    }

    @Test func theSheetSaysTheNameTheHostWhatItAddsAndTheNote() {
        let ask = R.ask(name: "Torrentio", host: "torrentio.strem.fun", provides: ["stream"], types: ["movie", "series"]) {}
        #expect(ask.title == "Install Torrentio?")
        #expect(ask.lines == ["From torrentio.strem.fun", "Adds sources for movies and series.", R.note])
        // Nothing it has words for: no Adds line, the addon still installs.
        let bare = R.ask(name: "X", host: "x.example", provides: ["something_new"], types: ["movie"]) {}
        #expect(bare.lines == ["From x.example", R.note])
    }

    @Test func aCardsInstallAsksFirstCancelInstallsNothingInstallInstallsOnce() async {
        let core = RealCore()
        let page = FindAddonsPage(core: core.client)
        let calls = Calls()
        let entry = ListedAddon(
            key: "https://opensubtitles-v3.strem.io/manifest.json", id: "org.example", host: "opensubtitles-v3.strem.io",
            name: "OpenSubtitles v3", description: nil, types: ["movie", "series"], provides: ["subtitles"],
            configuration: .none, configureUrl: "https://opensubtitles-v3.strem.io/configure", installed: .absent, copyKey: nil)
        page.press(entry, entry.action) { calls.runs += 1 }
        #expect(calls.runs == 0)
        #expect(page.gate.asking?.title == "Install OpenSubtitles v3?")
        #expect(page.gate.asking?.lines == ["From opensubtitles-v3.strem.io", "Adds subtitles for movies and series.", R.note])
        page.gate.cancel()
        #expect(page.gate.asking == nil)
        #expect(calls.runs == 0)
        // Install and Configure asks too.
        page.press(entry, .install(configure: true)) { calls.runs += 1 }
        page.gate.confirm()
        page.gate.confirm()
        #expect(calls.runs == 1)
        #expect(page.gate.asking == nil)
        // Turn on: the addon is installed already, so no sheet.
        page.press(entry, .turnOn(url: entry.key, word: "Off")) { calls.runs += 1 }
        #expect(calls.runs == 2)
        #expect(page.gate.asking == nil)
    }

    @Test func aPastedLinkIsReadThenAskedCancelKeepsItInstallInstallsOnce() async {
        let calls = Calls()
        let link = field(calls)
        let url = "https://addon.example/k3y/manifest.json"
        link.link = url
        await link.install()
        #expect(calls.previews.map(\.url) == [url])
        #expect(calls.installs.isEmpty)
        #expect(link.gate.asking?.title == "Install Local?")
        #expect(link.gate.asking?.lines == ["From addon.example", "Adds sources and subtitles for movies and series.", R.note])
        link.gate.cancel()
        #expect(link.link == url)
        #expect(calls.installs.isEmpty)
        await link.install()
        link.gate.confirm()
        link.gate.confirm()
        #expect(await eventually { !calls.installs.isEmpty && !link.installing })
        #expect(calls.installs.map(\.url) == [url])
        #expect(calls.installs.map(\.acknowledged) == [false])
        #expect(link.link == "")
        #expect(link.problem == nil)
    }

    @Test func aLinkWithASignInIsWarnedThenReadWithTheWordAndInstalledWithIt() async {
        let calls = Calls()
        let link = field(calls)
        let url = "https://addon.example/token/manifest.json"
        link.link = url
        await link.install()
        #expect(link.refused == url)
        #expect(link.gate.asking == nil)
        await link.install(acknowledged: true)
        #expect(link.refused == nil)
        #expect(calls.previews.map(\.acknowledged) == [false, true])
        #expect(calls.installs.isEmpty)
        link.gate.confirm()
        #expect(await eventually { !calls.installs.isEmpty })
        #expect(calls.installs.map(\.acknowledged) == [true])
    }

    @Test func aLinkThatCannotBeReadStaysUnderTheFieldAndAsksNothing() async {
        let calls = Calls()
        let link = field(calls, previewFails: CoreError("This addon's server did not answer in time", code: "timeout"))
        link.link = "https://addon.example/manifest.json"
        await link.install()
        #expect(link.problem == "This addon's server did not answer in time")
        #expect(link.gate.asking == nil)
        #expect(calls.installs.isEmpty)
        #expect(!link.checking)
    }
}
