import AppKit
import MarkdownCore
import MarkdownRender

/// The canonical dump of a decorated `NSTextStorage`: every attribute run,
/// every key, every value in a representation both implementations can
/// produce. Floating-point values are compared bit-for-bit by the runner.
enum AttributeDump {
    static func storage(_ storage: NSAttributedString) -> JSON {
        var runs: [JSON] = []
        let full = NSRange(location: 0, length: storage.length)
        storage.enumerateAttributes(in: full, options: []) { attributes, range, _ in
            let pairs = attributes.keys.map(\.rawValue).sorted().map { key in
                (key, value(attributes[NSAttributedString.Key(key)]!))
            }
            runs.append(.object([
                ("range", .range(range)),
                ("attributes", .object(pairs)),
            ]))
        }
        return .object([
            ("length", .int(storage.length)),
            ("runs", .array(runs)),
        ])
    }

    static func value(_ value: Any) -> JSON {
        switch value {
        case let font as NSFont:
            return fontJSON(font)
        case let color as NSColor:
            return colorJSON(color)
        case let style as NSParagraphStyle:
            return paragraphStyle(style)
        case let payload as FragmentPayload:
            return fragmentPayload(payload)
        case let identity as BlockIdentity:
            return .object([
                ("type", .string("BlockIdentity")),
                ("kind", .int(identity.kind)),
                ("ordinal", .int(identity.ordinal)),
            ])
        case let token as PathToken:
            return .object([("type", .string("PathToken")), ("token", ParseDump.pathToken(token))])
        case let url as URL:
            return .object([("type", .string("URL")), ("absoluteString", .string(url.absoluteString))])
        case let shadow as NSShadow:
            return .object([
                ("type", .string("NSShadow")),
                ("offset", .array([.double(shadow.shadowOffset.width), .double(shadow.shadowOffset.height)])),
                ("blurRadius", .double(shadow.shadowBlurRadius)),
                ("color", shadow.shadowColor.map(colorJSON) ?? .null),
            ])
        case let attachment as NSTextAttachment:
            return .object([
                ("type", .string("NSTextAttachment")),
                ("class", .string(String(describing: Swift.type(of: attachment)))),
                ("bounds", rect(attachment.bounds)),
            ])
        case let number as NSNumber:
            return numberJSON(number)
        case let string as String:
            return .object([("type", .string("String")), ("value", .string(string))])
        default:
            return .object([
                ("type", .string("unknown")),
                ("class", .string(String(describing: Swift.type(of: value)))),
                ("description", .string(String(describing: value))),
            ])
        }
    }

    static func numberJSON(_ number: NSNumber) -> JSON {
        // CFBoolean is the only NSNumber whose type ID differs; distinguish it
        // so a `true` flag never compares equal to the integer 1.
        if CFGetTypeID(number) == CFBooleanGetTypeID() {
            return .object([("type", .string("Bool")), ("value", .bool(number.boolValue))])
        }
        switch CFNumberGetType(number) {
        case .float32Type, .float64Type, .floatType, .doubleType, .cgFloatType:
            return .object([("type", .string("Double")), ("value", .double(number.doubleValue))])
        default:
            return .object([("type", .string("Int")), ("value", .int(number.intValue))])
        }
    }

    static func fontJSON(_ font: NSFont) -> JSON {
        let descriptor = font.fontDescriptor
        var features: [JSON] = []
        if let settings = descriptor.object(forKey: .featureSettings) as? [[NSFontDescriptor.FeatureKey: Any]] {
            for setting in settings {
                let pairs = setting.keys.map(\.rawValue).sorted().map { key in
                    (key, value(setting[NSFontDescriptor.FeatureKey(key)]!))
                }
                features.append(.object(pairs))
            }
        }
        var variation: [JSON] = []
        if let axes = descriptor.object(forKey: .variation) as? [NSNumber: NSNumber] {
            for key in axes.keys.sorted(by: { $0.intValue < $1.intValue }) {
                variation.append(.array([.int(key.intValue), .double(axes[key]!.doubleValue)]))
            }
        }
        return .object([
            ("type", .string("NSFont")),
            ("postScriptName", .string(font.fontName)),
            ("familyName", .string(font.familyName)),
            ("pointSize", .double(font.pointSize)),
            ("symbolicTraits", .int(Int(descriptor.symbolicTraits.rawValue))),
            ("features", .array(features)),
            ("variation", .array(variation)),
            ("ascender", .double(font.ascender)),
            ("descender", .double(font.descender)),
            ("leading", .double(font.leading)),
        ])
    }

