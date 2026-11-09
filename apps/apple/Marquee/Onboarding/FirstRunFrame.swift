import SwiftUI

/// A first-run sheet over the still wall (round2.html combo A): the phone's bottom sheet, its top corners round and,
/// on Login only, a grabber that goes back (a drag or a press); the Mac, iPad at regular width and TV center it at
/// its width (`FirstRunRules.sheetWidth`). Taller than the screen, it scrolls.
struct WallSheet<Content: View>: View {
    let sheet: FirstRunRules.Sheet
    /// The grabber's way back (phone, Login).
    var back: (() -> Void)?
    @ViewBuilder let content: Content
    @Environment(\.metrics) private var metrics

    var body: some View {
        let form = FirstRunRules.form(wide: metrics.wide, tv: tvForm)
        GeometryReader { page in
            ScrollView(.vertical) {
                Group {
                    if let width = FirstRunRules.sheetWidth(sheet, form: form) {
                        card(form)
                            .frame(maxWidth: width)
                            .padding(Theme.Space.m)
                    } else {
                        card(form)
                    }
                }
                .frame(maxWidth: .infinity, minHeight: page.size.height, alignment: form == .phone ? .bottom : .center)
            }
            .scrollIndicators(.hidden)
            .scrollBounceBehavior(.basedOnSize)
        }
    }

    private func card(_ form: FirstRunRules.Form) -> some View {
        let phone = form == .phone
        return VStack(alignment: .leading, spacing: 0) {
            if let back, FirstRunRules.grabber(sheet, form: form) { Grabber(back: back) }
            content
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, phone ? 20 : form == .tv ? 48 : 40)
        .padding(.top, phone ? (FirstRunRules.grabber(sheet, form: form) && back != nil ? 0 : 20) : 36)
        .padding(.bottom, phone ? 24 : 40)
        .background {
            if phone {
                UnevenRoundedRectangle(topLeadingRadius: Theme.Sheet.radius, topTrailingRadius: Theme.Sheet.radius, style: .continuous)
                    .fill(Theme.Sheet.fill)
                    .ignoresSafeArea(edges: .bottom)
            } else {
                RoundedRectangle(cornerRadius: Theme.Sheet.radius, style: .continuous).fill(Theme.Sheet.fill)
            }
        }
    }
}

/// The phone Login sheet's grabber: drag it down, or press it, to go back to Welcome (MOB-11, MOB-20).
private struct Grabber: View {
    let back: () -> Void

    var body: some View {
        Button(action: back) {
            Capsule().fill(.white.opacity(0.3)).frame(width: 40, height: 5)
                .frame(maxWidth: .infinity, minHeight: FirstRunRules.Size.grabber)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        #if !os(tvOS)
        .simultaneousGesture(DragGesture(minimumDistance: 12).onEnded { if $0.translation.height > 60 { back() } })
        #endif
        .accessibilityLabel("Back")
    }
}

extension EnvironmentValues {
    /// A page pushed from the Login sheet: drawn as the same sheet over the still wall (plans/first-screens.md 3.3).
    @Entry var onWall = false
}

/// A sign-in page (email, Other ways, the pairing code, a recovery kit): pushed from the Login sheet it is the same
/// sheet over the still wall, its Back (a press, Escape on the Mac) returning to Login; elsewhere the app's page.
struct SignInPage<Content: View>: View {
    let title: String
    /// False while the page holds the way out itself (a recovery kit on screen: I saved it).
    var backShown = true
    @ViewBuilder let content: Content
    @Environment(\.onWall) private var onWall
    @Environment(\.metrics) private var metrics
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        if onWall { wall } else { page }
    }

    private var wall: some View {
        ZStack {
            PosterWall(posters: WallOnce.shared.posters, columns: FirstRunRules.columns(FirstRunRules.form(wide: metrics.wide, tv: tvForm)),
                       running: false, dimmed: true)
            WallSheet(sheet: .login) {
                VStack(alignment: .leading, spacing: Theme.Space.m) {
                    if backShown { WallBack { dismiss() } }
                    SheetHead(title: title)
                    content
                }
            }
        }
        #if os(iOS)
        .toolbar(.hidden, for: .navigationBar)
        #else
        .onExitCommand(perform: backShown ? { dismiss() } : nil)
        #endif
    }

