import AppKit
import MarkdownCore
@testable import MarkdownRender

// Dumps for the render foundation: the resolved `StyleSheet` (`stylesheet`),
// the built-in syntax highlighter (`highlight`), and the VS Code theme
// importer plus the theme decoder (`vscode-theme`). `upleft-oracle` writes the
// same shapes from `crates/conformance/src/dump/{style_sheet,highlight,
// vscode_theme}.rs`; field names and order must match.

enum ThemeDump {
    static func theme(_ theme: Theme) -> JSON {
        .object([
            ("name", .string(theme.name)),
            ("appearance", .string(theme.appearance.rawValue)),
            ("palette", palette(theme.palette)),
            ("code", code(theme.code)),
            ("typography", typography(theme.typography)),
        ])
    }

    static func palette(_ p: ThemePalette) -> JSON {
        .object([
            ("background", .string(p.background.raw)),
            ("surface", .string(p.surface.raw)),
            ("text", .string(p.text.raw)),
            ("textSecondary", .string(p.textSecondary.raw)),
            ("textFaint", .string(p.textFaint.raw)),
            ("heading", .string(p.heading.raw)),
            ("marker", .string(p.marker.raw)),
            ("accent", .string(p.accent.raw)),
            ("link", .string(p.link.raw)),
            ("rule", .string(p.rule.raw)),
            ("selection", .string(p.selection.raw)),
            ("codeBackground", .string(p.codeBackground.raw)),
            ("inlineCodeBackground", .string(p.inlineCodeBackground.raw)),
            ("codeRule", .string(p.codeRule.raw)),
            ("railTick", .string(p.railTick.raw)),
            ("railTickCurrent", .string(p.railTickCurrent.raw)),
            ("quoteRule", .string(p.quoteRule.raw)),
            ("changeAdded", .string(p.changeAdded.raw)),
            ("changeRemoved", .string(p.changeRemoved.raw)),
            ("changeModified", .string(p.changeModified.raw)),
            ("pathMissing", .string(p.pathMissing.raw)),
            ("searchHit", .string(p.searchHit.raw)),
            ("searchHitCurrent", .string(p.searchHitCurrent.raw)),
            ("calloutNote", .string(p.calloutNote.raw)),
            ("calloutWarning", .string(p.calloutWarning.raw)),
            ("calloutSuccess", .string(p.calloutSuccess.raw)),
            ("calloutDanger", .string(p.calloutDanger.raw)),
            ("calloutImportant", .string(p.calloutImportant?.raw)),
        ])
    }

    static func code(_ c: CodeTheme) -> JSON {
        .object([
            ("keyword", .string(c.keyword.raw)),
            ("string", .string(c.string.raw)),
            ("number", .string(c.number.raw)),
            ("comment", .string(c.comment.raw)),
            ("type", .string(c.type.raw)),
            ("function", .string(c.function.raw)),
            ("variable", .string(c.variable.raw)),
            ("constant", .string(c.constant.raw)),
            ("operator", .string(c.operator.raw)),
            ("punctuation", .string(c.punctuation.raw)),
            ("attribute", .string(c.attribute.raw)),
            ("diffAdded", .string(c.diffAdded.raw)),
            ("diffRemoved", .string(c.diffRemoved.raw)),
            ("diffHeader", .string(c.diffHeader.raw)),
        ])
    }

    static func typography(_ t: TypographyConfig) -> JSON {
        .object([
            ("preset", .string(t.preset.rawValue)),
            ("bodySize", .double(t.bodySize)),
            ("scaleRatio", .double(t.scaleRatio)),
            ("lineHeightMultiple", .double(t.lineHeightMultiple)),
            ("measureCharacters", .double(t.measureCharacters)),
            ("monoFamily", .string(t.monoFamily)),
            ("monoSizeAdjust", .double(t.monoSizeAdjust)),
            ("monoLigatures", .bool(t.monoLigatures)),
            ("opticalMargins", .bool(t.opticalMargins)),
            ("mathScale", .double(t.mathScale)),
        ])
    }

