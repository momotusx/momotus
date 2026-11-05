import SwiftUI

/// A first-run question step (V1, Full Bleed, the owner's pick of 2026-10-30, design/exploration/onboarding-v2,
/// design/DECISION.md): the still fills the screen behind the step's header, the question, its lead line and the
/// choices. The phone keeps the block at the foot on a ramp anchored to it, in thumb reach; TV and wide screens
/// keep it in a column at the lower leading corner. A block taller than the screen scrolls over the still.
struct FramedStep<Bar: View>: View {
    let header: StepHeader
    let still: FirstRunStill?
    let question: String
    let lead: String?
    @ViewBuilder let bar: Bar
    @Environment(\.metrics) private var metrics
    @AccessibilityFocusState private var onQuestion: Bool

    var body: some View {
        GeometryReader { page in
            ScrollView(.vertical) {
                VStack(alignment: .leading, spacing: 0) {
                    header
                        .padding(.horizontal, metrics.wide ? (tvForm ? 0 : 56) : 12)
                        .frame(minHeight: metrics.wide && !tvForm ? 104 : nil)
                        // Over the phone's ramp where a tall block reaches it.
                        .zIndex(1)
                    Spacer(minLength: Theme.Space.m)
                    if metrics.wide { column } else { foot }
                }
                .frame(minHeight: page.size.height)
            }
            .scrollIndicators(.hidden)
            .background { FilmStill(still: still, wide: metrics.wide, insets: page.safeAreaInsets) }
        }
        .background(Color.black)
        // A step change puts a screen reader on the question; TV puts the remote on the first choice instead.
        .onAppear { if !tvForm { onQuestion = true } }
    }

    /// The phone's block at the foot, on its ramp: clear over it, black from just under the question's top line.
    private var foot: some View {
        block
            .padding(.horizontal, 16)
            .padding(.bottom, 34)
            .background(alignment: .top) {
                GeometryReader { ramp in
                    LinearGradient(stops: BackdropStops.gradient(FilmRules.ramp(height: ramp.size.height), .black),
                                   startPoint: .top, endPoint: .bottom)
                }
                .padding(.top, -FilmRules.above)
                .allowsHitTesting(false)
                .accessibilityHidden(true)
            }
    }

    private var column: some View {
        block
            .frame(maxWidth: FilmRules.column(tv: tvForm), alignment: .leading)
            .padding(.leading, tvForm ? 0 : FilmRules.inset)
            .padding(.trailing, tvForm ? 0 : 56)
            .padding(.bottom, tvForm ? 0 : 84)
    }

    private var block: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(question)
                .textFont(questionSize, weight: .bold, relativeTo: .largeTitle)
                .foregroundStyle(.white)
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityAddTraits(.isHeader)
                .accessibilityFocused($onQuestion)
            if let lead {
                Text(lead)
                    .textFont(leadSize, relativeTo: .body)
                    .foregroundStyle(Theme.Palette.muted)
                    .fixedSize(horizontal: false, vertical: true)
                    .padding(.top, metrics.wide ? 14 : 10)
            }
            bar
                // The choices are one group, named by the question.
                .accessibilityElement(children: .contain)
                .accessibilityLabel(question)
                .padding(.top, metrics.wide ? 34 : 22)
        }
        .multilineTextAlignment(.leading)
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private var questionSize: CGFloat { tvForm ? Theme.type.hero : metrics.wide ? 56 : 34 }
    private var leadSize: CGFloat { tvForm ? Theme.type.headline : metrics.wide ? 19 : 15 }
}

/// The still behind the whole screen, decorative, under its washes: faded in by opacity (at once under Reduce
/// Motion); with none the screen stays black. Wide screens take Apple's wash (from the leading edge, the foot and
/// under the header); the phone a light wash under the header and black under the home indicator, where the
/// block's own ramp ends.
struct FilmStill: View {
    let still: FirstRunStill?
    let wide: Bool
    let insets: EdgeInsets
    @State private var art: DecodedArt?
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    /// The Mac's sign-in stage draws it too (SignInScreen).
    init(still: FirstRunStill?, wide: Bool, insets: EdgeInsets) {
        self.still = still
        self.wide = wide
        self.insets = insets
    }

