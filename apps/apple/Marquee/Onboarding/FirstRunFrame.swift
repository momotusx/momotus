import SwiftUI

/// A first-run question step (B4, the owner's pick of 2026-10-28, design/DECISION.md): the step's header, then a
/// film frame on black holding the still with the question set as its subtitle, and the choices in the bar under
/// it (`FilmLayout`). TV and wide screens keep the header and the bar on Settings' column; the phone's frame spans
/// the width, its lead line under the frame.
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
            let pad = metrics.wide ? SettingsRules.pad(width: page.size.width, gutter: metrics.gutter, column: column) : 16
            VStack(spacing: 0) {
                header.padding(.horizontal, pad).padding(.vertical, tvForm ? 0 : metrics.wide ? 20 : 8)
                GeometryReader { space in
                    ScrollView(.vertical) {
                        FilmLayout(room: space.size.height, floor: page.size.height * FilmRules.floor, edge: edge) {
                            FilmFrame(still: still, question: question, lead: metrics.wide ? lead : nil)
                                .accessibilityFocused($onQuestion)
                            VStack(spacing: metrics.wide ? 0 : 20) {
                                if !metrics.wide, let lead {
                                    Text(lead)
                                        .textFont(14, relativeTo: .callout)
                                        .foregroundStyle(Theme.Palette.dim)
                                        .multilineTextAlignment(.center)
                                        .fixedSize(horizontal: false, vertical: true)
                                        .padding(.horizontal, 8)
                                }
                                bar
                                    // The choices are one group, named by the question.
                                    .accessibilityElement(children: .contain)
                                    .accessibilityLabel(question)
                            }
                            .padding(.horizontal, pad)
                            .padding(.top, tvForm ? 32 : metrics.wide ? 28 : 16)
                        }
                    }
                    .scrollIndicators(.hidden)
                }
            }
        }
        .background(Color.black)
        // A step change puts a screen reader on the question; TV puts the remote on the first choice instead.
        .onAppear { if !tvForm { onQuestion = true } }
    }

    private var column: CGFloat { tvForm ? SettingsRules.Sizes.tv.column : SettingsRules.Sizes.wide.column }
    private var edge: CGFloat { tvForm ? 24 : metrics.wide ? 24 : 16 }
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

/// "Step N of 4" with its dots; on the phone it is read only.
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
                    .foregroundStyle(Theme.Palette.dim)
            }
        } else {
            Color.clear.frame(width: 1, height: 1)
                .accessibilityElement()
                .accessibilityLabel(words)
        }
    }
}

/// The frame at the size its layout gives it: the still fills it, faded in by opacity (at once under Reduce
/// Motion); with none it stays black. The question is its subtitle, one 60 percent box per line, and a real
/// heading; on TV and wide screens the lead line rides under it.
struct FilmFrame: View {
    let still: FirstRunStill?
    let question: String
    let lead: String?
    @State private var art: DecodedArt?
    @Environment(\.metrics) private var metrics
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        GeometryReader { box in
            ZStack(alignment: .bottom) {
                Color.black
                if let art {
                    FilledArt(art: art, size: box.size, y: 0.4).transition(.opacity)
                }
                VStack(spacing: metrics.wide ? min(10, box.size.width * 0.01) : 10) {
                    Cue(text: question, size: questionSize(box.size.width), weight: .semibold)
                        .accessibilityAddTraits(.isHeader)
                    if let lead {
                        Cue(text: lead, size: leadSize(box.size.width), weight: .medium)
                    }
                }
                .padding(.horizontal, metrics.wide ? box.size.width * 0.06 : 14)
                .padding(.bottom, box.size.height * (metrics.wide ? 0.09 : 0.12))
            }
        }
        .clipped()
        .task(id: still?.background) {
            let loaded = await ArtLoader.shared.image(still?.background, maxPixels: ArtSize.backdrop)
            withAnimation(reduceMotion ? nil : Theme.Motion.panel) { art = loaded }
        }
    }

    /// The question's type follows the frame on a small window, so it always fits inside the picture.
    private func questionSize(_ width: CGFloat) -> CGFloat {
        tvForm ? Theme.type.title3 : metrics.wide ? min(40, max(16, width * 0.052)) : 22
    }

    private func leadSize(_ width: CGFloat) -> CGFloat {
        tvForm ? Theme.type.headline : min(19, max(12, width * 0.024))
    }
}