    /// `validated()` for every colour, `invalidColorPaths()`, and the
    /// contrast audit against both appearances and the theme's own.
    static func validation(_ theme: Theme) -> JSON {
        let failures = { (appearance: NSAppearance?) -> JSON in
            .array(theme.semanticContrastFailures(appearance: appearance).map { failure in
                .object([
                    ("path", .string(failure.path)),
                    ("ratio", .double(failure.ratio)),
                    ("minimum", .double(failure.minimum)),
                ])
            })
        }
        return .object([
            ("allColors", .array(theme.allColors().map { entry in
                .object([
                    ("path", .string(entry.path)),
                    ("raw", .string(entry.color.raw)),
                    ("validated", entry.color.validated().map(AttributeDump.colorJSON) ?? .null),
                ])
            })),
            ("invalidColorPaths", .array(theme.invalidColorPaths().map { .string($0) })),
            ("contrastDefault", failures(nil)),
            ("contrastAqua", failures(NSAppearance(named: .aqua)!)),
            ("contrastDarkAqua", failures(NSAppearance(named: .darkAqua)!)),
        ])
    }
}

enum StyleSheetDump {
    /// Colour strings covering every branch of `NSColor(hexString:)` and
    /// `ThemeColor.resolved()`/`validated()`.
    static let colorProbes = [
        "#11223344", "#112233", "112233", "  #aBcDeF\t", "#+12345", "#-00000", "#-00001", "#ggg", "#fff",
        "#12345", "#1234567", "#123456789", "", "#", "system:label", "system:labelColor", "system:accent",
        "system:controlAccentColor", "system:notAColour", "system:", "System:label", "system:systemGray",
        "system:underPageBackground", "#\u{301}112233", "\u{200B}#112233\u{200B}", "#１２３４５６",
    ]

    static func dump(themeName: String, dark: Bool) throws -> JSON {
        let appearance = NSAppearance(named: dark ? .darkAqua : .aqua)!
        guard let theme = ThemeStore.shared.themes.first(where: { $0.name == themeName }) else {
            throw OracleError.unknownTheme(themeName, ThemeStore.shared.themes.map(\.name))
        }
        let sheet = StyleSheet(theme: theme, appearance: appearance, reduceMotionOverride: true)

        var variants: [JSON] = []
        for (label, edit) in typographyVariants {
            var modified = theme
            edit(&modified.typography)
            let variant = StyleSheet(theme: modified, appearance: appearance, reduceMotionOverride: false)
            variants.append(.object([
                ("label", .string(label)),
                ("typography", ThemeDump.typography(modified.typography)),
                ("fonts", fonts(variant)),
                ("metrics", metrics(variant)),
            ]))
        }

        return .object([
            ("theme", ThemeDump.theme(theme)),
            ("revision", .int(sheet.revision)),
            ("appearance", .string(sheet.appearance.name.rawValue)),
            ("accessibility", .object([
                ("reduceMotion", .bool(sheet.reduceMotion)),
                ("increaseContrast", .bool(sheet.increaseContrast)),
                ("reduceTransparency", .bool(sheet.reduceTransparency)),
            ])),
            ("metrics", metrics(sheet)),
            ("fonts", fonts(sheet)),
            ("colors", colors(sheet)),
            ("typographyVariants", .array(variants)),
            ("validation", ThemeDump.validation(theme)),
            ("colorProbes", .array(colorProbes.map { raw in
                let color = ThemeColor(raw)
                return .object([
                    ("raw", .string(raw)),
                    ("validated", color.validated().map(AttributeDump.colorJSON) ?? .null),
                    ("resolved", AttributeDump.colorJSON(color.resolved())),
                    ("snapshot", AttributeDump.colorJSON(ColorResolver(appearance: appearance).resolve(color))),
                ])
            })),
            ("renderMetrics", renderMetrics(bodySize: theme.typography.bodySize, grid: sheet.baselineGrid)),
            ("themeStore", .object([
                ("themes", .array(ThemeStore.shared.themes.map { .string($0.name) })),
                ("current", .string(ThemeStore.shared.current.name)),
                ("revision", .int(ThemeStore.shared.revision)),
                ("slugs", .array([theme.name, "Été à Paris", "  --a__b--  ", "", "日本語テーマ", "x\u{301}y"].map {
                    .string(ThemeStore.slug($0))
                })),
            ])),
            ("fallback", ThemeDump.theme(.fallback)),
            ("contracts", contracts()),
        ])
    }

