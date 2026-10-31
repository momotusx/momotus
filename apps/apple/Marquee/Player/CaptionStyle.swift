import CoreText
import MediaAccessibility
import SwiftUI

/// The viewer's caption style (Settings, Accessibility, Subtitles & Captioning;
/// on a Mac, Accessibility, Captions), which AVKit applies to a stream's own
/// text tracks, read for the addon subtitles the app draws itself. An addon's
/// subtitle is plain text with no styling of its own, so every value applies,
/// those a style lets content override included.
struct CaptionStyle {
    /// A face the viewer picked, or nil for the system face.
    var face: String?
    /// Relative to the app's subtitle size.
    var scale: CGFloat
    var text: Color
    /// Behind the glyphs.
    var highlight: Color
    /// The box behind the lines.
    var window: Color
    /// One box's fill (plans/dual-subtitles.md 4 E): the viewer's window colour when their style shows one, else
    /// the colour behind the glyphs, drawn once behind both lines.
    var oneBoxFill: Color
    var radius: CGFloat
    var edge: MACaptionAppearanceTextEdgeStyle

    /// The profile's subtitle size over the system caption size: each app
    /// maps the core's four steps to its own player (plans/core-protocol.md).
    static func factor(_ size: SubtitleSize) -> CGFloat {
        switch size {
        case .small: 0.8
        case .medium: 1
        case .large: 1.25
        case .extraLarge: 1.5
        }
    }

    /// The same size for a file's own tracks, which AVFoundation draws: a
    /// percent of its default (`kCMTextMarkupAttribute_RelativeFontSize`), none
    /// at Medium.
    static func relativeFontSize(_ size: SubtitleSize) -> Int? {
        let percent = Int((factor(size) * 100).rounded())
        return percent == 100 ? nil : percent
    }

    /// One box takes the window colour when the style shows a window (its opacity over 0), as Android takes the
    /// style's window colour over its background.
    static func oneBoxUsesWindow(windowOpacity: CGFloat) -> Bool { windowOpacity > 0 }

    static func words(_ size: SubtitleSize) -> String {
        switch size {
        case .small: "Small"
        case .medium: "Medium"
        case .large: "Large"
        case .extraLarge: "Extra large"
        }
    }

    // ponytail: read on each new cue, no settings-change observer; a change made
    // while a cue is on screen shows from the next cue.
    static var current: CaptionStyle {
        let descriptor = MACaptionAppearanceCopyFontDescriptorForStyle(.user, nil, .default).takeRetainedValue()
        let highlight = color(MACaptionAppearanceCopyBackgroundColor(.user, nil), MACaptionAppearanceGetBackgroundOpacity(.user, nil))
        let windowOpacity = MACaptionAppearanceGetWindowOpacity(.user, nil)
        let window = color(MACaptionAppearanceCopyWindowColor(.user, nil), windowOpacity)
        return CaptionStyle(
            face: face(named: CTFontDescriptorCopyAttribute(descriptor, kCTFontNameAttribute) as? String),
            scale: MACaptionAppearanceGetRelativeCharacterSize(.user, nil),
            text: color(MACaptionAppearanceCopyForegroundColor(.user, nil), MACaptionAppearanceGetForegroundOpacity(.user, nil)),
            highlight: highlight,
            window: window,
            oneBoxFill: oneBoxUsesWindow(windowOpacity: windowOpacity) ? window : highlight,
            radius: MACaptionAppearanceGetWindowRoundedCornerRadius(.user, nil),
            edge: MACaptionAppearanceGetTextEdgeStyle(.user, nil))
    }

    /// System faces have private names (".AppleSystemUIFont"): the app's own
    /// face stands in for them, so the default style keeps the app's look.
    static func face(named name: String?) -> String? {
        guard let name, !name.isEmpty, !name.hasPrefix(".") else { return nil }
        return name
    }

    private static func color(_ value: Unmanaged<CGColor>, _ opacity: CGFloat) -> Color {
        Color(cgColor: value.takeRetainedValue()).opacity(opacity)
    }
}

/// The style's text edge, drawn in black as the system draws it.
struct CaptionEdge: ViewModifier {
    let edge: MACaptionAppearanceTextEdgeStyle

    func body(content: Content) -> some View {
        switch edge {
        case .raised: content.shadow(color: .black.opacity(0.9), radius: 0, x: 1, y: 1)
        case .depressed: content.shadow(color: .black.opacity(0.9), radius: 0, x: -1, y: -1)
        case .uniform: content.shadow(color: .black, radius: 0.7).shadow(color: .black, radius: 0.7).shadow(color: .black, radius: 0.7)
        case .dropShadow: content.shadow(color: .black.opacity(0.8), radius: 2, x: 1.5, y: 1.5)
        default: content
        }
    }
}
