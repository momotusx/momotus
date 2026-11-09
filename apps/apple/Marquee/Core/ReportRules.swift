import Foundation

/// Error reports' words (plans/error-reports.md 10), word for word apps/shared/report-words.json, which the web
/// and Android read too; a test checks every key. The row sits in Settings, This device (iOS and Mac; tvOS sends
/// nothing and has no row); the privacy page is `privacyPath` on the sync service's own host.
enum ReportWords {
    static let label = "Error reports"
    static let on = "On"
    static let off = "Off"
    static let note = "When Momotus crashes or something fails, it sends the Momotus team a short technical report: what failed, the app and system version and the device model. Never your account, addons, what you watch or file names, and we keep no network address. Turning this off deletes reports not sent yet."
    static let privacy = "Privacy"
    static let privacyPath = "/privacy"

    static let table: [String: String] = [
        "label": label, "on": on, "off": off, "note": note, "privacy": privacy, "privacyPath": privacyPath,
    ]

    /// The privacy page on `server`'s host, or nil in a build without one.
    static func privacyURL(server: String?) -> URL? {
        guard let server, !server.isEmpty else { return nil }
        var base = server
        while base.hasSuffix("/") { base.removeLast() }
        return URL(string: base + privacyPath)
    }
}

/// What the app hands the core for error reports (plans/error-reports.md 5.1, 6.3): the device fields, and
/// MetricKit's crash and hang diagnostics read from their JSON. The core strips, queues, scrubs and uploads.
enum ReportRules {
    /// A device field in the relay's charset: anything else becomes a space, trimmed to `max`.
    static func plain(_ text: String, max: Int) -> String {
        let allowed = Set("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789 ,._()+-")
        let cleaned = String(text.map { allowed.contains($0) ? $0 : " " }).trimmingCharacters(in: .whitespaces)
        return String(cleaned.prefix(max)).trimmingCharacters(in: .whitespaces)
    }

    /// The `reportContext` device: versions and the model, never the device's own name or an identifier.
    static func device(app: String, debug: Bool, version: String, core: String, os: String, model: String,
                       form: String, coreBuild: String) -> [String: any Sendable] {
        [
            "app": app,
            "channel": debug ? "debug" : "release",
            "version": plain(version, max: 32).isEmpty ? "0" : plain(version, max: 32),
            "core": plain(core, max: 40),
            "os": plain(os, max: 40),
            "model": plain(model, max: 60).isEmpty ? "Apple" : plain(model, max: 60),
            "abi": "arm64",
            "form": form,
            "coreBuild": coreBuild == "store" ? "store" : "own",
        ]
    }

    /// One MetricKit diagnostic's `jsonRepresentation()` as a `report`: `crash` (exception type, code, signal and
    /// termination reason in the message) or `anr` for a hang, with the attributed thread's frames as
    /// `binaryName 0xoffset` (unsymbolicated; the nightly script runs atos with the kept dSYM). Nil when it does
    /// not read as one.
    static func report(diagnostic data: Data, hang: Bool, at: Int64) -> [String: any Sendable]? {
        guard let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return nil }
        let meta = json["diagnosticMetaData"] as? [String: Any] ?? [:]
        let message: String
        if hang {
            message = "hang of \(meta["hangDuration"] as? String ?? "unknown length")"
        } else {
            let parts = [("exception type", meta["exceptionType"]), ("code", meta["exceptionCode"]), ("signal", meta["signal"])]
                .compactMap { name, value in value.map { "\(name) \($0)" } }
            let reason = (meta["terminationReason"] as? String).map { ": \($0)" } ?? ""
            message = "crash, " + (parts.isEmpty ? "no exception" : parts.joined(separator: ", ")) + reason
        }
        let tree = json["callStackTree"] as? [String: Any] ?? [:]
        let stacks = tree["callStacks"] as? [[String: Any]] ?? []
        let attributed = stacks.first { $0["threadAttributed"] as? Bool == true } ?? stacks.first
        var frames: [String] = []
        var level = attributed?["callStackRootFrames"] as? [[String: Any]] ?? []
        while let frame = level.first, frames.count < 48 {
            let binary = frame["binaryName"] as? String ?? "?"
            let offset = (frame["offsetIntoBinaryTextSegment"] as? NSNumber)?.int64Value ?? 0
            frames.append("\(binary) 0x\(String(offset, radix: 16))")
            level = frame["subFrames"] as? [[String: Any]] ?? []
        }
        return [
            "kind": hang ? "anr" : "crash",
            "at": at,
            "message": String(message.prefix(512)),
            "stack": frames,
        ]
    }
}
