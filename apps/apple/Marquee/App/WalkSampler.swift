#if MARQUEE_WALK
import Foundation
#if canImport(UIKit)
import UIKit
#endif

/// The walk build's resource sampler (plans/apple-second-engine.md section 3): every 10 s one line with the
/// process footprint, its CPU time so far, the thermal state and the battery. Only a walk build has it: Release
/// with `MARQUEE_WALK_BUILD="DEBUG MARQUEE_WALK"` (optimized, with the walk hooks; project.yml), never shipped.
enum WalkSampler {
    @MainActor static func start() {
        #if os(iOS)
        UIDevice.current.isBatteryMonitoringEnabled = true
        // A walk session holds the screen awake itself, so the phone does not lock between rows; the phone's own
        // auto-lock setting is never touched (the lead, 2026-10-26). Walk builds only.
        UIApplication.shared.isIdleTimerDisabled = true
        // The device gate's inputs, once (plans/iphone-mkv.md phase 3, the first gate row).
        let memory = ProcessInfo.processInfo.physicalMemory
        coreLog.notice("metric device machine=\(EngineChoice.machine, privacy: .public) memory_bytes=\(memory, privacy: .public) admits=\(EngineChoice.admits(memoryBytes: memory, model: EngineChoice.machine), privacy: .public)")
        #endif
        Task { @MainActor in
            while !Task.isCancelled {
                coreLog.notice("metric sample \(line(), privacy: .public)")
                try? await Task.sleep(for: .seconds(10))
            }
        }
    }

    @MainActor private static func line() -> String {
        let footprint = footprintBytes().map { String(format: "%.1f", Double($0) / 1_048_576) } ?? "n/a"
        return "footprint_mb=\(footprint) cpu_s=\(String(format: "%.2f", cpuSeconds())) thermal=\(thermal()) battery=\(battery())"
    }

    /// What the system counts against the app's memory limit (Xcode's memory gauge).
    private static func footprintBytes() -> UInt64? {
        var info = task_vm_info_data_t()
        var count = mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<natural_t>.size)
        let result = withUnsafeMutablePointer(to: &info) {
            $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) { task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count) }
        }
        return result == KERN_SUCCESS ? info.phys_footprint : nil
    }

    /// User and system CPU time of the whole process since launch.
    private static func cpuSeconds() -> Double {
        var usage = rusage()
        getrusage(RUSAGE_SELF, &usage)
        let seconds = { (t: timeval) in Double(t.tv_sec) + Double(t.tv_usec) / 1_000_000 }
        return seconds(usage.ru_utime) + seconds(usage.ru_stime)
    }

    private static func thermal() -> String {
        switch ProcessInfo.processInfo.thermalState {
        case .nominal: "nominal"
        case .fair: "fair"
        case .serious: "serious"
        case .critical: "critical"
        @unknown default: "unknown"
        }
    }

    /// Percent, or n/a where there is none or the system does not say.
    @MainActor private static func battery() -> String {
        #if os(iOS)
        let level = UIDevice.current.batteryLevel
        return level < 0 ? "n/a" : "\(Int((level * 100).rounded()))"
        #else
        return "n/a"
        #endif
    }
}
#endif
