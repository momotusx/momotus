import ImageIO
import SwiftUI

/// A decoded image; CGImage is immutable, so sharing it across threads is safe.
final class DecodedArt: @unchecked Sendable {
    let image: CGImage
    init(_ image: CGImage) { self.image = image }
}

/// Artwork loading for every screen. Image CDNs stall now and then (first
/// QUIC connections to metahub time out after about 20 s), so requests get a
/// short timeout and two retries. Decoding and downsampling run off the main
/// thread; decoded images stay in memory and the bytes in URLCache on disk.
actor ArtLoader {
    static let shared = ArtLoader()

    private let session: URLSession = {
        let config = URLSessionConfiguration.default
        config.timeoutIntervalForRequest = 12
        config.urlCache = .shared
        config.requestCachePolicy = .returnCacheDataElseLoad
        return URLSession(configuration: config)
    }()
    private let memory: NSCache<NSString, DecodedArt> = {
        let cache = NSCache<NSString, DecodedArt>()
        cache.totalCostLimit = 192 << 20
        return cache
    }()
    private var inFlight: [String: Task<DecodedArt?, Never>] = [:]

    /// The image at `url`, its longest side at most `maxPixels`, or nil when it
    /// cannot be loaded.
    func image(_ url: URL, maxPixels: Int) async -> DecodedArt? {
        let key = "\(url.absoluteString)#\(maxPixels)"
        if let hit = memory.object(forKey: key as NSString) { return hit }
        if let running = inFlight[key] { return await running.value }
        let session = session
        let task = Task.detached(priority: .userInitiated) {
            await Self.fetch(url, maxPixels: maxPixels, session: session)
        }
        inFlight[key] = task
        let art = await task.value
        inFlight[key] = nil
        if let art {
            memory.setObject(art, forKey: key as NSString, cost: art.image.bytesPerRow * art.image.height)
        }
        return art
    }

    private static func fetch(_ url: URL, maxPixels: Int, session: URLSession) async -> DecodedArt? {
        for attempt in 0..<3 {
            if attempt > 0 { try? await Task.sleep(for: .milliseconds(500 * attempt)) }
            guard let (data, response) = try? await session.data(from: url) else { continue }
            guard (response as? HTTPURLResponse)?.statusCode == 200 else { return nil }
            return decode(data, maxPixels: maxPixels)
        }
        return nil
    }

    private static func decode(_ data: Data, maxPixels: Int) -> DecodedArt? {
        guard let source = CGImageSourceCreateWithData(data as CFData, nil) else { return nil }
        let options: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceShouldCacheImmediately: true,
            kCGImageSourceThumbnailMaxPixelSize: maxPixels,
        ]
        guard let thumb = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary) else { return nil }
        // Redraw into 32-bit BGRA: some art decodes to layouts the renderer
        // rejects (CVPixelBufferCreate -6680 on an RGBA backdrop left it blank).
        guard let space = CGColorSpace(name: CGColorSpace.sRGB),
              let context = CGContext(
                data: nil, width: thumb.width, height: thumb.height, bitsPerComponent: 8, bytesPerRow: 0, space: space,
                bitmapInfo: CGImageAlphaInfo.premultipliedFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue)
        else { return DecodedArt(thumb) }
        context.draw(thumb, in: CGRect(x: 0, y: 0, width: thumb.width, height: thumb.height))
        guard let drawn = context.makeImage() else { return DecodedArt(thumb) }
        let alpha: Set<CGImageAlphaInfo> = [.premultipliedFirst, .premultipliedLast, .first, .last]
        return DecodedArt(alpha.contains(thumb.alphaInfo) ? trimmed(drawn, context: context) : drawn)
    }

    /// Crops fully transparent edges. Logos often carry wide margins, which put
    /// them tens of points right of the text they sit over.
    static func trimmed(_ image: CGImage, context: CGContext) -> CGImage {
        guard let data = context.data else { return image }
        let (width, height, row) = (context.width, context.height, context.bytesPerRow)
        let pixels = data.bindMemory(to: UInt8.self, capacity: row * height)
        var (minX, minY, maxX, maxY) = (width, height, -1, -1)
        for y in 0..<height {
            let line = pixels + y * row
            // BGRA, alpha last; rows run top to bottom, as cropping counts them.
            for x in 0..<width where line[x * 4 + 3] > 8 {
                minX = min(minX, x)
                maxX = max(maxX, x)
                minY = min(minY, y)
                maxY = max(maxY, y)
            }
        }
        guard maxX >= minX, maxY >= minY, minX > 0 || minY > 0 || maxX < width - 1 || maxY < height - 1 else { return image }
        return image.cropping(to: CGRect(x: minX, y: minY, width: maxX - minX + 1, height: maxY - minY + 1)) ?? image
    }
}

/// How large to decode each kind of art (longest side, pixels).
enum ArtSize {
    static let poster = 720
    static let backdrop = 2400
    static let logo = 900
}
