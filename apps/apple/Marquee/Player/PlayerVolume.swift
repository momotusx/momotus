/// The player's own volume and mute, which the Mac's Playback menu sets (the system volume stays the system's).
/// Pure, so the tests run it; `PlayerModel` hands it to whichever engine plays.
struct PlayerVolume: Equatable {
    /// Tenths: one menu step each.
    private(set) var tenths = 10
    var muted = false

    var level: Float { Float(tenths) / 10 }

    /// A step louder; turning it up also ends a mute, as the system's keys do.
    mutating func up() {
        muted = false
        tenths = min(10, tenths + 1)
    }

    mutating func down() { tenths = max(0, tenths - 1) }
}