    var body: some View {
        GeometryReader { box in
            ZStack(alignment: .top) {
                Color.black
                if let art {
                    FilledArt(art: art, size: box.size, y: wide ? 0.4 : 0.5).transition(.opacity)
                }
                if wide {
                    BackdropStops.wash(box.size, box: box.size, stops: FilmRules.side(width: box.size.width, height: box.size.height, tv: tvForm))
                    LinearGradient(stops: BackdropStops.gradient(FilmRules.foot, .black), startPoint: .bottom, endPoint: .top)
                    LinearGradient(stops: BackdropStops.gradient(FilmRules.top, .black), startPoint: .top, endPoint: .bottom)
                } else {
                    LinearGradient(stops: BackdropStops.gradient(FilmRules.phoneTop, .black), startPoint: .top, endPoint: .bottom)
                        .frame(height: FilmRules.phoneTopReach)
                }
                if !wide {
                    Color.black
                        .frame(height: insets.bottom)
                        .frame(maxHeight: .infinity, alignment: .bottom)
                }
            }
        }
        .ignoresSafeArea()
        .allowsHitTesting(false)
        .accessibilityHidden(true)
        .task(id: still?.background) {
            let loaded = await ArtLoader.shared.image(still?.background, maxPixels: ArtSize.backdrop)
            withAnimation(reduceMotion ? nil : Theme.Motion.panel) { art = loaded }
        }
    }
}

/// A first-run step's header: the mark and name at the left; Back (from step 2), "Step N of 4" with its dots
/// and the step's skip at the right. The phone reads the count without showing it, and keeps Back and Skip at
/// 48 pt targets.
struct StepHeader: View {
    let count: Int?
    var back: (() -> Void)?
    var skip: String?
    var skipDisabled = false
    var onSkip: () -> Void = {}
    @Environment(\.metrics) private var metrics

    var body: some View {
        HStack(alignment: .center, spacing: metrics.wide ? (tvForm ? 32 : 16) : 8) {
            Wordmark(size: tvForm ? 30 : 22)
            Spacer(minLength: 0)
            if let back, FilmRules.hasBack(count) {
                Button(action: back) { Label("Back", glyph: "chevron-left", size: tvForm ? 26 : 16) }
                    .modifier(HeaderLink(wide: metrics.wide))
            }
            if let count { StepCount(count: count, shown: metrics.wide) }
            if let skip {
                Button(action: onSkip) {
                    HStack(spacing: 4) {
                        Text(skip)
                        Glyph("chevron-right", size: tvForm ? 26 : 16)
                    }
                }
                .modifier(HeaderLink(wide: metrics.wide))
                .disabled(skipDisabled)
            }
        }
    }
}

/// Back and Skip: the header's text links.
private struct HeaderLink: ViewModifier {
    let wide: Bool

    func body(content: Content) -> some View {
        content
            .labelStyle(PillLabelStyle())
            .buttonStyle(.textLink)
            .textFont(Theme.type.callout, weight: .semibold, relativeTo: .callout)
            .foregroundStyle(Theme.Palette.text)
            .frame(minWidth: wide ? nil : 48, minHeight: wide ? nil : 48)
            .contentShape(Rectangle())
    }
}

/// "Step N of 4" with its dots; on the phone it is read only. Its words are white over the art, never dim, so
/// they keep 4.5:1 over the header's wash.
private struct StepCount: View {
    let count: Int
    let shown: Bool

    var body: some View {
        let words = "Step \(count) of \(FilmRules.steps)"
        if shown {
            HStack(spacing: 14) {
                HStack(spacing: 8) {
                    ForEach(1...FilmRules.steps, id: \.self) { i in
                        Capsule()
                            .fill(.white.opacity(i == count ? 1 : 0.4))
                            .frame(width: i == count ? (tvForm ? 28 : 22) : (tvForm ? 10 : 8), height: tvForm ? 10 : 8)
                    }
                }
                .accessibilityHidden(true)
                Text(words)
                    .textFont(tvForm ? Theme.type.caption : 15, weight: .semibold, relativeTo: .caption)
                    .foregroundStyle(Theme.Palette.text)
            }
        } else {
            Color.clear.frame(width: 1, height: 1)
                .accessibilityElement()
                .accessibilityLabel(words)
        }
    }
}