    static let typographyVariants: [(String, (inout TypographyConfig) -> Void)] = [
        ("working", { $0.preset = .working }),
        ("menlo-ligatures", { $0.monoFamily = "Menlo"; $0.monoLigatures = true }),
        ("missing-face", { $0.monoFamily = "This Face Does Not Exist" }),
        ("empty-face", { $0.monoFamily = "" }),
        ("wide-measure", { $0.measureCharacters = 400; $0.bodySize = 19; $0.lineHeightMultiple = 1.3 }),
        ("narrow-measure", { $0.measureCharacters = 10; $0.bodySize = 13.5; $0.scaleRatio = 1.333 }),
        ("tiny", { $0.bodySize = 3; $0.lineHeightMultiple = 1; $0.mathScale = 1.2 }),
    ]

    static func metrics(_ sheet: StyleSheet) -> JSON {
        .object([
            ("baselineGrid", .double(sheet.baselineGrid)),
            ("lineHeight", .double(sheet.lineHeight)),
            ("averageCharacterWidth", .double(sheet.averageCharacterWidth)),
            ("measureWidth", .double(sheet.measureWidth)),
            ("mathPointSize", .double(sheet.mathPointSize)),
            ("headingSpacing", .array((0...7).map { level in
                let spacing = sheet.headingSpacing(level: level)
                return .array([.double(spacing.before), .double(spacing.after)])
            })),
            ("headingSizes", .array((0...7).map { level in
                .double(StyleSheet.headingSize(level: level, typography: sheet.theme.typography))
            })),
        ])
    }

    static func fonts(_ sheet: StyleSheet) -> JSON {
        let font = AttributeDump.fontJSON
        let mono = sheet.monoFont()
        let sizes: [CGFloat?] = [nil, 9, 11, 12.5, 13, 14, mono.pointSize, 20]
        return .object([
            ("body", font(sheet.bodyFont())),
            ("headings", .array((0...7).map { font(sheet.headingFont(level: $0)) })),
            ("mono", font(mono)),
            ("monoSized", .array(sizes.map { font(sheet.monoFont(size: $0)) })),
            ("monoAttributes", .array(sizes.map { size in
                let attributes = sheet.monoFontAttributes(size: size)
                return .object([
                    ("font", font(attributes[.font] as! NSFont)),
                    ("ligature", .int(attributes[.ligature] as! Int)),
                ])
            })),
            ("emphasis", .array([(false, false), (true, false), (false, true), (true, true)].map {
                font(sheet.emphasisFont(bold: $0.0, italic: $0.1))
            })),
        ])
    }

