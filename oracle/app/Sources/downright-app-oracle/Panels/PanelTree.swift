import AppKit
import QuartzCore

/// The laid-out view tree of a panel: every view's class, geometry, text,
/// colours, accessibility and layers. Mirrored field for field by
/// `crates/conformance/src/dump/panel/tree.rs`; both sides read the same
/// AppKit properties in the same order.
enum PanelTree {
    static let rowLimit = 400

    static func rect(_ rect: NSRect) -> JSON {
        .array([.double(Double(rect.origin.x)), .double(Double(rect.origin.y)),
                .double(Double(rect.size.width)), .double(Double(rect.size.height))])
    }

    static func size(_ size: NSSize) -> JSON {
        .array([.double(Double(size.width)), .double(Double(size.height))])
    }

    static func point(_ point: CGPoint) -> JSON {
        .array([.double(Double(point.x)), .double(Double(point.y))])
    }

    /// The runtime class name, as `object_getClass` reports it (which is
    /// what the Rust side reads), except for Downright's own classes, whose
    /// unqualified Swift name is the Objective-C name the port registers.
    static func className(_ object: AnyObject) -> String {
        let runtime = NSStringFromClass(type(of: object))
        for module in ["DownrightApp", "MarkdownRender", "MarkdownCore", "downright_app_oracle"] where runtime.contains(module) {
            return String(describing: type(of: object))
        }
        return runtime
    }

    /// sRGB components, resolved in `view`'s effective appearance.
    static func color(_ color: NSColor?, in view: NSView) -> JSON {
        guard let color else { return .null }
        var result: JSON = .null
        view.effectiveAppearance.performAsCurrentDrawingAppearance {
            if let rgb = color.usingColorSpace(.sRGB) {
                result = .array([.double(Double(rgb.redComponent)), .double(Double(rgb.greenComponent)),
                                 .double(Double(rgb.blueComponent)), .double(Double(rgb.alphaComponent))])
            } else {
                result = .string("unconvertible")
            }
        }
        return result
    }

    static func cgColor(_ color: CGColor?) -> JSON {
        guard let color else { return .null }
        guard let space = CGColorSpace(name: CGColorSpace.sRGB),
              let converted = color.converted(to: space, intent: .defaultIntent, options: nil),
              let components = converted.components else { return .string("unconvertible") }
        return .array(components.map { .double(Double($0)) })
    }

    static func font(_ font: NSFont?) -> JSON {
        guard let font else { return .null }
        return .array([.string(font.fontName), .double(Double(font.pointSize))])
    }

    static func window(_ window: NSWindow) -> JSON {
        .object([
            ("class", .string(className(window))),
            ("frame", rect(window.frame)),
            ("contentLayoutRect", rect(window.contentLayoutRect)),
            ("styleMask", .int(Int(window.styleMask.rawValue))),
            ("title", .string(window.title)),
            ("opaque", .bool(window.isOpaque)),
            ("hasShadow", .bool(window.hasShadow)),
            ("level", .int(window.level.rawValue)),
        ])
    }

    static func layer(_ layer: CALayer, in view: NSView) -> JSON {
        var pairs: [(String, JSON)] = [
            ("class", .string(className(layer))),
            ("frame", rect(layer.frame)),
            ("hidden", .bool(layer.isHidden)),
            ("opacity", .double(Double(layer.opacity))),
            ("cornerRadius", .double(Double(layer.cornerRadius))),
            ("masksToBounds", .bool(layer.masksToBounds)),
            ("backgroundColor", cgColor(layer.backgroundColor)),
            ("borderWidth", .double(Double(layer.borderWidth))),
            ("borderColor", cgColor(layer.borderColor)),
            ("shadowOpacity", .double(Double(layer.shadowOpacity))),
            ("shadowRadius", .double(Double(layer.shadowRadius))),
            ("zPosition", .double(Double(layer.zPosition))),
            ("hasMask", .bool(layer.mask != nil)),
        ]
        let transform = layer.transform
        pairs.append(("transform", .array([
            .double(Double(transform.m11)), .double(Double(transform.m12)),
            .double(Double(transform.m21)), .double(Double(transform.m22)),
            .double(Double(transform.m41)), .double(Double(transform.m42)),
        ])))
        if let text = layer as? CATextLayer {
            pairs.append(("string", .string(text.string as? String)))
            pairs.append(("fontSize", .double(Double(text.fontSize))))
            pairs.append(("foregroundColor", cgColor(text.foregroundColor)))
        }
        if let shape = layer as? CAShapeLayer {
            pairs.append(("fillColor", cgColor(shape.fillColor)))
            pairs.append(("strokeColor", cgColor(shape.strokeColor)))
            pairs.append(("lineWidth", .double(Double(shape.lineWidth))))
            pairs.append(("strokeStart", .double(Double(shape.strokeStart))))
            pairs.append(("strokeEnd", .double(Double(shape.strokeEnd))))
            pairs.append(("pathBounds", shape.path.map { rect($0.boundingBoxOfPath) } ?? .null))
        }
        if let gradient = layer as? CAGradientLayer {
            let colors = (gradient.colors ?? []).map { cgColor(($0 as! CGColor)) }
            pairs.append(("colors", .array(colors)))
            pairs.append(("startPoint", point(gradient.startPoint)))
            pairs.append(("endPoint", point(gradient.endPoint)))
        }
        // Sublayers a view owns directly; the layers of subviews are dumped
        // with those subviews.
        let own = (layer.sublayers ?? []).filter { !($0.delegate is NSView) }
        pairs.append(("sublayers", .array(own.map { self.layer($0, in: view) })))
        return .object(pairs)
    }

