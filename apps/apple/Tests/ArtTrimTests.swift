import CoreGraphics
import Foundation
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

// App Transport Security refuses plain HTTP to a public host, so art an addon gives as http:// is asked over https://
// (OpenSubtitles' logo is http://www.strem.io/...); the core's loopback logos stay as they are.
@Test func plainHTTPArtIsAskedOverHTTPSExceptOnLoopback() throws {
    let secure = { (s: String) in ArtLoader.secure(URL(string: s)!).absoluteString }
    #expect(secure("http://www.strem.io/images/addons/opensubtitles-logo.png") == "https://www.strem.io/images/addons/opensubtitles-logo.png")
    #expect(secure("HTTP://s.ytimg.com/vi/x/hq.jpg?a=1") == "https://s.ytimg.com/vi/x/hq.jpg?a=1")
    #expect(secure("https://image.tmdb.org/t/p/w1280/a.jpg") == "https://image.tmdb.org/t/p/w1280/a.jpg")
    for local in ["http://127.0.0.1:7781/logo/0123abcd/torrentio", "http://localhost:9/a.png", "http://[::1]:9/a.png"] {
        #expect(secure(local) == local)
    }
}
