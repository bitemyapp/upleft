//! `MTFontMathTable.swift` (and `MTFontMathTableV2.swift`): the OpenType MATH
//! table, read from the `.plist` SwiftMath ships beside each font.
//!
//! SwiftMath keeps the plist as an `NSDictionary` and looks values up by
//! glyph *name* on every call. The plist is loaded through Foundation exactly
//! as SwiftMath loads it (`NSDictionary(contentsOf:)`) and then copied into
//! Rust maps once; lookups go through the same glyph-name round trip.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use objc2::AnyThread;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_core_foundation::{CFRetained, CGFloat, CGRect, CGSize};
use objc2_core_graphics::CGGlyph;
use objc2_core_text::{CTFont, CTFontOrientation};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString, NSURL};

use super::mt_font::GraphicsFont;

/// A number from the plist: what `intValue` and `floatValue` would return.
#[derive(Clone, Copy, Debug)]
pub struct PlistNumber {
    pub int_value: i64,
    pub float_value: f32,
    pub bool_value: bool,
}

#[derive(Clone, Debug)]
pub struct PlistGlyphPart {
    pub advance: Option<PlistNumber>,
    pub end_connector: Option<PlistNumber>,
    pub start_connector: Option<PlistNumber>,
    pub extender: Option<PlistNumber>,
    pub glyph: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct PlistAssembly {
    pub parts: Option<Vec<PlistGlyphPart>>,
}

/// The contents of a math-table plist.
#[derive(Clone, Debug, Default)]
pub struct RawMathTable {
    pub version: Option<String>,
    pub constants: Option<HashMap<String, PlistNumber>>,
    pub v_variants: Option<HashMap<String, Vec<String>>>,
    pub h_variants: Option<HashMap<String, Vec<String>>>,
    pub italic: Option<HashMap<String, PlistNumber>>,
    pub accents: Option<HashMap<String, PlistNumber>>,
    pub v_assembly: Option<HashMap<String, PlistAssembly>>,
}

fn number(object: &AnyObject) -> Option<PlistNumber> {
    let number = object.downcast_ref::<NSNumber>()?;
    Some(PlistNumber {
        int_value: number.integerValue() as i64,
        float_value: number.floatValue(),
        bool_value: number.boolValue(),
    })
}

fn string(object: &AnyObject) -> Option<String> {
    object
        .downcast_ref::<NSString>()
        .map(|string| string.to_string())
}

fn dictionary(object: &AnyObject) -> Option<&NSDictionary<AnyObject, AnyObject>> {
    let dictionary = object.downcast_ref::<NSDictionary>()?;
    // SAFETY: plist dictionaries hold objects; the generic parameters are
    // only a view.
    Some(unsafe {
        &*(dictionary as *const NSDictionary as *const NSDictionary<AnyObject, AnyObject>)
    })
}

fn array(object: &AnyObject) -> Option<&NSArray<AnyObject>> {
    let array = object.downcast_ref::<NSArray>()?;
    Some(unsafe { &*(array as *const NSArray as *const NSArray<AnyObject>) })
}

fn entries(dictionary: &NSDictionary<AnyObject, AnyObject>) -> Vec<(String, Retained<AnyObject>)> {
    let (keys, values) = dictionary.to_vecs();
    keys.iter()
        .zip(values)
        .filter_map(|(key, value)| Some((string(key)?, value)))
        .collect()
}

fn number_map(object: Option<&AnyObject>) -> Option<HashMap<String, PlistNumber>> {
    let dictionary = dictionary(object?)?;
    Some(
        entries(dictionary)
            .into_iter()
            .filter_map(|(key, value)| Some((key, number(&value)?)))
            .collect(),
    )
}

fn variants_map(object: Option<&AnyObject>) -> Option<HashMap<String, Vec<String>>> {
    let dictionary = dictionary(object?)?;
    Some(
        entries(dictionary)
            .into_iter()
            .filter_map(|(key, value)| {
                let array = array(&value)?;
                Some((key, array.iter().filter_map(|name| string(&name)).collect()))
            })
            .collect(),
    )
}

fn assembly_map(object: Option<&AnyObject>) -> Option<HashMap<String, PlistAssembly>> {
    let dictionary = dictionary(object?)?;
    Some(
        entries(dictionary)
            .into_iter()
            .filter_map(|(key, value)| {
                let info = dictionary_owned(&value)?;
                let parts = info.get("parts").and_then(|parts| {
                    let parts = array(parts)?;
                    Some(
                        parts
                            .iter()
                            .map(|part| {
                                let part = dictionary_owned(&part).unwrap_or_default();
                                PlistGlyphPart {
                                    advance: part.get("advance").and_then(|v| number(v)),
                                    end_connector: part.get("endConnector").and_then(|v| number(v)),
                                    start_connector: part
                                        .get("startConnector")
                                        .and_then(|v| number(v)),
                                    extender: part.get("extender").and_then(|v| number(v)),
                                    glyph: part.get("glyph").and_then(|v| string(v)),
                                }
                            })
                            .collect(),
                    )
                });
                Some((key, PlistAssembly { parts }))
            })
            .collect(),
    )
}

fn dictionary_owned(object: &AnyObject) -> Option<HashMap<String, Retained<AnyObject>>> {
    Some(entries(dictionary(object)?).into_iter().collect())
}

impl RawMathTable {
    /// `NSDictionary(contentsOf: url)`, copied into Rust maps.
    pub fn load(path: &Path) -> Option<RawMathTable> {
        let url = NSURL::fileURLWithPath(&NSString::from_str(path.to_str()?));
        #[allow(deprecated)]
        let dictionary = unsafe {
            NSDictionary::<AnyObject, AnyObject>::initWithContentsOfURL(NSDictionary::alloc(), &url)
        }?;
        let top = entries(&dictionary).into_iter().collect::<HashMap<_, _>>();
        let get = |key: &str| top.get(key).map(|value| &**value);
        Some(RawMathTable {
            version: get("version").and_then(string),
            constants: number_map(get("constants")),
            v_variants: variants_map(get("v_variants")),
            h_variants: variants_map(get("h_variants")),
            italic: number_map(get("italic")),
            accents: number_map(get("accents")),
            v_assembly: assembly_map(get("v_assembly")),
        })
    }
}

/// A part of a glyph assembly.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GlyphPart {
    /// The glyph that represents this part
    pub glyph: CGGlyph,
    /// Full advance width/height for this part, in the direction of the extension in points.
    pub full_advance: CGFloat,
    /// Advance width/ height of the straight bar connector material at the beginning of the glyph in points.
    pub start_connector_length: CGFloat,
    /// Advance width/ height of the straight bar connector material at the end of the glyph in points.
    pub end_connector_length: CGFloat,
    /// If this part is an extender. If set, the part can be skipped or repeated.
    pub is_extender: bool,
}

