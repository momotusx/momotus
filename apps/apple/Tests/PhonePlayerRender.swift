import AppKit
import ImageIO
import SwiftUI
import Testing
import UniformTypeIdentifiers

// The iPhone player's controls drawn from the PhoneFrame seam over a still,
// across and upright, to look at before a device walk: a sketch with the
// seam's numbers, not the app's own view (which needs a running player).
// Runs only with MARQUEE_RENDER_DIR set (a still.png in it; the PNGs land there).

@MainActor @Test func renderThePhonePlayerFrame() throws {
    guard let dir = ProcessInfo.processInfo.environment["MARQUEE_RENDER_DIR"] else { return }
    let still = NSImage(contentsOfFile: dir + "/still.png")
    let shapes: [(String, CGSize, EdgeInsets)] = [
        ("phone-landscape", CGSize(width: 874, height: 402), EdgeInsets(top: 0, leading: 62, bottom: 21, trailing: 62)),
        ("phone-portrait", CGSize(width: 402, height: 874), EdgeInsets(top: 62, leading: 0, bottom: 34, trailing: 0)),
    ]
    for (name, size, safe) in shapes {
        let renderer = ImageRenderer(content: PhoneSketch(size: size, safe: safe, still: still))
        renderer.scale = 2
        let image = try #require(renderer.cgImage)
        let url = URL(filePath: "\(dir)/\(name).png") as CFURL
        let destination = try #require(CGImageDestinationCreateWithURL(url, UTType.png.identifier as CFString, 1, nil))
        CGImageDestinationAddImage(destination, image, nil)
        #expect(CGImageDestinationFinalize(destination))
    }
}

private struct PhoneSketch: View {
    let size: CGSize
    let safe: EdgeInsets
    let still: NSImage?

    private var landscape: Bool { size.width > size.height }
    private var left: CGFloat { safe.leading + PhoneFrame.side(safe: safe.leading) }
    private var right: CGFloat { size.width - safe.trailing - PhoneFrame.side(safe: safe.trailing) }
    private var picture: CGSize {
        let width = min(size.width, size.height * 16 / 9)
        return CGSize(width: width, height: width * 9 / 16)
    }
    /// The scrubber's bar, 6 pt, then the compact actions row (36 pt), over the foot gap.
    private let actionsHeight: CGFloat = 36
    private var blockHeight: CGFloat { PhoneFrame.barHeight + 6 + actionsHeight + PhoneFrame.bottomGap }
    private var foot: CGFloat { size.height - safe.bottom - PhoneFrame.bottomGap }

    var body: some View {
        ZStack(alignment: .topLeading) {
            Color.black
            if let still {
                Image(nsImage: still).resizable()
                    .frame(width: picture.width, height: picture.height)
                    .position(x: size.width / 2, y: size.height / 2)
            }
            VStack(spacing: 0) {
                LinearGradient(colors: [.black.opacity(0.6), .clear], startPoint: .top, endPoint: .bottom).frame(height: 120)
                Spacer()
                LinearGradient(colors: [.clear, .black.opacity(0.72)], startPoint: .top, endPoint: .bottom).frame(height: 300)
            }
            .frame(width: size.width, height: size.height)
            subtitle
            topBar
            transport.position(x: size.width / 2, y: size.height / 2)
            scrubber
            actions
        }
        .frame(width: size.width, height: size.height)
        .foregroundStyle(.white)
        .environment(\.colorScheme, .dark)
    }

    private var topBar: some View {
        HStack(spacing: 10) {
            Circle().fill(.white.opacity(0.16)).frame(width: 40, height: 40)
                .overlay(Text("<").font(.system(size: 20, weight: .semibold)))
                .frame(width: 44, height: 44)
            Text("Sintel").font(.system(size: 22, weight: .bold))
            Spacer()
            Circle().fill(.white.opacity(0.16)).frame(width: 40, height: 40)
                .overlay(Text("PiP").font(.system(size: 11, weight: .semibold)))
            Text("Ends at 4:31 PM").font(.system(size: 15)).foregroundStyle(.white.opacity(0.8))
        }
        .frame(width: right - left)
        .position(x: (left + right) / 2, y: safe.top + PhoneFrame.topGap + 22)
    }

    private var transport: some View {
        HStack(spacing: PhoneFrame.transportGap(landscape: landscape)) {
            skip("-10")
            Circle().fill(.white).frame(width: 68, height: 68)
                .overlay(Text("II").font(.system(size: 22, weight: .heavy)).foregroundStyle(.black))
            skip("+10")
        }
    }

