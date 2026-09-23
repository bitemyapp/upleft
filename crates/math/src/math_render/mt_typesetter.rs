//! `MTTypesetter.swift`: lays a finalized math list out as a display tree.

use std::sync::Arc;

use objc2::rc::Retained;
use objc2_app_kit::NSColor;
use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGGlyph, CGRectGetMaxX, CGRectGetMaxY, CGRectGetMinY};
use objc2_core_text::{
    CTFontOrientation, kCTFontAttributeName, kCTForegroundColorAttributeName, kCTKernAttributeName,
};
use objc2_foundation::{
    NSAttributedString, NSMutableAttributedString, NSNumber, NSRange, NSString,
};

use super::mt_color::color_from_hex_string;
use super::mt_font::MTFont;
use super::mt_font_math_table::{GlyphPart, bounding_rect_for_glyph};
use super::mt_math_list::{
    AtomKind, MTColumnAlignment, MTFontStyle, MTLineStyle, MTMathAtom, MTMathAtomRef,
    MTMathAtomType, MTMathListRef,
};
use super::mt_math_list_display::{LinePosition, MTDisplay, NS_NOT_FOUND, cf_object, key};
use super::mt_unicode::{MathCharacter, unicode_symbol as sym};
use crate::swift;

// MARK: - Inter Element Spacing

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InterElementSpaceType {
    Invalid = -1,
    None = 0,
    Thin,
    /// Thin but not in script mode
    NsThin,
    NsMedium,
    NsThick,
}

use InterElementSpaceType as S;

#[rustfmt::skip]
const INTER_ELEMENT_SPACE_ARRAY: [[InterElementSpaceType; 8]; 9] = [
    //   ordinary   operator   binary       relation   open        close      punct      fraction
    [S::None,     S::Thin,   S::NsMedium, S::NsThick, S::None,     S::None,    S::None,    S::NsThin],   // ordinary
    [S::Thin,     S::Thin,   S::Invalid,  S::NsThick, S::None,     S::None,    S::None,    S::NsThin],   // operator
    [S::NsMedium, S::NsMedium, S::Invalid, S::Invalid, S::NsMedium, S::Invalid, S::Invalid, S::NsMedium], // binary
    [S::NsThick,  S::NsThick, S::Invalid, S::None,    S::NsThick,  S::None,    S::None,    S::NsThick],  // relation
    [S::None,     S::None,   S::Invalid,  S::None,    S::None,     S::None,    S::None,    S::None],     // open
    [S::None,     S::Thin,   S::NsMedium, S::NsThick, S::None,     S::None,    S::None,    S::NsThin],   // close
    [S::NsThin,   S::NsThin, S::Invalid,  S::NsThin,  S::NsThin,   S::NsThin,  S::NsThin,  S::NsThin],   // punct
    [S::NsThin,   S::Thin,   S::NsMedium, S::NsThick, S::NsThin,   S::None,    S::NsThin,  S::NsThin],   // fraction
    [S::NsMedium, S::NsThin, S::NsMedium, S::NsThick, S::None,     S::None,    S::None,    S::NsThin],   // radical
];

/// The index for the given type. If row is true, the index is for the row
/// (i.e. left element) otherwise it is for the column (right element).
fn get_inter_element_space_array_index_for_type(type_: MTMathAtomType, row: bool) -> usize {
    match type_ {
        // A placeholder is treated as ordinary
        MTMathAtomType::Color
        | MTMathAtomType::Textcolor
        | MTMathAtomType::ColorBox
        | MTMathAtomType::Ordinary
        | MTMathAtomType::Placeholder => 0,
        MTMathAtomType::LargeOperator => 1,
        MTMathAtomType::BinaryOperator => 2,
        MTMathAtomType::Relation => 3,
        MTMathAtomType::Open => 4,
        MTMathAtomType::Close => 5,
        MTMathAtomType::Punctuation => 6,
        // Fraction and inner are treated the same.
        MTMathAtomType::Fraction | MTMathAtomType::Inner => 7,
        MTMathAtomType::Radical => {
            if row {
                // Radicals have inter element spaces only when on the left side.
                8
            } else {
                // assert(false, …) is compiled out; the Int.max index traps.
                usize::MAX
            }
        }
        _ => usize::MAX,
    }
}

// MARK: - Italics

/// mathit
pub fn get_italicized(ch: &str) -> u32 {
    let mut unicode = ch.utf32_char();

    // Special cases for italics
    if ch == "h" {
        return sym::PLANKS_CONSTANT;
    }

    if ch.is_upper_english() {
        unicode = sym::CAPITAL_ITALIC_START.wrapping_add(ch.utf32_char().wrapping_sub('A' as u32));
    } else if ch.is_lower_english() {
        unicode = sym::LOWER_ITALIC_START.wrapping_add(ch.utf32_char().wrapping_sub('a' as u32));
    } else if ch.is_capital_greek() {
        // Capital Greek characters
        unicode = sym::GREEK_CAPITAL_ITALIC_START + (ch.utf32_char() - sym::CAPITAL_GREEK_START);
    } else if ch.is_lower_greek() {
        // Greek characters
        unicode = sym::GREEK_LOWER_ITALIC_START + (ch.utf32_char() - sym::LOWER_GREEK_START);
    } else if ch.is_greek_symbol() {
        return sym::GREEK_SYMBOL_ITALIC_START + ch.greek_symbol_order().unwrap();
    }
    // Note there are no italicized numbers in unicode so we don't support italicizing numbers.
    unicode
}

/// mathbf
pub fn get_bold(ch: &str) -> u32 {
    let mut unicode = ch.utf32_char();
    if ch.is_upper_english() {
        unicode =
            sym::MATH_CAPITAL_BOLD_START.wrapping_add(ch.utf32_char().wrapping_sub('A' as u32));
    } else if ch.is_lower_english() {
        unicode = sym::MATH_LOWER_BOLD_START.wrapping_add(ch.utf32_char().wrapping_sub('a' as u32));
    } else if ch.is_capital_greek() {
        unicode = sym::GREEK_CAPITAL_BOLD_START + (ch.utf32_char() - sym::CAPITAL_GREEK_START);
    } else if ch.is_lower_greek() {
        unicode = sym::GREEK_LOWER_BOLD_START + (ch.utf32_char() - sym::LOWER_GREEK_START);
    } else if ch.is_greek_symbol() {
        return sym::GREEK_SYMBOL_BOLD_START + ch.greek_symbol_order().unwrap();
    } else if ch.is_number() {
        unicode = sym::NUMBER_BOLD_START.wrapping_add(ch.utf32_char().wrapping_sub('0' as u32));
    }
    unicode
}

/// mathbfit
pub fn get_bold_italic(ch: &str) -> u32 {
    let mut unicode = ch.utf32_char();
    if ch.is_upper_english() {
        unicode = sym::MATH_CAPITAL_BOLD_ITALIC_START
            .wrapping_add(ch.utf32_char().wrapping_sub('A' as u32));
    } else if ch.is_lower_english() {
        unicode = sym::MATH_LOWER_BOLD_ITALIC_START
            .wrapping_add(ch.utf32_char().wrapping_sub('a' as u32));
    } else if ch.is_capital_greek() {
        unicode =
            sym::GREEK_CAPITAL_BOLD_ITALIC_START + (ch.utf32_char() - sym::CAPITAL_GREEK_START);
    } else if ch.is_lower_greek() {
        unicode = sym::GREEK_LOWER_BOLD_ITALIC_START + (ch.utf32_char() - sym::LOWER_GREEK_START);
    } else if ch.is_greek_symbol() {
        return sym::GREEK_SYMBOL_BOLD_ITALIC_START + ch.greek_symbol_order().unwrap();
    } else if ch.is_number() {
        // No bold italic for numbers so we just bold them.
        unicode = get_bold(ch);
    }
    unicode
}

/// LaTeX default
pub fn get_default_style(ch: &str) -> u32 {
    if ch.is_lower_english() || ch.is_upper_english() || ch.is_lower_greek() || ch.is_greek_symbol()
    {
        return get_italicized(ch);
    } else if ch.is_number() || ch.is_capital_greek() {
        // In the default style numbers and capital greek is roman
        return ch.utf32_char();
    } else if ch == "." {
        // . is treated as a number in our code, but it doesn't change fonts.
        return ch.utf32_char();
    } else {
        panic!("Unknown character {ch} for default style.");
    }
}

/// mathcal/mathscr (caligraphic or script)
pub fn get_caligraphic(ch: &str) -> u32 {
    // Caligraphic has lots of exceptions:
    match ch {
        "B" => return 0x212C, // Script B (bernoulli)
        "E" => return 0x2130, // Script E (emf)
        "F" => return 0x2131, // Script F (fourier)
        "H" => return 0x210B, // Script H (hamiltonian)
        "I" => return 0x2110, // Script I
        "L" => return 0x2112, // Script L (laplace)
        "M" => return 0x2133, // Script M (M-matrix)
        "R" => return 0x211B, // Script R (Riemann integral)
        "e" => return 0x212F, // Script e (Natural exponent)
        "g" => return 0x210A, // Script g (real number)
        "o" => return 0x2134, // Script o (order)
        _ => {}
    }
    if ch.is_upper_english() {
        sym::MATH_CAPITAL_SCRIPT_START.wrapping_add(ch.utf32_char().wrapping_sub('A' as u32))
    } else {
        // Latin Modern Math does not have lower case caligraphic characters, and
        // caligraphic characters don't exist for greek or numbers: the default
        // treatment instead.
        get_default_style(ch)
    }
}

/// mathtt (monospace)
pub fn get_typewriter(ch: &str) -> u32 {
    if ch.is_upper_english() {
        return sym::MATH_CAPITAL_TT_START.wrapping_add(ch.utf32_char().wrapping_sub('A' as u32));
    } else if ch.is_lower_english() {
        return sym::MATH_LOWER_TT_START.wrapping_add(ch.utf32_char().wrapping_sub('a' as u32));
    } else if ch.is_number() {
        return sym::NUMBER_TT_START.wrapping_add(ch.utf32_char().wrapping_sub('0' as u32));
    }
    // Monospace characters don't exist for greek, we give them the default treatment.
    get_default_style(ch)
}

/// mathsf
pub fn get_sans_serif(ch: &str) -> u32 {
    if ch.is_upper_english() {
        return sym::MATH_CAPITAL_SANS_SERIF_START
            .wrapping_add(ch.utf32_char().wrapping_sub('A' as u32));
    } else if ch.is_lower_english() {
        return sym::MATH_LOWER_SANS_SERIF_START
            .wrapping_add(ch.utf32_char().wrapping_sub('a' as u32));
    } else if ch.is_number() {
        return sym::NUMBER_SANS_SERIF_START.wrapping_add(ch.utf32_char().wrapping_sub('0' as u32));
    }
    // Sans-serif characters don't exist for greek, we give them the default treatment.
    get_default_style(ch)
}