    static func colors(_ sheet: StyleSheet) -> JSON {
        let color = AttributeDump.colorJSON
        let named: [(String, NSColor)] = [
            ("background", sheet.background), ("surface", sheet.surface), ("text", sheet.text),
            ("textSecondary", sheet.textSecondary), ("textFaint", sheet.textFaint), ("marker", sheet.marker),
            ("accent", sheet.accent), ("link", sheet.link), ("rule", sheet.rule),
            ("codeBackground", sheet.codeBackground), ("inlineCodeBackground", sheet.inlineCodeBackground),
            ("codeRule", sheet.codeRule), ("railTick", sheet.railTick), ("railTickCurrent", sheet.railTickCurrent),
            ("quoteRule", sheet.quoteRule), ("pathMissing", sheet.pathMissing), ("searchHit", sheet.searchHit),
            ("searchHitCurrent", sheet.searchHitCurrent), ("selection", sheet.selection),
        ]
        let luminance = { (value: NSColor) -> JSON in .double(StyleSheet.relativeLuminance(value)) }
        return .object([
            ("palette", .object(named.map { ($0.0, color($0.1)) })),
            ("luminance", .object(named.map { ($0.0, luminance($0.1)) })),
            ("headings", .array((0...7).map { color(sheet.headingColor(level: $0)) })),
            ("callouts", .array(CalloutKind.allCases.map { kind in
                .object([
                    ("kind", .string(kind.rawValue)),
                    ("color", color(sheet.calloutColor(kind))),
                    ("symbol", .string(sheet.calloutSymbol(kind))),
                ])
            })),
            ("changes", .array([ChangeKind.inserted, .deleted, .modified].map { kind in
                .object([("kind", .string(kind.rawValue)), ("color", color(sheet.changeColor(kind)))])
            })),
            ("code", .array(SyntaxToken.allCases.map { token in
                .object([("token", .string(token.rawValue)), ("color", color(sheet.codeColor(token)))])
            })),
            ("startWindowPrimaryAction", color(sheet.startWindowPrimaryAction)),
            ("onAccent", color(sheet.onAccent)),
            ("taskFieldColor", color(sheet.taskFieldColor)),
            ("taskRingChecked", color(sheet.taskRingColor(checked: true))),
            ("taskRingOpen", color(sheet.taskRingColor(checked: false))),
            ("taskTickColor", color(sheet.taskTickColor)),
            ("panelAlpha", .array([(0.12, false), (0.12, true), (0.7, true), (1.0, false)].map {
                color(sheet.accent.panelAlpha($0.0, increaseContrast: $0.1))
            })),
            ("contrast", .array([
                StyleSheet.contrastRatio(sheet.text, sheet.background),
                StyleSheet.contrastRatio(.white, sheet.startWindowPrimaryAction),
                StyleSheet.contrastRatio(sheet.accent, sheet.surface),
                StyleSheet.contrastRatio(.labelColor, sheet.background),
            ].map { .double($0) })),
            ("blend", .array([-0.5, 0, 0.25, 1.0 / 3.0, 0.5, 1, 2].map { t in
                color(ColorResolver.blend(sheet.text, sheet.accent, t))
            })),
            ("blendCatalog", color(ColorResolver.blend(.labelColor, sheet.background, 0.5))),
        ])
    }

    static func renderMetrics(bodySize: CGFloat, grid: CGFloat) -> JSON {
        let values: [CGFloat] = [0, 0.4, 1, 12.5, 13, 25.99, 26, 26.01, 39, -7.5]
        return .object([
            ("constants", .array([
                RenderMetrics.gutterWidth, RenderMetrics.revealSlack, RenderMetrics.verticalInset,
                RenderMetrics.codeBleed, RenderMetrics.minimumProseWidth, RenderMetrics.codeInsetX,
                RenderMetrics.codeInsetY, RenderMetrics.codeHeaderHeight, RenderMetrics.codeRuleWidth,
                RenderMetrics.codeCornerRadius, RenderMetrics.inlineCodeCornerRadius, RenderMetrics.codeBlockGap,
                RenderMetrics.taskBoxSide, RenderMetrics.taskBoxGap, RenderMetrics.taskBoxClearance,
                RenderMetrics.taskMarkerColumn, RenderMetrics.taskBoxCornerRatio, RenderMetrics.taskBoxStrokeRatio,
                RenderMetrics.taskTickStrokeRatio, RenderMetrics.chipHeight, RenderMetrics.calloutRuleWidth,
                RenderMetrics.calloutInsetX, RenderMetrics.calloutIconInsetX, RenderMetrics.calloutInsetY,
                RenderMetrics.calloutCornerRadius, RenderMetrics.quoteRuleWidth, RenderMetrics.tableRowPadding,
                RenderMetrics.tableColumnGap, RenderMetrics.tableRuleWidth, RenderMetrics.imageCornerRadius,
                RenderMetrics.imageShadowRadius, RenderMetrics.imageCaptionGap, RenderMetrics.thematicBreakSpace,
            ].map { .double($0) })),
            ("integers", .array([RenderMetrics.codeTabColumns, RenderMetrics.codeCollapseLineCount].map { .int($0) })),
            ("taskTick", .array(RenderMetrics.taskTick.map { .array([.double($0.x), .double($0.y)]) })),
            ("indentUnit", .array([bodySize, 16, 13.5, 11].map { .double(RenderMetrics.indentUnit(bodySize: $0)) })),
            ("snapUp", .array(values.map { .double(RenderMetrics.snap($0, grid: grid)) })),
            ("snapDown", .array(values.map { .double(RenderMetrics.snap($0, grid: grid, rounding: .down)) })),
            ("snapSmallGrid", .array(values.map { .double(RenderMetrics.snap($0, grid: 0.5)) })),
        ])
    }

