import Foundation
#if canImport(MetricKit) && !os(tvOS)
import MetricKit
#endif
#if canImport(UIKit)
import UIKit
#endif

/// Error reports on iOS and the Mac (plans/error-reports.md 5.1, 6.3): each core this app opens hears who the app
/// is (`reportContext`), and MetricKit's crash and hang diagnostics go to it as reports. tvOS sends nothing: no
/// `reportContext`, no row in Settings. A Release build sends; a Debug run only with `-MQReports YES` (a live check).
@MainActor
enum Diagnostics {
    static var wanted: Bool {
        #if os(tvOS)
        return false
        #elseif DEBUG
        return UserDefaults.standard.bool(forKey: "MQReports")
        #else
        return true
        #endif
    }

    /// The `reportContext` fields for this device.
    static func context(coreVersion: String) -> [String: any Sendable] {
        #if os(macOS)
        let app = "macos", form = "desktop", system = "macOS"
        #else
        let app = "ios", system = "iOS"
        let form = UIDevice.current.userInterfaceIdiom == .pad ? "tablet" : "phone"
        #endif
        let v = ProcessInfo.processInfo.operatingSystemVersion
        #if DEBUG
        let debug = true
        #else
        let debug = false
        #endif
        return [
            "device": ReportRules.device(
                app: app, debug: debug,
                version: Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "0",
                core: coreVersion, os: "\(system) \(v.majorVersion).\(v.minorVersion)", model: model(),
                form: form, coreBuild: CoreBuild.field),
            "server": AccountServer.default ?? "",
        ]
    }

    /// The model identifier (`iPhone14,2`, `Mac14,2`): never the name the owner gave the device.
    static func model() -> String {
        #if os(macOS)
        var size = 0
        sysctlbyname("hw.model", nil, &size, nil, 0)
        var chars = [CChar](repeating: 0, count: max(size, 1))
        sysctlbyname("hw.model", &chars, &size, nil, 0)
        return String(cString: chars)
        #else
        var info = utsname()
        uname(&info)
        return withUnsafeBytes(of: &info.machine) { String(decoding: $0.prefix { $0 != 0 }, as: UTF8.self) }
        #endif
    }

    #if canImport(MetricKit) && !os(tvOS)
    private static var subscriber: Subscriber?

    /// Starts listening for MetricKit diagnostics; each goes to `send` as a `report`.
    static func listen(send: @escaping @MainActor @Sendable ([String: any Sendable]) -> Void) {
        guard subscriber == nil else { return }
        let made = Subscriber(send: send)
        MXMetricManager.shared.add(made)
        subscriber = made
    }

    /// Sendable: its one stored value is an immutable `@Sendable` closure, and MetricKit calls it off the main actor.
    final class Subscriber: NSObject, MXMetricManagerSubscriber, Sendable {
        let send: @MainActor @Sendable ([String: any Sendable]) -> Void
        init(send: @escaping @MainActor @Sendable ([String: any Sendable]) -> Void) { self.send = send }

        nonisolated func didReceive(_ payloads: [MXDiagnosticPayload]) {
            var reports: [[String: any Sendable]] = []
            for payload in payloads {
                let at = Int64(payload.timeStampEnd.timeIntervalSince1970 * 1000)
                for crash in payload.crashDiagnostics ?? [] {
                    if let r = ReportRules.report(diagnostic: crash.jsonRepresentation(), hang: false, at: at) { reports.append(r) }
                }
                for hang in payload.hangDiagnostics ?? [] {
                    if let r = ReportRules.report(diagnostic: hang.jsonRepresentation(), hang: true, at: at) { reports.append(r) }
                }
            }
            let ready = reports
            let send = send
            Task { @MainActor in for r in ready { send(r) } }
        }
    }
    #endif
}