/// mathfrak
pub fn get_fraktur(ch: &str) -> u32 {
    // Fraktur has exceptions:
    match ch {
        "C" => return 0x212D, // C Fraktur
        "H" => return 0x210C, // Hilbert space
        "I" => return 0x2111, // Imaginary
        "R" => return 0x211C, // Real
        "Z" => return 0x2128, // Z Fraktur
        _ => {}
    }
    if ch.is_upper_english() {
        return sym::MATH_CAPITAL_FRAKTUR_START
            .wrapping_add(ch.utf32_char().wrapping_sub('A' as u32));
    } else if ch.is_lower_english() {
        return sym::MATH_LOWER_FRAKTUR_START
            .wrapping_add(ch.utf32_char().wrapping_sub('a' as u32));
    }
    // Fraktur characters don't exist for greek & numbers, we give them the default treatment.
    get_default_style(ch)
}

/// mathbb (double struck)
pub fn get_blackboard(ch: &str) -> u32 {
    // Blackboard has lots of exceptions:
    match ch {
        "C" => return 0x2102, // Complex numbers
        "H" => return 0x210D, // Quarternions
        "N" => return 0x2115, // Natural numbers
        "P" => return 0x2119, // Primes
        "Q" => return 0x211A, // Rationals
        "R" => return 0x211D, // Reals
        "Z" => return 0x2124, // Integers
        _ => {}
    }
    if ch.is_upper_english() {
        return sym::MATH_CAPITAL_BLACKBOARD_START
            .wrapping_add(ch.utf32_char().wrapping_sub('A' as u32));
    } else if ch.is_lower_english() {
        return sym::MATH_LOWER_BLACKBOARD_START
            .wrapping_add(ch.utf32_char().wrapping_sub('a' as u32));
    } else if ch.is_number() {
        return sym::NUMBER_BLACKBOARD_START.wrapping_add(ch.utf32_char().wrapping_sub('0' as u32));
    }
    // Blackboard characters don't exist for greek, we give them the default treatment.
    get_default_style(ch)
}

pub fn style_character(ch: &str, font_style: MTFontStyle) -> u32 {
    match font_style {
        MTFontStyle::DefaultStyle => get_default_style(ch),
        MTFontStyle::Roman => ch.utf32_char(),
        MTFontStyle::Bold => get_bold(ch),
        MTFontStyle::Italic => get_italicized(ch),
        MTFontStyle::BoldItalic => get_bold_italic(ch),
        MTFontStyle::Caligraphic => get_caligraphic(ch),
        MTFontStyle::Typewriter => get_typewriter(ch),
        MTFontStyle::SansSerif => get_sans_serif(ch),
        MTFontStyle::Fraktur => get_fraktur(ch),
        MTFontStyle::Blackboard => get_blackboard(ch),
    }
}

pub fn change_font(str: &str, font_style: MTFontStyle) -> String {
    let mut retval = String::with_capacity(str.len() * 4);
    for ch in swift::characters(str) {
        let unicode = style_character(ch, font_style);
        retval.push(char::from_u32(unicode).expect("UnicodeScalar(unicode)!"));
    }
    retval
}

fn get_bbox_details(bbox: CGRect, ascent: &mut CGFloat, descent: &mut CGFloat) {
    *ascent = swift::max(0.0, CGRectGetMaxY(bbox) - 0.0);

    // Descent is how much the line goes below the origin. However if the line is all above the origin, then descent can't be negative.
    *descent = swift::max(0.0, 0.0 - CGRectGetMinY(bbox));
}

// MARK: - MTTypesetter

/// What `makeScripts` reads from the display it attaches scripts to.
#[derive(Clone, Copy)]
struct ScriptTarget {
    is_ct_line: bool,
    ascent: CGFloat,
    descent: CGFloat,
}

impl ScriptTarget {
    /// `display?.hasScript = true`, then the dimensions `makeScripts` reads.
    fn of(display: &mut MTDisplay) -> ScriptTarget {
        display.has_script = true;
        ScriptTarget {
            is_ct_line: display.as_ct_line().is_some(),
            ascent: display.ascent(),
            descent: display.descent(),
        }
    }
}

// Delimiter shortfall from plain.tex
const K_DELIMITER_FACTOR: CGFloat = 901.0;
const K_DELIMITER_SHORTFALL_POINTS: CGFloat = 5.0;

const K_BASE_LINE_SKIP_MULTIPLIER: CGFloat = 1.2; // default base line stretch is 12 pt for 10pt font.
const K_LINE_SKIP_MULTIPLIER: CGFloat = 0.1; // default is 1pt for 10pt font.
const K_LINE_SKIP_LIMIT_MULTIPLIER: CGFloat = 0.0;
const K_JOT_MULTIPLIER: CGFloat = 0.3; // A jot is 3pt for a 10pt font.

pub struct MTTypesetter {
    font: Arc<MTFont>,
    display_atoms: Vec<MTDisplay>,
    current_position: CGPoint,
    current_line: Retained<NSMutableAttributedString>,
    /// List of atoms that make the line
    current_atoms: Vec<MTMathAtomRef>,
    current_line_index_range: NSRange,
    style: MTLineStyle,
    style_font_cache: Option<Arc<MTFont>>,
    cramped: bool,
    spaced: bool,
}

fn atom_type(atom: &MTMathAtomRef) -> MTMathAtomType {
    atom.borrow().type_
}

impl MTTypesetter {
    /// `createLineForMathList(_:font:style:)`: finalizes the list, then typesets it.
    pub fn create_line_for_math_list(
        math_list: Option<&MTMathListRef>,
        font: &Arc<MTFont>,
        style: MTLineStyle,
    ) -> Option<MTDisplay> {
        let finalized_list = math_list.map(|list| list.borrow().finalized());
        // default is not cramped
        Self::create_line_for_math_list_cramped(finalized_list.as_ref(), font, style, false)
    }

    /// Internal: typesets without finalizing.
    pub fn create_line_for_math_list_cramped(
        math_list: Option<&MTMathListRef>,
        font: &Arc<MTFont>,
        style: MTLineStyle,
        cramped: bool,
    ) -> Option<MTDisplay> {
        Self::create_line_for_math_list_spaced(math_list, font, style, cramped, false)
    }

    /// Internal.
    pub fn create_line_for_math_list_spaced(
        math_list: Option<&MTMathListRef>,
        font: &Arc<MTFont>,
        style: MTLineStyle,
        cramped: bool,
        spaced: bool,
    ) -> Option<MTDisplay> {
        let math_list = math_list.expect("mathList!");
        let preprocessed_atoms = Self::preprocess_math_list(math_list);
        let mut typesetter = MTTypesetter::new(font.clone(), style, cramped, spaced);
        typesetter.create_display_atoms(&preprocessed_atoms);
        let last = math_list
            .borrow()
            .atoms
            .last()
            .map_or(NSRange::new(0, 0), |atom| atom.borrow().index_range);
        let line = MTDisplay::math_list(
            typesetter.display_atoms,
            NSRange::new(0, last.location + last.length),
        );
        Some(line)
    }

    /// `MTTypesetter.placeholderColor`.
    pub fn placeholder_color() -> Retained<NSColor> {
        NSColor::blueColor()
    }

    fn new(font: Arc<MTFont>, style: MTLineStyle, cramped: bool, spaced: bool) -> MTTypesetter {
        MTTypesetter {
            font,
            display_atoms: Vec::new(),
            current_position: CGPoint::new(0.0, 0.0),
            cramped,
            spaced,
            current_line: NSMutableAttributedString::new(),
            current_atoms: Vec::new(),
            style,
            style_font_cache: None,
            current_line_index_range: NSRange::new(NS_NOT_FOUND, NS_NOT_FOUND),
        }
    }

    fn set_style(&mut self, style: MTLineStyle) {
        self.style = style;
        self.style_font_cache = None;
    }

    fn style_font(&mut self) -> Arc<MTFont> {
        if self.style_font_cache.is_none() {
            let size = Self::get_style_size(self.style, &self.font);
            self.style_font_cache = Some(self.font.copy_with_size(size));
        }
        self.style_font_cache.clone().unwrap()
    }

    /// Removes the atom types TeX does not have and applies Rule 14 (merging
    /// ordinary characters). Mutates the atoms of `ml` in place, as SwiftMath does.
    pub fn preprocess_math_list(ml: &MTMathListRef) -> Vec<MTMathAtomRef> {
        let atoms: Vec<MTMathAtomRef> = ml.borrow().atoms.clone();
        let mut preprocessed: Vec<MTMathAtomRef> = Vec::with_capacity(atoms.len());
        let mut prev_node: Option<MTMathAtomRef> = None;
        for atom in atoms {
            {
                let mut a = atom.borrow_mut();
                if a.type_ == MTMathAtomType::Variable || a.type_ == MTMathAtomType::Number {
                    // This is not a TeX type node. TeX does this during parsing the input.
                    // switch to using the italic math font
                    // We convert it to ordinary
                    let new_font = change_font(&a.nucleus, a.font_style);
                    a.type_ = MTMathAtomType::Ordinary;
                    a.nucleus = new_font;
                } else if a.type_ == MTMathAtomType::UnaryOperator {
                    // Neither of these are TeX nodes. TeX treats these as Ordinary. So will we.
                    a.type_ = MTMathAtomType::Ordinary;
                }
            }

            if atom_type(&atom) == MTMathAtomType::Ordinary {
                // This is Rule 14 to merge ordinary characters.
                // combine ordinary atoms together
                if let Some(prev) = &prev_node {
                    let fusible = {
                        let prev = prev.borrow();
                        prev.type_ == MTMathAtomType::Ordinary
                            && prev.sub_script().is_none()
                            && prev.super_script().is_none()
                    };
                    if fusible {
                        MTMathAtom::fuse(prev, &atom);
                        // skip the current node, we are done here.
                        continue;
                    }
                }
            }

            prev_node = Some(atom.clone());
            preprocessed.push(atom);
        }
        preprocessed
    }

    /// The size of the font in this style.
    pub fn get_style_size(style: MTLineStyle, font: &MTFont) -> CGFloat {
        let original = font.font_size();
        match style {
            MTLineStyle::Display | MTLineStyle::Text => original,
            MTLineStyle::Script => original * font.table().script_scale_down(),
            MTLineStyle::ScriptOfScript => original * font.table().script_script_scale_down(),
        }
    }

