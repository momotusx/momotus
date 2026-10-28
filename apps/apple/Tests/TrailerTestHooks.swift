import Foundation

/// The one hook HeroTrailer.swift reads, for the test target (the app's DebugHooks.swift brings the whole app with it):
/// a walk's `-MQMute`, off here.
enum DebugHooks {
    static var mute: Bool { UserDefaults.standard.bool(forKey: "MQMute") }
}
