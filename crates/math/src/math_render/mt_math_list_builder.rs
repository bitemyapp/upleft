//! `MTMathListBuilder.swift`: LaTeX → `MTMathList`.

use std::collections::HashMap;
use std::fmt;
use std::sync::LazyLock;

use super::mt_math_atom_factory::MTMathAtomFactory;
use super::mt_math_list::{
    AtomKind, MTLineStyle, MTMathAtom, MTMathAtomRef, MTMathAtomType, MTMathList, MTMathListRef,
};
use crate::swift::{self, CharacterIndex};

#[derive(Clone, Debug)]
pub struct MTEnvProperties {
    pub env_name: Option<String>,
    pub ended: bool,
    pub num_rows: usize,
}

impl MTEnvProperties {
    fn new(name: Option<String>) -> Self {
        MTEnvProperties {
            env_name: name,
            num_rows: 0,
            ended: false,
        }
    }
}

/// The error encountered when parsing a LaTeX string.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum MTParseErrors {
    /// The braces { } do not match.
    MismatchBraces = 1,
    /// A command in the string is not recognized.
    InvalidCommand,
    /// An expected character such as ] was not found.
    CharacterNotFound,
    /// The \left or \right command was not followed by a delimiter.
    MissingDelimiter,
    /// The delimiter following \left or \right was not a valid delimiter.
    InvalidDelimiter,
    /// There is no \right corresponding to the \left command.
    MissingRight,
    /// There is no \left corresponding to the \right command.
    MissingLeft,
    /// The environment given to the \begin command is not recognized
    InvalidEnv,
    /// A command is used which is only valid inside a \begin,\end environment
    MissingEnv,
    /// There is no \begin corresponding to the \end command.
    MissingBegin,
    /// There is no \end corresponding to the \begin command.
    MissingEnd,
    /// The number of columns do not match the environment
    InvalidNumColumns,
    /// Internal error, due to a programming mistake.
    InternalError,
    /// Limit control applied incorrectly
    InvalidLimits,
}

/// `MTParseError`, the `NSError` domain.
pub const MT_PARSE_ERROR: &str = "ParseError";

/// The `NSError` SwiftMath reports: domain `ParseError`, a code, and a
/// localized description.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MTParseError {
    pub code: MTParseErrors,
    pub message: String,
}

impl MTParseError {
    pub fn new(code: MTParseErrors, message: impl Into<String>) -> Self {
        MTParseError {
            code,
            message: message.into(),
        }
    }

    pub fn domain(&self) -> &'static str {
        MT_PARSE_ERROR
    }

    /// `NSError.code`.
    pub fn code_value(&self) -> i32 {
        self.code as i32
    }

    /// `NSError.localizedDescription`.
    pub fn localized_description(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for MTParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for MTParseError {}

pub const SPACE_TO_COMMANDS: &[(f64, &str)] = &[
    (3.0, ","),
    (4.0, ">"),
    (5.0, ";"),
    (-3.0, "!"),
    (18.0, "quad"),
    (36.0, "qquad"),
];

pub fn style_to_command(style: MTLineStyle) -> &'static str {
    match style {
        MTLineStyle::Display => "displaystyle",
        MTLineStyle::Text => "textstyle",
        MTLineStyle::Script => "scriptstyle",
        MTLineStyle::ScriptOfScript => "scriptscriptstyle",
    }
}