    fn add_inter_element_space(
        &mut self,
        prev_node: Option<&MTMathAtomRef>,
        type_: MTMathAtomType,
    ) {
        let mut inter_element_space: CGFloat = 0.0;
        if let Some(prev) = prev_node {
            inter_element_space = self.get_inter_element_space(atom_type(prev), type_);
        } else if self.spaced {
            // For the first atom of a spaced list, treat it as if it is preceded by an open.
            inter_element_space = self.get_inter_element_space(MTMathAtomType::Open, type_);
        }
        self.current_position.x += inter_element_space;
    }

    fn current_line_length(&self) -> usize {
        self.current_line.length()
    }

    fn create_display_atoms(&mut self, preprocessed: &[MTMathAtomRef]) {
        // items should contain all the nodes that need to be layed out.
        // convert to a list of DisplayAtoms
        let mut prev_node: Option<MTMathAtomRef> = None;
        let mut last_type: Option<MTMathAtomType> = None;
        for atom in preprocessed {
            let type_ = atom_type(atom);
            match type_ {
                MTMathAtomType::Number
                | MTMathAtomType::Variable
                | MTMathAtomType::UnaryOperator => {
                    // These should never appear as they should have been removed by preprocessing
                    // (assertionFailure is compiled out).
                }
                MTMathAtomType::Boundary => {
                    // A boundary atom should never be inside a mathlist (assertionFailure).
                }
                MTMathAtomType::Space => {
                    // stash the existing layout
                    if self.current_line_length() > 0 {
                        self.add_display_line();
                    }
                    let space = atom
                        .borrow()
                        .as_space()
                        .expect("atom as! MTMathSpace")
                        .space;
                    // add the desired space
                    let mu = self.style_font().table().mu_unit();
                    self.current_position.x += space * mu;
                    // Since this is extra space, the desired interelement space between the prevAtom
                    // and the next node is still preserved. To avoid resetting the prevAtom and lastType
                    // we skip to the next node.
                    continue;
                }
                MTMathAtomType::Style => {
                    // stash the existing layout
                    if self.current_line_length() > 0 {
                        self.add_display_line();
                    }
                    let style = atom
                        .borrow()
                        .as_style()
                        .expect("atom as! MTMathStyle")
                        .style;
                    self.set_style(style);
                    // We need to preserve the prevNode for any interelement space changes.
                    // so we skip to the next node.
                    continue;
                }
                MTMathAtomType::Color => {
                    // stash the existing layout
                    if self.current_line_length() > 0 {
                        self.add_display_line();
                    }
                    let (inner, color_string) = {
                        let a = atom.borrow();
                        let AtomKind::Color(color) = &a.kind else {
                            panic!("atom as! MTMathColor")
                        };
                        (color.inner_list.clone(), color.color_string.clone())
                    };
                    let mut display = Self::create_line_for_math_list(
                        inner.as_ref(),
                        &self.font.clone(),
                        self.style,
                    )
                    .unwrap();
                    display.local_text_color = color_from_hex_string(&color_string);
                    display.set_position(self.current_position);
                    self.current_position.x += display.width();
                    self.display_atoms.push(display);
                }
                MTMathAtomType::Textcolor => {
                    // stash the existing layout
                    if self.current_line_length() > 0 {
                        self.add_display_line();
                    }
                    let (inner, color_string) = {
                        let a = atom.borrow();
                        let AtomKind::TextColor(color) = &a.kind else {
                            panic!("atom as! MTMathTextColor")
                        };
                        (color.inner_list.clone(), color.color_string.clone())
                    };
                    let mut display = Self::create_line_for_math_list(
                        inner.as_ref(),
                        &self.font.clone(),
                        self.style,
                    )
                    .unwrap();
                    display.local_text_color = color_from_hex_string(&color_string);

                    if let Some(prev) = &prev_node {
                        let sub_display = &display.sub_displays()[0];
                        let sub_display_type = {
                            let line = sub_display
                                .as_ct_line()
                                .expect("(subDisplay as? MTCTLineDisplay)!");
                            atom_type(&line.atoms[0])
                        };
                        let inter_element_space =
                            self.get_inter_element_space(atom_type(prev), sub_display_type);
                        if self.current_line_length() > 0 {
                            if inter_element_space > 0.0 {
                                // add a kerning of that space to the previous character
                                self.add_kern_to_last_character(inter_element_space);
                            }
                        } else {
                            // increase the space
                            self.current_position.x += inter_element_space;
                        }
                    }

                    display.set_position(self.current_position);
                    self.current_position.x += display.width();
                    self.display_atoms.push(display);
                }
                MTMathAtomType::ColorBox => {
                    // stash the existing layout
                    if self.current_line_length() > 0 {
                        self.add_display_line();
                    }
                    let (inner, color_string) = {
                        let a = atom.borrow();
                        let AtomKind::Colorbox(color) = &a.kind else {
                            panic!("atom as! MTMathColorbox")
                        };
                        (color.inner_list.clone(), color.color_string.clone())
                    };
                    let mut display = Self::create_line_for_math_list(
                        inner.as_ref(),
                        &self.font.clone(),
                        self.style,
                    )
                    .unwrap();

                    display.local_background_color = color_from_hex_string(&color_string);
                    display.set_position(self.current_position);
                    self.current_position.x += display.width();
                    self.display_atoms.push(display);
                }
                MTMathAtomType::Radical => {
                    // stash the existing layout
                    if self.current_line_length() > 0 {
                        self.add_display_line();
                    }
                    let (radicand, degree, index_range, has_scripts) = {
                        let a = atom.borrow();
                        let rad = a.as_radical().expect("atom as! MTRadical");
                        (
                            rad.radicand.clone(),
                            rad.degree.clone(),
                            a.index_range,
                            a.sub_script().is_some() || a.super_script().is_some(),
                        )
                    };
                    // Radicals are considered as Ord in rule 16.
                    self.add_inter_element_space(prev_node.as_ref(), MTMathAtomType::Ordinary);
                    let mut display_rad = self.make_radical(radicand.as_ref(), index_range);
                    if degree.is_some() {
                        // add the degree to the radical
                        let degree = Self::create_line_for_math_list(
                            degree.as_ref(),
                            &self.font.clone(),
                            MTLineStyle::ScriptOfScript,
                        )
                        .unwrap();
                        let style_font = self.style_font();
                        display_rad.set_degree(degree, style_font.table());
                    }
                    let target = has_scripts.then(|| ScriptTarget::of(&mut display_rad));
                    let width = display_rad.width();
                    self.display_atoms.push(display_rad);
                    self.current_position.x += width;

                    // add super scripts || subscripts
                    if has_scripts {
                        self.make_scripts(atom, target, index_range.location, 0.0);
                    }
                    // change type to ordinary
                    //atom.type = .ordinary;
                }
                MTMathAtomType::Fraction => {
                    // stash the existing layout
                    if self.current_line_length() > 0 {
                        self.add_display_line();
                    }
                    self.add_inter_element_space(prev_node.as_ref(), type_);
                    let mut display = self.make_fraction(atom);
                    let (index_range, has_scripts) = {
                        let a = atom.borrow();
                        (
                            a.index_range,
                            a.sub_script().is_some() || a.super_script().is_some(),
                        )
                    };
                    let target = has_scripts.then(|| ScriptTarget::of(&mut display));
                    let width = display.width();
                    self.display_atoms.push(display);
                    self.current_position.x += width;
                    // add super scripts || subscripts
                    if has_scripts {
                        self.make_scripts(atom, target, index_range.location, 0.0);
                    }
                }
                MTMathAtomType::LargeOperator => {
                    // stash the existing layout
                    if self.current_line_length() > 0 {
                        self.add_display_line();
                    }
                    self.add_inter_element_space(prev_node.as_ref(), type_);
                    let display = self.make_large_op(atom);
                    self.display_atoms.push(display);
                }
                MTMathAtomType::Inner => {
                    // stash the existing layout
                    if self.current_line_length() > 0 {
                        self.add_display_line();
                    }
                    self.add_inter_element_space(prev_node.as_ref(), type_);
                    let (has_boundary, inner_list) = {
                        let a = atom.borrow();
                        let inner = a.as_inner().expect("atom as! MTInner");
                        (
                            inner.left_boundary().is_some() || inner.right_boundary().is_some(),
                            inner.inner_list.clone(),
                        )
                    };
                    let mut display = if has_boundary {
                        self.make_left_right(atom)
                    } else {
                        Self::create_line_for_math_list_cramped(
                            inner_list.as_ref(),
                            &self.font.clone(),
                            self.style,
                            self.cramped,
                        )
                        .unwrap()
                    };
                    display.set_position(self.current_position);
                    let (index_range, has_scripts) = {
                        let a = atom.borrow();
                        (
                            a.index_range,
                            a.sub_script().is_some() || a.super_script().is_some(),
                        )
                    };
                    let target = has_scripts.then(|| ScriptTarget::of(&mut display));
                    self.current_position.x += display.width();
                    self.display_atoms.push(display);
                    // add super scripts || subscripts
                    if has_scripts {
                        self.make_scripts(atom, target, index_range.location, 0.0);
                    }
                }
                MTMathAtomType::Underline | MTMathAtomType::Overline | MTMathAtomType::Accent => {
                    // stash the existing layout
                    if self.current_line_length() > 0 {
                        self.add_display_line();
                    }
                    // Underline, overline and accent are considered as Ord in rule 16.
                    self.add_inter_element_space(prev_node.as_ref(), MTMathAtomType::Ordinary);
                    atom.borrow_mut().type_ = MTMathAtomType::Ordinary;

                    let mut display = match type_ {
                        MTMathAtomType::Underline => self.make_underline(atom),
                        MTMathAtomType::Overline => self.make_overline(atom),
                        _ => self.make_accent(atom),
                    };
                    let (index_range, has_scripts) = {
                        let a = atom.borrow();
                        (
                            a.index_range,
                            a.sub_script().is_some() || a.super_script().is_some(),
                        )
                    };
                    let target = has_scripts.then(|| ScriptTarget::of(&mut display));
                    let width = display.width();
                    self.display_atoms.push(display);
                    self.current_position.x += width;
                    // add super scripts || subscripts
                    if has_scripts {
                        self.make_scripts(atom, target, index_range.location, 0.0);
                    }
                }
                MTMathAtomType::Table => {
                    // stash the existing layout
                    if self.current_line_length() > 0 {
                        self.add_display_line();
                    }
                    // We will consider tables as inner
                    self.add_inter_element_space(prev_node.as_ref(), MTMathAtomType::Inner);
                    atom.borrow_mut().type_ = MTMathAtomType::Inner;

                    let display = self.make_table(atom);
                    let width = display.width();
                    self.display_atoms.push(display);
                    self.current_position.x += width;
                    // A table doesn't have subscripts or superscripts
                }
                MTMathAtomType::Ordinary
                | MTMathAtomType::BinaryOperator
                | MTMathAtomType::Relation
                | MTMathAtomType::Open
                | MTMathAtomType::Close
                | MTMathAtomType::Placeholder
                | MTMathAtomType::Punctuation => {
                    // the rendering for all the rest is pretty similar
                    // All we need is render the character and set the interelement space.
                    if let Some(prev) = &prev_node {
                        let inter_element_space =
                            self.get_inter_element_space(atom_type(prev), type_);
                        if self.current_line_length() > 0 {
                            if inter_element_space > 0.0 {
                                // add a kerning of that space to the previous character
                                self.add_kern_to_last_character(inter_element_space);
                            }
                        } else {
                            // increase the space
                            self.current_position.x += inter_element_space;
                        }
                    }
                    let (nucleus, index_range, fused_atoms, has_sub, has_super) = {
                        let a = atom.borrow();
                        (
                            a.nucleus.clone(),
                            a.index_range,
                            a.fused_atoms.clone(),
                            a.sub_script().is_some(),
                            a.super_script().is_some(),
                        )
                    };
                    let nucleus_string = NSString::from_str(&nucleus);
                    let current: Retained<NSAttributedString> = if type_
                        == MTMathAtomType::Placeholder
                    {
                        let color = Self::placeholder_color().CGColor();
                        let attributed = NSMutableAttributedString::from_nsstring(&nucleus_string);
                        unsafe {
                            attributed.addAttribute_value_range(
                                key(kCTForegroundColorAttributeName),
                                cf_object(&*color),
                                NSRange::new(0, attributed.length()),
                            );
                        }
                        Retained::into_super(attributed)
                    } else {
                        NSAttributedString::from_nsstring(&nucleus_string)
                    };
                    self.current_line.appendAttributedString(&current);
                    // add the atom to the current range
                    if self.current_line_index_range.location == NS_NOT_FOUND {
                        self.current_line_index_range = index_range;
                    } else {
                        self.current_line_index_range.length += index_range.length;
                    }
                    // add the fused atoms
                    if !fused_atoms.is_empty() {
                        self.current_atoms.extend(fused_atoms);
                    } else {
                        self.current_atoms.push(atom.clone());
                    }

                    // add super scripts || subscripts
                    if has_sub || has_super {
                        // stash the existing line
                        // We don't check currentLine.length here since we want to allow empty lines with super/sub scripts.
                        self.add_display_line();
                        let mut delta: CGFloat = 0.0;
                        if !nucleus.is_empty() {
                            // Use the italic correction of the last character.
                            let last = swift::last_character(&nucleus).unwrap();
                            let glyph = self.find_glyph_for_character(last);
                            delta = self.style_font().table().get_italic_correction(glyph);
                        }
                        if delta > 0.0 && !has_sub {
                            // Add a kern of delta
                            self.current_position.x += delta;
                        }
                        let target = ScriptTarget::of(self.display_atoms.last_mut().unwrap());
                        self.make_scripts(
                            atom,
                            Some(target),
                            (index_range.location + index_range.length).wrapping_sub(1),
                            delta,
                        );
                    }
                }
            }
            last_type = Some(atom_type(atom));
            prev_node = Some(atom.clone());
        }
        if self.current_line_length() > 0 {
            self.add_display_line();
        }
        if self.spaced
            && let Some(last_type) = last_type
        {
            // If spaced then add an interelement space between the last type and close
            let inter_element_space =
                self.get_inter_element_space(last_type, MTMathAtomType::Close);
            if let Some(display) = self.display_atoms.last_mut() {
                let width = display.width();
                display.set_width(width + inter_element_space);
            }
        }
    }

