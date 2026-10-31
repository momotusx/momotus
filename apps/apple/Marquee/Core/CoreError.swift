import Foundation

/// A refusal from the core: its raw text (for the log), the kind of failure a
/// screen words, and for the errors an app acts on a code.
struct CoreError: LocalizedError, Sendable {
    /// The core's raw text: for the log only, never shown (the owner's rule).
    let message: String
    /// Stable name for the errors an app acts on (sync, profiles); never parse `message`.
    var code: String?
    /// With `locked`: how long until the next PIN try.
    var retryAfterMs: Int64?
    /// With `viewsOpen`: the views still open.
    var views: [String]?
    /// The reply's `kind`: what a screen shows for it (`ErrorWords`); `said`
    /// when `message` is already the core's sentence for the screen.
    var kind: String?
    init(_ message: String, code: String? = nil, retryAfterMs: Int64? = nil, views: [String]? = nil, kind: String? = nil) {
        self.message = message
        self.code = code
        self.retryAfterMs = retryAfterMs
        self.views = views
        self.kind = kind
    }
    /// The words a screen shows: never the raw text.
    var errorDescription: String? { kind == "said" ? message : ErrorWords.of(kind: kind) }
}

/// The failure words, apps/shared/error-words.json bundled as it is (the web
/// and Android read the same file; ErrorWordsTests checks the keys).
enum ErrorWords {
    /// Every entry by kind, the file's `about` note left out.
    static let table: [String: String] = {
        guard let url = Bundle(for: BundleToken.self).url(forResource: "error-words", withExtension: "json"),
              let data = try? Data(contentsOf: url),
              var words = try? JSONDecoder().decode([String: String].self, from: data)
        else { fatalError("error-words.json is bundled with the app (project.yml)") }
        words["about"] = nil
        return words
    }()

    static var failed: String { table["failed"] ?? "" }

    /// The words for a kind; a missing or unknown one is `failed`.
    static func of(kind: String?) -> String { kind.flatMap { table[$0] } ?? failed }

    /// Any failure in words: a core refusal by its kind, anything else (a
    /// decode, a system error) `failed`. Never an error's own text.
    static func of(_ error: any Error) -> String {
        (error as? CoreError)?.errorDescription ?? failed
    }
}

private final class BundleToken {}