static FRACTION_COMMANDS: LazyLock<HashMap<&'static str, &'static [&'static str]>> =
    LazyLock::new(|| {
        HashMap::from([
            ("over", &[][..]),
            ("atop", &[][..]),
            ("choose", &["(", ")"][..]),
            ("brack", &["[", "]"][..]),
            ("brace", &["{", "}"][..]),
        ])
    });

/// Parses LaTeX into an `MTMathList`.
pub struct MTMathListBuilder {
    string: String,
    characters: CharacterIndex,
    current_char_index: usize,
    current_inner_atom: Option<MTMathAtomRef>,
    current_env: Option<MTEnvProperties>,
    current_font_style: super::mt_math_list::MTFontStyle,
    spaces_allowed: bool,
    /// Contains any error that occurred during parsing.
    pub error: Option<MTParseError>,
}

impl MTMathListBuilder {
    pub fn new(string: &str) -> Self {
        MTMathListBuilder {
            error: None,
            characters: CharacterIndex::new(string),
            string: string.to_owned(),
            current_char_index: 0,
            current_inner_atom: None,
            current_env: None,
            current_font_style: Default::default(),
            spaces_allowed: false,
        }
    }

    // MARK: - Character-handling routines

    fn has_characters(&self) -> bool {
        self.current_char_index < self.characters.len()
    }

    /// Gets the next character and increments the index.
    fn get_next_character(&mut self) -> std::ops::Range<usize> {
        let index = self.current_char_index;
        self.current_char_index += 1;
        self.char_range(index)
    }

    fn char_range(&self, index: usize) -> std::ops::Range<usize> {
        let ch = self.characters.get(&self.string, index);
        let start = ch.as_ptr() as usize - self.string.as_ptr() as usize;
        start..start + ch.len()
    }

    fn ch(&self, range: &std::ops::Range<usize>) -> &str {
        &self.string[range.clone()]
    }

    fn next_character(&mut self) -> String {
        let range = self.get_next_character();
        self.string[range].to_owned()
    }

    fn unlook_character(&mut self) {
        if self.current_char_index > 0 {
            self.current_char_index -= 1;
        }
    }

    fn expect_character(&mut self, ch: &str) -> bool {
        self.skip_spaces();

        if self.has_characters() {
            let next_char = self.get_next_character();
            if swift::equal(self.ch(&next_char), ch) {
                return true;
            } else {
                self.unlook_character();
                return false;
            }
        }
        false
    }

    // MARK: - MTMathList builder functions

    /// Builds a mathlist from the internal `string`. Returns nil if there is an error.
    pub fn build(&mut self) -> Option<MTMathListRef> {
        let list = self.build_internal(false);
        if self.has_characters() && self.error.is_none() {
            let message = format!("Mismatched braces: {}", self.string);
            self.set_error(MTParseErrors::MismatchBraces, message);
            return None;
        }
        if self.error.is_some() {
            return None;
        }
        list
    }

    /// Construct a math list from a given string, or nil on a parse error.
    pub fn build_from_string(string: &str) -> Option<MTMathListRef> {
        let mut builder = MTMathListBuilder::new(string);
        builder.build()
    }

    /// Construct a math list from a given string; the error, if any, is
    /// returned in `error`.
    pub fn build_from_string_with_error(
        string: &str,
        error: &mut Option<MTParseError>,
    ) -> Option<MTMathListRef> {
        let mut builder = MTMathListBuilder::new(string);
        let output = builder.build();
        if builder.error.is_some() {
            *error = builder.error;
            return None;
        }
        output
    }

    pub fn build_internal(&mut self, one_char_only: bool) -> Option<MTMathListRef> {
        self.build_internal_until(one_char_only, None)
    }

    pub fn build_internal_until(
        &mut self,
        one_char_only: bool,
        stop: Option<&str>,
    ) -> Option<MTMathListRef> {
        let list = MTMathList::new();
        let mut prev_atom: Option<MTMathAtomRef> = None;
        while self.has_characters() {
            if self.error.is_some() {
                return None; // If there is an error thus far then bail out.
            }

            let atom: Option<MTMathAtomRef>;
            let char_range = self.get_next_character();
            let char: &str = &self.string[char_range.clone()];
            // `char` borrows self.string; take an owned copy for the calls below.
            let char = char.to_owned();

            if one_char_only && (char == "^" || char == "}" || char == "_" || char == "&") {
                // this is not the character we are looking for.
                // They are meant for the caller to look at.
                self.unlook_character();
                return Some(list);
            }
            // If there is a stop character, keep scanning 'til we find it
            if let Some(stop) = stop
                && swift::equal(&char, stop)
            {
                return Some(list);
            }

            if char == "^" {
                let needs_new = match &prev_atom {
                    None => true,
                    Some(prev) => {
                        let prev = prev.borrow();
                        prev.super_script().is_some() || !prev.is_script_allowed()
                    }
                };
                if needs_new {
                    // If there is no previous atom, or if it already has a superscript
                    // or if scripts are not allowed for it, then add an empty node.
                    let empty = MTMathAtom::with_type(MTMathAtomType::Ordinary, "");
                    list.borrow_mut().add(Some(empty.clone()));
                    prev_atom = Some(empty);
                }
                // this is a superscript for the previous atom
                // note: if the next char is the stopChar it will be consumed by the ^ and so it doesn't count as stop
                let super_script = self.build_internal(true);
                prev_atom
                    .as_ref()
                    .unwrap()
                    .borrow_mut()
                    .set_super_script(super_script);
                continue;
            } else if char == "_" {
                let needs_new = match &prev_atom {
                    None => true,
                    Some(prev) => {
                        let prev = prev.borrow();
                        prev.sub_script().is_some() || !prev.is_script_allowed()
                    }
                };
                if needs_new {
                    let empty = MTMathAtom::with_type(MTMathAtomType::Ordinary, "");
                    list.borrow_mut().add(Some(empty.clone()));
                    prev_atom = Some(empty);
                }
                // this is a subscript for the previous atom
                let sub_script = self.build_internal(true);
                prev_atom
                    .as_ref()
                    .unwrap()
                    .borrow_mut()
                    .set_sub_script(sub_script);
                continue;
            } else if char == "{" {
                // this puts us in a recursive routine, and sets oneCharOnly to false and no stop character
                if let Some(sub_list) = self.build_internal_until(false, Some("}")) {
                    prev_atom = sub_list.borrow().atoms.last().cloned();
                    list.borrow_mut().append(Some(&sub_list));
                    if one_char_only {
                        return Some(list);
                    }
                }
                continue;
            } else if char == "}" {
                // We encountered a closing brace when there is no stop set, that means there was no
                // corresponding opening brace.
                self.set_error(MTParseErrors::MismatchBraces, "Mismatched braces.");
                return None;
            } else if char == "\\" {
                let command = self.read_command();
                let done = self.stop_command(&command, &list, stop);
                if done.is_some() {
                    return done;
                } else if self.error.is_some() {
                    return None;
                }
                if self.apply_modifier(&command, prev_atom.as_ref()) {
                    continue;
                }

                if let Some(font_style) = MTMathAtomFactory::font_style_with_name(&command) {
                    let old_spaces_allowed = self.spaces_allowed;
                    // Text has special consideration where it allows spaces without escaping.
                    self.spaces_allowed = command == "text";
                    let old_font_style = self.current_font_style;
                    self.current_font_style = font_style;
                    if let Some(sublist) = self.build_internal(true) {
                        // Restore the font style.
                        self.current_font_style = old_font_style;
                        self.spaces_allowed = old_spaces_allowed;

                        prev_atom = sublist.borrow().atoms.last().cloned();
                        list.borrow_mut().append(Some(&sublist));
                        if one_char_only {
                            return Some(list);
                        }
                    }
                    continue;
                }
                atom = self.atom_for_command(&command);
                if atom.is_none() {
                    // this was an unknown command,
                    // we flag an error and return
                    self.set_error(MTParseErrors::InternalError, "Internal error");
                    return None;
                }
            } else if char == "&" {
                // used for column separation in tables
                if self.current_env.is_some() {
                    return Some(list);
                } else {
                    // Create a new table with the current list and a default env
                    if let Some(table) = self.build_table(None, Some(list.clone()), false) {
                        return Some(MTMathList::with_atom(table));
                    } else {
                        return None;
                    }
                }
            } else if self.spaces_allowed && char == " " {
                // If spaces are allowed then spaces do not need escaping with a \ before being used.
                atom = MTMathAtomFactory::atom_for_latex_symbol(" ");
            } else {
                atom = MTMathAtomFactory::atom_for_character(&char);
                if atom.is_none() {
                    // Not a recognized character
                    continue;
                }
            }

            let atom = atom.expect("Atom shouldn't be nil");
            atom.borrow_mut().font_style = self.current_font_style;
            list.borrow_mut().add(Some(atom.clone()));
            prev_atom = Some(atom);

            if one_char_only {
                return Some(list);
            }
        }
        if let Some(stop) = stop {
            if stop == "}" {
                // We did not find a corresponding closing brace.
                self.set_error(MTParseErrors::MismatchBraces, "Missing closing brace");
            } else {
                // we never found our stop character
                let error_message = format!("Expected character not found: {stop}");
                self.set_error(MTParseErrors::CharacterNotFound, error_message);
            }
        }
        Some(list)
    }

    // MARK: - MTMathList to LaTeX conversion

    /// This converts the MTMathList to LaTeX.
    pub fn math_list_to_string(ml: Option<&MTMathListRef>) -> String {
        let mut str = String::new();
        let mut currentfont_style = super::mt_math_list::MTFontStyle::DefaultStyle;
        if let Some(atom_list) = ml {
            let atoms: Vec<MTMathAtomRef> = atom_list.borrow().atoms.clone();
            for atom_ref in &atoms {
                let atom = atom_ref.borrow();
                if currentfont_style != atom.font_style {
                    if currentfont_style != super::mt_math_list::MTFontStyle::DefaultStyle {
                        str += "}";
                    }
                    if atom.font_style != super::mt_math_list::MTFontStyle::DefaultStyle {
                        let font_style_name =
                            MTMathAtomFactory::font_name_for_style(atom.font_style);
                        str += &format!("\\{font_style_name}{{");
                    }
                    currentfont_style = atom.font_style;
                }
                if atom.type_ == MTMathAtomType::Fraction {
                    if let AtomKind::Fraction(frac) = &atom.kind {
                        if frac.has_rule {
                            str += &format!(
                                "\\frac{{{}}}{{{}}}",
                                Self::math_list_to_string(Some(frac.numerator.as_ref().unwrap())),
                                Self::math_list_to_string(Some(frac.denominator.as_ref().unwrap()))
                            );
                        } else {
                            let command = if frac.left_delimiter.is_empty()
                                && frac.right_delimiter.is_empty()
                            {
                                "atop".to_owned()
                            } else if frac.left_delimiter == "(" && frac.right_delimiter == ")" {
                                "choose".to_owned()
                            } else if frac.left_delimiter == "{" && frac.right_delimiter == "}" {
                                "brace".to_owned()
                            } else if frac.left_delimiter == "[" && frac.right_delimiter == "]" {
                                "brack".to_owned()
                            } else {
                                format!(
                                    "atopwithdelims{}{}",
                                    frac.left_delimiter, frac.right_delimiter
                                )
                            };
                            str += &format!(
                                "{{{} \\{} {}}}",
                                Self::math_list_to_string(Some(frac.numerator.as_ref().unwrap())),
                                command,
                                Self::math_list_to_string(Some(frac.denominator.as_ref().unwrap()))
                            );
                        }
                    }
                } else if atom.type_ == MTMathAtomType::Radical {
                    str += "\\sqrt";
                    if let AtomKind::Radical(rad) = &atom.kind {
                        if let Some(degree) = &rad.degree {
                            str += &format!("[{}]", Self::math_list_to_string(Some(degree)));
                        }
                        str += &format!(
                            "{{{}}}",
                            Self::math_list_to_string(Some(rad.radicand.as_ref().unwrap()))
                        );
                    }
                } else if atom.type_ == MTMathAtomType::Inner {
                    if let AtomKind::Inner(inner) = &atom.kind {
                        if inner.left_boundary().is_some() || inner.right_boundary().is_some() {
                            if let Some(left) = inner.left_boundary() {
                                str += &format!("\\left{} ", Self::delim_to_string(&left.borrow()));
                            } else {
                                str += "\\left. ";
                            }

                            str += &Self::math_list_to_string(Some(
                                inner.inner_list.as_ref().unwrap(),
                            ));

                            if let Some(right) = inner.right_boundary() {
                                str +=
                                    &format!("\\right{} ", Self::delim_to_string(&right.borrow()));
                            } else {
                                str += "\\right. ";
                            }
                        } else {
                            str += &format!(
                                "{{{}}}",
                                Self::math_list_to_string(Some(inner.inner_list.as_ref().unwrap()))
                            );
                        }
                    }
                } else if atom.type_ == MTMathAtomType::Table {
                    if let AtomKind::Table(table) = &atom.kind {
                        if !table.environment.is_empty() {
                            str += &format!("\\begin{{{}}}", table.environment);
                        }

                        for i in 0..table.num_rows() {
                            let row = &table.cells[i];
                            for (j, cell) in row.iter().enumerate() {
                                if table.environment == "matrix" {
                                    let remove = {
                                        let cell = cell.borrow();
                                        !cell.atoms.is_empty()
                                            && cell.atoms[0].borrow().type_ == MTMathAtomType::Style
                                    };
                                    if remove {
                                        // remove first atom
                                        cell.borrow_mut().atoms.remove(0);
                                    }
                                }
                                if table.environment == "eqalign"
                                    || table.environment == "aligned"
                                    || table.environment == "split"
                                {
                                    let remove = {
                                        let cell = cell.borrow();
                                        j == 1
                                            && !cell.atoms.is_empty()
                                            && cell.atoms[0].borrow().type_
                                                == MTMathAtomType::Ordinary
                                            && swift::count(&cell.atoms[0].borrow().nucleus) == 0
                                    };
                                    if remove {
                                        // remove empty nucleus added for spacing
                                        cell.borrow_mut().atoms.remove(0);
                                    }
                                }
                                str += &Self::math_list_to_string(Some(cell));
                                if j + 1 < row.len() {
                                    str += "&";
                                }
                            }
                            if i + 1 < table.num_rows() {
                                str += "\\\\ ";
                            }
                        }
                        if !table.environment.is_empty() {
                            str += &format!("\\end{{{}}}", table.environment);
                        }
                    }
                } else if atom.type_ == MTMathAtomType::Overline {
                    if let AtomKind::OverLine(overline) = &atom.kind {
                        str += "\\overline";
                        str += &format!(
                            "{{{}}}",
                            Self::math_list_to_string(Some(overline.inner_list.as_ref().unwrap()))
                        );
                    }
                } else if atom.type_ == MTMathAtomType::Underline {
                    if let AtomKind::UnderLine(underline) = &atom.kind {
                        str += "\\underline";
                        str += &format!(
                            "{{{}}}",
                            Self::math_list_to_string(Some(underline.inner_list.as_ref().unwrap()))
                        );
                    }
                } else if atom.type_ == MTMathAtomType::Accent {
                    if let AtomKind::Accent(accent) = &atom.kind {
                        str += &format!(
                            "\\{}{{{}}}",
                            MTMathAtomFactory::accent_name(&atom).unwrap(),
                            Self::math_list_to_string(Some(accent.inner_list.as_ref().unwrap()))
                        );
                    }
                } else if atom.type_ == MTMathAtomType::LargeOperator {
                    let op = atom.as_large_operator().expect("atom as! MTLargeOperator");
                    let command = MTMathAtomFactory::latex_symbol_name(&atom).unwrap();
                    let original_op = MTMathAtomFactory::atom_for_latex_symbol(&command).unwrap();
                    let original_limits = original_op
                        .borrow()
                        .as_large_operator()
                        .expect("as! MTLargeOperator")
                        .limits;
                    str += &format!("\\{command} ");
                    if original_limits != op.limits {
                        if op.limits {
                            str += "\\limits ";
                        } else {
                            str += "\\nolimits ";
                        }
                    }
                } else if atom.type_ == MTMathAtomType::Space {
                    if let AtomKind::Space(space) = &atom.kind {
                        if let Some((_, command)) = SPACE_TO_COMMANDS
                            .iter()
                            .find(|(value, _)| *value == space.space)
                        {
                            str += &format!("\\{command} ");
                        } else {
                            str += &format!("\\mkern{:.1}mu", space.space);
                        }
                    }
                } else if atom.type_ == MTMathAtomType::Style {
                    if let AtomKind::Style(style) = &atom.kind {
                        str += &format!("\\{} ", style_to_command(style.style));
                    }
                } else if atom.nucleus.is_empty() {
                    str += "{}";
                } else if atom.nucleus == "\u{2236}" {
                    // math colon
                    str += ":";
                } else if atom.nucleus == "\u{2212}" {
                    // math minus
                    str += "-";
                } else if let Some(command) = MTMathAtomFactory::latex_symbol_name(&atom) {
                    str += &format!("\\{command} ");
                } else {
                    str += &atom.nucleus;
                }

                if let Some(super_script) = atom.super_script() {
                    str += &format!("^{{{}}}", Self::math_list_to_string(Some(super_script)));
                }

                if let Some(sub_script) = atom.sub_script() {
                    str += &format!("_{{{}}}", Self::math_list_to_string(Some(sub_script)));
                }
            }
        }
        if currentfont_style != super::mt_math_list::MTFontStyle::DefaultStyle {
            str += "}";
        }
        str
    }

    pub fn delim_to_string(delim: &MTMathAtom) -> String {
        if let Some(command) = MTMathAtomFactory::get_delimiter_name(delim) {
            let single_chars = ["(", ")", "[", "]", "<", ">", "|", ".", "/"];
            if single_chars.contains(&command.as_str()) {
                return command;
            } else if command == "||" {
                return "\\|".to_owned();
            } else {
                return format!("\\{command}");
            }
        }
        String::new()
    }

    fn atom_for_command(&mut self, command: &str) -> Option<MTMathAtomRef> {
        if let Some(atom) = MTMathAtomFactory::atom_for_latex_symbol(command) {
            return Some(atom);
        }
        if let Some(accent) = MTMathAtomFactory::accent_with_name(command) {
            // The command is an accent
            let inner = self.build_internal(true);
            accent.borrow_mut().set_inner_list(inner);
            Some(accent)
        } else if command == "frac" {
            // A fraction command has 2 arguments
            let frac = MTMathAtom::fraction(true);
            let numerator = self.build_internal(true);
            frac.borrow_mut().as_fraction_mut().unwrap().numerator = numerator;
            let denominator = self.build_internal(true);
            frac.borrow_mut().as_fraction_mut().unwrap().denominator = denominator;
            Some(frac)
        } else if command == "binom" {
            // A binom command has 2 arguments
            let frac = MTMathAtom::fraction(false);
            let numerator = self.build_internal(true);
            frac.borrow_mut().as_fraction_mut().unwrap().numerator = numerator;
            let denominator = self.build_internal(true);
            {
                let mut atom = frac.borrow_mut();
                let fraction = atom.as_fraction_mut().unwrap();
                fraction.denominator = denominator;
                fraction.left_delimiter = "(".to_owned();
                fraction.right_delimiter = ")".to_owned();
            }
            Some(frac)
        } else if command == "sqrt" {
            // A sqrt command with one argument
            let rad = MTMathAtom::radical();
            if !self.has_characters() {
                let radicand = self.build_internal(true);
                rad.borrow_mut().as_radical_mut().unwrap().radicand = radicand;
                return Some(rad);
            }
            let ch = self.next_character();
            if ch == "[" {
                // special handling for sqrt[degree]{radicand}
                let degree = self.build_internal_until(false, Some("]"));
                rad.borrow_mut().as_radical_mut().unwrap().degree = degree;
                let radicand = self.build_internal(true);
                rad.borrow_mut().as_radical_mut().unwrap().radicand = radicand;
            } else {
                self.unlook_character();
                let radicand = self.build_internal(true);
                rad.borrow_mut().as_radical_mut().unwrap().radicand = radicand;
            }
            Some(rad)
        } else if command == "left" {
            // Save the current inner while a new one gets built.
            let old_inner = self.current_inner_atom.take();
            let inner = MTMathAtom::inner();
            self.current_inner_atom = Some(inner.clone());
            let left = self.get_boundary_atom("left");
            inner
                .borrow_mut()
                .as_inner_mut()
                .unwrap()
                .set_left_boundary(left);
            if inner.borrow().as_inner().unwrap().left_boundary().is_none() {
                return None;
            }
            let inner_list = self.build_internal(false);
            let current = self.current_inner_atom.clone().unwrap();
            current.borrow_mut().as_inner_mut().unwrap().inner_list = inner_list;
            if current
                .borrow()
                .as_inner()
                .unwrap()
                .right_boundary()
                .is_none()
            {
                // A right node would have set the right boundary so we must be missing the right node.
                self.set_error(MTParseErrors::MissingRight, "Missing \\right");
                return None;
            }
            // reinstate the old inner atom.
            let new_inner = self.current_inner_atom.take();
            self.current_inner_atom = old_inner;
            new_inner
        } else if command == "overline" {
            // The overline command has 1 arguments
            let over = MTMathAtom::over_line();
            let inner = self.build_internal(true);
            over.borrow_mut().set_inner_list(inner);
            Some(over)
        } else if command == "underline" {
            // The underline command has 1 arguments
            let under = MTMathAtom::under_line();
            let inner = self.build_internal(true);
            under.borrow_mut().set_inner_list(inner);
            Some(under)
        } else if command == "begin" {
            let env = self.read_environment()?;
            self.build_table(Some(env), None, false)
        } else if command == "color" || command == "textcolor" || command == "colorbox" {
            // A color command has 2 arguments
            let math_color = match command {
                "color" => MTMathAtom::color(),
                "textcolor" => MTMathAtom::text_color(),
                _ => MTMathAtom::colorbox(),
            };
            let color = self.read_color()?;
            math_color.borrow_mut().as_color_mut().unwrap().color_string = color;
            let inner = self.build_internal(true);
            math_color.borrow_mut().set_inner_list(inner);
            Some(math_color)
        } else {
            let error_message = format!("Invalid command \\{command}");
            self.set_error(MTParseErrors::InvalidCommand, error_message);
            None
        }
    }

    fn read_color(&mut self) -> Option<String> {
        if !self.expect_character("{") {
            // We didn't find an opening brace, so no env found.
            self.set_error(MTParseErrors::CharacterNotFound, "Missing {");
            return None;
        }

        // Ignore spaces and nonascii.
        self.skip_spaces();

        // a string of all upper and lower case characters.
        let mut mutable = String::new();
        while self.has_characters() {
            let ch = self.next_character();
            if ch == "#"
                || swift::in_closed_range(&ch, "A", "Z")
                || swift::in_closed_range(&ch, "a", "z")
                || swift::in_closed_range(&ch, "0", "9")
            {
                mutable.push_str(&ch);
            } else {
                // we went too far
                self.unlook_character();
                break;
            }
        }

        if !self.expect_character("}") {
            // We didn't find an closing brace, so invalid format.
            self.set_error(MTParseErrors::CharacterNotFound, "Missing }");
            return None;
        }
        Some(mutable)
    }

    fn skip_spaces(&mut self) {
        while self.has_characters() {
            let range = self.get_next_character();
            let ch = swift::utf32_char(self.ch(&range));
            if !(0x21..=0x7E).contains(&ch) {
                // skip non ascii characters and spaces
                continue;
            } else {
                self.unlook_character();
                return;
            }
        }
    }

    fn stop_command(
        &mut self,
        command: &str,
        list: &MTMathListRef,
        stop_char: Option<&str>,
    ) -> Option<MTMathListRef> {
        if command == "right" {
            let Some(current) = self.current_inner_atom.clone() else {
                self.set_error(MTParseErrors::MissingLeft, "Missing \\left");
                return None;
            };
            let right = self.get_boundary_atom("right");
            current
                .borrow_mut()
                .as_inner_mut()
                .unwrap()
                .set_right_boundary(right);
            if current
                .borrow()
                .as_inner()
                .unwrap()
                .right_boundary()
                .is_none()
            {
                return None;
            }
            // return the list read so far.
            return Some(list.clone());
        } else if let Some(delims) = FRACTION_COMMANDS.get(command) {
            let frac = if command == "over" {
                MTMathAtom::fraction(true)
            } else {
                MTMathAtom::fraction(false)
            };
            if delims.len() == 2 {
                let mut atom = frac.borrow_mut();
                let fraction = atom.as_fraction_mut().unwrap();
                fraction.left_delimiter = delims[0].to_owned();
                fraction.right_delimiter = delims[1].to_owned();
            }
            frac.borrow_mut().as_fraction_mut().unwrap().numerator = Some(list.clone());
            let denominator = self.build_internal_until(false, stop_char);
            frac.borrow_mut().as_fraction_mut().unwrap().denominator = denominator;
            if self.error.is_some() {
                return None;
            }
            let frac_list = MTMathList::new();
            frac_list.borrow_mut().add(Some(frac));
            return Some(frac_list);
        } else if command == "\\" || command == "cr" {
            if let Some(env) = self.current_env.as_mut() {
                // Stop the current list and increment the row count
                env.num_rows += 1;
                return Some(list.clone());
            } else {
                // Create a new table with the current list and a default env
                if let Some(table) = self.build_table(None, Some(list.clone()), true) {
                    return Some(MTMathList::with_atom(table));
                }
            }
        } else if command == "end" {
            if self.current_env.is_none() {
                self.set_error(MTParseErrors::MissingBegin, "Missing \\begin");
                return None;
            }
            let env = self.read_environment()?;
            let current_name = self.current_env.as_ref().unwrap().env_name.clone();
            if Some(&env) != current_name.as_ref() {
                let error_message = format!(
                    "Begin environment name {} does not match end name: {}",
                    current_name.as_deref().unwrap_or("(none)"),
                    env
                );
                self.set_error(MTParseErrors::InvalidEnv, error_message);
                return None;
            }
            // Finish the current environment.
            self.current_env.as_mut().unwrap().ended = true;
            return Some(list.clone());
        }
        None
    }

    /// Applies the modifier to the atom. Returns true if modifier applied.
    fn apply_modifier(&mut self, modifier: &str, atom: Option<&MTMathAtomRef>) -> bool {
        if modifier == "limits" || modifier == "nolimits" {
            let is_operator =
                atom.is_some_and(|atom| atom.borrow().type_ == MTMathAtomType::LargeOperator);
            if !is_operator {
                let error_message = if modifier == "limits" {
                    "Limits can only be applied to an operator."
                } else {
                    "No limits can only be applied to an operator."
                };
                self.set_error(MTParseErrors::InvalidLimits, error_message);
            } else {
                let atom = atom.unwrap();
                let mut atom = atom.borrow_mut();
                let op = atom
                    .as_large_operator_mut()
                    .expect("atom as! MTLargeOperator");
                op.limits = modifier == "limits";
            }
            return true;
        }
        false
    }

    fn set_error(&mut self, code: MTParseErrors, message: impl Into<String>) {
        // Only record the first error.
        if self.error.is_none() {
            self.error = Some(MTParseError::new(code, message));
        }
    }

    /// `atom(forCommand:)`: an older copy of `atomForCommand` the builder no
    /// longer calls. It differs in two places: `\sqrt` at the end of the input
    /// reads past it, and `\color`/`\colorbox` force-unwrap the colour.
    pub fn atom_for_command_legacy(&mut self, command: &str) -> Option<MTMathAtomRef> {
        if let Some(atom) = MTMathAtomFactory::atom_for_latex_symbol(command) {
            return Some(atom);
        }
        match command {
            "sqrt" => {
                let rad = MTMathAtom::radical();
                assert!(self.has_characters(), "getNextCharacter past the end");
                let ch = self.next_character();
                if ch == "[" {
                    let degree = self.build_internal_until(false, Some("]"));
                    rad.borrow_mut().as_radical_mut().unwrap().degree = degree;
                } else {
                    self.unlook_character();
                }
                let radicand = self.build_internal(true);
                rad.borrow_mut().as_radical_mut().unwrap().radicand = radicand;
                Some(rad)
            }
            "color" | "colorbox" => {
                let math_color = if command == "color" {
                    MTMathAtom::color()
                } else {
                    MTMathAtom::colorbox()
                };
                let color = self.read_color().expect("readColor()!");
                math_color.borrow_mut().as_color_mut().unwrap().color_string = color;
                let inner = self.build_internal(true);
                math_color.borrow_mut().set_inner_list(inner);
                Some(math_color)
            }
            "textcolor" => {
                self.set_error(
                    MTParseErrors::InvalidCommand,
                    format!("Invalid command \\{command}"),
                );
                None
            }
            _ => self.atom_for_command(command),
        }
    }

    fn read_environment(&mut self) -> Option<String> {
        if !self.expect_character("{") {
            // We didn't find an opening brace, so no env found.
            self.set_error(MTParseErrors::CharacterNotFound, "Missing {");
            return None;
        }

        self.skip_spaces();
        let env = self.read_string();

        if !self.expect_character("}") {
            // We didn"t find an closing brace, so invalid format.
            self.set_error(MTParseErrors::CharacterNotFound, "Missing }");
            return None;
        }
        Some(env)
    }

    fn build_table(
        &mut self,
        env: Option<String>,
        first_list: Option<MTMathListRef>,
        is_row: bool,
    ) -> Option<MTMathAtomRef> {
        // Save the current env till an new one gets built.
        let old_env = self.current_env.take();

        self.current_env = Some(MTEnvProperties::new(env));

        let mut current_row = 0;

        let mut rows: Vec<Vec<MTMathListRef>> = vec![Vec::new()];
        if let Some(first_list) = first_list {
            rows[current_row].push(first_list);
            if is_row {
                self.current_env.as_mut().unwrap().num_rows += 1;
                current_row += 1;
                rows.push(Vec::new());
            }
        }
        while !self.current_env.as_ref().unwrap().ended && self.has_characters() {
            let list = self.build_internal(false)?;
            // If there is an error building the list, bail out early (above).
            rows[current_row].push(list);
            if self.current_env.as_ref().unwrap().num_rows > current_row {
                current_row = self.current_env.as_ref().unwrap().num_rows;
                rows.push(Vec::new());
            }
        }

        let env = self.current_env.as_ref().unwrap();
        if !env.ended && env.env_name.is_some() {
            self.set_error(MTParseErrors::MissingEnd, "Missing \\end");
            return None;
        }

        let mut error = self.error.clone();
        let env_name = self.current_env.as_ref().unwrap().env_name.clone();
        let table = MTMathAtomFactory::table(env_name.as_deref(), &rows, &mut error);
        if table.is_none() && self.error.is_none() {
            self.error = error;
            return None;
        }
        self.current_env = old_env;
        table
    }

    fn get_boundary_atom(&mut self, delimiter_type: &str) -> Option<MTMathAtomRef> {
        let Some(delim) = self.read_delimiter() else {
            let error_message = format!("Missing delimiter for \\{delimiter_type}");
            self.set_error(MTParseErrors::MissingDelimiter, error_message);
            return None;
        };
        let boundary = MTMathAtomFactory::boundary_for_delimiter(&delim);
        if boundary.is_none() {
            let error_message = format!("Invalid delimiter for {delimiter_type}: {delim}");
            self.set_error(MTParseErrors::InvalidDelimiter, error_message);
            return None;
        }
        boundary
    }

    fn read_delimiter(&mut self) -> Option<String> {
        self.skip_spaces();
        if self.has_characters() {
            let char = self.next_character();
            if char == "\\" {
                let command = self.read_command();
                if command == "|" {
                    return Some("||".to_owned());
                }
                return Some(command);
            } else {
                return Some(char);
            }
        }
        None
    }

    fn read_command(&mut self) -> String {
        const SINGLE_CHARS: &[&str] = &[
            "{", "}", "$", "#", "%", "_", "|", " ", ",", ">", ";", "!", "\\",
        ];
        if self.has_characters() {
            let char = self.next_character();
            if SINGLE_CHARS.contains(&char.as_str()) {
                return char;
            } else {
                self.unlook_character();
            }
        }
        self.read_string()
    }

    fn read_string(&mut self) -> String {
        // a string of all upper and lower case characters.
        let mut output = String::new();
        while self.has_characters() {
            let range = self.get_next_character();
            let char = self.ch(&range);
            if swift::is_lowercase(char) || swift::is_uppercase(char) {
                output.push_str(char);
            } else {
                self.unlook_character();
                break;
            }
        }
        output
    }
}
