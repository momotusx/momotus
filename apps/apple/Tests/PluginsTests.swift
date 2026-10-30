import Foundation
import Testing

// Nuvio plugin repositories (plans/plugins-and-marketplace.md 7.7, 7.8): the words against
// apps/shared/plugin-words.json, the sheet's and Settings' lines, and the plugins' rows decoded and
// listed last under "From plugins".

private func shared() throws -> [String: String] {
    let url = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../../shared/plugin-words.json")
    return try JSONDecoder().decode([String: String].self, from: Data(contentsOf: url))
}

@Test func pluginWordsAreTheSharedOnes() throws {
    #expect(try shared() == PluginWords.all)
}

@Test func aListedRepositorySaysWhatItHasAndIsFoundByAnyOfIt() throws {
    let json = """
    {"url": "u", "host": "raw.githubusercontent.com", "name": "Türk Plugins", "description": "",
     "providers": ["Dizibox", "HDFilmCehennemi"], "languages": ["tr"], "types": ["movie", "series"], "installed": false}
    """
    let r = try JSONDecoder().decode(ListedRepo.self, from: Data(json.utf8))
    #expect(PluginWords.directoryLine(r) == "2 providers for films and series. Languages: tr.")
    #expect(PluginWords.matches(r, "turk"))
    #expect(PluginWords.matches(r, "dizibox TR"))
    #expect(!PluginWords.matches(r, "anime"))
    #expect(PluginWords.matches(r, "  "))
}

@Test func theSheetNamesFiveProvidersAndCountsTheRest() {
    #expect(PluginWords.adds(["One"]) == "Adds 1 provider: One.")
    #expect(PluginWords.adds(["A", "B"]) == "Adds 2 providers: A and B.")
    #expect(PluginWords.adds("ABCDEFGH".map(String.init)) == "Adds 8 providers: A, B, C, D, E and 3 more.")
}

@Test func theSettingsLineCountsProvidersOnInRepositoriesThatAreOn() throws {
    let json = """
    [{"url": "u1", "host": "h", "name": null, "description": "", "enabled": true, "defaultKey": null, "problem": null,
      "providers": [{"id": "a", "name": "A", "version": "", "description": "", "languages": [], "types": [], "enabled": true},
                    {"id": "b", "name": "B", "version": "", "description": "", "languages": [], "types": [], "enabled": false}]},
     {"url": "u2", "host": "h", "name": "R", "description": "", "enabled": false, "defaultKey": "nuvio-tr", "problem": null,
      "providers": [{"id": "c", "name": "C", "version": "", "description": "", "languages": ["tr"], "types": ["movie"], "enabled": true}]}]
    """
    let repos = try JSONDecoder().decode([PluginRepoInfo].self, from: Data(json.utf8))
    #expect(PluginWords.summary([]) == PluginWords.summaryNone)
    #expect(PluginWords.summary(repos) == "2 repositories, 1 provider on")
    #expect(repos[0].shownName == "h")
}

@Test func pluginRowsComeLastUnderFromPlugins() throws {
    let row = """
    {"key": "%@", "quality": "1080p", "features": [], "sizeBytes": null, "status": {"kind": "direct"}, "via": "V",
     "releaseName": "R", "languages": [], "reasons": [], "group": "best", "failoverCount": 0, "line": null,
     "sourceClass": "ready", "bitrateMbps": null, "origin": "%@", "host": %@, "sourceName": %@}
    """
    func make(_ key: String, _ origin: String, _ host: String, _ source: String) -> String {
        String(format: row, key, origin, host, source)
    }
    let addonRow = make("a", "addon", "null", "null")
    let pluginRow = make("plugin:r/p:h", "plugin", #""cdn.example.com""#, #""Dizibox""#)
    let json = """
    {"recommended": null, "rows": [\(addonRow)], "pendingAddons": [], "slowAddons": [],
     "failedAddons": [], "done": true, "frozen": false, "conditions": null, "note": null,
     "sections": [{"quality": "fhd", "keys": ["a"]}],
     "plugins": {"rows": [\(pluginRow)], "pending": false}}
    """
    let s = try JSONDecoder().decode(VersionsSnapshot.self, from: Data(json.utf8))
    let sections = VersionWords.sections(s)
    #expect(sections.map(\.title) == [StreamChoice.words.page.sections["fhd"] ?? "fhd", PluginWords.section])
    let found = try #require(sections.last?.rows.first)
    #expect(VersionWords.rowWords(found) == "Dizibox, plays from cdn.example.com")
    // An older core sends none.
    let older = try JSONDecoder().decode(VersionsSnapshot.self, from: Data(json.replacingOccurrences(of: "\"plugins\"", with: "\"ignored\"").utf8))
    #expect(VersionWords.sections(older).count == 1)
    // The plugins' row counts beside the addons' (batch-2 walk F3).
    #expect(VersionWords.count(s) == "2 versions")
    // No addon row: the plugins' one is the page's version, chosen by hand; while they still look,
    // the page is still finding, never "0 versions".
    let alone = try JSONDecoder().decode(VersionsSnapshot.self, from: Data(json.replacingOccurrences(of: "[\(addonRow)]", with: "[]").utf8))
    #expect(VersionWords.count(alone) == "1 version")
    #expect(VersionWords.status(alone) == PluginWords.choose)
    let looking = try JSONDecoder().decode(VersionsSnapshot.self, from: Data(json
        .replacingOccurrences(of: "[\(addonRow)]", with: "[]")
        .replacingOccurrences(of: "[\(pluginRow)], \"pending\": false", with: "[], \"pending\": true").utf8))
    #expect(VersionWords.count(looking) == nil)
    #expect(VersionWords.status(looking) == StreamChoice.words.page.finding)
}
