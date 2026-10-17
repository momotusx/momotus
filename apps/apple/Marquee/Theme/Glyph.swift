import SwiftUI

/// A Lucide icon (DECISION: one outline set) from the asset catalog, sized to
/// the text beside it and scaled with Dynamic Type. Names are Lucide's, as the
/// web uses them: `Glyph("play")`, `Glyph("eye-off", size: 26)`.
struct Glyph: View {
    let name: String
    @ScaledMetric private var size: CGFloat

    init(_ name: String, size: CGFloat = Theme.type.body + 1, relativeTo style: Font.TextStyle = .body) {
        self.name = name
        _size = ScaledMetric(wrappedValue: size, relativeTo: style)
    }

    var body: some View {
        Image(name)
            .renderingMode(.template)
            .resizable()
            .scaledToFit()
            .frame(width: size, height: size)
            .accessibilityHidden(true)
    }
}

extension Label where Title == Text, Icon == Glyph {
    /// A word with its Lucide icon: `Label("Play", glyph: "play-fill")`.
    @MainActor init(_ title: String, glyph: String, size: CGFloat = Theme.type.body + 1) {
        self.init { Text(title) } icon: { Glyph(glyph, size: size) }
    }
}

extension Button where Label == SwiftUI.Label<Text, Image> {
    /// A menu item with its Lucide icon. Menus draw only plain images, so the
    /// asset goes in as is; the menu sizes it.
    init(_ title: String, glyph: String, role: ButtonRole? = nil, action: @escaping () -> Void) {
        self.init(role: role, action: action) { SwiftUI.Label(title, image: glyph) }
    }
}