/// Which Swift class a table is: `MTFontMathTable` traps on a malformed or
/// incomplete plist, `MTFontMathTableV2` reads zeros and skips.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MathTableFlavor {
    V1,
    V2,
}

/// The Math table of an OpenType font.
#[derive(Debug)]
pub struct MTFontMathTable {
    cg_font: Arc<GraphicsFont>,
    ct_font: CFRetained<CTFont>,
    units_per_em: u32,
    font_size: CGFloat,
    /// V2 computes `fontUnitsToPt` and `muUnit` from the size it was asked for.
    requested_size: CGFloat,
    math_table: Arc<RawMathTable>,
    flavor: MathTableFlavor,
}

// SAFETY: CGFont and CTFont are immutable, thread-safe Core Foundation objects.
unsafe impl Send for MTFontMathTable {}
unsafe impl Sync for MTFontMathTable {}

impl MTFontMathTable {
    /// `MTFontMathTable(withFont:mathTable:)`.
    pub fn new(
        cg_font: Arc<GraphicsFont>,
        ct_font: CFRetained<CTFont>,
        math_table: Arc<RawMathTable>,
        flavor: MathTableFlavor,
        requested_size: CGFloat,
    ) -> MTFontMathTable {
        let units_per_em = unsafe { ct_font.units_per_em() };
        let font_size = unsafe { ct_font.size() };
        if flavor == MathTableFlavor::V1 {
            let version = math_table
                .version
                .as_deref()
                .expect("_mathTable[\"version\"] as! String");
            if version != "1.3" {
                panic!("Invalid version of math table plist: {version}");
            }
        }
        MTFontMathTable {
            cg_font,
            ct_font,
            units_per_em,
            font_size,
            requested_size,
            math_table,
            flavor,
        }
    }

    pub fn raw(&self) -> &RawMathTable {
        &self.math_table
    }

