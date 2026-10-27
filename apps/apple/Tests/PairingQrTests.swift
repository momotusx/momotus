import CoreImage
import Foundation
import Testing

// The TV's pairing QR (plans/tv-qr-login.md 5.5 and 7): drawn at whole pixels a
// module with a four-module quiet zone, and read back by a second decoder
// (Core Image's detector) after a blur and a half-size downscale, as a phone
// camera across a room sees it.

private let code = "https://momotus.normalisdead.mom/pair#v=3&s=https%3A%2F%2Fmomotus.normalisdead.mom"
    + "&p=0123456789abcdef0123456789abcdef&k=AbCdEfGhIjKlMnOpQrStUvWxYz0123456789-_AbCdE"
    + "&g=AbCdEfGhIjKlMnOpQrStUvWxYz0123456789-_AbCdE&n=Living%20room%20TV&f=tv"

@Suite struct PairingQrTests {
    @Test func theTvPairingCodeHasWholePixelModulesAndAQuietZone() throws {
        let image = try #require(QRCode.render(code, pairing: true))
        let modules = image.width
        // The Apple TV screen: 440 pt at 1x (1080p) and 2x (4K).
        for scale in [1.0, 2.0] {
            let side = QRCode.side(points: 440, scale: scale, modules: modules)
            let cell = side * scale / CGFloat(modules)
            #expect(cell == cell.rounded(), "scale \(scale): \(cell) px a module")
            #expect(side <= 440 && side > 440 - CGFloat(modules) / scale)
            #expect(cell >= 6, "scale \(scale): \(cell) px a module")
        }
        // Four white modules on every side.
        let pixels = try luma(image)
        for i in 0..<modules {
            for edge in 0..<4 {
                #expect(pixels[edge * modules + i] > 200 && pixels[(modules - 1 - edge) * modules + i] > 200)
                #expect(pixels[i * modules + edge] > 200 && pixels[i * modules + modules - 1 - edge] > 200)
            }
        }
    }

    @Test func theTvPairingCodeReadsBackBlurredAndAtHalfSize() throws {
        let image = try #require(QRCode.render(code, pairing: true))
        let cell = QRCode.side(points: 440, scale: 1, modules: image.width) / CGFloat(image.width)
        let drawn = CIImage(cgImage: image)
            .samplingNearest()
            .transformed(by: CGAffineTransform(scaleX: cell, y: cell))
        let seen = drawn
            .clampedToExtent()
            .applyingGaussianBlur(sigma: 1)
            .cropped(to: drawn.extent)
            .transformed(by: CGAffineTransform(scaleX: 0.5, y: 0.5))
        let detector = try #require(CIDetector(ofType: CIDetectorTypeQRCode, context: nil, options: [CIDetectorAccuracy: CIDetectorAccuracyHigh]))
        let read = detector.features(in: seen).compactMap { ($0 as? CIQRCodeFeature)?.messageString }
        #expect(read == [code])
        print("tv-qr apple: modules=\(image.width) cell=\(cell) pt at 440 pt (1x), \(cell / 2) px a module after halving")
    }
}

/// One byte of grey a pixel, row by row.
private func luma(_ image: CGImage) throws -> [UInt8] {
    var pixels = [UInt8](repeating: 0, count: image.width * image.height)
    let context = try #require(CGContext(
        data: &pixels, width: image.width, height: image.height, bitsPerComponent: 8, bytesPerRow: image.width,
        space: CGColorSpaceCreateDeviceGray(), bitmapInfo: CGImageAlphaInfo.none.rawValue
    ))
    context.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
    return pixels
}
