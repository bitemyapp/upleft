import AppKit
import QuickLookThumbnailing
@testable import DownrightThumb

/// Swift side of the `quicklook-thumbnail` suite: the Quick Look thumbnail
/// extension's `ThumbnailProvider`, asked for a thumbnail of a corpus file
/// and drawn the way Quick Look's thumbnail host draws a current-context
/// reply. Windowless; nothing is registered with Quick Look.
///
///   downright-app-oracle quicklook-thumbnail <file.md> <out.png>
///       [--width W] [--height H] [--scale S] [--layout out.json]
///
/// The request is `OracleThumbnailRequest`, a `QLFileThumbnailRequest` that
/// answers the file URL and `maximumSize` W×H (default 256×256) at scale S
/// (default 2). The reply's context size and drawing block are read through
/// QLThumbnailReply's own getters (`contextSize`, `drawingBlock`). The block
/// draws into an sRGB, premultiplied-RGBA bitmap of ceil(size × S) pixels,
/// scaled by S, with an unflipped `NSGraphicsContext` current. The PNG is
/// that bitmap (a blank 1×1 when there is no reply, or no pixels to draw
/// into, in which case the block is not called); the layout JSON records the
/// handler's error, the context size, the block's result and the bitmap's
/// pixel size. Rust:
/// `crates/conformance/src/dump/quicklook_thumbnail.rs`.
enum QuickLookThumbnailDump {
    static func run(input: URL, output: String, flags: [String]) throws {
        var width: CGFloat = 256
        var height: CGFloat = 256
        var scale: CGFloat = 2
        var layout: URL?
        var index = 0
        while index < flags.count {
            guard index + 1 < flags.count else { throw AppOracleError(description: "\(flags[index]) needs a value") }
            let value = flags[index + 1]
            switch flags[index] {
            case "--width": width = CGFloat(Double(value) ?? 256)
            case "--height": height = CGFloat(Double(value) ?? 256)
            case "--scale": scale = CGFloat(Double(value) ?? 2)
            case "--layout": layout = URL(fileURLWithPath: value)
            default: throw AppOracleError(description: "unknown flag \(flags[index])")
            }
            index += 2
        }

        let provider = ThumbnailProvider()
        let request = OracleThumbnailRequest(url: input, maximumSize: CGSize(width: width, height: height), scale: scale)
        var result: (QLThumbnailReply?, Error?)?
        provider.provideThumbnail(for: request) { reply, error in result = (reply, error) }
        guard let (reply, error) = result else {
            throw AppOracleError(description: "the thumbnail handler was not called before provideThumbnail returned")
        }

        var fields: [(String, JSON)] = []
        if let error = error as NSError? {
            fields.append(("error", .object([("domain", .string(error.domain)), ("code", .int(error.code))])))
        } else {
            fields.append(("error", .null))
        }
        var png = try blankPNG()
        if let reply {
            let contextSize = (reply.value(forKey: "contextSize") as! NSValue).sizeValue
            fields.append(("contextSize", .array([.double(Double(contextSize.width)), .double(Double(contextSize.height))])))
            guard let blockObject = reply.value(forKey: "drawingBlock") else {
                throw AppOracleError(description: "the reply has no current-context drawing block")
            }
            typealias DrawingBlock = @convention(block) () -> Bool
            let block = unsafeBitCast(blockObject as AnyObject, to: DrawingBlock.self)
            let pixelWidth = Int((contextSize.width * scale).rounded(.up))
            let pixelHeight = Int((contextSize.height * scale).rounded(.up))
            if pixelWidth > 0 && pixelHeight > 0 {
                let (drew, _, data) = try render(size: contextSize, scale: scale, draw: block)
                fields.append(("drew", .bool(drew)))
                png = data
            } else {
                // No bitmap to draw into: the block is not called.
                fields.append(("drew", .null))
            }
            fields.append(("pixelSize", .array([.int(pixelWidth), .int(pixelHeight)])))
        } else {
            fields.append(("contextSize", .null))
        }
        try png.write(to: URL(fileURLWithPath: output))
        if let layout {
            try JSON.object(fields).text.write(to: layout, atomically: true, encoding: .utf8)
        }
    }

    /// Draws `draw` as Quick Look draws a current-context reply.
    static func render(size: CGSize, scale: CGFloat, draw: () -> Bool) throws -> (Bool, (Int, Int), Data) {
        let width = Int((size.width * scale).rounded(.up))
        let height = Int((size.height * scale).rounded(.up))
        guard let space = CGColorSpace(name: CGColorSpace.sRGB),
              let context = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0,
                                      space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else {
            throw AppOracleError(description: "cannot create a \(width)×\(height) bitmap context")
        }
        context.scaleBy(x: scale, y: scale)
        let previous = NSGraphicsContext.current
        NSGraphicsContext.current = NSGraphicsContext(cgContext: context, flipped: false)
        let drew = draw()
        NSGraphicsContext.current = previous
        guard let image = context.makeImage(),
              let data = NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:]) else {
            throw AppOracleError(description: "PNG encoding failed")
        }
        return (drew, (width, height), data)
    }

    static func blankPNG() throws -> Data {
        try render(size: CGSize(width: 1, height: 1), scale: 1, draw: { true }).2
    }
}

/// A `QLFileThumbnailRequest` for the oracle: Quick Look creates the real
/// ones itself.
final class OracleThumbnailRequest: QLFileThumbnailRequest {
    private let url: URL
    private let size: CGSize
    private let requestScale: CGFloat

    init(url: URL, maximumSize: CGSize, scale: CGFloat) {
        self.url = url
        size = maximumSize
        requestScale = scale
        super.init()
    }

    required init?(coder: NSCoder) { nil }

    override var fileURL: URL { url }
    override var maximumSize: CGSize { size }
    override var minimumSize: CGSize { .zero }
    override var scale: CGFloat { requestScale }
}
