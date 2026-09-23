//! `MTUnicode.swift`: named code points and SwiftMath's `Character` helpers.

use crate::swift;

/// `UnicodeSymbol`.
pub mod unicode_symbol {
    pub const MULTIPLICATION: &str = "\u{00D7}";
    pub const DIVISION: &str = "\u{00F7}";
    pub const FRACTION_SLASH: &str = "\u{2044}";
    pub const WHITE_SQUARE: &str = "\u{25A1}";
    pub const BLACK_SQUARE: &str = "\u{25A0}";
    pub const LESS_EQUAL: &str = "\u{2264}";
    pub const GREATER_EQUAL: &str = "\u{2265}";
    pub const NOT_EQUAL: &str = "\u{2260}";
    pub const SQUARE_ROOT: &str = "\u{221A}";
    pub const CUBE_ROOT: &str = "\u{221B}";
    pub const INFINITY: &str = "\u{221E}";
    pub const ANGLE: &str = "\u{2220}";
    pub const DEGREE: &str = "\u{00B0}";

    pub const CAPITAL_GREEK_START: u32 = 0x0391;
    pub const CAPITAL_GREEK_END: u32 = 0x03A9;
    pub const LOWER_GREEK_START: u32 = 0x03B1;
    pub const LOWER_GREEK_END: u32 = 0x03C9;
    pub const PLANKS_CONSTANT: u32 = 0x210e;
    pub const LOWER_ITALIC_START: u32 = 0x1D44E;
    pub const CAPITAL_ITALIC_START: u32 = 0x1D434;
    pub const GREEK_LOWER_ITALIC_START: u32 = 0x1D6FC;
    pub const GREEK_CAPITAL_ITALIC_START: u32 = 0x1D6E2;
    pub const GREEK_SYMBOL_ITALIC_START: u32 = 0x1D716;

    pub const MATH_CAPITAL_BOLD_START: u32 = 0x1D400;
    pub const MATH_LOWER_BOLD_START: u32 = 0x1D41A;
    pub const GREEK_CAPITAL_BOLD_START: u32 = 0x1D6A8;
    pub const GREEK_LOWER_BOLD_START: u32 = 0x1D6C2;
    pub const GREEK_SYMBOL_BOLD_START: u32 = 0x1D6DC;
    pub const NUMBER_BOLD_START: u32 = 0x1D7CE;

    pub const MATH_CAPITAL_BOLD_ITALIC_START: u32 = 0x1D468;
    pub const MATH_LOWER_BOLD_ITALIC_START: u32 = 0x1D482;
    pub const GREEK_CAPITAL_BOLD_ITALIC_START: u32 = 0x1D71C;
    pub const GREEK_LOWER_BOLD_ITALIC_START: u32 = 0x1D736;
    pub const GREEK_SYMBOL_BOLD_ITALIC_START: u32 = 0x1D750;

    pub const MATH_CAPITAL_SCRIPT_START: u32 = 0x1D49C;
    pub const MATH_CAPITAL_TT_START: u32 = 0x1D670;
    pub const MATH_LOWER_TT_START: u32 = 0x1D68A;
    pub const NUMBER_TT_START: u32 = 0x1D7F6;
    pub const MATH_CAPITAL_SANS_SERIF_START: u32 = 0x1D5A0;
    pub const MATH_LOWER_SANS_SERIF_START: u32 = 0x1D5BA;
    pub const NUMBER_SANS_SERIF_START: u32 = 0x1D7E2;
    pub const MATH_CAPITAL_FRAKTUR_START: u32 = 0x1D504;
    pub const MATH_LOWER_FRAKTUR_START: u32 = 0x1D51E;
    pub const MATH_CAPITAL_BLACKBOARD_START: u32 = 0x1D538;
    pub const MATH_LOWER_BLACKBOARD_START: u32 = 0x1D552;
    pub const NUMBER_BLACKBOARD_START: u32 = 0x1D7D8;
}

use unicode_symbol as sym;

/// `extension Character` in `MTUnicode.swift`. A "character" is a `&str`
/// holding one grapheme cluster.
pub trait MathCharacter {
    fn utf32_char(&self) -> u32;
    fn is_lower_english(&self) -> bool;
    fn is_upper_english(&self) -> bool;
    /// SwiftMath's own `isNumber` (`"0"..."9"`), which shadows the standard
    /// library's inside the module.
    fn is_number(&self) -> bool;
    fn is_lower_greek(&self) -> bool;
    fn is_capital_greek(&self) -> bool;
    fn greek_symbol_order(&self) -> Option<u32>;
    fn is_greek_symbol(&self) -> bool;
}

impl MathCharacter for str {
    fn utf32_char(&self) -> u32 {
        swift::utf32_char(self)
    }

    fn is_lower_english(&self) -> bool {
        swift::in_closed_range(self, "a", "z")
    }

    fn is_upper_english(&self) -> bool {
        swift::in_closed_range(self, "A", "Z")
    }

    fn is_number(&self) -> bool {
        swift::in_closed_range(self, "0", "9")
    }

    fn is_lower_greek(&self) -> bool {
        let uch = self.utf32_char();
        (sym::LOWER_GREEK_START..=sym::LOWER_GREEK_END).contains(&uch)
    }

    fn is_capital_greek(&self) -> bool {
        let uch = self.utf32_char();
        (sym::CAPITAL_GREEK_START..=sym::CAPITAL_GREEK_END).contains(&uch)
    }

    fn greek_symbol_order(&self) -> Option<u32> {
        const GREEK_SYMBOLS: [u32; 6] = [0x03F5, 0x03D1, 0x03F0, 0x03D5, 0x03F1, 0x03D6];
        let uch = self.utf32_char();
        GREEK_SYMBOLS
            .iter()
            .position(|&symbol| symbol == uch)
            .map(|position| position as u32)
    }

    fn is_greek_symbol(&self) -> bool {
        self.greek_symbol_order().is_some()
    }
}