    static func dump(_ view: NSView) -> JSON {
        var pairs: [(String, JSON)] = [
            ("class", .string(className(view))),
            ("frame", rect(view.frame)),
            ("bounds", rect(view.bounds)),
            ("hidden", .bool(view.isHidden)),
            ("alpha", .double(Double(view.alphaValue))),
            ("intrinsicContentSize", size(view.intrinsicContentSize)),
        ]
        if let field = view as? NSTextField {
            pairs.append(("text", .string(field.stringValue)))
            pairs.append(("font", font(field.font)))
            pairs.append(("textColor", color(field.textColor, in: view)))
            pairs.append(("alignment", .int(field.alignment.rawValue)))
            pairs.append(("lineBreakMode", .int(Int(field.lineBreakMode.rawValue))))
            pairs.append(("maximumNumberOfLines", .int(field.maximumNumberOfLines)))
            pairs.append(("editable", .bool(field.isEditable)))
            pairs.append(("placeholder", .string(field.placeholderString)))
        }
        if let button = view as? NSButton {
            pairs.append(("title", .string(button.title)))
            pairs.append(("state", .int(button.state.rawValue)))
            pairs.append(("font", font(button.font)))
            pairs.append(("hasImage", .bool(button.image != nil)))
            pairs.append(("contentTintColor", color(button.contentTintColor, in: view)))
            pairs.append(("bezelStyle", .int(Int(button.bezelStyle.rawValue))))
            pairs.append(("bordered", .bool(button.isBordered)))
            pairs.append(("keyEquivalent", .string(button.keyEquivalent)))
        }
        if let control = view as? NSControl {
            pairs.append(("enabled", .bool(control.isEnabled)))
        }
        if let imageView = view as? NSImageView {
            pairs.append(("hasImage", .bool(imageView.image != nil)))
            pairs.append(("contentTintColor", color(imageView.contentTintColor, in: view)))
        }
        if let textView = view as? NSTextView {
            pairs.append(("string", .string(textView.string)))
            pairs.append(("font", font(textView.font)))
            pairs.append(("textColor", color(textView.textColor, in: view)))
            pairs.append(("backgroundColor", color(textView.backgroundColor, in: view)))
        }
        if let table = view as? NSTableView {
            pairs.append(("rows", .int(table.numberOfRows)))
            pairs.append(("selectedRow", .int(table.selectedRow)))
            let count = min(table.numberOfRows, rowLimit)
            pairs.append(("rowRects", .array((0..<count).map { rect(table.rect(ofRow: $0)) })))
        }
        if let stack = view as? NSStackView {
            pairs.append(("spacing", .double(Double(stack.spacing))))
            pairs.append(("orientation", .int(stack.orientation.rawValue)))
        }
        if let scroll = view as? NSScrollView {
            pairs.append(("documentVisibleRect", rect(scroll.documentVisibleRect)))
        }
        pairs.append(("toolTip", .string(view.toolTip)))
        pairs.append(("axRole", .string(view.accessibilityRole()?.rawValue)))
        pairs.append(("axLabel", .string(view.accessibilityLabel())))
        pairs.append(("axValue", .string(view.accessibilityValue() as? String)))
        pairs.append(("axHelp", .string(view.accessibilityHelp())))
        pairs.append(("layer", view.layer.map { layer($0, in: view) } ?? .null))
        pairs.append(("subviews", .array(view.subviews.map(dump))))
        return .object(pairs)
    }
}
