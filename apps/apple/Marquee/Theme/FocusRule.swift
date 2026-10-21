import SwiftUI
#if os(macOS)
import AppKit
#endif

/// Where focus shows (design/DECISION.md: "desktop and phone show it for
/// keyboard focus only"). On the Mac, SwiftUI's `isFocused` is the nearest
/// focusable ancestor's: a page that takes the keyboard (the player, the PIN
/// keypad) lit every control inside it at once. Without keyboard navigation
/// no control takes focus itself, so nothing shows; with it, each control's
/// own focus does. TV always moves focus with the remote.
enum FocusRule {
    static func shown(focused: Bool, keyboardNavigation: Bool) -> Bool {
        focused && keyboardNavigation
    }

    /// The ring's palette color: the key blue on the Mac (Apple's keyboard ring), white on
    /// tvOS, which has no key color and keeps its v2 ring (D9).
    enum Ring { case key, accent }
    static var ring: Ring {
        #if os(macOS)
        .key
        #else
        .accent
        #endif
    }
}

enum KeyboardNavigation {
    /// The Mac's "Keyboard navigation" setting (Tab moves between controls).
    /// TV moves focus with the remote, and iOS only focuses a control for a
    /// keyboard (the ring itself is never drawn on a touch screen).
    @MainActor static var on: Bool {
        #if os(macOS)
        NSApp?.isFullKeyboardAccessEnabled ?? false
        #else
        true
        #endif
    }
}

/// A control's own keyboard focus, for its ring, lift and accent: `isFocused`
/// filtered by `FocusRule`.
@propertyWrapper
struct KeyboardFocused: DynamicProperty {
    @Environment(\.isFocused) private var focused

    @MainActor var wrappedValue: Bool {
        FocusRule.shown(focused: focused, keyboardNavigation: KeyboardNavigation.on)
    }
}