    private var page: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.m) {
                ScreenTitle(text: title)
                content
            }
            .frame(maxWidth: 760, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, metrics.gutter)
            .padding(.vertical, Theme.Space.l)
        }
        .scrollIndicators(.hidden)
        .background(Theme.Palette.bg)
        .topScrim()
    }
}

/// A profile form's fields (plans/first-screens.md 3.4): the avatar at 72, the Name field (never focused on open, so no
/// keyboard covers Continue; 30 characters at most), and the five swatches on the grey avatar tokens.
struct ProfileFields: View {
    @Binding var typed: String
    let placeholder: String
    let glyph: String
    let selected: String?
    /// The name the avatar's letter shows while the field is empty.
    let fallbackName: String
    let pick: (String) -> Void
    private var words: FirstRunWords { .shared }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack(spacing: 16) {
                ProfileAvatar(ProfileRules.Avatar(glyph: glyph, color: selected), name: typed.isEmpty ? fallbackName : typed, size: 72)
                nameField
            }
            swatches
        }
    }

    private var nameField: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(words.nameLabel)
                .textFont(12, weight: .medium, relativeTo: .caption)
                .foregroundStyle(Color.white.opacity(0.64))
                .accessibilityHidden(true)
            TextField(words.nameLabel, text: $typed, prompt: Text(placeholder))
                .textFieldStyle(.plain)
                .textFont(18, relativeTo: .body)
                .foregroundStyle(.white)
                #if os(iOS)
                .textContentType(.nickname)
                .submitLabel(.done)
                #endif
                .onChange(of: typed) { _, now in if now.count > 30 { typed = String(now.prefix(30)) } }
        }
        .padding(.horizontal, 16)
        .frame(maxWidth: .infinity, minHeight: 60, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 16, style: .continuous).fill(.white.opacity(0.1)))
        .overlay(RoundedRectangle(cornerRadius: 16, style: .continuous).strokeBorder(.white.opacity(0.55), lineWidth: 1.5))
    }

    private var swatches: some View {
        HStack(spacing: 12) {
            ForEach(FirstRunRules.swatches, id: \.self) { colour in
                let on = colour == selected
                Button { pick(colour) } label: {
                    Circle()
                        .fill(ProfileAvatar.color(colour))
                        .frame(width: FirstRunRules.Size.swatch, height: FirstRunRules.Size.swatch)
                        .overlay {
                            if on { Circle().strokeBorder(.white, lineWidth: 2).padding(-5) }
                        }
                        .contentShape(Circle())
                }
                .buttonStyle(FocusPlainStyle(shape: Circle()))
                .accessibilityLabel("\(words.colourLabel): \(colour.capitalized)")
                .accessibilityAddTraits(on ? .isSelected : [])
            }
        }
        .padding(.horizontal, 5)
        .accessibilityElement(children: .contain)
        .accessibilityLabel(words.colourLabel)
    }
}

/// A sheet's Back link, at its top left: one step back inside first run.
struct WallBack: View {
    let back: () -> Void

    var body: some View {
        Button(action: back) { Label("Back", glyph: "chevron-left") }
            .buttonStyle(.textLink)
            .textFont(17, weight: .semibold, relativeTo: .body)
            .foregroundStyle(tvForm ? Theme.Palette.text : Theme.Palette.key)
            .frame(minHeight: FirstRunRules.Size.link)
            .contentShape(Rectangle())
    }
}

/// A sheet's heading with its link at the right (Not now, Skip) and its line under it. The heading is the step's real
/// heading: a screen reader lands on it when the step changes (off TV).
struct SheetHead: View {
    let title: String
    var line: String?
    var link: String?
    var linkDisabled = false
    var onLink: () -> Void = {}
    @Environment(\.metrics) private var metrics
    @AccessibilityFocusState private var heard: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(alignment: .center, spacing: Theme.Space.s) {
                Text(title)
                    .textFont(tvForm ? 34 : metrics.wide ? 32 : 28, weight: .bold, relativeTo: .title)
                    .foregroundStyle(.white)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityAddTraits(.isHeader)
                    .accessibilityFocused($heard)
                Spacer(minLength: 0)
                if let link {
                    Button(link, action: onLink)
                        .buttonStyle(.textLink)
                        .textFont(17, weight: .semibold, relativeTo: .body)
                        .foregroundStyle(tvForm ? Theme.Palette.text : Theme.Palette.key)
                        .frame(minWidth: FirstRunRules.Size.link, minHeight: FirstRunRules.Size.link)
                        .contentShape(Rectangle())
                        .disabled(linkDisabled)
                }
            }
            if let line {
                Text(line)
                    .textFont(16, relativeTo: .body)
                    .foregroundStyle(Theme.Palette.muted)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .onAppear { if !tvForm { heard = true } }
    }
}