    /// Kerns the last composed character sequence of the current line.
    fn add_kern_to_last_character(&mut self, inter_element_space: CGFloat) {
        let range = self
            .current_line
            .mutableString()
            .rangeOfComposedCharacterSequenceAtIndex(self.current_line.length() - 1);
        let value = NSNumber::numberWithDouble(inter_element_space);
        unsafe {
            self.current_line
                .addAttribute_value_range(key(kCTKernAttributeName), &value, range);
        }
    }

    fn add_display_line(&mut self) -> usize {
        // add the font
        let style_font = self.style_font();
        unsafe {
            self.current_line.addAttribute_value_range(
                key(kCTFontAttributeName),
                cf_object(style_font.ct_font()),
                NSRange::new(0, self.current_line.length()),
            );
        }
        let line = std::mem::replace(&mut self.current_line, NSMutableAttributedString::new());
        let atoms = std::mem::take(&mut self.current_atoms);
        let display_atom = MTDisplay::ct_line(
            Retained::into_super(line),
            self.current_position,
            self.current_line_index_range,
            Some(&style_font),
            atoms,
        );
        let width = display_atom.width();
        self.display_atoms.push(display_atom);
        // update the position
        self.current_position.x += width;
        // clear the range
        self.current_line_index_range = NSRange::new(NS_NOT_FOUND, NS_NOT_FOUND);
        self.display_atoms.len() - 1
    }

    // MARK: - Spacing

    /// Returned in units of mu = 1/18 em.
    fn get_spacing_in_mu(&self, type_: InterElementSpaceType) -> i32 {
        match type_ {
            S::Invalid => -1,
            S::None => 0,
            S::Thin => 3,
            S::NsThin => {
                if self.style.is_not_script() {
                    3
                } else {
                    0
                }
            }
            S::NsMedium => {
                if self.style.is_not_script() {
                    4
                } else {
                    0
                }
            }
            S::NsThick => {
                if self.style.is_not_script() {
                    5
                } else {
                    0
                }
            }
        }
    }

    fn get_inter_element_space(&mut self, left: MTMathAtomType, right: MTMathAtomType) -> CGFloat {
        let left_index = get_inter_element_space_array_index_for_type(left, true);
        let right_index = get_inter_element_space_array_index_for_type(right, false);
        let space_array = &INTER_ELEMENT_SPACE_ARRAY[left_index];
        let space_type = space_array[right_index];

        let space_multipler = self.get_spacing_in_mu(space_type);
        if space_multipler > 0 {
            // 1 em = size of font in pt. space multipler is in multiples mu or 1/18 em
            return space_multipler as CGFloat * self.style_font().table().mu_unit();
        }
        0.0
    }

    // MARK: - Subscript/Superscript

    fn script_style(&self) -> MTLineStyle {
        match self.style {
            MTLineStyle::Display | MTLineStyle::Text => MTLineStyle::Script,
            MTLineStyle::Script | MTLineStyle::ScriptOfScript => MTLineStyle::ScriptOfScript,
        }
    }

    /// subscript is always cramped
    fn subscript_cramped(&self) -> bool {
        true
    }

    /// superscript is cramped only if the current style is cramped
    fn super_script_cramped(&self) -> bool {
        self.cramped
    }

    fn super_script_shift_up(&mut self) -> CGFloat {
        if self.cramped {
            self.style_font().table().superscript_shift_up_cramped()
        } else {
            self.style_font().table().superscript_shift_up()
        }
    }

    /// Make scripts for the last atom. `index` is the index of the element
    /// which is getting the sub/super scripts.
    fn make_scripts(
        &mut self,
        atom: &MTMathAtomRef,
        display: Option<ScriptTarget>,
        index: usize,
        delta: CGFloat,
    ) {
        let mut super_script_shift_up: CGFloat = 0.0;
        let mut subscript_shift_down: CGFloat = 0.0;

        let display_is_line = display.is_some_and(|display| display.is_ct_line);
        if !display_is_line {
            let display = display.expect("display!");
            // get the font in script style
            let script_font_size = Self::get_style_size(self.script_style(), &self.font);
            let script_font = self.font.copy_with_size(script_font_size);
            let script_font_metrics = script_font.table();

            // if it is not a simple line then
            super_script_shift_up =
                display.ascent - script_font_metrics.superscript_baseline_drop_max();
            subscript_shift_down =
                display.descent + script_font_metrics.subscript_baseline_drop_min();
        }

        let (super_list, sub_list) = {
            let a = atom.borrow();
            (a.super_script().cloned(), a.sub_script().cloned())
        };
        let font = self.font.clone();
        let index = index as isize;

        let Some(super_list) = super_list else {
            let mut subscript = Self::create_line_for_math_list_cramped(
                sub_list.as_ref(),
                &font,
                self.script_style(),
                self.subscript_cramped(),
            )
            .unwrap();
            if let Some(list) = subscript.as_math_list_mut() {
                list.type_ = LinePosition::Subscript;
                list.index = index;
            }

            let table_font = self.style_font();
            let table = table_font.table();
            subscript_shift_down = swift::fmax(subscript_shift_down, table.subscript_shift_down());
            subscript_shift_down = swift::fmax(
                subscript_shift_down,
                subscript.ascent() - table.subscript_top_max(),
            );
            // add the subscript
            subscript.set_position(CGPoint::new(
                self.current_position.x,
                self.current_position.y - subscript_shift_down,
            ));
            let width = subscript.width();
            self.display_atoms.push(subscript);
            // update the position
            self.current_position.x += width + table.space_after_script();
            return;
        };

        let mut super_script = Self::create_line_for_math_list_cramped(
            Some(&super_list),
            &font,
            self.script_style(),
            self.super_script_cramped(),
        )
        .unwrap();
        if let Some(list) = super_script.as_math_list_mut() {
            list.type_ = LinePosition::Superscript;
            list.index = index;
        }
        let shift_up = self.super_script_shift_up();
        super_script_shift_up = swift::fmax(super_script_shift_up, shift_up);
        let table_font = self.style_font();
        let table = table_font.table();
        super_script_shift_up = swift::fmax(
            super_script_shift_up,
            super_script.descent() + table.superscript_bottom_min(),
        );

        let Some(sub_list) = sub_list else {
            super_script.set_position(CGPoint::new(
                self.current_position.x,
                self.current_position.y + super_script_shift_up,
            ));
            let width = super_script.width();
            self.display_atoms.push(super_script);
            // update the position
            self.current_position.x += width + table.space_after_script();
            return;
        };
        let mut ssubscript = Self::create_line_for_math_list_cramped(
            Some(&sub_list),
            &font,
            self.script_style(),
            self.subscript_cramped(),
        )
        .unwrap();
        if let Some(list) = ssubscript.as_math_list_mut() {
            list.type_ = LinePosition::Subscript;
            list.index = index;
        }
        subscript_shift_down = swift::fmax(subscript_shift_down, table.subscript_shift_down());

        // joint positioning of subscript & superscript
        let sub_super_script_gap = (super_script_shift_up - super_script.descent())
            + (subscript_shift_down - ssubscript.ascent());
        if sub_super_script_gap < table.sub_superscript_gap_min() {
            // Set the gap to atleast as much
            subscript_shift_down += table.sub_superscript_gap_min() - sub_super_script_gap;
            let superscript_bottom_delta = table.superscript_bottom_max_with_subscript()
                - (super_script_shift_up - super_script.descent());
            if superscript_bottom_delta > 0.0 {
                // superscript is lower than the max allowed by the font with a subscript.
                super_script_shift_up += superscript_bottom_delta;
                subscript_shift_down -= superscript_bottom_delta;
            }
        }
        // The delta is the italic correction above that shift superscript position
        super_script.set_position(CGPoint::new(
            self.current_position.x + delta,
            self.current_position.y + super_script_shift_up,
        ));
        let super_width = super_script.width();
        self.display_atoms.push(super_script);
        ssubscript.set_position(CGPoint::new(
            self.current_position.x,
            self.current_position.y - subscript_shift_down,
        ));
        let sub_width = ssubscript.width();
        self.display_atoms.push(ssubscript);
        self.current_position.x +=
            swift::max(super_width + delta, sub_width) + table.space_after_script();
    }