    /// MU unit in points.
    pub fn mu_unit(&self) -> CGFloat {
        match self.flavor {
            MathTableFlavor::V1 => self.font_size / 18.0,
            MathTableFlavor::V2 => self.requested_size / 18.0,
        }
    }

    pub fn font_units_to_pt(&self, font_units: i64) -> CGFloat {
        match self.flavor {
            MathTableFlavor::V1 => {
                font_units as CGFloat * self.font_size / self.units_per_em as CGFloat
            }
            MathTableFlavor::V2 => {
                font_units as CGFloat * self.requested_size / self.units_per_em as CGFloat
            }
        }
    }

    pub fn constant_from_table(&self, const_name: &str) -> CGFloat {
        let value = self
            .math_table
            .constants
            .as_ref()
            .and_then(|constants| constants.get(const_name));
        match (self.flavor, value) {
            (_, Some(value)) => self.font_units_to_pt(value.int_value),
            (MathTableFlavor::V2, None) => 0.0,
            (MathTableFlavor::V1, None) => panic!("math table constant {const_name} is missing"),
        }
    }

    pub fn percent_from_table(&self, percent_name: &str) -> CGFloat {
        let value = self
            .math_table
            .constants
            .as_ref()
            .and_then(|constants| constants.get(percent_name));
        match (self.flavor, value) {
            (_, Some(value)) => value.float_value as CGFloat / 100.0,
            (MathTableFlavor::V2, None) => 0.0,
            (MathTableFlavor::V1, None) => panic!("math table percent {percent_name} is missing"),
        }
    }

    fn font_size(&self) -> CGFloat {
        self.font_size
    }

    // MARK: - Fractions
    pub fn fraction_numerator_display_style_shift_up(&self) -> CGFloat {
        self.constant_from_table("FractionNumeratorDisplayStyleShiftUp")
    }
    pub fn fraction_numerator_shift_up(&self) -> CGFloat {
        self.constant_from_table("FractionNumeratorShiftUp")
    }
    pub fn fraction_denominator_display_style_shift_down(&self) -> CGFloat {
        self.constant_from_table("FractionDenominatorDisplayStyleShiftDown")
    }
    pub fn fraction_denominator_shift_down(&self) -> CGFloat {
        self.constant_from_table("FractionDenominatorShiftDown")
    }
    pub fn fraction_numerator_display_style_gap_min(&self) -> CGFloat {
        self.constant_from_table("FractionNumDisplayStyleGapMin")
    }
    pub fn fraction_numerator_gap_min(&self) -> CGFloat {
        self.constant_from_table("FractionNumeratorGapMin")
    }
    pub fn fraction_denominator_display_style_gap_min(&self) -> CGFloat {
        self.constant_from_table("FractionDenomDisplayStyleGapMin")
    }
    pub fn fraction_denominator_gap_min(&self) -> CGFloat {
        self.constant_from_table("FractionDenominatorGapMin")
    }
    pub fn fraction_rule_thickness(&self) -> CGFloat {
        self.constant_from_table("FractionRuleThickness")
    }
    pub fn skewed_fraction_horizonal_gap(&self) -> CGFloat {
        self.constant_from_table("SkewedFractionHorizontalGap")
    }
    pub fn skewed_fraction_vertical_gap(&self) -> CGFloat {
        self.constant_from_table("SkewedFractionVerticalGap")
    }

    // MARK: - Non-standard
    pub fn fraction_delimiter_size(&self) -> CGFloat {
        1.01 * self.font_size()
    }
    /// Modified constant from 2.4 to 2.39, it matches KaTeX and looks better.
    pub fn fraction_delimiter_display_style_size(&self) -> CGFloat {
        2.39 * self.font_size()
    }

    // MARK: - Stacks
    pub fn stack_top_display_style_shift_up(&self) -> CGFloat {
        self.constant_from_table("StackTopDisplayStyleShiftUp")
    }
    pub fn stack_top_shift_up(&self) -> CGFloat {
        self.constant_from_table("StackTopShiftUp")
    }
    pub fn stack_display_style_gap_min(&self) -> CGFloat {
        self.constant_from_table("StackDisplayStyleGapMin")
    }
    pub fn stack_gap_min(&self) -> CGFloat {
        self.constant_from_table("StackGapMin")
    }
    pub fn stack_bottom_display_style_shift_down(&self) -> CGFloat {
        self.constant_from_table("StackBottomDisplayStyleShiftDown")
    }
    pub fn stack_bottom_shift_down(&self) -> CGFloat {
        self.constant_from_table("StackBottomShiftDown")
    }
    pub fn stretch_stack_bottom_shift_down(&self) -> CGFloat {
        self.constant_from_table("StretchStackBottomShiftDown")
    }
    pub fn stretch_stack_gap_above_min(&self) -> CGFloat {
        self.constant_from_table("StretchStackGapAboveMin")
    }
    pub fn stretch_stack_gap_below_min(&self) -> CGFloat {
        self.constant_from_table("StretchStackGapBelowMin")
    }
    pub fn stretch_stack_top_shift_up(&self) -> CGFloat {
        self.constant_from_table("StretchStackTopShiftUp")
    }

