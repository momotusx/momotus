import Foundation
import Testing

// A view's commands and ops reach the core in the order they are sent, and none waits on another's reply.

@MainActor @Test func quickCommandsReachTheCoreInTheOrderSent() async throws {
    let core = RealCore()
    #expect(await core.ready())
    let search = LiveRecord<SearchSnapshot>("search", core: core.client)
    search.start()
    #expect(await eventually { search.snapshot != nil })
    let sub = try #require(search.sub)
    // Leaving Search on the Mac: remember, then clear. The clear goes out by the core's own at-once
    // send, so a remember still waiting to start (a Task) would land after it and Recent would lose
    // the query. (The core takes a typed query after its debounce, so each one is in before the pair.)
    for word in ["dune", "arrival", "heat"] {
        search.command(["type": "setQuery", "query": word])
        #expect(await eventually { search.snapshot?.query == word })
        search.command(["type": "remember"])
        core.client.commandNow(sub, ["type": "setQuery", "query": ""])
    }
    #expect(await eventually { search.snapshot?.recent == ["heat", "arrival", "dune"] })
    #expect(search.failure == nil)
}

@MainActor @Test func aSettingChangedTwiceLandsAsLastChanged() async throws {
    let core = RealCore()
    #expect(await core.ready())
    let prefs = LiveRecord<Prefs>("settings", core: core.client)
    prefs.start()
    #expect(await eventually { prefs.snapshot != nil })
    // A toggle on, off and on again, at once: the last one wins, as tapped.
    for value in [false, true, false] {
        prefs.performNow("setPrefs", ["patch": ["autoplayNext": value]])
    }
    #expect(await eventually { prefs.snapshot?.autoplayNext == false })
    try await Task.sleep(for: .milliseconds(200))
    #expect(prefs.snapshot?.autoplayNext == false)
    #expect(prefs.failure == nil)
}

@MainActor @Test func aFieldTheCoreCannotReadIsAFailureNotASilence() async throws {
    #expect(throws: CoreError.self) { try CoreClient.encode(op: "command", id: 1, fields: ["when": Date()]) }
    let core = RealCore()
    #expect(await core.ready())
    var failed: CoreError?
    core.client.command(1, ["type": "setQuery", "when": Date()]) { failed = $0 }
    #expect(failed?.code == "unencodable")
}

@Test func aCommandIsDeliveredWhenItIsSent() throws {
    // Read from the sources: the order holds because nothing stands between the call and the core
    // (no Task to be scheduled, no wait for an earlier reply, so a slow command holds nothing back).
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    func read(_ path: String) throws -> String { try String(contentsOf: app.appending(path: path), encoding: .utf8) }
    let record = try read("Core/LiveRecord.swift")
    let command = try body(of: "func command(_ fields: [String: any Sendable])", in: record)
    #expect(command.contains("core.command(sub, fields) {"))
    #expect(!command.contains("Task") && !command.contains("await"))
    #expect(try body(of: "func performNow(_ op: String, _ fields: [String: any Sendable])", in: record).contains("core.sendNow(op, fields)"))
    let send = try body(of: "done: @escaping @MainActor @Sendable (CoreError?) -> Void)", in: read("Core/CoreClient.swift"))
    #expect(send.contains("deliver(text)") && !send.contains("await"))
    // The callers whose commands depend on order (a stall's start and end, track picks, a fail-over,
    // a quick Library on and off) take the same path: no command goes out from inside a Task.
    for path in ["Player/PlayerModel.swift", "Title/TitleModel.swift"] {
        let text = try read(path)
        #expect(!text.contains("await core.command(") && !text.contains("await self.core.command("), "\(path) sends a command from a Task")
    }
    // Settings and addons changed by taps: in the order tapped.
    for path in ["Settings/SettingsScreen.swift", "Settings/StreamChoiceScreen.swift", "Settings/AddonsScreen.swift"] {
        let text = try read(path)
        #expect(!text.contains("Task { await prefs.perform(") && !text.contains("Task { await addons.perform("), "\(path) sends an op from a Task")
    }
}
