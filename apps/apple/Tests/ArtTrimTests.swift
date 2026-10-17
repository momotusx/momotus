import CoreGraphics
import Testing

// Logos come with transparent margins (metahub's Breaking Bad logo starts 170
// px into an 800 px canvas); trimming them puts the logo on the text column.

private func context(width: Int, height: Int) throws -> CGContext {
    try #require(CGContext(
        data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0,
        space: CGColorSpace(name: CGColorSpace.sRGB)!,
        bitmapInfo: CGImageAlphaInfo.premultipliedFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue))
}

@Test func transparentMarginsAreCroppedToTheDrawnPixels() throws {
    let ctx = try context(width: 40, height: 20)
    ctx.setFillColor(CGColor(red: 0.2, green: 0.5, blue: 0.3, alpha: 1))
    // Drawing space has its origin bottom left: this block is 4 px from the top.
    ctx.fill(CGRect(x: 10, y: 6, width: 12, height: 10))
    let image = try #require(ctx.makeImage())
    let trimmed = ArtLoader.trimmed(image, context: ctx)
    #expect((trimmed.width, trimmed.height) == (12, 10))
}

@Test func anImageWithoutMarginsStaysWhole() throws {
    let ctx = try context(width: 8, height: 8)
    ctx.setFillColor(CGColor(red: 1, green: 1, blue: 1, alpha: 1))
    ctx.fill(CGRect(x: 0, y: 0, width: 8, height: 8))
    let image = try #require(ctx.makeImage())
    #expect(ArtLoader.trimmed(image, context: ctx) === image)
}

@Test func aFullyTransparentImageStaysAsItIs() throws {
    let ctx = try context(width: 8, height: 8)
    let image = try #require(ctx.makeImage())
    #expect(ArtLoader.trimmed(image, context: ctx) === image)
}
