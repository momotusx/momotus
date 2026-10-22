import SwiftUI

/// The title's Library and Watched marks as one capsule (Mac, iPad; the desktop frame's `.pair2`):
/// one 40 pt pill on the secondary fill, a cell per mark, each a toggle with its state in its label.
/// A series has the Library cell alone (`titleMarks`).
struct MarkCapsule: View {
    let marks: [TitleMark]
    let toggle: (TitleMark) -> Void

    var body: some View {
        HStack(spacing: 0) {
            ForEach(Array(marks.enumerated()), id: \.offset) { index, mark in
                if index > 0 { Rectangle().fill(.white.opacity(0.1)).frame(width: 1) }
                Button { toggle(mark) } label: {
                    Label(mark.words, glyph: mark.glyph, size: HeroLayout.icon)
                        .labelStyle(PillLabelStyle())
                        .textFont(15, weight: .semibold, relativeTo: .body)
                        .foregroundStyle(.white)
                        .padding(.horizontal, 18)
                        .frame(height: 40)
                }
                .buttonStyle(FocusPlainStyle(shape: Capsule(), scale: 1))
                // A state, spoken as one: "In Library, toggle, on".
                .accessibilityLabel(mark.name)
                .accessibilityAddTraits(.isToggle)
                .accessibilityValue(mark.on ? "On" : "Off")
            }
        }
        .fixedSize()
        .background(SecondaryFill(shape: Capsule()))
    }
}

/// Apple's Library control on the iPhone: a 48 pt round button whose plus turns into a check once the
/// title is in the library; its label says the state and the action.
struct LibraryControl: View {
    let on: Bool
    let toggle: () -> Void

    var body: some View {
        Button(action: toggle) {
            Glyph(on ? "check" : "plus", size: 22)
                .dynamicTypeSize(...DynamicTypeSize.xxxLarge)
                .foregroundStyle(.white)
                .frame(width: 48, height: 48)
                .background(SecondaryFill(shape: Circle()))
        }
        .buttonStyle(FocusPlainStyle(shape: Circle(), scale: Theme.Focus.scaleCard))
        .accessibilityLabel(on ? "In Library, remove" : "Add to Library")
    }
}

private extension TitleMark {
    var on: Bool {
        switch self { case .library(let on), .watched(let on): on }
    }
    var name: String {
        switch self { case .library: "In Library"; case .watched: "Watched" }
    }
    var words: String {
        switch self {
        case .library(let on): on ? "In Library" : "Add to Library"
        case .watched(let on): on ? "Watched" : "Mark as watched"
        }
    }
    var glyph: String {
        switch self {
        case .library(let on): on ? "check" : "plus"
        case .watched: "eye"
        }
    }
}