    // MARK: - super/sub scripts
    pub fn superscript_shift_up(&self) -> CGFloat {
        self.constant_from_table("SuperscriptShiftUp")
    }
    pub fn superscript_shift_up_cramped(&self) -> CGFloat {
        self.constant_from_table("SuperscriptShiftUpCramped")
    }
    pub fn subscript_shift_down(&self) -> CGFloat {
        self.constant_from_table("SubscriptShiftDown")
    }
    pub fn superscript_baseline_drop_max(&self) -> CGFloat {
        self.constant_from_table("SuperscriptBaselineDropMax")
    }
    pub fn subscript_baseline_drop_min(&self) -> CGFloat {
        self.constant_from_table("SubscriptBaselineDropMin")
    }
    pub fn superscript_bottom_min(&self) -> CGFloat {
        self.constant_from_table("SuperscriptBottomMin")
    }
    pub fn subscript_top_max(&self) -> CGFloat {
        self.constant_from_table("SubscriptTopMax")
    }
    pub fn sub_superscript_gap_min(&self) -> CGFloat {
        self.constant_from_table("SubSuperscriptGapMin")
    }
    pub fn superscript_bottom_max_with_subscript(&self) -> CGFloat {
        self.constant_from_table("SuperscriptBottomMaxWithSubscript")
    }
    pub fn space_after_script(&self) -> CGFloat {
        self.constant_from_table("SpaceAfterScript")
    }

    // MARK: - radicals
    pub fn radical_extra_ascender(&self) -> CGFloat {
        self.constant_from_table("RadicalExtraAscender")
    }
    pub fn radical_rule_thickness(&self) -> CGFloat {
        self.constant_from_table("RadicalRuleThickness")
    }
    pub fn radical_display_style_vertical_gap(&self) -> CGFloat {
        self.constant_from_table("RadicalDisplayStyleVerticalGap")
    }
    pub fn radical_vertical_gap(&self) -> CGFloat {
        self.constant_from_table("RadicalVerticalGap")
    }
    pub fn radical_kern_before_degree(&self) -> CGFloat {
        self.constant_from_table("RadicalKernBeforeDegree")
    }
    pub fn radical_kern_after_degree(&self) -> CGFloat {
        self.constant_from_table("RadicalKernAfterDegree")
    }
    pub fn radical_degree_bottom_raise_percent(&self) -> CGFloat {
        self.percent_from_table("RadicalDegreeBottomRaisePercent")
    }

    // MARK: - Limits
    pub fn upper_limit_baseline_rise_min(&self) -> CGFloat {
        self.constant_from_table("UpperLimitBaselineRiseMin")
    }
    pub fn upper_limit_gap_min(&self) -> CGFloat {
        self.constant_from_table("UpperLimitGapMin")
    }
    pub fn lower_limit_gap_min(&self) -> CGFloat {
        self.constant_from_table("LowerLimitGapMin")
    }
    pub fn lower_limit_baseline_drop_min(&self) -> CGFloat {
        self.constant_from_table("LowerLimitBaselineDropMin")
    }
    /// \xi_13 in TeX, not present in OpenType so we always set it to 0.
    pub fn limit_extra_ascender_descender(&self) -> CGFloat {
        0.0
    }

    // MARK: - Underline
    pub fn underbar_vertical_gap(&self) -> CGFloat {
        self.constant_from_table("UnderbarVerticalGap")
    }
    pub fn underbar_rule_thickness(&self) -> CGFloat {
        self.constant_from_table("UnderbarRuleThickness")
    }
    pub fn underbar_extra_descender(&self) -> CGFloat {
        self.constant_from_table("UnderbarExtraDescender")
    }