/// A line set as a film's subtitle: white, the player's 60 percent scrim behind each line.
private struct Cue: View {
    let text: String
    let size: CGFloat
    let weight: Font.Weight

    var body: some View {
        // A thin space at each end gives the first and last box a little air.
        var line = AttributedString("\u{2009}\(text)\u{2009}")
        line.backgroundColor = Color.black.opacity(FilmRules.scrim)
        return Text(line)
            .textFont(size, weight: weight, relativeTo: .title3)
            .foregroundStyle(.white)
            .multilineTextAlignment(.center)
            .fixedSize(horizontal: false, vertical: true)
    }
}

/// A choice in the bar: a pill filled white at 12 percent, its icon in a circle, its title and line; white
/// under TV focus.
struct FilmChoice: View {
    let symbol: String
    let title: String
    let text: String
    @Environment(\.settingsLit) private var lit
    @Environment(\.metrics) private var metrics

    var body: some View {
        HStack(spacing: tvForm ? 24 : metrics.wide ? 16 : 12) {
            Circle()
                .fill(lit ? Color.black.opacity(0.08) : Color.white.opacity(0.12))
                .frame(width: icon, height: icon)
                .overlay { Glyph(symbol, size: icon * 0.46).foregroundStyle(lit ? Theme.Palette.onPrimary : .white) }
            VStack(alignment: .leading, spacing: 4) {
                Text(title)
                    .textFont(tvForm ? Theme.type.headline : metrics.wide ? 18 : 16, weight: .semibold, relativeTo: .headline)
                    .foregroundStyle(lit ? Theme.Palette.onPrimary : Theme.Palette.text)
                Text(text)
                    .textFont(tvForm ? Theme.type.caption : metrics.wide ? 13 : 12, relativeTo: .footnote)
                    .foregroundStyle(lit ? Color.black.opacity(0.64) : Theme.Palette.dim)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: 0)
        }
        .multilineTextAlignment(.leading)
        .padding(.leading, tvForm ? 20 : metrics.wide ? 20 : 14)
        .padding(.trailing, tvForm ? 36 : metrics.wide ? 22 : 18)
        .padding(.vertical, 12)
        .frame(maxWidth: .infinity, minHeight: tvForm ? 120 : metrics.wide ? 88 : 72, alignment: .leading)
        .frame(maxHeight: .infinity)
        .contentShape(Capsule())
    }

    private var icon: CGFloat { tvForm ? 76 : metrics.wide ? 48 : 44 }
}

extension ButtonStyle where Self == LitRowStyle {
    /// A first-run choice: white 12 percent at rest, 20 under the Mac's pointer, white under TV focus.
    static var filmChoice: LitRowStyle {
        LitRowStyle(radius: Theme.Radius.pill, rest: .white.opacity(0.12), hover: .white.opacity(0.2))
    }
}

/// The bar's choices: side by side across the column on TV and wide screens (one height), stacked on the phone
/// and wherever three would not fit side by side (the Mac's first-run sheet).
struct FilmChoices<Content: View>: View {
    @ViewBuilder let content: Content
    @Environment(\.metrics) private var metrics

    var body: some View {
        if metrics.wide {
            ViewThatFits(in: .horizontal) {
                HStack(alignment: .top, spacing: tvForm ? 24 : 16) {
                    Group(subviews: content) { choices in
                        // The ideal is the narrowest a choice may be, so three sit side by side wherever they fit.
                        ForEach(choices) { $0.frame(minWidth: tvForm ? 400 : 300, idealWidth: tvForm ? 400 : 300, maxWidth: tvForm ? 520 : 410) }
                    }
                }
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity)
                stacked.frame(maxWidth: tvForm ? 720 : 560).frame(maxWidth: .infinity)
            }
        } else {
            stacked
        }
    }

    private var stacked: some View {
        VStack(spacing: 10) { content }
    }
}