    // MARK: - Fractions

    fn numerator_shift_up(&mut self, has_rule: bool) -> CGFloat {
        let display = self.style == MTLineStyle::Display;
        let font = self.style_font();
        let table = font.table();
        match (has_rule, display) {
            (true, true) => table.fraction_numerator_display_style_shift_up(),
            (true, false) => table.fraction_numerator_shift_up(),
            (false, true) => table.stack_top_display_style_shift_up(),
            (false, false) => table.stack_top_shift_up(),
        }
    }

    fn numerator_gap_min(&mut self) -> CGFloat {
        let display = self.style == MTLineStyle::Display;
        let font = self.style_font();
        if display {
            font.table().fraction_numerator_display_style_gap_min()
        } else {
            font.table().fraction_numerator_gap_min()
        }
    }

    fn denominator_shift_down(&mut self, has_rule: bool) -> CGFloat {
        let display = self.style == MTLineStyle::Display;
        let font = self.style_font();
        let table = font.table();
        match (has_rule, display) {
            (true, true) => table.fraction_denominator_display_style_shift_down(),
            (true, false) => table.fraction_denominator_shift_down(),
            (false, true) => table.stack_bottom_display_style_shift_down(),
            (false, false) => table.stack_bottom_shift_down(),
        }
    }

    fn denominator_gap_min(&mut self) -> CGFloat {
        let display = self.style == MTLineStyle::Display;
        let font = self.style_font();
        if display {
            font.table().fraction_denominator_display_style_gap_min()
        } else {
            font.table().fraction_denominator_gap_min()
        }
    }

    fn stack_gap_min(&mut self) -> CGFloat {
        let display = self.style == MTLineStyle::Display;
        let font = self.style_font();
        if display {
            font.table().stack_display_style_gap_min()
        } else {
            font.table().stack_gap_min()
        }
    }

    fn fraction_delimiter_height(&mut self) -> CGFloat {
        let display = self.style == MTLineStyle::Display;
        let font = self.style_font();
        if display {
            font.table().fraction_delimiter_display_style_size()
        } else {
            font.table().fraction_delimiter_size()
        }
    }

    fn fraction_style(&self) -> MTLineStyle {
        if self.style == MTLineStyle::ScriptOfScript {
            return MTLineStyle::ScriptOfScript;
        }
        self.style.inc()
    }

    fn make_fraction(&mut self, atom: &MTMathAtomRef) -> MTDisplay {
        let (numerator, denominator, has_rule, left_delimiter, right_delimiter, index_range) = {
            let a = atom.borrow();
            let frac = a.as_fraction().expect("atom as! MTFraction");
            (
                frac.numerator.clone(),
                frac.denominator.clone(),
                frac.has_rule,
                frac.left_delimiter.clone(),
                frac.right_delimiter.clone(),
                a.index_range,
            )
        };
        let font = self.font.clone();
        // lay out the parts of the fraction
        let numerator_display = Self::create_line_for_math_list_cramped(
            numerator.as_ref(),
            &font,
            self.fraction_style(),
            false,
        )
        .unwrap();
        let denominator_display = Self::create_line_for_math_list_cramped(
            denominator.as_ref(),
            &font,
            self.fraction_style(),
            true,
        )
        .unwrap();

        // determine the location of the numerator
        let mut numerator_shift_up = self.numerator_shift_up(has_rule);
        let mut denominator_shift_down = self.denominator_shift_down(has_rule);
        let bar_location = self.style_font().table().axis_height();
        let bar_thickness = if has_rule {
            self.style_font().table().fraction_rule_thickness()
        } else {
            0.0
        };

        if has_rule {
            // This is the difference between the lowest edge of the numerator and the top edge of the fraction bar
            let distance_from_numerator_to_bar = (numerator_shift_up - numerator_display.descent())
                - (bar_location + bar_thickness / 2.0);
            // The distance should at least be displayGap
            if distance_from_numerator_to_bar < self.numerator_gap_min() {
                // This makes the distance between the bottom of the numerator and the top edge of the fraction bar
                // at least minNumeratorGap.
                numerator_shift_up += self.numerator_gap_min() - distance_from_numerator_to_bar;
            }

            // Do the same for the denominator
            // This is the difference between the top edge of the denominator and the bottom edge of the fraction bar
            let distance_from_denominator_to_bar = (bar_location - bar_thickness / 2.0)
                - (denominator_display.ascent() - denominator_shift_down);
            // The distance should at least be denominator gap
            if distance_from_denominator_to_bar < self.denominator_gap_min() {
                // This makes the distance between the top of the denominator and the bottom of the fraction bar to be exactly
                // minDenominatorGap
                denominator_shift_down +=
                    self.denominator_gap_min() - distance_from_denominator_to_bar;
            }
        } else {
            // This is the distance between the numerator and the denominator
            let clearance = (numerator_shift_up - numerator_display.descent())
                - (denominator_display.ascent() - denominator_shift_down);
            // This is the minimum clearance between the numerator and denominator.
            let min_gap = self.stack_gap_min();
            if clearance < min_gap {
                numerator_shift_up += (min_gap - clearance) / 2.0;
                denominator_shift_down += (min_gap - clearance) / 2.0;
            }
        }

        let mut display = MTDisplay::fraction(
            numerator_display,
            denominator_display,
            self.current_position,
            index_range,
        );

        display.set_numerator_up(numerator_shift_up);
        display.set_denominator_down(denominator_shift_down);
        display.set_fraction_line(bar_thickness, bar_location);
        if left_delimiter.is_empty() && right_delimiter.is_empty() {
            display
        } else {
            self.add_delimiters_to_fraction_display(
                display,
                &left_delimiter,
                &right_delimiter,
                index_range,
            )
        }
    }

    // The last `position.x +=` is a dead store in SwiftMath too.
    #[allow(unused_assignments)]
    fn add_delimiters_to_fraction_display(
        &mut self,
        mut display: MTDisplay,
        left_delimiter: &str,
        right_delimiter: &str,
        index_range: NSRange,
    ) -> MTDisplay {
        let mut inner_elements = Vec::new();
        let mut position = CGPoint::new(0.0, 0.0);
        if !left_delimiter.is_empty() {
            let glyph_height = self.fraction_delimiter_height();
            let mut left_glyph = self.find_glyph_for_boundary(left_delimiter, glyph_height);
            left_glyph.set_position(position);
            position.x += left_glyph.width();
            inner_elements.push(left_glyph);
        }

        display.set_position(position);
        position.x += display.width();
        inner_elements.push(display);

        if !right_delimiter.is_empty() {
            let glyph_height = self.fraction_delimiter_height();
            let mut right_glyph = self.find_glyph_for_boundary(right_delimiter, glyph_height);
            right_glyph.set_position(position);
            position.x += right_glyph.width();
            inner_elements.push(right_glyph);
        }
        let mut inner_display = MTDisplay::math_list(inner_elements, index_range);
        inner_display.set_position(self.current_position);
        inner_display
    }

    // MARK: - Radicals

    fn radical_vertical_gap(&mut self) -> CGFloat {
        let display = self.style == MTLineStyle::Display;
        let font = self.style_font();
        if display {
            font.table().radical_display_style_vertical_gap()
        } else {
            font.table().radical_vertical_gap()
        }
    }

    fn get_radical_glyph_with_height(&mut self, radical_height: CGFloat) -> MTDisplay {
        let (mut glyph_ascent, mut glyph_descent, mut glyph_width) = (0.0, 0.0, 0.0);

        let radical_glyph = self.find_glyph_for_character("\u{221A}");
        let glyph = self.find_glyph(
            radical_glyph,
            radical_height,
            &mut glyph_ascent,
            &mut glyph_descent,
            &mut glyph_width,
        );

        let mut glyph_display = None;
        if glyph_ascent + glyph_descent < radical_height {
            // the glyphs is not as large as required. A glyph needs to be constructed using the extenders.
            glyph_display = self.construct_glyph(radical_glyph, radical_height);
        }

        glyph_display.unwrap_or_else(|| {
            // No constructed display so use the glyph we got.
            let mut display = MTDisplay::glyph(
                glyph,
                NSRange::new(NS_NOT_FOUND, 0),
                Some(self.style_font()),
            );
            display.set_ascent(glyph_ascent);
            display.set_descent(glyph_descent);
            display.set_width(glyph_width);
            display
        })
    }

