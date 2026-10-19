import Darwin
import Foundation

/// Time since the process started, from the kernel's record of it, so cold
/// start numbers include loading the binary.
enum Launch {
    static func elapsed() -> Double {
        var info = kinfo_proc()
        var size = MemoryLayout<kinfo_proc>.size
        var mib: [Int32] = [CTL_KERN, KERN_PROC, KERN_PROC_PID, getpid()]
        guard sysctl(&mib, 4, &info, &size, nil, 0) == 0 else { return -1 }
        let start = info.kp_proc.p_un.__p_starttime
        return Date().timeIntervalSince1970 - (Double(start.tv_sec) + Double(start.tv_usec) / 1_000_000)
    }

    @MainActor private static var framed = false

    /// Logs `first_frame_s` once, on the main queue turn after the root view
    /// appeared: the first frame has been committed by then.
    @MainActor static func firstFrame() {
        guard !framed else { return }
        framed = true
        DispatchQueue.main.async {
            coreLog.notice("metric first_frame_s=\(elapsed(), format: .fixed(precision: 3), privacy: .public)")
        }
    }
}
