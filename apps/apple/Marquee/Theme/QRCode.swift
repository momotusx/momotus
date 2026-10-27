import CoreImage
import CoreImage.CIFilterBuiltins
import SwiftUI

/// A QR code drawn locally from text, crisp at any size.
struct QRCode: View {
    let text: String
    /// A pairing code (plans/tv-qr-login.md 5.5): error correction L and a
    /// four-module white quiet zone, read from across a room, with its spoken name.
    var pairing = false

    @Environment(\.displayScale) private var displayScale

    var body: some View {
        if let image = Self.render(text, pairing: pairing) {
            if pairing {
                // Every module a whole number of pixels, centred on white: no edge blurs (plans/tv-qr-login.md 5.5).
                GeometryReader { box in
                    let side = Self.side(points: min(box.size.width, box.size.height), scale: displayScale, modules: image.width)
                    Image(decorative: image, scale: 1)
                        .interpolation(.none)
                        .resizable()
                        .frame(width: side, height: side)
                        .frame(width: box.size.width, height: box.size.height)
                }
                .background(Color.white)
                .accessibilityElement()
                .accessibilityLabel(AccountWords.shared.tvQrLabel)
            } else {
                Image(decorative: image, scale: 1)
                    .interpolation(.none)
                    .resizable()
                    .scaledToFit()
                    .padding(10)
                    .background(RoundedRectangle(cornerRadius: Theme.Radius.s).fill(.white))
                    .accessibilityLabel("QR code")
            }
        }
    }

    /// The largest side, in points, at most `points`, at which each of `modules` is a whole number of pixels.
    static func side(points: CGFloat, scale: CGFloat, modules: Int) -> CGFloat {
        let cell = max(1, (points * scale / CGFloat(modules)).rounded(.down))
        return cell * CGFloat(modules) / scale
    }

    static func render(_ text: String, pairing: Bool = false) -> CGImage? {
        let filter = CIFilter.qrCodeGenerator()
        filter.message = Data(text.utf8)
        filter.correctionLevel = pairing ? "L" : "M"
        guard var output = filter.outputImage else { return nil }
        if pairing {
            // The generator leaves one module around the code: three more make the four a dark screen needs.
            let zone = output.extent.insetBy(dx: -3, dy: -3)
            output = output.composited(over: CIImage(color: .white).cropped(to: zone))
        }
        return CIContext().createCGImage(output, from: output.extent)
    }
}