    fn make_radical(&mut self, radicand: Option<&MTMathListRef>, range: NSRange) -> MTDisplay {
        let font = self.font.clone();
        let inner_display =
            Self::create_line_for_math_list_cramped(radicand, &font, self.style, true).unwrap();
        let mut clearance = self.radical_vertical_gap();
        let radical_rule_thickness = self.style_font().table().radical_rule_thickness();
        let radical_height =
            inner_display.ascent() + inner_display.descent() + clearance + radical_rule_thickness;

        let mut glyph = self.get_radical_glyph_with_height(radical_height);

        // Note this is a departure from Latex. Latex assumes that glyphAscent == thickness.
        // Open type math makes no such assumption, and ascent and descent are independent of the thickness.
        // Latex computes delta as descent - (h(inner) + d(inner) + clearance)
        // but since we may not have ascent == thickness, we modify the delta calculation slightly.
        // If the font designer followes Latex conventions, it will be identical.
        let delta = (glyph.descent() + glyph.ascent())
            - (inner_display.ascent()
                + inner_display.descent()
                + clearance
                + radical_rule_thickness);
        if delta > 0.0 {
            clearance += delta / 2.0; // increase the clearance to center the radicand inside the sign.
        }

        // we need to shift the radical glyph up, to coincide with the baseline of inner.
        // The new ascent of the radical glyph should be thickness + adjusted clearance + h(inner)
        let radical_ascent = radical_rule_thickness + clearance + inner_display.ascent();
        let shift_up = radical_ascent - glyph.ascent(); // Note: if the font designer followed latex conventions, this is the same as glyphAscent == thickness.
        glyph.set_shift_down(-shift_up);

        let (glyph_ascent, glyph_descent, glyph_width) =
            (glyph.ascent(), glyph.descent(), glyph.width());
        let (inner_descent, inner_width) = (inner_display.descent(), inner_display.width());
        let mut radical = MTDisplay::radical(inner_display, glyph, self.current_position, range);
        let extra_ascender = self.style_font().table().radical_extra_ascender();
        radical.set_ascent(radical_ascent + extra_ascender);
        radical.set_radical_metrics(extra_ascender, radical_rule_thickness);
        // Note: Until we have radical construction from parts, it is possible that glyphAscent+glyphDescent is less
        // than the requested height of the glyph (i.e. radicalHeight), so in the case the innerDisplay has a larger
        // descent we use the innerDisplay's descent.
        radical.set_descent(swift::max(
            glyph_ascent + glyph_descent - radical_ascent,
            inner_descent,
        ));
        radical.set_width(glyph_width + inner_width);
        radical
    }

    // MARK: - Glyphs

    fn find_glyph(
        &mut self,
        glyph: CGGlyph,
        height: CGFloat,
        glyph_ascent: &mut CGFloat,
        glyph_descent: &mut CGFloat,
        glyph_width: &mut CGFloat,
    ) -> CGGlyph {
        let style_font = self.style_font();
        let mut glyphs = style_font.table().get_vertical_variants_for_glyph(glyph);
        let num_variants = glyphs.len();

        let mut bboxes =
            vec![CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(0.0, 0.0)); num_variants];
        let mut advances = vec![CGSize::new(0.0, 0.0); num_variants];

