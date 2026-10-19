import Foundation

/// A real core opened in a temporary directory, removed when the test ends.
@MainActor final class RealCore {
    let directory = FileManager.default.temporaryDirectory.appending(path: "mq-test-\(UUID().uuidString)", directoryHint: .isDirectory)
    let client: CoreClient

    init() {
        client = CoreClient(directory: directory)
    }

    /// Waits until the core answers (it opens off the main thread).
    func ready() async -> Bool {
        for _ in 0..<100 {
            if await client.ping() != nil { return true }
            try? await Task.sleep(for: .milliseconds(50))
        }
        return false
    }

    deinit {
        try? FileManager.default.removeItem(at: directory)
    }
}

/// Polls `condition` on the main actor for up to `seconds`.
@MainActor func eventually(_ seconds: Double = 5, _ condition: () -> Bool) async -> Bool {
    let end = ContinuousClock.now + .seconds(seconds)
    while ContinuousClock.now < end {
        if condition() { return true }
        try? await Task.sleep(for: .milliseconds(20))
    }
    return condition()
}
