import SwiftUI

/// Play or Resume with the resume point, always the title's primary (plans/play-every-version.md,
/// the shared table's title row): never Settings, an account step or Try again in its place. Where
/// nothing can play and the page holding the way on would hold only Back, Play stays but is inert.
struct PlayBlock: View {
    let target: PlayTarget?
    /// With no target, the day the next episode comes out.
    let nextAirs: String?
    /// With no target and no day: the page lists episodes (the next has no date).
    let hasEpisodes: Bool
    /// The title's runtime ("57 min"), so Resume says the time left.
    let runtime: String?
    let versions: VersionsSnapshot?
    let failure: String?
    let wide: Bool
    /// Plays (or opens Versions, as `TitlePlay` says); true starts over from the beginning.
    let play: (_ fromStart: Bool) -> Void
    /// TV: the page enters on this block's primary.
    var focus: FocusState<Bool>.Binding
    /// The v3 page (iOS and macOS): no note under the button (the page shows `VersionNote` under
    /// the hero), and on the iPhone the 178 pt pill with its time left under it.
    var v3 = false
    @Environment(\.dynamicTypeSize) private var typeSize

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            if target == nil {
                // Nothing to play yet: the day it comes, or why there is no Play,
                // never a Play that finds nothing.
                let airs = nextAirs.flatMap { nextAirsWords($0) }
                Label(airs ?? noPlayWords(hasEpisodes: hasEpisodes), glyph: airs == nil ? "circle-alert" : "calendar")
                    .textFont(Theme.type.headline, weight: .semibold, relativeTo: .headline)
                    .foregroundStyle(Theme.Palette.text)
            } else {
                // Start over beside Resume, under it at the accessibility sizes.
                let row = wide || !typeSize.isAccessibilitySize ? AnyLayout(HStackLayout(spacing: 10)) : AnyLayout(VStackLayout(spacing: 10))
                row {
                    Button { play(false) } label: {
                        HStack(spacing: 10) {
                            Glyph("play-fill")
                            Text(target?.label ?? "Play")
                            if let resume = target?.resumeMs, resume > 0, !(v3 && !wide) {
                                #if os(tvOS)
                                Text(resumeText(resumeMs: resume, leftMs: target?.leftMs, runtime: runtime)).fontWeight(.regular).opacity(0.8)
                                #else
                                // The frame's 13 pt sub in the pill: "Resume S6:E2  31 min left".
                                Text(resumeText(resumeMs: resume, leftMs: target?.leftMs, runtime: runtime))
                                    .textFont(13, relativeTo: .footnote).opacity(0.8)
                                #endif
                            }
                        }
                        .lineLimit(wide || v3 ? 1 : nil)
                        .frame(minWidth: wide ? 160 : nil, maxWidth: wide ? nil : .infinity)
                    }
                    .buttonStyle(PillButtonStyle(prominent: true))
                    // One line sized to its words where there is room for it (TV, Mac,
                    // iPad): the pill itself, not only its words, keeps its width, so
                    // "Resume 13 min left" never breaks or spills (the tvOS walk, ATV-F2).
                    .fixedSize(horizontal: wide, vertical: false)
                    .focused(focus)
                    // The reason shows right under it: dimmed, not focusable, does nothing.
                    .disabled(inert)
                    .accessibilityLabel(playWords)
                    #if !os(tvOS)
                    // Start over from Resume's menu and a named action; the Mac also has it in the
                    // Playback menu (plans/design-v3-phase3-apple.md 2.1).
                    .contextMenu {
                        if (target?.resumeMs ?? 0) > 0 {
                            Button { play(true) } label: { Label("Start Over", glyph: "rotate-ccw") }
                        }
                    }
                    .accessibilityActions {
                        if (target?.resumeMs ?? 0) > 0 { Button("Start over") { play(true) } }
                    }
                    #endif
                    #if os(tvOS)
                    if let resume = target?.resumeMs, resume > 0 {
                        Button { play(true) } label: {
                            Label("Start over", glyph: "rotate-ccw")
                                .frame(maxWidth: wide || !typeSize.isAccessibilitySize ? nil : .infinity)
                        }
                        .buttonStyle(PillButtonStyle())
                        .fixedSize(horizontal: wide, vertical: false)
                        .disabled(inert)
                    }
                    #endif
                }
            }
            #if os(tvOS)
            Group {
                if let failure {
                    Label("Versions could not load: \(failure)", glyph: "triangle-alert")
                        .foregroundStyle(Theme.Palette.muted)
                } else if target != nil {
                    VersionLine(versions: versions)
                }
            }
            // Wraps whole on a small phone or a large text size: the status and source decide the pick.
            .textFont(Theme.type.callout, relativeTo: .callout)
            .fixedSize(horizontal: false, vertical: true)
            #endif
        }
    }

    /// The versions could not load, or nothing here can play and the way on would hold only Back.
    private var inert: Bool { failure != nil || TitlePlay.of(versions) == .inert }

    /// "Resume Season 2, Episode 3, 36 min left": the code said as words.
    private var playWords: String {
        let label = spokenCodes(target?.label ?? "Play")
        guard let resume = target?.resumeMs, resume > 0 else { return label }
        return "\(label), \(resumeText(resumeMs: resume, leftMs: target?.leftMs, runtime: runtime))"
    }
}

/// The Mac hero's action row: the primary, Trailer, the marks capsule, Versions
/// (plans/design-v3-phase3-apple.md 2.1).
struct WideActions<Play: View>: View {
    let trailer: URL?
    let marks: [TitleMark]
    let toggle: (TitleMark) -> Void
    let versionsReady: Bool
    let showVersions: () -> Void
    @ViewBuilder let play: Play
    @Environment(\.openURL) private var openURL

    var body: some View {
        FlowRow(spacing: 12) {
            play
            if let trailer {
                Button { openURL(trailer) } label: { Label("Trailer", glyph: "film", size: HeroLayout.icon) }
                    .buttonStyle(PillButtonStyle())
            }
            MarkCapsule(marks: marks, toggle: toggle)
            Button(action: showVersions) { Label("Versions", glyph: "layers", size: HeroLayout.icon) }
                .buttonStyle(PillButtonStyle())
                .disabled(!versionsReady)
        }
    }
}
