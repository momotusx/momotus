import Foundation
import Network

/// A stand-in for Trakt's API and sign-in on loopback (as apps/web/e2e/trakt-fake.ts):
/// the device code flow, approved or refused by the test, the user, empty
/// lists, scrobbles and history writes counted as Trakt counts them. A debug
/// core reaches it through `MQ_DEV_TRAKT`. Its tokens are made up per run.
final class FakeTrakt: @unchecked Sendable {
    static let userCode = "MQ7K2P4D"
    static let user = "marquee-tester"

    private let listener: NWListener
    private let queue = DispatchQueue(label: "fake-trakt")
    private let lock = NSLock()
    private var approved = false
    private var seenPaths: [String] = []
    let run = UUID().uuidString.prefix(8).lowercased()
    private(set) var port: UInt16 = 0

    var url: String { "http://127.0.0.1:\(port)" }
    /// The tokens it hands out, to look for where the app could keep or show one.
    var secrets: [String] { ["fake-access-\(run)", "fake-refresh-\(run)"] }

    init() throws {
        listener = try NWListener(using: .tcp, on: .any)
        listener.newConnectionHandler = { [weak self] connection in self?.serve(connection) }
        let ready = DispatchSemaphore(value: 0)
        listener.stateUpdateHandler = { state in
            if case .ready = state { ready.signal() }
        }
        listener.start(queue: queue)
        _ = ready.wait(timeout: .now() + 5)
        port = listener.port?.rawValue ?? 0
    }

    deinit {
        listener.cancel()
    }

    func approve() {
        lock.withLock { approved = true }
    }

    /// Requests seen, as "METHOD /path".
    var seen: [String] { lock.withLock { seenPaths } }

    private func serve(_ connection: NWConnection) {
        connection.start(queue: queue)
        receive(connection, buffer: Data())
    }

    private func receive(_ connection: NWConnection, buffer: Data) {
        connection.receive(minimumIncompleteLength: 1, maximumLength: 65_536) { [weak self] data, _, done, error in
            guard let self else { return }
            var buffer = buffer
            if let data { buffer.append(data) }
            if let request = Self.request(from: buffer) {
                self.answer(request, on: connection)
            } else if !done, error == nil {
                self.receive(connection, buffer: buffer)
            } else {
                connection.cancel()
            }
        }
    }

    /// A whole request (headers and the body its length names), or nil while more is coming.
    private static func request(from data: Data) -> (method: String, path: String, body: [String: Any])? {
        guard let end = data.range(of: Data("\r\n\r\n".utf8)) else { return nil }
        let head = String(decoding: data[..<end.lowerBound], as: UTF8.self)
        let lines = head.components(separatedBy: "\r\n")
        let first = lines.first?.split(separator: " ") ?? []
        guard first.count >= 2 else { return nil }
        let length = lines.dropFirst().compactMap { line -> Int? in
            let parts = line.split(separator: ":", maxSplits: 1)
            guard parts.count == 2, parts[0].lowercased() == "content-length" else { return nil }
            return Int(parts[1].trimmingCharacters(in: .whitespaces))
        }.first ?? 0
        let bodyStart = end.upperBound
        guard data.count - bodyStart >= length else { return nil }
        let body = (try? JSONSerialization.jsonObject(with: data[bodyStart..<(bodyStart + length)])) as? [String: Any] ?? [:]
        let path = String(first[1].split(separator: "?").first ?? "")
        return (String(first[0]), path, body)
    }

    private func answer(_ request: (method: String, path: String, body: [String: Any]), on connection: NWConnection) {
        let route = "\(request.method) \(request.path)"
        let (approved, run) = lock.withLock { () -> (Bool, String) in
            seenPaths.append(route)
            return (self.approved, self.run)
        }
        let at = "2026-01-01T00:00:00.000Z"
        let (status, value): (Int, Any) = switch route {
        case "POST /oauth/device/code":
            (200, ["device_code": "device-\(run)", "user_code": Self.userCode, "verification_url": "https://trakt.tv/activate", "expires_in": 600, "interval": 1])
        case "POST /oauth/device/token":
            approved
                ? (200, ["access_token": "fake-access-\(run)", "refresh_token": "fake-refresh-\(run)", "expires_in": 7_776_000,
                         "created_at": Int(Date().timeIntervalSince1970), "scope": "public", "token_type": "bearer"])
                : (400, [String: Any]())
        case "POST /oauth/revoke": (200, [String: Any]())
        case "GET /users/settings": (200, ["user": ["username": Self.user, "ids": ["slug": Self.user, "uuid": "uuid-\(run)"]]])
        case "GET /sync/last_activities":
            (200, ["all": at, "movies": ["watched_at": at, "paused_at": at], "episodes": ["watched_at": at, "paused_at": at]])
        default:
            request.method == "GET" ? (200, [Any]()) : (404, [String: Any]())
        }
        let body = (try? JSONSerialization.data(withJSONObject: value)) ?? Data()
        var head = "HTTP/1.1 \(status) OK\r\ncontent-type: application/json\r\ncontent-length: \(body.count)\r\nconnection: close\r\n\r\n"
        if status != 200 { head = head.replacingOccurrences(of: " OK\r\n", with: " Error\r\n") }
        connection.send(content: Data(head.utf8) + body, completion: .contentProcessed { _ in connection.cancel() })
    }
}
