import SwiftUI

extension View {
    /// The hero's box: `top` (the iPhone's large title) at its top edge and its words held to the
    /// foot of `height`. The two share one column, so words taller than the room left (a large text
    /// size, a landscape phone) grow the box instead of running under the title (HeroFrameTests).
    func heroFrame<Top: View>(height: CGFloat, alignment: Alignment, @ViewBuilder top: () -> Top) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            top()
            Spacer(minLength: 0)
            frame(maxWidth: .infinity, alignment: Alignment(horizontal: alignment.horizontal, vertical: .bottom))
        }
        .frame(maxWidth: .infinity, minHeight: height, alignment: alignment)
    }
}

/// The hero's ramps (BackdropStops), so every word and secondary reads on any picture. Wide, bottom
/// first as the frames layer them: the vignette, the 80 degree wash, the foot into the page, laid
/// out in the desktop frame's box off TV. The phone: the top wash and the foot held in points.
struct HeroScrims: View {
    let wide: Bool
    let page: Color
    #if os(tvOS)
    private let held = false
    #else
    private let held = true
    #endif

    var body: some View {
        GeometryReader { geo in
            if wide {
                let box = BackdropStops.rampBox(geo.size, held: held)
                ZStack {
                    BackdropStops.vignetteGradient
                    BackdropStops.wash(geo.size, box: box)
                    LinearGradient(stops: BackdropStops.gradient(BackdropStops.heroFoot(geo.size, box: box), page), startPoint: .bottom, endPoint: .top)
                }
            } else {
                ZStack {
                    LinearGradient(stops: BackdropStops.gradient(BackdropStops.heroPhoneTop, page), startPoint: .top, endPoint: .bottom)
                    BackdropStops.phoneFoot(geo.size, page)
                }
            }
        }
        .allowsHitTesting(false)
    }
}