    static func colorJSON(_ color: NSColor) -> JSON {
        switch color.type {
        case .catalog:
            return .object([
                ("type", .string("NSColor")),
                ("colorType", .string("catalog")),
                ("catalog", .string(color.catalogNameComponent)),
                ("name", .string(color.colorNameComponent)),
            ])
        case .componentBased:
            let space = color.colorSpace
            var components = [CGFloat](repeating: 0, count: color.numberOfComponents)
            color.getComponents(&components)
            return .object([
                ("type", .string("NSColor")),
                ("colorType", .string("componentBased")),
                ("colorSpace", .string(space.localizedName ?? "\(space.colorSpaceModel.rawValue)")),
                ("components", .array(components.map { .double($0) })),
            ])
        case .pattern:
            return .object([("type", .string("NSColor")), ("colorType", .string("pattern"))])
        @unknown default:
            return .object([
                ("type", .string("NSColor")),
                ("colorType", .string("unknown")),
                ("description", .string(color.description)),
            ])
        }
    }

    static func paragraphStyle(_ style: NSParagraphStyle) -> JSON {
        .object([
            ("type", .string("NSParagraphStyle")),
            ("alignment", .int(style.alignment.rawValue)),
            ("firstLineHeadIndent", .double(style.firstLineHeadIndent)),
            ("headIndent", .double(style.headIndent)),
            ("tailIndent", .double(style.tailIndent)),
            ("lineBreakMode", .int(Int(style.lineBreakMode.rawValue))),
            ("maximumLineHeight", .double(style.maximumLineHeight)),
            ("minimumLineHeight", .double(style.minimumLineHeight)),
            ("lineSpacing", .double(style.lineSpacing)),
            ("paragraphSpacing", .double(style.paragraphSpacing)),
            ("paragraphSpacingBefore", .double(style.paragraphSpacingBefore)),
            ("baseWritingDirection", .int(style.baseWritingDirection.rawValue)),
            ("lineHeightMultiple", .double(style.lineHeightMultiple)),
            ("defaultTabInterval", .double(style.defaultTabInterval)),
            ("tabStops", .array(style.tabStops.map { tab in
                .array([.int(Int(tab.alignment.rawValue)), .double(tab.location)])
            })),
            ("hyphenationFactor", .double(Double(style.hyphenationFactor))),
            ("usesDefaultHyphenation", .bool(style.usesDefaultHyphenation)),
            ("tighteningFactorForTruncation", .double(Double(style.tighteningFactorForTruncation))),
            ("allowsDefaultTighteningForTruncation", .bool(style.allowsDefaultTighteningForTruncation)),
            ("lineBreakStrategy", .int(Int(style.lineBreakStrategy.rawValue))),
            ("headerLevel", .int(style.headerLevel)),
            ("textBlocks", .array(style.textBlocks.map { .string(String(describing: Swift.type(of: $0))) })),
            ("textLists", .array(style.textLists.map { .string($0.markerFormat.rawValue) })),
        ])
    }

    static func fragmentPayload(_ payload: FragmentPayload) -> JSON {
        .object([
            ("type", .string("FragmentPayload")),
            ("kind", .string(payload.kind.rawValue)),
            ("sourceRange", .range(payload.sourceRange)),
            ("blockIdentity", ParseDump.identity(payload.blockIdentity)),
            ("detail", .string(payload.detail)),
            ("hasTableData", .bool(payload.tableData != nil)),
            ("isCollapsed", .bool(payload.isCollapsed)),
        ])
    }

    static func rect(_ rect: NSRect) -> JSON {
        .array([
            .double(rect.origin.x), .double(rect.origin.y),
            .double(rect.size.width), .double(rect.size.height),
        ])
    }
}