/// A section's heading inside a sheet.
struct SheetSection: View {
    let title: String

    var body: some View {
        Text(title)
            .textFont(17, weight: .bold, relativeTo: .headline)
            .foregroundStyle(.white)
            .fixedSize(horizontal: false, vertical: true)
            .accessibilityAddTraits(.isHeader)
    }
}

/// One answer of a radio group: its title, its line on wide forms, the radio at the end; chosen, ringed in white.
struct SheetOption: View {
    let title: String
    var line: String?
    let on: Bool
    let pick: () -> Void

    var body: some View {
        Button(action: pick) { Face(title: title, line: line, on: on) }
            .buttonStyle(LitRowStyle(radius: 16, rest: .white.opacity(on ? 0.14 : 0.06), hover: .white.opacity(on ? 0.2 : 0.1)))
            .accessibilityAddTraits(on ? .isSelected : [])
    }

    private struct Face: View {
        let title: String
        let line: String?
        let on: Bool
        @Environment(\.settingsLit) private var lit

        var body: some View {
            HStack(spacing: 14) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(title).textFont(16, weight: .semibold, relativeTo: .body)
                        .foregroundStyle(lit ? Theme.Palette.onPrimary : Theme.Palette.text)
                    if let line {
                        Text(line).textFont(13, relativeTo: .footnote)
                            .foregroundStyle(lit ? Color.black.opacity(0.6) : Color.white.opacity(0.64))
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
                Spacer(minLength: 0)
                Circle()
                    .strokeBorder(on ? (lit ? Color.black : .white) : Color.white.opacity(0.4), lineWidth: on ? 7 : 2)
                    .frame(width: 22, height: 22)
            }
            .multilineTextAlignment(.leading)
            .padding(.horizontal, 14)
            .padding(.vertical, 10)
            .frame(maxWidth: .infinity, minHeight: FirstRunRules.Size.option, alignment: .leading)
            .overlay {
                if on, !lit {
                    RoundedRectangle(cornerRadius: 16, style: .continuous).strokeBorder(.white.opacity(0.7), lineWidth: 1.5)
                }
            }
            .contentShape(Rectangle())
        }
    }
}

/// A chip of "Subtitles start in": 48 high, white when chosen.
struct SheetChip: View {
    let title: String
    var flag: String?
    var glyph: String?
    let on: Bool
    let pick: () -> Void

    var body: some View {
        Button(action: pick) { Face(title: title, flag: flag, glyph: glyph, on: on) }
            .buttonStyle(LitRowStyle(radius: FirstRunRules.Size.chip / 2, rest: on ? .white : .white.opacity(0.08), hover: on ? .white : .white.opacity(0.16)))
            .accessibilityAddTraits(on ? .isSelected : [])
    }

    private struct Face: View {
        let title: String
        let flag: String?
        let glyph: String?
        let on: Bool
        @Environment(\.settingsLit) private var lit

        var body: some View {
            HStack(spacing: 8) {
                if let flag {
                    Image(flag).resizable().frame(width: 24, height: 24).clipShape(Circle()).accessibilityHidden(true)
                } else if let glyph {
                    Glyph(glyph, size: 18).accessibilityHidden(true)
                }
                Text(title).textFont(15, weight: .semibold, relativeTo: .callout)
            }
            .foregroundStyle(on || lit ? Theme.Palette.onPrimary : Theme.Palette.text)
            .padding(.leading, flag == nil && glyph == nil ? 16 : 12)
            .padding(.trailing, 16)
            .frame(minHeight: FirstRunRules.Size.chip)
            .contentShape(Capsule())
        }
    }
}
