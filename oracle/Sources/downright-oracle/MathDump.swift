import AppKit
import CoreText
@testable import MarkdownRender
@testable import SwiftMath

/// The math layer: SwiftMath as Downright drives it.
///
///   downright-oracle math      <file.tex> <out.png>
///   downright-oracle math-tree <file.tex> <out.json>
///
/// A `.tex` input's first line is `inline` or `display`; everything after the
/// first newline is the LaTeX, verbatim. Both commands use the Paper Light
/// style sheet in the light appearance, the way the reader shows math:
///
/// - `inline` is `InlineMathDisplay`'s call: `mathPointSize`, no padding.
/// - `display` is `MathFragment`'s call: `mathPointSize * 1.12`, 8pt padding.
///
/// `math` writes the bitmap `drawNSImage` would draw for `MathRenderer.image`'s
/// result in a 2x context (`NSImage.cgImage(forProposedRect:context:hints:)`),
/// or a 1×1 magenta pixel when the renderer returns nil. `math-tree` dumps the
/// display tree `MTMathImage.asImage` draws, or the parse error.
enum MathDump {
    struct Input {
        let display: Bool
        let latex: String
    }

    static func read(_ url: URL) throws -> Input {
        let data = try Data(contentsOf: url)
        let bytes = [UInt8](data)
        let newline = bytes.firstIndex(of: 0x0A) ?? bytes.count
        let style = String(decoding: bytes[..<newline], as: UTF8.self)
        let rest = newline < bytes.count ? Array(bytes[(newline + 1)...]) : []
        let latex = String(decoding: rest, as: UTF8.self)
        switch style {
        case "inline": return Input(display: false, latex: latex)
        case "display": return Input(display: true, latex: latex)
        default: throw OracleError.usage("a .tex input starts with `inline` or `display`, not \(style)")
        }
    }

    /// Paper Light, light appearance — the style sheet's math point size and
    /// text colour, which are what `MathFragment` and `InlineMathDisplay` pass.
    static func parameters() throws -> (pointSize: CGFloat, color: NSColor) {
        guard let theme = ThemeStore.shared.themes.first(where: { $0.name == "Paper Light" }) else {
            throw OracleError.unknownTheme("Paper Light", ThemeStore.shared.themes.map(\.name))
        }
        let style = StyleSheet(theme: theme, appearance: NSAppearance(named: .aqua)!, reduceMotionOverride: true)
        return (style.mathPointSize, style.text)
    }

    static func request(_ input: Input) throws -> (pointSize: CGFloat, color: NSColor, padding: CGFloat) {
        let (base, color) = try parameters()
        return input.display ? (base * 1.12, color, 8) : (base, color, 0)
    }

    // MARK: - Image

    static func image(_ url: URL, to output: String) throws {
        let input = try read(url)
        let (pointSize, color, padding) = try request(input)
        let image = MathRenderer.image(
            latex: input.latex, display: input.display, pointSize: pointSize, color: color, padding: padding)
        let png = try image.map(rasterize) ?? sentinel()
        try png.write(to: URL(fileURLWithPath: output))
    }

