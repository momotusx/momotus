import Foundation
import Testing

// The owner's rule: a failure shows its kind's words (apps/shared/error-words.json), never the core's
// raw text or a status.

@Test func theFailureWordsAreTheSharedFile() {
    let kinds: Set = [
        "failed", "network", "timeout", "notFound", "unauthorized", "forbidden", "rateLimited", "serverError",
        "httpError", "unreadable", "blockedAddress", "invalidUrl", "coreStopped", "coreNotStarted",
    ]
    #expect(Set(ErrorWords.table.keys) == kinds)
    for (kind, words) in ErrorWords.table {
        #expect(!words.isEmpty && words.range(of: #"\d{3}|http|error:"#, options: [.regularExpression, .caseInsensitive]) == nil, "\(kind)")
    }
}

@Test func aFailureShowsItsKindsWordsNeverItsRawText() {
    let raw = "network: error sending request for url (http://127.0.0.1:9/manifest.json): Connection refused"
    #expect(CoreError(raw, kind: "network").localizedDescription == ErrorWords.table["network"])
    #expect(ErrorWords.of(CoreError("http status 404", kind: "notFound")) == ErrorWords.table["notFound"])
    // No kind (an older core, an error made here) and an unknown kind are `failed`.
    #expect(ErrorWords.of(CoreError("internal error: task 7 panicked")) == ErrorWords.failed)
    #expect(ErrorWords.of(CoreError("x", kind: "somethingNew")) == ErrorWords.failed)
    #expect(ErrorWords.of(CocoaError(.fileReadCorruptFile)) == ErrorWords.failed)
    // The core's own sentence is kept.
    #expect(ErrorWords.of(CoreError("Kids profiles never run plugins.", kind: "said")) == "Kids profiles never run plugins.")
}

@Test func aReplyCarriesItsKindAndADeadCoreSaysSoInWords() {
    let router = CoreRouter()
    let seen = Seen()
    router.wait(for: 1) { result in if case .failure(let e) = result { seen.errors.append(e) } }
    router.wait(for: 2) { result in if case .failure(let e) = result { seen.errors.append(e) } }
    router.onMessage(json: #"{"id":1,"ok":false,"error":"http status 403","kind":"forbidden"}"#)
    router.onMessage(json: #"{"core":"died","reason":"task 7 panicked"}"#)
    let errors = seen.errors
    #expect(errors.map(\.kind) == ["forbidden", "coreStopped"])
    #expect(errors.map(\.localizedDescription) == [ErrorWords.of(kind: "forbidden"), ErrorWords.of(kind: "coreStopped")])
}

/// The router answers its waiters on the calling thread here, one at a time.
private final class Seen: @unchecked Sendable { var errors: [CoreError] = [] }