        // Get the bounds for these glyphs
        bounding_rects_and_advances(&style_font, &mut glyphs, &mut bboxes, &mut advances);
        let (mut ascent, mut descent, mut width) = (0.0, 0.0, 0.0);
        for i in 0..num_variants {
            let bounds = bboxes[i];
            width = advances[i].width;
            get_bbox_details(bounds, &mut ascent, &mut descent);

            if ascent + descent >= height {
                *glyph_ascent = ascent;
                *glyph_descent = descent;
                *glyph_width = width;
                return glyphs[i];
            }
        }
        *glyph_ascent = ascent;
        *glyph_descent = descent;
        *glyph_width = width;
        glyphs[num_variants.wrapping_sub(1)]
    }

    fn construct_glyph(&mut self, glyph: CGGlyph, glyph_height: CGFloat) -> Option<MTDisplay> {
        let style_font = self.style_font();
        let parts = style_font.table().get_vertical_glyph_assembly(glyph);
        if parts.is_empty() {
            return None;
        }
        let mut glyphs: Vec<CGGlyph> = Vec::new();
        let mut offsets: Vec<f64> = Vec::new();
        let mut height: CGFloat = 0.0;
        self.construct_glyph_with_parts(
            &parts,
            glyph_height,
            &mut glyphs,
            &mut offsets,
            &mut height,
        );
        let mut first = glyphs[0];
        let width = unsafe {
            style_font.ct_font().advances_for_glyphs(
                CTFontOrientation::Horizontal,
                std::ptr::NonNull::from(&mut first),
                std::ptr::null_mut(),
                1,
            )
        };
        let offsets: Vec<f32> = offsets.iter().map(|&offset| offset as f32).collect();
        let mut display = MTDisplay::glyph_construction(&glyphs, &offsets, Some(style_font));
        display.set_width(width);
        display.set_ascent(height);
        display.set_descent(0.0); // it's upto the rendering to adjust the display up or down.
        Some(display)
    }

    /// `offsets` hold the `NSNumber` values: doubles, or floats after spreading.
    fn construct_glyph_with_parts(
        &mut self,
        parts: &[GlyphPart],
        glyph_height: CGFloat,
        glyphs: &mut Vec<CGGlyph>,
        offsets: &mut Vec<f64>,
        height: &mut CGFloat,
    ) {
        // Loop forever until the glyph height is valid
        let min_distance = self.style_font().table().min_connector_overlap();
        for num_extenders in 0..usize::MAX {
            let mut glyphs_rv: Vec<CGGlyph> = Vec::new();
            let mut offsets_rv: Vec<f64> = Vec::new();

            let mut prev: Option<GlyphPart> = None;
            let mut min_offset: CGFloat = 0.0;
            let mut max_delta = CGFloat::MAX; // the maximum amount we can increase the offsets by

            for part in parts {
                let mut repeats = 1;
                if part.is_extender {
                    repeats = num_extenders;
                }
                // add the extender num extender times
                for _ in 0..repeats {
                    glyphs_rv.push(part.glyph);
                    if let Some(prev) = prev {
                        let max_overlap =
                            swift::min(prev.end_connector_length, part.start_connector_length);
                        // the minimum amount we can add to the offset
                        let min_offset_delta = prev.full_advance - max_overlap;
                        // The maximum amount we can add to the offset.
                        let max_offset_delta = prev.full_advance - min_distance;
                        // we can increase the offsets by at most max - min.
                        max_delta = swift::min(max_delta, max_offset_delta - min_offset_delta);
                        min_offset += min_offset_delta;
                    }
                    offsets_rv.push(min_offset);
                    prev = Some(*part);
                }
            }

            let Some(prev) = prev else {
                continue; // maybe only extenders
            };
            let min_height = min_offset + prev.full_advance;
            let max_height = min_height + max_delta * (glyphs_rv.len() as CGFloat - 1.0);
            if min_height >= glyph_height {
                // we are done
                *glyphs = glyphs_rv;
                *offsets = offsets_rv;
                *height = min_height;
                return;
            } else if glyph_height <= max_height {
                // spread the delta equally between all the connectors
                let delta = glyph_height - min_height;
                let delta_increase = delta as f32 / (glyphs_rv.len() as f32 - 1.0);
                let mut last_offset: CGFloat = 0.0;
                for (i, offset_slot) in offsets_rv.iter_mut().enumerate() {
                    let offset = *offset_slot as f32 + i as f32 * delta_increase;
                    *offset_slot = offset as f64;
                    last_offset = offset as CGFloat;
                }
                // we are done
                *glyphs = glyphs_rv;
                *offsets = offsets_rv;
                *height = last_offset + prev.full_advance;
                return;
            }
        }
    }

    /// `findGlyphForCharacterAtIndex(_:inString:)`, given the character.
    fn find_glyph_for_character(&mut self, character: &str) -> CGGlyph {
        // Get the character at index taking into account UTF-32 characters
        let mut chars: Vec<u16> = character.encode_utf16().collect();

        // Get the glyph from the font
        let mut glyph = vec![0 as CGGlyph; chars.len()];
        let style_font = self.style_font();
        let found = match (
            std::ptr::NonNull::new(chars.as_mut_ptr()),
            std::ptr::NonNull::new(glyph.as_mut_ptr()),
        ) {
            (Some(chars_ptr), Some(glyph_ptr)) => unsafe {
                style_font.ct_font().glyphs_for_characters(
                    chars_ptr,
                    glyph_ptr,
                    chars.len() as isize,
                )
            },
            _ => false,
        };
        if !found {
            // the font did not contain a glyph for our character, so we just return 0 (notdef)
            return 0;
        }
        glyph[0]
    }

    // MARK: - Large Operators

    fn make_large_op(&mut self, atom: &MTMathAtomRef) -> MTDisplay {
        let (nucleus, op_limits, index_range, has_sub) = {
            let a = atom.borrow();
            let op = a.as_large_operator().expect("atom as! MTLargeOperator");
            (
                a.nucleus.clone(),
                op.limits,
                a.index_range,
                a.sub_script().is_some(),
            )
        };
        let limits = op_limits && self.style == MTLineStyle::Display;
        let delta: CGFloat;
        if swift::count(&nucleus) == 1 {
            let mut glyph =
                self.find_glyph_for_character(swift::first_character(&nucleus).unwrap());
            let style_font = self.style_font();
            if self.style == MTLineStyle::Display && glyph != 0 {
                // Enlarge the character in display style.
                glyph = style_font.table().get_larger_glyph(glyph);
            }
            // This is be the italic correction of the character.
            delta = style_font.table().get_italic_correction(glyph);

            // vertically center
            let bbox = bounding_rect_for_glyph(style_font.ct_font(), glyph);
            let width = unsafe {
                style_font.ct_font().advances_for_glyphs(
                    CTFontOrientation::Horizontal,
                    std::ptr::NonNull::from(&mut glyph),
                    std::ptr::null_mut(),
                    1,
                )
            };
            let (mut ascent, mut descent) = (0.0, 0.0);
            get_bbox_details(bbox, &mut ascent, &mut descent);
            let shift_down = 0.5 * (ascent - descent) - style_font.table().axis_height();
            let mut glyph_display = MTDisplay::glyph(glyph, index_range, Some(style_font.clone()));
            glyph_display.set_ascent(ascent);
            glyph_display.set_descent(descent);
            glyph_display.set_width(width);
            if has_sub && !limits {
                // Remove italic correction from the width of the glyph if
                // there is a subscript and limits is not set.
                let width = glyph_display.width();
                glyph_display.set_width(width - delta);
            }
            glyph_display.set_shift_down(shift_down);
            glyph_display.set_position(self.current_position);
            self.add_limits_to_display(glyph_display, atom, delta)
        } else {
            // Create a regular node
            let line = NSMutableAttributedString::from_nsstring(&NSString::from_str(&nucleus));
            // add the font
            let style_font = self.style_font();
            unsafe {
                line.addAttribute_value_range(
                    key(kCTFontAttributeName),
                    cf_object(style_font.ct_font()),
                    NSRange::new(0, line.length()),
                );
            }
            let display_atom = MTDisplay::ct_line(
                Retained::into_super(line),
                self.current_position,
                index_range,
                Some(&style_font),
                vec![atom.clone()],
            );
            delta = 0.0;
            self.add_limits_to_display(display_atom, atom, delta)
        }
    }

    fn add_limits_to_display(
        &mut self,
        mut display: MTDisplay,
        op: &MTMathAtomRef,
        delta: CGFloat,
    ) -> MTDisplay {
        let (super_list, sub_list, limits, index_range) = {
            let a = op.borrow();
            (
                a.super_script().cloned(),
                a.sub_script().cloned(),
                a.as_large_operator().unwrap().limits,
                a.index_range,
            )
        };
        // If there is no subscript or superscript, just return the current display
        if sub_list.is_none() && super_list.is_none() {
            self.current_position.x += display.width();
            return display;
        }
        if limits && self.style == MTLineStyle::Display {
            // make limits
            let font = self.font.clone();
            let mut super_script = None;
            let mut sub_script = None;
            if super_list.is_some() {
                super_script = Self::create_line_for_math_list_cramped(
                    super_list.as_ref(),
                    &font,
                    self.script_style(),
                    self.super_script_cramped(),
                );
            }
            if sub_list.is_some() {
                sub_script = Self::create_line_for_math_list_cramped(
                    sub_list.as_ref(),
                    &font,
                    self.script_style(),
                    self.subscript_cramped(),
                );
            }
            let super_descent = super_script.as_ref().map(MTDisplay::descent);
            let sub_ascent = sub_script.as_ref().map(MTDisplay::ascent);
            let mut ops_display =
                MTDisplay::large_op_limits(display, super_script, sub_script, delta / 2.0, 0.0);
            let style_font = self.style_font();
            let table = style_font.table();
            if let Some(super_descent) = super_descent {
                let upper_limit_gap = swift::max(
                    table.upper_limit_gap_min(),
                    table.upper_limit_baseline_rise_min() - super_descent,
                );
                ops_display.set_upper_limit_gap(upper_limit_gap);
            }
            if let Some(sub_ascent) = sub_ascent {
                let lower_limit_gap = swift::max(
                    table.lower_limit_gap_min(),
                    table.lower_limit_baseline_drop_min() - sub_ascent,
                );
                ops_display.set_lower_limit_gap(lower_limit_gap);
            }
            ops_display.set_position(self.current_position);
            ops_display.range = index_range;
            self.current_position.x += ops_display.width();
            ops_display
        } else {
            self.current_position.x += display.width();
            let target = ScriptTarget::of(&mut display);
            self.make_scripts(op, Some(target), index_range.location, delta);
            display
        }
    }

    // MARK: - Large delimiters

    // The last `position.x +=` is a dead store in SwiftMath too.
    #[allow(unused_assignments)]
    fn make_left_right(&mut self, atom: &MTMathAtomRef) -> MTDisplay {
        let (inner_list, left, right, index_range) = {
            let a = atom.borrow();
            let inner = a.as_inner().expect("atom as! MTInner");
            (
                inner.inner_list.clone(),
                inner.left_boundary().map(|b| b.borrow().nucleus.clone()),
                inner.right_boundary().map(|b| b.borrow().nucleus.clone()),
                a.index_range,
            )
        };
        let font = self.font.clone();
        let mut inner_list_display = Self::create_line_for_math_list_spaced(
            inner_list.as_ref(),
            &font,
            self.style,
            self.cramped,
            true,
        )
        .unwrap();
        let axis_height = self.style_font().table().axis_height();
        // delta is the max distance from the axis
        let delta = swift::max(
            inner_list_display.ascent() - axis_height,
            inner_list_display.descent() + axis_height,
        );
        let d1 = (delta / 500.0) * K_DELIMITER_FACTOR; // This represents atleast 90% of the formula
        let d2 = 2.0 * delta - K_DELIMITER_SHORTFALL_POINTS; // This represents a shortfall of 5pt
        // The size of the delimiter glyph should cover at least 90% of the formula or
        // be at most 5pt short.
        let glyph_height = swift::max(d1, d2);

        let mut inner_elements = Vec::new();
        let mut position = CGPoint::new(0.0, 0.0);
        if let Some(left) = left.filter(|nucleus| !nucleus.is_empty()) {
            let mut left_glyph = self.find_glyph_for_boundary(&left, glyph_height);
            left_glyph.set_position(position);
            position.x += left_glyph.width();
            inner_elements.push(left_glyph);
        }

        inner_list_display.set_position(position);
        position.x += inner_list_display.width();
        inner_elements.push(inner_list_display);

        if let Some(right) = right.filter(|nucleus| !nucleus.is_empty()) {
            let mut right_glyph = self.find_glyph_for_boundary(&right, glyph_height);
            right_glyph.set_position(position);
            position.x += right_glyph.width();
            inner_elements.push(right_glyph);
        }
        MTDisplay::math_list(inner_elements, index_range)
    }

    fn find_glyph_for_boundary(&mut self, delimiter: &str, glyph_height: CGFloat) -> MTDisplay {
        let (mut glyph_ascent, mut glyph_descent, mut glyph_width) = (0.0, 0.0, 0.0);
        let left_glyph = self.find_glyph_for_character(swift::first_character(delimiter).unwrap());
        let glyph = self.find_glyph(
            left_glyph,
            glyph_height,
            &mut glyph_ascent,
            &mut glyph_descent,
            &mut glyph_width,
        );

        let mut glyph_display = None;
        if glyph_ascent + glyph_descent < glyph_height {
            // we didn't find a pre-built glyph that is large enough
            glyph_display = self.construct_glyph(left_glyph, glyph_height);
        }

        let mut glyph_display = glyph_display.unwrap_or_else(|| {
            // Create a glyph display
            let mut display = MTDisplay::glyph(
                glyph,
                NSRange::new(NS_NOT_FOUND, 0),
                Some(self.style_font()),
            );
            display.set_ascent(glyph_ascent);
            display.set_descent(glyph_descent);
            display.set_width(glyph_width);
            display
        });
        // Center the glyph on the axis
        let shift_down = 0.5 * (glyph_display.ascent() - glyph_display.descent())
            - self.style_font().table().axis_height();
        glyph_display.set_shift_down(shift_down);
        glyph_display
    }

    // MARK: - Underline/Overline

    fn make_underline(&mut self, atom: &MTMathAtomRef) -> MTDisplay {
        let (inner_list, index_range) = {
            let a = atom.borrow();
            (a.inner_list().cloned(), a.index_range)
        };
        let font = self.font.clone();
        let inner_list_display = Self::create_line_for_math_list_cramped(
            inner_list.as_ref(),
            &font,
            self.style,
            self.cramped,
        )
        .unwrap();
        let (inner_ascent, inner_descent, inner_width) = (
            inner_list_display.ascent(),
            inner_list_display.descent(),
            inner_list_display.width(),
        );
        let mut under_display =
            MTDisplay::line(inner_list_display, self.current_position, index_range);
        let style_font = self.style_font();
        let table = style_font.table();
        // Move the line down by the vertical gap.
        under_display.set_line_metrics(
            -(inner_descent + table.underbar_vertical_gap()),
            table.underbar_rule_thickness(),
        );
        under_display.set_ascent(inner_ascent);
        under_display.set_descent(
            inner_descent
                + table.underbar_vertical_gap()
                + table.underbar_rule_thickness()
                + table.underbar_extra_descender(),
        );
        under_display.set_width(inner_width);
        under_display
    }

    fn make_overline(&mut self, atom: &MTMathAtomRef) -> MTDisplay {
        let (inner_list, index_range) = {
            let a = atom.borrow();
            (a.inner_list().cloned(), a.index_range)
        };
        let font = self.font.clone();
        let inner_list_display =
            Self::create_line_for_math_list_cramped(inner_list.as_ref(), &font, self.style, true)
                .unwrap();
        let (inner_ascent, inner_descent, inner_width) = (
            inner_list_display.ascent(),
            inner_list_display.descent(),
            inner_list_display.width(),
        );
        let mut over_display =
            MTDisplay::line(inner_list_display, self.current_position, index_range);
        let style_font = self.style_font();
        let table = style_font.table();
        over_display.set_line_metrics(
            inner_ascent + table.overbar_vertical_gap(),
            table.underbar_rule_thickness(),
        );
        over_display.set_ascent(
            inner_ascent
                + table.overbar_vertical_gap()
                + table.overbar_rule_thickness()
                + table.overbar_extra_ascender(),
        );
        over_display.set_descent(inner_descent);
        over_display.set_width(inner_width);
        over_display
    }

    // MARK: - Accents

    fn is_single_char_accentee(&self, accent: &MTMathAtom) -> bool {
        let inner_list = accent.inner_list().expect("accent.innerList!").borrow();
        if inner_list.atoms.len() != 1 {
            // Not a single char list.
            return false;
        }
        let inner_atom = inner_list.atoms[0].borrow();
        if swift::count(&inner_atom.nucleus) != 1 {
            // A complex atom, not a simple char.
            return false;
        }
        if inner_atom.sub_script().is_some() || inner_atom.super_script().is_some() {
            return false;
        }
        true
    }

    /// The distance the accent must be moved from the beginning.
    fn get_skew(
        &mut self,
        accent: &MTMathAtomRef,
        width: CGFloat,
        accent_glyph: CGGlyph,
    ) -> CGFloat {
        if accent.borrow().nucleus.is_empty() {
            // No accent
            return 0.0;
        }
        let accent_adjustment = self
            .style_font()
            .table()
            .get_top_accent_adjustment(accent_glyph);
        let accentee_adjustment;
        if !self.is_single_char_accentee(&accent.borrow()) {
            // use the center of the accentee
            accentee_adjustment = width / 2.0;
        } else {
            let nucleus = {
                let a = accent.borrow();
                let inner = a.inner_list().unwrap().borrow();
                let inner_atom = inner.atoms[0].borrow();
                inner_atom.nucleus.clone()
            };
            let accentee_glyph =
                self.find_glyph_for_character(swift::last_character(&nucleus).unwrap());
            accentee_adjustment = self
                .style_font()
                .table()
                .get_top_accent_adjustment(accentee_glyph);
        }
        // The adjustments need to aligned, so skew is just the difference.
        accentee_adjustment - accent_adjustment
    }

    /// Find the largest horizontal variant if exists, with width less than max width.
    fn find_variant_glyph(
        &mut self,
        glyph: CGGlyph,
        max_width: CGFloat,
        glyph_ascent: &mut CGFloat,
        glyph_descent: &mut CGFloat,
        glyph_width: &mut CGFloat,
    ) -> CGGlyph {
        let style_font = self.style_font();
        let mut glyphs = style_font.table().get_horizontal_variants_for_glyph(glyph);
        let num_variants = glyphs.len();

        let mut cur_glyph = glyphs[0]; // if no other glyph is found, we'll return the first one.
        let mut bboxes =
            vec![CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(0.0, 0.0)); num_variants];
        let mut advances = vec![CGSize::new(0.0, 0.0); num_variants];
        // Get the bounds for these glyphs
        bounding_rects_and_advances(&style_font, &mut glyphs, &mut bboxes, &mut advances);
        for i in 0..num_variants {
            let bounds = bboxes[i];
            let (mut ascent, mut descent) = (0.0, 0.0);
            let width = CGRectGetMaxX(bounds);
            get_bbox_details(bounds, &mut ascent, &mut descent);

            if width > max_width {
                if i == 0 {
                    // glyph dimensions are not yet set
                    *glyph_width = advances[i].width;
                    *glyph_ascent = ascent;
                    *glyph_descent = descent;
                }
                return cur_glyph;
            } else {
                cur_glyph = glyphs[i];
                *glyph_width = advances[i].width;
                *glyph_ascent = ascent;
                *glyph_descent = descent;
            }
        }
        // We exhausted all the variants and none was larger than the width, so we return the largest
        cur_glyph
    }

    fn make_accent(&mut self, atom: &MTMathAtomRef) -> MTDisplay {
        let (inner_list, nucleus, index_range) = {
            let a = atom.borrow();
            (a.inner_list().cloned(), a.nucleus.clone(), a.index_range)
        };
        let font = self.font.clone();
        let mut accentee =
            Self::create_line_for_math_list_cramped(inner_list.as_ref(), &font, self.style, true)
                .unwrap();
        if nucleus.is_empty() {
            // no accent!
            return accentee;
        }
        let end = swift::last_character(&nucleus).unwrap();
        let mut accent_glyph = self.find_glyph_for_character(end);
        let accentee_width = accentee.width();
        let (mut glyph_ascent, mut glyph_descent, mut glyph_width) = (0.0, 0.0, 0.0);
        accent_glyph = self.find_variant_glyph(
            accent_glyph,
            accentee_width,
            &mut glyph_ascent,
            &mut glyph_descent,
            &mut glyph_width,
        );
        let delta = swift::min(
            accentee.ascent(),
            self.style_font().table().accent_base_height(),
        );
        let skew = self.get_skew(atom, accentee_width, accent_glyph);
        let height = accentee.ascent() - delta; // This is always positive since delta <= height.
        let accent_position = CGPoint::new(skew, height);
        let mut accent_glyph_display =
            MTDisplay::glyph(accent_glyph, index_range, Some(self.style_font()));
        accent_glyph_display.set_ascent(glyph_ascent);
        accent_glyph_display.set_descent(glyph_descent);
        accent_glyph_display.set_width(glyph_width);
        accent_glyph_display.set_position(accent_position);

        let has_scripts = {
            let a = atom.borrow();
            a.sub_script().is_some() || a.super_script().is_some()
        };
        if self.is_single_char_accentee(&atom.borrow()) && has_scripts {
            // Attach the super/subscripts to the accentee instead of the accent.
            let inner_atom = inner_list.as_ref().unwrap().borrow().atoms[0].clone();
            {
                let mut accent = atom.borrow_mut();
                let (super_script, sub_script) =
                    (accent.super_script().cloned(), accent.sub_script().cloned());
                let mut inner_atom = inner_atom.borrow_mut();
                inner_atom.set_super_script(super_script);
                inner_atom.set_sub_script(sub_script);
                accent.set_super_script(None);
                accent.set_sub_script(None);
            }
            // Remake the accentee (now with sub/superscripts)
            // Note: Latex adjusts the heights in case the height of the char is different in non-cramped mode. However this shouldn't be the case since cramping
            // only affects fractions and superscripts. We skip adjusting the heights.
            accentee = Self::create_line_for_math_list_cramped(
                inner_list.as_ref(),
                &font,
                self.style,
                self.cramped,
            )
            .unwrap();
        }

        let (accentee_ascent, accentee_descent, accentee_width) =
            (accentee.ascent(), accentee.descent(), accentee.width());
        let mut display = MTDisplay::accent(accent_glyph_display, accentee, index_range);
        display.set_width(accentee_width);
        display.set_descent(accentee_descent);
        let ascent = accentee_ascent - delta + glyph_ascent;
        display.set_ascent(swift::max(accentee_ascent, ascent));
        display.set_position(self.current_position);

        display
    }

    // MARK: - Table

    fn make_table(&mut self, atom: &MTMathAtomRef) -> MTDisplay {
        let (num_columns, num_rows, index_range) = {
            let a = atom.borrow();
            let table = a.as_table().expect("atom as! MTMathTable");
            (table.num_columns(), table.num_rows(), a.index_range)
        };
        if num_columns == 0 || num_rows == 0 {
            // Empty table
            return MTDisplay::math_list(Vec::new(), index_range);
        }

        let mut column_widths = vec![0.0; num_columns];
        let displays = self.typeset_cells(atom, &mut column_widths);

        // Position all the columns in each row
        let mut row_displays = Vec::with_capacity(displays.len());
        for row in displays {
            let row_display = self.make_row_with_columns(row, atom, &column_widths);
            row_displays.push(row_display);
        }

        // Position all the rows
        self.position_rows(&mut row_displays, atom);
        let mut table_display = MTDisplay::math_list(row_displays, index_range);
        table_display.set_position(self.current_position);
        table_display
    }

    /// Typeset every cell in the table. As a side-effect calculate the max column width of each column.
    fn typeset_cells(
        &mut self,
        atom: &MTMathAtomRef,
        column_widths: &mut [CGFloat],
    ) -> Vec<Vec<MTDisplay>> {
        let cells: Vec<Vec<MTMathListRef>> = atom.borrow().as_table().unwrap().cells.clone();
        let font = self.font.clone();
        let mut displays = Vec::with_capacity(cells.len());
        for row in &cells {
            let mut col_displays = Vec::with_capacity(row.len());
            for (i, cell) in row.iter().enumerate() {
                let disp = Self::create_line_for_math_list(Some(cell), &font, self.style).unwrap();
                column_widths[i] = swift::max(disp.width(), column_widths[i]);
                col_displays.push(disp);
            }
            displays.push(col_displays);
        }
        displays
    }

    fn make_row_with_columns(
        &mut self,
        mut cols: Vec<MTDisplay>,
        atom: &MTMathAtomRef,
        column_widths: &[CGFloat],
    ) -> MTDisplay {
        let (alignments, inter_column_spacing): (Vec<MTColumnAlignment>, CGFloat) = {
            let a = atom.borrow();
            let table = a.as_table().unwrap();
            (
                (0..cols.len())
                    .map(|i| table.get_alignment_for_column(i))
                    .collect(),
                table.inter_column_spacing,
            )
        };
        let mu_unit = self.style_font().table().mu_unit();
        let mut column_start: CGFloat = 0.0;
        let mut row_range = NSRange::new(NS_NOT_FOUND, 0);
        for (i, col) in cols.iter_mut().enumerate() {
            let col_width = column_widths[i];
            let alignment = alignments[i];
            let mut cell_pos = column_start;
            match alignment {
                MTColumnAlignment::Right => cell_pos += col_width - col.width(),
                MTColumnAlignment::Center => cell_pos += (col_width - col.width()) / 2.0,
                // No changes if left aligned
                MTColumnAlignment::Left => cell_pos += 0.0,
            }
            if row_range.location != NS_NOT_FOUND {
                row_range = union_range(row_range, col.range);
            } else {
                row_range = col.range;
            }

            col.set_position(CGPoint::new(cell_pos, 0.0));
            column_start += col_width + inter_column_spacing * mu_unit;
        }
        // Create a display for the row
        MTDisplay::math_list(cols, row_range)
    }

    fn position_rows(&mut self, rows: &mut [MTDisplay], atom: &MTMathAtomRef) {
        let inter_row_additional_spacing = atom
            .borrow()
            .as_table()
            .unwrap()
            .inter_row_additional_spacing;
        let style_font = self.style_font();
        let font_size = style_font.font_size();
        // Position the rows
        // We will first position the rows starting from 0 and then in the second pass center the whole table vertically.
        let mut curr_pos: CGFloat = 0.0;
        let openup = inter_row_additional_spacing * K_JOT_MULTIPLIER * font_size;
        let baseline_skip = openup + K_BASE_LINE_SKIP_MULTIPLIER * font_size;
        let line_skip = openup + K_LINE_SKIP_MULTIPLIER * font_size;
        let line_skip_limit = openup + K_LINE_SKIP_LIMIT_MULTIPLIER * font_size;
        let mut prev_row_descent: CGFloat = 0.0;
        let mut ascent: CGFloat = 0.0;
        let mut first = true;
        for row in rows.iter_mut() {
            if first {
                row.set_position(CGPoint::new(0.0, 0.0));
                ascent += row.ascent();
                first = false;
            } else {
                let mut skip = baseline_skip;
                if skip - (prev_row_descent + row.ascent()) < line_skip_limit {
                    // rows are too close to each other. Space them apart further
                    skip = prev_row_descent + row.ascent() + line_skip;
                }
                // We are going down so we decrease the y value.
                curr_pos -= skip;
                row.set_position(CGPoint::new(0.0, curr_pos));
            }
            prev_row_descent = row.descent();
        }

        // Vertically center the whole structure around the axis
        // The descent of the structure is the position of the last row
        // plus the descent of the last row.
        let descent = -curr_pos + prev_row_descent;
        let shift_down = 0.5 * (ascent - descent) - style_font.table().axis_height();

        for row in rows.iter_mut() {
            let position = row.position();
            row.set_position(CGPoint::new(position.x, position.y - shift_down));
        }
    }
}

/// `NSUnionRange`.
fn union_range(range1: NSRange, range2: NSRange) -> NSRange {
    let max1 = range1.location.wrapping_add(range1.length);
    let max2 = range2.location.wrapping_add(range2.length);
    let location = range1.location.min(range2.location);
    NSRange::new(location, max1.max(max2).wrapping_sub(location))
}

/// `CTFontGetBoundingRectsForGlyphs` then `CTFontGetAdvancesForGlyphs` over `glyphs`.
fn bounding_rects_and_advances(
    font: &MTFont,
    glyphs: &mut [CGGlyph],
    bboxes: &mut [CGRect],
    advances: &mut [CGSize],
) {
    let count = glyphs.len() as isize;
    let Some(glyph_ptr) = std::ptr::NonNull::new(glyphs.as_mut_ptr()) else {
        return;
    };
    unsafe {
        font.ct_font().bounding_rects_for_glyphs(
            CTFontOrientation::Horizontal,
            glyph_ptr,
            bboxes.as_mut_ptr(),
            count,
        );
        font.ct_font().advances_for_glyphs(
            CTFontOrientation::Horizontal,
            glyph_ptr,
            advances.as_mut_ptr(),
            count,
        );
    }
}