    static func contracts() -> JSON {
        let policy = { (p: DecorationPolicy) -> JSON in
            .array([
                p.showsInsertionPoint, p.hidesBlockMarkers, p.hidesInlineMarkers, p.revealsAtCaret,
                p.revealsAtAllCursors, p.showsGutterMarkers, p.highlightsMarkers, p.rendersFragments,
                p.collapsesLongCodeBlocks,
            ].map { .bool($0) })
        }
        var configuration = MarkdownRenderConfiguration(codeCollapseThreshold: 0, largeFileThresholdMegabytes: 5000)
        let clampedInit = [configuration.codeCollapseThreshold, configuration.largeFileThresholdMegabytes]
        configuration.codeCollapseThreshold = 20_000
        configuration.largeFileThresholdMegabytes = -3
        let defaults = MarkdownRenderConfiguration()
        return .object([
            ("modes", .array(RenderMode.allCases.map { mode in
                .object([
                    ("raw", .string(mode.rawValue)),
                    ("title", .string(mode.title)),
                    ("normalized", .string(mode.normalizedForEditing.rawValue)),
                    ("policy", policy(mode.policy)),
                ])
            })),
            ("userFacingModes", .array(RenderMode.userFacingModes.map { .string($0.rawValue) })),
            ("fragmentKinds", .array([
                FragmentKind.codeBlock, .collapsedCodeBlock, .table, .inlineMath, .blockMath, .mermaid, .image,
                .thematicBreak, .frontMatter, .callout, .listOrnament,
            ].map { .array([.string($0.rawValue), .bool($0.replacesGlyphs)]) })),
            ("revealPolicies", .array(MarkdownRevealPolicy.allCases.map { .string($0.rawValue) })),
            ("configuration", .array([
                .bool(defaults.showInvisibles), .string(defaults.revealPolicy.rawValue),
                .bool(defaults.typographicSubstitution), .bool(defaults.typewriterScrolling),
                .bool(defaults.reflowHardWrappedParagraphs), .int(defaults.codeCollapseThreshold),
                .int(defaults.largeFileThresholdMegabytes), .int(clampedInit[0]), .int(clampedInit[1]),
                .int(configuration.codeCollapseThreshold), .int(configuration.largeFileThresholdMegabytes),
            ])),
            ("attributeKeys", .array([
                NSAttributedString.Key.drHidden, .drMarker, .drFragment, .drBlock, .drHeading, .drLink,
                .drPathToken, .drPathExists, .drCheckbox, .drChange, .drChangeGhost, .drReference, .drElided,
                .drGutterMarker, .drSearchHit, .drCurrentSearchHit, .drSpeechHighlight, .drInlineCode,
                .drSourceFocus, .drInvisible,
            ].map { .string($0.rawValue) })),
            ("previewAppearances", .array(PreviewAppearance.allCases.map { appearance in
                .array([
                    .string(appearance.rawValue), .string(appearance.title),
                    .string(appearance.nsAppearance?.name.rawValue),
                ])
            })),
            ("gutterChrome", .array([AttributeDump.fontJSON(GutterChrome.titleFont),
                                     AttributeDump.fontJSON(GutterChrome.bodyFont)])),
        ])
    }
}
