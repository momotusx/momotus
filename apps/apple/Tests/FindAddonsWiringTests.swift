import Foundation
import Testing

// Find addons through the app's own wiring, on a real core opened in a
// temporary directory (reviewer round 2): the page's open path and Try again,
// a catalog that cannot open, and ProfileSession's switch, each against the
// shared card store the screen uses.

/// A request that waits until the test answers it.
@MainActor private final class Pending {
    private var waiting: CheckedContinuation<Void, any Error>?
    private(set) var started = false

    func request() async throws {
        started = true
        try await withCheckedThrowingContinuation { waiting = $0 }
    }

    func answer(_ error: any Error) {
        waiting?.resume(throwing: error)
        waiting = nil
    }

    func waitForStart() async {
        while !started { await Task.yield() }
    }
}

private let timeout = CoreError("This addon's server did not answer in time", code: "timeout", kind: "said")

@Suite(.serialized) @MainActor struct FindAddonsWiringTests {
    private let cards = InstallCards.shared

    /// A finished card ("done", with its words) and one still running ("busy").
    private func seed() async -> (Pending, Task<InstallCards.Card?, Never>) {
        cards.forgetAll()
        await cards.run("done") { throw timeout }
        let pending = Pending()
        let running = Task { await cards.run("busy", pending.request) }
        await pending.waitForStart()
        return (pending, running)
    }

    @Test func openingThePageForgetsTheLastVisitsResultsOnce() async {
        let core = RealCore()
        #expect(await core.ready())
        let (pending, running) = await seed()
        let page = FindAddonsPage(core: core.client)
        page.appear()
        #expect(cards["done"] == InstallCards.Card())
        #expect(cards["busy"].busy)
        // A result on this visit stays when the page appears again (back from a page on top).
        await cards.run("done") { throw timeout }
        page.appear()
        #expect(cards["done"].code == "timeout")
        // The page opened again: it goes.
        FindAddonsPage(core: core.client).appear()
        #expect(cards["done"] == InstallCards.Card())
        // The request still running lands on its own card.
        pending.answer(timeout)
        _ = await running.value
        #expect(cards["busy"].code == "timeout")
        cards.forgetAll()
    }

    @Test func tryAgainForgetsResultsKeepsRunningCardsAndOpensAFreshView() async {
        let core = RealCore()
        #expect(await core.ready())
        let (pending, running) = await seed()
        let page = FindAddonsPage(core: core.client)
        let before = page.catalog
        page.reload()
        #expect(page.catalog !== before)
        #expect(cards["done"] == InstallCards.Card())
        #expect(cards["busy"].busy)
        pending.answer(timeout)
        _ = await running.value
        cards.forgetAll()
    }

    @Test func aCatalogThatCannotOpenFailsThePageAndIsSaid() async throws {
        // A data directory under a plain file: the core cannot open there.
        let file = FileManager.default.temporaryDirectory.appending(path: "mq-test-\(UUID().uuidString)")
        try Data().write(to: file)
        defer { try? FileManager.default.removeItem(at: file) }
        let client = CoreClient(directory: file.appending(path: "data", directoryHint: .isDirectory))
        let page = FindAddonsPage(core: client)
        page.catalog.start()
        #expect(await eventually { page.catalog.failure != nil })
        guard case .failed(let words) = page.status(of: "official") else {
            Issue.record("the page is \(page.status(of: "official")), not failed")
            return
        }
        #expect(words == ErrorWords.of(kind: "coreNotStarted"))
        #expect(page.status(of: "official").announcement(after: .loading) == words)
    }

    @Test func switchingProfileForgetsEveryCardAndALateAnswerLandsNowhere() async throws {
        let core = RealCore()
        #expect(await core.ready())
        let session = ProfileSession(core: core.client, sync: SyncModel(core: core.client, records: FakeKeychain().record,
                                                                         defaults: try #require(UserDefaults(suiteName: "mq-test-\(UUID().uuidString)"))))
        #expect(await eventually { session.snapshot != nil })
        try await session.perform("createProfile", ["name": "Kid", "avatar": ProfileRules.formatAvatar(glyph: "cat", color: "teal")])
        #expect(await eventually { session.snapshot?.profiles.count == 2 })
        let kid = try #require(session.snapshot?.profiles.first { !$0.isOwner })
        let (pending, running) = await seed()
        #expect(await session.switchTo(kid.id, pin: nil))
        #expect(cards["done"] == InstallCards.Card())
        #expect(cards["busy"] == InstallCards.Card())
        // The old profile's request answers after the switch: nothing lands.
        pending.answer(timeout)
        #expect(await running.value == nil)
        #expect(cards["busy"] == InstallCards.Card())
    }
}