    // MARK: - Overline
    pub fn overbar_vertical_gap(&self) -> CGFloat {
        self.constant_from_table("OverbarVerticalGap")
    }
    pub fn overbar_rule_thickness(&self) -> CGFloat {
        self.constant_from_table("OverbarRuleThickness")
    }
    pub fn overbar_extra_ascender(&self) -> CGFloat {
        self.constant_from_table("OverbarExtraAscender")
    }

    // MARK: - Constants
    pub fn axis_height(&self) -> CGFloat {
        self.constant_from_table("AxisHeight")
    }
    pub fn script_scale_down(&self) -> CGFloat {
        self.percent_from_table("ScriptPercentScaleDown")
    }
    pub fn script_script_scale_down(&self) -> CGFloat {
        self.percent_from_table("ScriptScriptPercentScaleDown")
    }
    pub fn math_leading(&self) -> CGFloat {
        self.constant_from_table("MathLeading")
    }
    pub fn delimited_sub_formula_min_height(&self) -> CGFloat {
        self.constant_from_table("DelimitedSubFormulaMinHeight")
    }

    // MARK: - Accent
    pub fn accent_base_height(&self) -> CGFloat {
        self.constant_from_table("AccentBaseHeight")
    }
    pub fn flattened_accent_base_height(&self) -> CGFloat {
        self.constant_from_table("FlattenedAccentBaseHeight")
    }

    // MARK: - Variants

    fn name_for_glyph(&self, glyph: CGGlyph) -> Arc<str> {
        self.cg_font.name(glyph)
    }

    fn glyph_with_name(&self, name: &str) -> CGGlyph {
        self.cg_font.glyph(name)
    }

    /// All the vertical variants of the glyph, or the glyph itself.
    pub fn get_vertical_variants_for_glyph(&self, glyph: CGGlyph) -> Vec<CGGlyph> {
        match &self.math_table.v_variants {
            Some(variants) => self.get_variants_for_glyph(glyph, variants),
            None if self.flavor == MathTableFlavor::V2 => Vec::new(),
            None => panic!("v_variants missing from the math table"),
        }
    }

    /// All the horizontal variants of the glyph, or the glyph itself.
    pub fn get_horizontal_variants_for_glyph(&self, glyph: CGGlyph) -> Vec<CGGlyph> {
        match &self.math_table.h_variants {
            Some(variants) => self.get_variants_for_glyph(glyph, variants),
            None if self.flavor == MathTableFlavor::V2 => Vec::new(),
            None => panic!("h_variants missing from the math table"),
        }
    }

    pub fn get_variants_for_glyph(
        &self,
        glyph: CGGlyph,
        variants: &HashMap<String, Vec<String>>,
    ) -> Vec<CGGlyph> {
        let glyph_name = self.name_for_glyph(glyph);
        match variants.get(&*glyph_name) {
            Some(variant_glyphs) if !variant_glyphs.is_empty() => variant_glyphs
                .iter()
                .map(|name| self.glyph_with_name(name))
                .collect(),
            // There are no extra variants, so just add the current glyph to it.
            _ => vec![self.glyph_with_name(&glyph_name)],
        }
    }

    /// A larger vertical variant of the given glyph, or the glyph itself.
    pub fn get_larger_glyph(&self, glyph: CGGlyph) -> CGGlyph {
        let Some(variants) = &self.math_table.v_variants else {
            if self.flavor == MathTableFlavor::V2 {
                return glyph;
            }
            panic!("v_variants missing from the math table");
        };
        let glyph_name = self.name_for_glyph(glyph);
        let Some(variant_glyphs) = variants.get(&*glyph_name).filter(|v| !v.is_empty()) else {
            // There are no extra variants, so just return the current glyph.
            return glyph;
        };
        // Find the first variant with a different name.
        for glyph_variant_name in variant_glyphs {
            if **glyph_variant_name != *glyph_name {
                return self.glyph_with_name(glyph_variant_name);
            }
        }
        // We did not find any variants of this glyph so return it.
        glyph
    }

    // MARK: - Italic Correction

    /// The italic correction for the given glyph, or 0.
    pub fn get_italic_correction(&self, glyph: CGGlyph) -> CGFloat {
        let glyph_name = self.name_for_glyph(glyph);
        let italics = match &self.math_table.italic {
            Some(italics) => italics,
            None if self.flavor == MathTableFlavor::V2 => return 0.0,
            None => panic!("italic missing from the math table"),
        };
        match (italics.get(&*glyph_name), self.flavor) {
            (Some(value), _) => self.font_units_to_pt(value.int_value),
            (None, MathTableFlavor::V1) => self.font_units_to_pt(0),
            (None, MathTableFlavor::V2) => 0.0,
        }
    }

