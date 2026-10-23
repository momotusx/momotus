import Foundation
import Testing

// A view's commands reach the core in the order they are sent, and none waits on another's reply.

@MainActor @Test func quickCommandsReachTheCoreInTheOrderSent() async throws {
    let core = RealCore()
    #expect(await core.ready())
    let search = LiveRecord<SearchSnapshot>("search", core: core.client)
    search.start()
    #expect(await eventually { search.snapshot != nil })
    // Leaving Search on the Mac: remember, then clear, back to back. Out of order, Recent loses the
    // query. (The core takes a typed query after its debounce, so each one is in before the pair.)
    for word in ["dune", "arrival", "heat"] {
        search.command(["type": "setQuery", "query": word])
        #expect(await eventually { search.snapshot?.query == word })
        search.command(["type": "remember"])
        search.command(["type": "setQuery", "query": ""])
    }
    #expect(await eventually { search.snapshot?.recent == ["heat", "arrival", "dune"] })
    #expect(search.failure == nil)
}

@Test func aCommandIsDeliveredWhenItIsSent() throws {
    // Read from the sources: the order holds because nothing stands between the call and the core
    // (no Task to be scheduled, no wait for an earlier reply, so a slow command holds nothing back).
    let app = URL(fileURLWithPath: #filePath).deletingLastPathComponent().appending(path: "../Marquee")
    let record = try body(of: "func command(_ fields: [String: any Sendable])",
                          in: String(contentsOf: app.appending(path: "Core/LiveRecord.swift"), encoding: .utf8))
    #expect(record.contains("core.command(sub, fields) {"))
    #expect(!record.contains("Task") && !record.contains("await"))
    let client = try body(of: "failed: @escaping @MainActor @Sendable (CoreError) -> Void)",
                          in: String(contentsOf: app.appending(path: "Core/CoreClient.swift"), encoding: .utf8))
    #expect(client.contains("deliver(text)") && !client.contains("await"))
    // The callers whose commands depend on order (a stall's start and end, track picks, a fail-over,
    // a quick Library on and off) take the same path: no command goes out from inside a Task.
    for path in ["Player/PlayerModel.swift", "Title/TitleModel.swift"] {
        let text = try String(contentsOf: app.appending(path: path), encoding: .utf8)
        #expect(!text.contains("await core.command(") && !text.contains("await self.core.command("), "\(path) sends a command from a Task")
    }
}