    /// What `drawNSImage` hands to `CGContext.draw`: the image rasterised for
    /// a 2x context.
    static func rasterize(_ image: NSImage) throws -> Data {
        let scale: CGFloat = 2
        let width = max(1, Int((image.size.width * scale).rounded(.up)))
        let height = max(1, Int((image.size.height * scale).rounded(.up)))
        guard let context = CGContext(
            data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0,
            space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
        else { throw OracleError.usage("no bitmap context") }
        context.scaleBy(x: scale, y: scale)
        var proposed = CGRect(origin: .zero, size: image.size)
        let drawingContext = NSGraphicsContext(cgContext: context, flipped: true)
        guard let cgImage = image.cgImage(forProposedRect: &proposed, context: drawingContext, hints: nil) else {
            return try sentinel()
        }
        return try png(cgImage)
    }

    static func png(_ image: CGImage) throws -> Data {
        guard let data = NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:]) else {
            throw OracleError.usage("PNG encoding failed")
        }
        return data
    }

    /// A 1×1 opaque magenta pixel: "the renderer returned nil".
    static func sentinel() throws -> Data {
        let context = CGContext(
            data: nil, width: 1, height: 1, bitsPerComponent: 8, bytesPerRow: 0,
            space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
        context.setFillColor(CGColor(srgbRed: 1, green: 0, blue: 1, alpha: 1))
        context.fill(CGRect(x: 0, y: 0, width: 1, height: 1))
        return try png(context.makeImage()!)
    }

    // MARK: - Display tree

    static func tree(_ url: URL, to output: String) throws {
        let input = try read(url)
        let (pointSize, color, padding) = try request(input)
        try write(treeJSON(input, pointSize: pointSize, color: color, padding: padding), to: output)
    }

    /// `MathRenderer.image`'s steps up to the bitmap, stopping at the display
    /// list `MTMathImage.asImage` draws.
    static func treeJSON(_ input: Input, pointSize: CGFloat, color: NSColor, padding: CGFloat) -> JSON {
        let trimmed = input.latex.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return .object([("empty", .bool(true))]) }
        let fontSize = (pointSize * 4).rounded() / 4
        let source = swiftMathSource(from: trimmed)
        let renderer = MTMathImage(
            latex: source, fontSize: fontSize, textColor: color,
            labelMode: input.display ? .display : .text,
            textAlignment: input.display ? .center : .left)
        var header: [(String, JSON)] = [
            ("source", .string(source)),
            ("fontSize", .double(fontSize)),
            ("padding", .double((padding * 2).rounded())),
            ("style", .string(input.display ? "display" : "text")),
        ]
        var error: NSError?
        guard let mathList = MTMathListBuilder.build(fromString: source, error: &error), error == nil else {
            header.append(("error", errorJSON(error)))
            return .object(header)
        }
        header.append(("error", .null))
        header.append(("list", listJSON(mathList)))
        guard let displayList = MTTypesetter.createLineForMathList(mathList, font: renderer.font, style: renderer.currentStyle) else {
            header.append(("display", .null))
            return .object(header)
        }
        // MTMathImage.asImage's layout, verbatim.
        let size = CGSize(width: displayList.width, height: displayList.ascent + displayList.descent)
        displayList.textColor = color
        var textX = CGFloat(0)
        switch renderer.textAlignment {
        case .left: textX = 0
        case .center: textX = (size.width - displayList.width) / 2
        case .right: textX = size.width - displayList.width
        }
        var height = displayList.ascent + displayList.descent
        if height < fontSize / 2 { height = fontSize / 2 }
        let textY = (size.height - height) / 2 + displayList.descent
        displayList.position = CGPoint(x: textX, y: textY)
        header.append(("size", .array([.double(size.width), .double(size.height)])))
        header.append(("display", displayJSON(displayList)))
        return .object(header)
    }

    static func errorJSON(_ error: NSError?) -> JSON {
        guard let error else { return .object([("domain", .null)]) }
        return .object([
            ("domain", .string(error.domain)),
            ("code", .int(error.code)),
            ("message", .string(error.localizedDescription)),
        ])
    }

    // MARK: Math list

    static func listJSON(_ list: MTMathList?) -> JSON {
        guard let list else { return .null }
        return .array(list.atoms.map(atomJSON))
    }

    static func atomJSON(_ atom: MTMathAtom) -> JSON {
        var pairs: [(String, JSON)] = [
            ("type", .int(atom.type.rawValue)),
            ("class", .string(String(describing: Swift.type(of: atom)))),
            ("nucleus", .string(atom.nucleus)),
            ("range", .range(atom.indexRange)),
            ("fontStyle", .int(atom.fontStyle.rawValue)),
            ("superScript", listJSON(atom.superScript)),
            ("subScript", listJSON(atom.subScript)),
            ("fused", .int(atom.fusedAtoms.count)),
        ]
        switch atom {
        case let fraction as MTFraction:
            pairs += [
                ("hasRule", .bool(fraction.hasRule)),
                ("leftDelimiter", .string(fraction.leftDelimiter)),
                ("rightDelimiter", .string(fraction.rightDelimiter)),
                ("numerator", listJSON(fraction.numerator)),
                ("denominator", listJSON(fraction.denominator)),
            ]
        case let radical as MTRadical:
            pairs += [("radicand", listJSON(radical.radicand)), ("degree", listJSON(radical.degree))]
        case let op as MTLargeOperator:
            pairs += [("limits", .bool(op.limits))]
        case let inner as MTInner:
            pairs += [
                ("innerList", listJSON(inner.innerList)),
                ("leftBoundary", inner.leftBoundary.map(atomJSON) ?? .null),
                ("rightBoundary", inner.rightBoundary.map(atomJSON) ?? .null),
            ]
        case let over as MTOverLine:
            pairs += [("innerList", listJSON(over.innerList))]
        case let under as MTUnderLine:
            pairs += [("innerList", listJSON(under.innerList))]
        case let accent as MTAccent:
            pairs += [("innerList", listJSON(accent.innerList))]
        case let space as MTMathSpace:
            pairs += [("space", .double(space.space))]
        case let style as MTMathStyle:
            pairs += [("style", .int(style.style.rawValue))]
        case let color as MTMathColor:
            pairs += [("colorString", .string(color.colorString)), ("innerList", listJSON(color.innerList))]
        case let color as MTMathTextColor:
            pairs += [("colorString", .string(color.colorString)), ("innerList", listJSON(color.innerList))]
        case let box as MTMathColorbox:
            pairs += [("colorString", .string(box.colorString)), ("innerList", listJSON(box.innerList))]
        case let table as MTMathTable:
            pairs += [
                ("environment", .string(table.environment)),
                ("alignments", .array(table.alignments.map { alignment in
                    switch alignment {
                    case .left: return .string("left")
                    case .center: return .string("center")
                    case .right: return .string("right")
                    }
                })),
                ("interColumnSpacing", .double(table.interColumnSpacing)),
                ("interRowAdditionalSpacing", .double(table.interRowAdditionalSpacing)),
                ("cells", .array(table.cells.map { row in .array(row.map { listJSON($0) }) })),
            ]
        default:
            break
        }
        return .object(pairs)
    }

    // MARK: Displays

    static func point(_ point: CGPoint) -> JSON { .array([.double(point.x), .double(point.y)]) }

    static func ctFontJSON(_ font: CTFont?) -> JSON {
        guard let font else { return .null }
        return .object([
            ("name", .string(CTFontCopyPostScriptName(font) as String)),
            ("size", .double(CTFontGetSize(font))),
        ])
    }

    static func cgColorJSON(_ color: CGColor?) -> JSON {
        guard let color else { return .null }
        return .object([
            ("colorSpace", .string((color.colorSpace?.name as String?) ?? "")),
            ("components", .array((color.components ?? []).map { .double($0) })),
        ])
    }

    static func nsColorJSON(_ color: NSColor?) -> JSON {
        color.map(AttributeDump.colorJSON) ?? .null
    }

    static func displayJSON(_ display: MTDisplay?) -> JSON {
        guard let display else { return .null }
        var pairs: [(String, JSON)] = [
            ("class", .string(String(describing: Swift.type(of: display)))),
            ("position", point(display.position)),
            ("width", .double(display.width)),
            ("ascent", .double(display.ascent)),
            ("descent", .double(display.descent)),
            ("range", .range(display.range)),
            ("hasScript", .bool(display.hasScript)),
            ("textColor", nsColorJSON(display.textColor)),
            ("localTextColor", nsColorJSON(display.localTextColor)),
            ("localBackgroundColor", nsColorJSON(display.localBackgroundColor)),
        ]
        switch display {
        case let line as MTCTLineDisplay:
            pairs += [
                ("string", .string(line.attributedString?.string ?? "")),
                ("attributes", attributesJSON(line.attributedString)),
                ("runs", runsJSON(line.line)),
                ("atoms", .array(line.atoms.map { atom in
                    .object([
                        ("type", .int(atom.type.rawValue)),
                        ("nucleus", .string(atom.nucleus)),
                        ("range", .range(atom.indexRange)),
                    ])
                })),
            ]
        case let list as MTMathListDisplay:
            pairs += [
                ("linePosition", .int(list.type.rawValue)),
                ("index", .int(list.index)),
                ("subDisplays", .array(list.subDisplays.map(displayJSON))),
            ]
        case let fraction as MTFractionDisplay:
            pairs += [
                ("numerator", displayJSON(fraction.numerator)),
                ("denominator", displayJSON(fraction.denominator)),
                ("numeratorUp", .double(fraction.numeratorUp)),
                ("denominatorDown", .double(fraction.denominatorDown)),
                ("linePosition", .double(fraction.linePosition)),
                ("lineThickness", .double(fraction.lineThickness)),
            ]
        case let radical as MTRadicalDisplay:
            let mirror = Mirror(reflecting: radical)
            let glyph = mirror.children.first { $0.label == "_radicalGlyph" }?.value as? MTDisplay
            let shift = mirror.children.first { $0.label == "_radicalShift" }?.value as? CGFloat
            pairs += [
                ("radicand", displayJSON(radical.radicand)),
                ("degree", displayJSON(radical.degree)),
                ("radicalGlyph", displayJSON(glyph)),
                ("radicalShift", shift.map { .double($0) } ?? .null),
                ("topKern", .double(radical.topKern)),
                ("lineThickness", .double(radical.lineThickness)),
            ]
        case let glyph as MTGlyphDisplay:
            pairs += [
                ("glyph", .int(Int(glyph.glyph))),
                ("font", ctFontJSON(glyph.font?.ctFont)),
                ("shiftDown", .double(glyph.shiftDown)),
            ]
        case let construction as MTGlyphConstructionDisplay:
            pairs += [
                ("glyphs", .array(construction.glyphs.map { .int(Int($0)) })),
                ("positions", .array(construction.positions.map(point))),
                ("font", ctFontJSON(construction.font?.ctFont)),
                ("shiftDown", .double(construction.shiftDown)),
            ]
        case let limits as MTLargeOpLimitsDisplay:
            pairs += [
                ("nucleus", displayJSON(limits.nucleus)),
                ("upperLimit", displayJSON(limits.upperLimit)),
                ("lowerLimit", displayJSON(limits.lowerLimit)),
                ("limitShift", .double(limits.limitShift)),
                ("upperLimitGap", .double(limits.upperLimitGap)),
                ("lowerLimitGap", .double(limits.lowerLimitGap)),
                ("extraPadding", .double(limits.extraPadding)),
            ]
        case let line as MTLineDisplay:
            pairs += [
                ("inner", displayJSON(line.inner)),
                ("lineShiftUp", .double(line.lineShiftUp)),
                ("lineThickness", .double(line.lineThickness)),
            ]
        case let accent as MTAccentDisplay:
            pairs += [
                ("accentee", displayJSON(accent.accentee)),
                ("accent", displayJSON(accent.accent)),
            ]
        default:
            break
        }
        return .object(pairs)
    }

    /// Every attribute run of a line's attributed string: font, kern, colour.
    static func attributesJSON(_ string: NSAttributedString?) -> JSON {
        guard let string else { return .null }
        var runs: [JSON] = []
        string.enumerateAttributes(in: NSRange(location: 0, length: string.length), options: []) { attributes, range, _ in
            var pairs: [(String, JSON)] = [("range", .range(range))]
            for key in attributes.keys.map(\.rawValue).sorted() {
                let value = attributes[NSAttributedString.Key(key)]!
                if key == (kCTFontAttributeName as String) {
                    pairs.append((key, ctFontJSON((value as! CTFont))))
                } else if key == (kCTForegroundColorAttributeName as String) {
                    pairs.append((key, cgColorJSON((value as! CGColor))))
                } else if let number = value as? NSNumber {
                    pairs.append((key, AttributeDump.numberJSON(number)))
                } else {
                    pairs.append((key, .string(String(describing: value))))
                }
            }
            runs.append(.object(pairs))
        }
        return .array(runs)
    }

    /// The glyphs Core Text laid out for a line.
    static func runsJSON(_ line: CTLine?) -> JSON {
        guard let line else { return .null }
        let runs = CTLineGetGlyphRuns(line) as! [CTRun]
        return .array(runs.map { run in
            let count = CTRunGetGlyphCount(run)
            var glyphs = [CGGlyph](repeating: 0, count: count)
            var positions = [CGPoint](repeating: .zero, count: count)
            CTRunGetGlyphs(run, CFRange(location: 0, length: count), &glyphs)
            CTRunGetPositions(run, CFRange(location: 0, length: count), &positions)
            let range = CTRunGetStringRange(run)
            let attributes = CTRunGetAttributes(run) as NSDictionary
            let font = attributes[kCTFontAttributeName as String].map { $0 as! CTFont }
            return .object([
                ("stringRange", .array([.int(range.location), .int(range.length)])),
                ("font", ctFontJSON(font)),
                ("glyphs", .array(glyphs.map { .int(Int($0)) })),
                ("positions", .array(positions.map(point))),
            ])
        })
    }

    // MARK: - MathRenderer's private source rewrite (verbatim)

    /// `MathRenderer.swiftMathSource(from:)` is private, so the tree dump
    /// carries a copy; the `math` command goes through the real one.
    static func swiftMathSource(from latex: String) -> String {
        var output = ""
        var cursor = latex.startIndex

        while cursor < latex.endIndex {
            guard latex[cursor] == "\\" else {
                output.append(latex[cursor])
                cursor = latex.index(after: cursor)
                continue
            }

            let commandStart = cursor
            cursor = latex.index(after: cursor)
            while cursor < latex.endIndex, latex[cursor].isLetter {
                cursor = latex.index(after: cursor)
            }
            let command = String(latex[latex.index(after: commandStart)..<cursor])

            guard command == "mathop", cursor < latex.endIndex, latex[cursor] == "{",
                  let closingBrace = matchingBrace(in: latex, openingAt: cursor) else {
                output.append(contentsOf: latex[commandStart..<cursor])
                continue
            }

            let contentStart = latex.index(after: cursor)
            output.append(contentsOf: latex[contentStart..<closingBrace])
            cursor = latex.index(after: closingBrace)
        }

        return output
    }

    private static func matchingBrace(in source: String, openingAt opening: String.Index) -> String.Index? {
        var depth = 0
        var cursor = opening

        while cursor < source.endIndex {
            if source[cursor] == "\\" {
                cursor = source.index(after: cursor)
                if cursor < source.endIndex { cursor = source.index(after: cursor) }
                continue
            }
            if source[cursor] == "{" {
                depth += 1
            } else if source[cursor] == "}" {
                depth -= 1
                if depth == 0 { return cursor }
            }
            cursor = source.index(after: cursor)
        }

        return nil
    }
}