    // MARK: - Accents

    /// The adjustment to the top accent for the given glyph; the centre of
    /// the advance when the table has none.
    pub fn get_top_accent_adjustment(&self, glyph: CGGlyph) -> CGFloat {
        let glyph_name = self.name_for_glyph(glyph);
        let value = match &self.math_table.accents {
            Some(accents) => accents.get(&*glyph_name),
            None if self.flavor == MathTableFlavor::V2 => None,
            None => panic!("accents missing from the math table"),
        };
        if let Some(value) = value {
            self.font_units_to_pt(value.int_value)
        } else {
            // If no top accent is defined then it is the center of the advance width.
            let mut glyph = glyph;
            let mut advances = CGSize::new(0.0, 0.0);
            unsafe {
                self.ct_font.advances_for_glyphs(
                    CTFontOrientation::Horizontal,
                    std::ptr::NonNull::from(&mut glyph),
                    &mut advances,
                    1,
                );
            }
            advances.width / 2.0
        }
    }

    // MARK: - Glyph Construction

    /// Minimum overlap of connecting glyphs during glyph construction.
    pub fn min_connector_overlap(&self) -> CGFloat {
        self.constant_from_table("MinConnectorOverlap")
    }

    /// The glyph parts for constructing vertical variants of this glyph, or
    /// an empty array.
    pub fn get_vertical_glyph_assembly(&self, glyph: CGGlyph) -> Vec<GlyphPart> {
        let glyph_name = self.name_for_glyph(glyph);
        let assembly_table = match &self.math_table.v_assembly {
            Some(table) => table,
            None if self.flavor == MathTableFlavor::V2 => return Vec::new(),
            None => panic!("v_assembly missing from the math table"),
        };
        let Some(assembly_info) = assembly_table.get(&*glyph_name) else {
            // No vertical assembly defined for glyph
            return Vec::new();
        };
        let Some(parts) = &assembly_info.parts else {
            // parts should always have been defined, but if it isn't return nil
            return Vec::new();
        };
        let mut rv = Vec::with_capacity(parts.len());
        for part_info in parts {
            match self.flavor {
                MathTableFlavor::V1 => {
                    let full_advance =
                        self.font_units_to_pt(part_info.advance.expect("advance").int_value);
                    let end_connector_length = self
                        .font_units_to_pt(part_info.end_connector.expect("endConnector").int_value);
                    let start_connector_length = self.font_units_to_pt(
                        part_info.start_connector.expect("startConnector").int_value,
                    );
                    let is_extender = part_info.extender.expect("extender").bool_value;
                    let glyph = self.glyph_with_name(part_info.glyph.as_deref().expect("glyph"));
                    rv.push(GlyphPart {
                        glyph,
                        full_advance,
                        start_connector_length,
                        end_connector_length,
                        is_extender,
                    });
                }
                MathTableFlavor::V2 => {
                    let (Some(adv), Some(end), Some(start), Some(ext), Some(name)) = (
                        part_info.advance,
                        part_info.end_connector,
                        part_info.start_connector,
                        part_info.extender,
                        part_info.glyph.as_deref(),
                    ) else {
                        continue;
                    };
                    let full_advance = self.font_units_to_pt(adv.int_value);
                    let end_connector_length = self.font_units_to_pt(end.int_value);
                    let start_connector_length = self.font_units_to_pt(start.int_value);
                    let is_extender = ext.bool_value;
                    let glyph = self.glyph_with_name(name);
                    rv.push(GlyphPart {
                        glyph,
                        full_advance,
                        start_connector_length,
                        end_connector_length,
                        is_extender,
                    });
                }
            }
        }
        rv
    }
}

/// `getBboxDetails`'s input helper: bounds for one glyph.
pub(crate) fn bounding_rect_for_glyph(font: &CTFont, glyph: CGGlyph) -> CGRect {
    let mut glyph = glyph;
    unsafe {
        font.bounding_rects_for_glyphs(
            CTFontOrientation::Horizontal,
            std::ptr::NonNull::from(&mut glyph),
            std::ptr::null_mut(),
            1,
        )
    }
}