/// A choice: a pill filled white at 12 percent, its icon in a circle, its title and line, a chevron at the end;
/// white under TV focus.
struct FilmChoice: View {
    let symbol: String
    let title: String
    let text: String
    @Environment(\.settingsLit) private var lit
    @Environment(\.metrics) private var metrics

    var body: some View {
        HStack(spacing: tvForm ? 22 : metrics.wide ? 16 : 12) {
            Circle()
                .fill(lit ? Color.black.opacity(0.08) : Color.white.opacity(0.12))
                .frame(width: icon, height: icon)
                .overlay { Glyph(symbol, size: icon * 0.46).foregroundStyle(lit ? Theme.Palette.onPrimary : .white) }
            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                    .textFont(tvForm ? Theme.type.headline : metrics.wide ? 19 : 16, weight: .semibold, relativeTo: .headline)
                    .foregroundStyle(lit ? Theme.Palette.onPrimary : Theme.Palette.text)
                Text(text)
                    .textFont(tvForm ? Theme.type.caption : metrics.wide ? 15 : 13, relativeTo: .footnote)
                    .foregroundStyle(lit ? Color.black.opacity(0.64) : Theme.Palette.muted)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: 0)
            Glyph("chevron-right", size: tvForm ? 26 : 18)
                .foregroundStyle(lit ? Color.black.opacity(0.4) : Color.white.opacity(0.4))
        }
        .multilineTextAlignment(.leading)
        .padding(.leading, tvForm ? 22 : metrics.wide ? 18 : 12)
        .padding(.trailing, tvForm ? 32 : metrics.wide ? 24 : 14)
        .padding(.vertical, tvForm ? 20 : metrics.wide ? 16 : 12)
        .frame(maxWidth: .infinity, minHeight: tvForm ? 120 : metrics.wide ? 92 : 72, alignment: .leading)
        .contentShape(RoundedRectangle(cornerRadius: Self.radius(wide: metrics.wide), style: .continuous))
    }

    private var icon: CGFloat { tvForm ? 68 : metrics.wide ? 52 : 44 }

    static func radius(wide: Bool) -> CGFloat { tvForm ? 60 : wide ? 46 : 26 }
}

extension ButtonStyle where Self == LitRowStyle {
    /// A first-run choice: white 12 percent at rest, 20 under the Mac's pointer, white under TV focus.
    static func filmChoice(wide: Bool) -> LitRowStyle {
        LitRowStyle(radius: FilmChoice.radius(wide: wide), rest: .white.opacity(0.12), hover: .white.opacity(0.2))
    }

    /// A subtitle language: white 12 percent at rest, 24 when chosen, white under TV focus.
    static func filmLanguage(on: Bool) -> LitRowStyle {
        LitRowStyle(radius: tvForm ? 22 : 16, rest: .white.opacity(on ? 0.24 : 0.12), hover: .white.opacity(on ? 0.28 : 0.2))
    }
}

/// The choices, stacked on every form.
struct FilmChoices<Content: View>: View {
    @ViewBuilder let content: Content
    @Environment(\.metrics) private var metrics

    var body: some View {
        VStack(spacing: metrics.wide ? 12 : 10) { content }
    }
}

/// A subtitle language in the step's two columns: its name, a line when it has one, a check when chosen.
struct FilmLanguage: View {
    let name: String
    let line: String?
    let on: Bool
    @Environment(\.settingsLit) private var lit

    var body: some View {
        HStack(spacing: 8) {
            VStack(alignment: .leading, spacing: 2) {
                Text(name).textFont(tvForm ? Theme.type.headline : 16, weight: .semibold, relativeTo: .headline)
                if let line {
                    Text(line)
                        .textFont(tvForm ? Theme.type.caption : 13, relativeTo: .footnote)
                        .foregroundStyle(lit ? Color.black.opacity(0.64) : Theme.Palette.muted)
                }
            }
            Spacer(minLength: 0)
            if on { Glyph("check", size: tvForm ? 26 : 18) }
        }
        .multilineTextAlignment(.leading)
        .foregroundStyle(lit ? Theme.Palette.onPrimary : Theme.Palette.text)
        .padding(.horizontal, 14)
        .padding(.vertical, 8)
        .frame(maxWidth: .infinity, minHeight: tvForm ? 72 : 52, alignment: .leading)
        .contentShape(Rectangle())
    }
}