    private func skip(_ words: String) -> some View {
        Circle().fill(.white.opacity(0.16)).frame(width: 50, height: 50)
            .overlay(Text(words).font(.system(size: 13, weight: .bold)))
    }

    private var scrubber: some View {
        HStack(spacing: 8) {
            Text("1:50").font(.system(size: 12)).monospacedDigit()
            GeometryReader { geo in
                ZStack(alignment: .leading) {
                    Capsule().fill(.white.opacity(0.3))
                    Capsule().fill(.white).frame(width: geo.size.width * 0.13)
                }
            }
            .frame(height: 4)
            Text("-12:57").font(.system(size: 12)).monospacedDigit()
        }
        .padding(.horizontal, PhoneFrame.barInset)
        .frame(width: right - left, height: PhoneFrame.barHeight)
        .background(Capsule().fill(.white.opacity(0.14)))
        .position(x: (left + right) / 2, y: foot - actionsHeight - 6 - PhoneFrame.barHeight / 2)
    }

    private var actions: some View {
        HStack(spacing: 16) {
            action("Audio", "Default")
            action("Subtitles", "German")
            action("Versions", "720p")
            action("Speed", "1x")
        }
        .position(x: size.width / 2, y: foot - actionsHeight / 2)
    }

    private func action(_ title: String, _ value: String) -> some View {
        HStack(spacing: 4) {
            Text(title).font(.system(size: 13, weight: .semibold))
            Text(value).font(.system(size: 12)).foregroundStyle(.white.opacity(0.75))
        }
    }

    /// The Large subtitle where the app's rule puts it with the controls up,
    /// at the size it takes there (its box: two 25 pt lines and 6 pt above and below).
    private var subtitle: some View {
        let place = PhoneFrame.subtitle(
            screenHeight: size.height, pictureHeight: picture.height, safeBottom: safe.bottom, controlsShown: true,
            scrubberTop: size.height - safe.bottom - blockHeight, boxHeight: 2 * 25 * 1.2 + 12)
        return Text("Diese Klinge birgt eine\nfinstere Vergangenheit.")
            .font(.system(size: 25, weight: .semibold))
            .multilineTextAlignment(.center)
            .shadow(color: .black, radius: 0.7)
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
            .scaleEffect(place.scale, anchor: .bottom)
            .frame(width: size.width, height: place.foot, alignment: .bottom)
            .frame(width: size.width, height: size.height, alignment: .top)
    }
}

/// The app's own column on the device's containers, with a horizontal scroll
/// row in the block as the actions have and the places' scroll margin over
/// it as the shell sets it under the player: the scrubber's bar and the row
/// sit at the safe area's foot (the iPhone walk, round 3: the 66 pt bar
/// margin had lifted them to 61% of the height across).
@MainActor @Test func thePhoneScrubberSitsAtTheSafeFoot() {
    // iPhone 13 Pro: across 844 x 390 with a 21 pt foot, upright 390 x 844 with 47 and 34.
    for size in [CGSize(width: 844, height: 369), CGSize(width: 390, height: 763)] {
        let frames = Frames()
        let column = PhoneOSD {
            Color.clear.frame(height: 44)
        } bottom: {
            VStack(spacing: 6) {
                Capsule().frame(height: PhoneFrame.barHeight)
                    .onGeometryChange(for: CGRect.self) { $0.frame(in: .global) } action: { frames.bar = $0 }
                ScrollView(.horizontal) { Text("Audio Default    Subtitles German    Versions 720p").padding(.vertical, 6) }
                    .onGeometryChange(for: CGRect.self) { $0.frame(in: .global) } action: { frames.row = $0 }
            }
        }
        let shell = column
            .contentMargins(.bottom, TabBarMetrics.scrollMargin(immersive: true), for: .scrollContent)
            .frame(width: size.width, height: size.height)
        let host = NSHostingView(rootView: shell)
        host.frame = CGRect(origin: .zero, size: size)
        host.layoutSubtreeIfNeeded()
        let foot = size.height - PhoneFrame.bottomGap
        #expect(abs(frames.row.maxY - foot) < 0.5, "\(size): the row ends at \(frames.row.maxY), not \(foot)")
        #expect(frames.row.height < 60, "\(size): the row stretched to \(frames.row.height) pt")
        #expect(abs(frames.bar.maxY + 6 - frames.row.minY) < 0.5)
    }
}

@Test func thePlacesKeepTheBarsMarginButNotUnderThePlayer() {
    #expect(TabBarMetrics.scrollMargin(immersive: false) == TabBarMetrics.height)
    #expect(TabBarMetrics.scrollMargin(immersive: true) == 0)
}

@MainActor private final class Frames {
    var bar = CGRect.null
    var row = CGRect.null
}
