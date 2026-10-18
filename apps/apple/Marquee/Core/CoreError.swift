import Foundation

/// A refusal from the core: its words, and for the errors an app acts on a code.
struct CoreError: LocalizedError, Sendable {
    let message: String
    /// Stable name for the errors an app acts on (sync, profiles); never parse `message`.
    var code: String?
    /// With `locked`: how long until the next PIN try.
    var retryAfterMs: Int64?
    /// With `viewsOpen`: the views still open.
    var views: [String]?
    init(_ message: String, code: String? = nil, retryAfterMs: Int64? = nil, views: [String]? = nil) {
        self.message = message
        self.code = code
        self.retryAfterMs = retryAfterMs
        self.views = views
    }
    var errorDescription: String? { message }
}
