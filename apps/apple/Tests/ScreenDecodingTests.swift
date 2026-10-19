import Foundation
import Testing

// Record view snapshots recorded from the real core by scripts/capture-fixtures.ts
// (search for "thrones", Discover switched to series, a one-title library,
// two addons, default settings, an import waiting for its link).

private func fixture<T: Decodable>(_ name: String, as type: T.Type) throws -> T {
    let url = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "Fixtures/\(name)")
    return try JSONDecoder().decode(T.self, from: Data(contentsOf: url))
}

@Test func searchRowsReuseHomeRows() throws {
    let search = try fixture("search.json", as: SearchSnapshot.self)
    #expect(search.query == "thrones")
    #expect(!search.rows.isEmpty)
    #expect(search.rows.allSatisfy { $0.source == "Cinemeta" })
}

@Test func discoverFollowsTheSelectedType() throws {
    let discover = try fixture("discover.json", as: DiscoverSnapshot.self)
    #expect(discover.kind == "series")
    #expect(discover.kinds.map(\.id).contains("movie"))
    #expect(discover.catalog == discover.catalogs.first?.id)
    #expect(!discover.genres.isEmpty)
    #expect(!discover.items.isEmpty)
    #expect(discover.state == .ready)
}

@Test func libraryCountsKinds() throws {
    let library = try fixture("library.json", as: LibrarySnapshot.self)
    #expect(library.sort == "recent")
    #expect(library.kinds.map(\.label) == ["Series"])
    #expect(library.items.first?.name == "Breaking Bad")
}

@Test func addonsListTheirCapabilities() throws {
    let addons = try fixture("addons.json", as: AddonsSnapshot.self).addons
    #expect(addons.map(\.name) == ["Cinemeta", "Sample Source"])
    #expect(addons[0].provides.contains("catalog"))
    #expect(addons[1].provides == ["stream"])
}

@Test func settingsDecodeDefaults() throws {
    let prefs = try fixture("settings.json", as: Prefs.self)
    #expect(prefs.streams.quality == "best")
    #expect(prefs.streams.maxHeight == nil)
    #expect(prefs.autoplayNext)
}

@Test func importWaitsForTheLink() throws {
    guard case .waitingForLink(let code, let link, _) = try fixture("import.json", as: ImportSnapshot.self) else {
        Issue.record("expected waitingForLink")
        return
    }
    #expect(!code.isEmpty)
    #expect(link.hasPrefix("https://link.stremio.com/"))
}

@Test func pairingPhasesDecode() throws {
    // Shapes from apps/web/src/core/generated/PairingSnapshot.ts.
    let decode = { (json: String) in try JSONDecoder().decode(PairingSnapshot.self, from: Data(json.utf8)) }
    #expect(try decode(#"{"phase":"waiting","qr":"marquee-pair:abc","expiresMs":1760000000000}"#)
        == .waiting(qr: "marquee-pair:abc", expiresMs: 1_760_000_000_000))
    #expect(try decode(#"{"phase":"confirm","code":"123456"}"#) == .confirm(code: "123456"))
    #expect(try decode(#"{"phase":"failed","reason":"no answer"}"#) == .failed(reason: "no answer"))
}

@Test func storedRecordsKeep64BitNumbersExact() throws {
    // The keystore record is opaque: it must come back byte for byte in meaning.
    let json = #"{"phase":"joined","record":{"device":18446744073709551615,"epoch":3,"token":"t","flags":[true,null]}}"#
    guard case .joined(let record) = try JSONDecoder().decode(PairingSnapshot.self, from: Data(json.utf8)) else {
        Issue.record("expected joined")
        return
    }
    let text = String(decoding: try JSONEncoder().encode(record), as: UTF8.self)
    #expect(text.contains("18446744073709551615"))
    #expect(try JSONDecoder().decode(JSONValue.self, from: Data(text.utf8)) == record)
}


// TurkceAltyazi.org built into Marquee (plans/m13-turkcealtyazi.md 3.7).

@Test func aBuiltInSourceSaysSoInsteadOfAHost() throws {
    let base = #"{"url": "https://5a0d1888fa64-turkcealtyaziorg.baby-beamup.club/addon/manifest.json", "host": "5a0d1888fa64-turkcealtyaziorg.baby-beamup.club", "name": "TurkceAltyazi.org", "description": null, "logo": null, "version": "1.1.1", "enabled": true, "provides": ["subtitles"], "types": ["movie", "series"], "configurable": false, "configureUrl": null"#
    let builtIn = try JSONDecoder().decode(AddonInfo.self, from: Data((base + #", "builtIn": true}"#).utf8))
    #expect(builtIn.hostLine == "Built into Marquee, runs on this device")
    #expect(builtIn.configureUrl == nil)
    let fetched = try JSONDecoder().decode(AddonInfo.self, from: Data((base + #", "builtIn": false}"#).utf8))
    #expect(fetched.hostLine == "5a0d1888fa64-turkcealtyaziorg.baby-beamup.club")
    // An older core sends no field: the host, as before.
    let older = try JSONDecoder().decode(AddonInfo.self, from: Data((base + "}").utf8))
    #expect(older.builtIn == nil && older.hostLine == older.host)
}
