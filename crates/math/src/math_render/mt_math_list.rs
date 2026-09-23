//! `MTMathList.swift`: atoms and math lists.
//!
//! SwiftMath's atoms and lists are classes, and the parser and typesetter
//! rely on reference semantics (an atom already in a list gets its scripts
//! attached later; `finalized` fuses into an atom it has already appended;
//! the typesetter rewrites atom types in place). They are shared, mutable
//! cells here for the same reason: [`MTMathAtomRef`] and [`MTMathListRef`].
//!
//! A Swift subclass is an [`AtomKind`] variant. As in Swift, `copy()`
//! dispatches on the atom's *type* and `finalized` on its *class*.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use objc2_foundation::NSRange;

pub type MTMathAtomRef = Rc<RefCell<MTMathAtom>>;
pub type MTMathListRef = Rc<RefCell<MTMathList>>;

/// The type of atom in a `MTMathList`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(i32)]
pub enum MTMathAtomType {
    /// A number or text in ordinary format - Ord in TeX
    Ordinary = 1,
    /// A number - Does not exist in TeX
    Number,
    /// A variable (i.e. text in italic format) - Does not exist in TeX
    Variable,
    /// A large operator such as (sin/cos, integral etc.) - Op in TeX
    LargeOperator,
    /// A binary operator - Bin in TeX
    BinaryOperator,
    /// A unary operator - Does not exist in TeX.
    UnaryOperator,
    /// A relation, e.g. = > < etc. - Rel in TeX
    Relation,
    /// Open brackets - Open in TeX
    Open,
    /// Close brackets - Close in TeX
    Close,
    /// A fraction e.g 1/2 - generalized fraction node in TeX
    Fraction,
    /// A radical operator e.g. sqrt(2)
    Radical,
    /// Punctuation such as , - Punct in TeX
    Punctuation,
    /// A placeholder square for future input. Does not exist in TeX
    Placeholder,
    /// An inner atom, i.e. an embedded math list - Inner in TeX
    Inner,
    /// An underlined atom - Under in TeX
    Underline,
    /// An overlined atom - Over in TeX
    Overline,
    /// An accented atom - Accent in TeX
    Accent,
    /// A left atom - Left & Right in TeX.
    Boundary = 101,
    /// Spacing between math atoms.
    Space = 201,
    /// Denotes style changes during rendering.
    Style,
    Color,
    Textcolor,
    ColorBox,
    /// A table atom (TeX's `\halign`).
    Table = 1001,
}

impl MTMathAtomType {
    pub fn raw_value(self) -> i32 {
        self as i32
    }

    pub fn is_not_binary_operator(self) -> bool {
        matches!(
            self,
            MTMathAtomType::BinaryOperator
                | MTMathAtomType::Relation
                | MTMathAtomType::Open
                | MTMathAtomType::Punctuation
                | MTMathAtomType::LargeOperator
        )
    }

    pub fn is_script_allowed(self) -> bool {
        self < MTMathAtomType::Boundary
    }
}

impl fmt::Display for MTMathAtomType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            MTMathAtomType::Ordinary => "Ordinary",
            MTMathAtomType::Number => "Number",
            MTMathAtomType::Variable => "Variable",
            MTMathAtomType::LargeOperator => "Large Operator",
            MTMathAtomType::BinaryOperator => "Binary Operator",
            MTMathAtomType::UnaryOperator => "Unary Operator",
            MTMathAtomType::Relation => "Relation",
            MTMathAtomType::Open => "Open",
            MTMathAtomType::Close => "Close",
            MTMathAtomType::Fraction => "Fraction",
            MTMathAtomType::Radical => "Radical",
            MTMathAtomType::Punctuation => "Punctuation",
            MTMathAtomType::Placeholder => "Placeholder",
            MTMathAtomType::Inner => "Inner",
            MTMathAtomType::Underline => "Underline",
            MTMathAtomType::Overline => "Overline",
            MTMathAtomType::Accent => "Accent",
            MTMathAtomType::Boundary => "Boundary",
            MTMathAtomType::Space => "Space",
            MTMathAtomType::Style => "Style",
            MTMathAtomType::Color => "Color",
            MTMathAtomType::Textcolor => "TextColor",
            MTMathAtomType::ColorBox => "Colorbox",
            MTMathAtomType::Table => "Table",
        })
    }
}

/// The font style of a character.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum MTFontStyle {
    /// The default latex rendering style. i.e. variables are italic and numbers are roman.
    #[default]
    DefaultStyle = 0,
    /// Roman font style i.e. \mathrm
    Roman,
    /// Bold font style i.e. \mathbf
    Bold,
    /// Caligraphic font style i.e. \mathcal
    Caligraphic,
    /// Typewriter (monospace) style i.e. \mathtt
    Typewriter,
    /// Italic style i.e. \mathit
    Italic,
    /// San-serif font i.e. \mathss
    SansSerif,
    /// Fractur font i.e \mathfrak
    Fraktur,
    /// Blackboard font i.e. \mathbb
    Blackboard,
    /// Bold italic
    BoldItalic,
}

/// Styling of a line of math.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(i32)]
pub enum MTLineStyle {
    /// Display style
    #[default]
    Display = 0,
    /// Text style (inline)
    Text,
    /// Script style (for sub/super scripts)
    Script,
    /// Script script style (for scripts of scripts)
    ScriptOfScript,
}

impl MTLineStyle {
    pub fn raw_value(self) -> i32 {
        self as i32
    }

    pub fn inc(self) -> MTLineStyle {
        match self {
            MTLineStyle::Display => MTLineStyle::Text,
            MTLineStyle::Text => MTLineStyle::Script,
            MTLineStyle::Script => MTLineStyle::ScriptOfScript,
            MTLineStyle::ScriptOfScript => MTLineStyle::Display,
        }
    }

    pub fn is_not_script(self) -> bool {
        self < MTLineStyle::Script
    }
}

/// Alignment for a column of MTMathTable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MTColumnAlignment {
    Left,
    Center,
    Right,
}

// MARK: - Subclass data

#[derive(Clone, Debug)]
pub struct MTFraction {
    pub has_rule: bool,
    pub left_delimiter: String,
    pub right_delimiter: String,
    pub numerator: Option<MTMathListRef>,
    pub denominator: Option<MTMathListRef>,
}

impl Default for MTFraction {
    fn default() -> Self {
        MTFraction {
            has_rule: true,
            left_delimiter: String::new(),
            right_delimiter: String::new(),
            numerator: None,
            denominator: None,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct MTRadical {
    /// Denotes the term under the square root sign
    pub radicand: Option<MTMathListRef>,
    /// Denotes the degree of the radical, i.e. the value to the top left of the radical sign
    pub degree: Option<MTMathListRef>,
}

#[derive(Clone, Debug, Default)]
pub struct MTLargeOperator {
    /// Whether limits (if present) are displayed above and below the operator in display mode.
    pub limits: bool,
}

#[derive(Clone, Debug, Default)]
pub struct MTInner {
    pub inner_list: Option<MTMathListRef>,
    left_boundary: Option<MTMathAtomRef>,
    right_boundary: Option<MTMathAtomRef>,
}

impl MTInner {
    pub fn left_boundary(&self) -> Option<&MTMathAtomRef> {
        self.left_boundary.as_ref()
    }

    pub fn right_boundary(&self) -> Option<&MTMathAtomRef> {
        self.right_boundary.as_ref()
    }

    /// The `leftBoundary` setter: a non-boundary atom raises.
    pub fn set_left_boundary(&mut self, boundary: Option<MTMathAtomRef>) {
        if let Some(left) = &boundary
            && left.borrow().type_ != MTMathAtomType::Boundary
        {
            panic!("Left boundary must be of type .boundary");
        }
        self.left_boundary = boundary;
    }

    /// The `rightBoundary` setter: a non-boundary atom raises.
    pub fn set_right_boundary(&mut self, boundary: Option<MTMathAtomRef>) {
        if let Some(right) = &boundary
            && right.borrow().type_ != MTMathAtomType::Boundary
        {
            panic!("Right boundary must be of type .boundary");
        }
        self.right_boundary = boundary;
    }
}

/// `MTOverLine`, `MTUnderLine`, `MTAccent`: an atom around an inner list.
#[derive(Clone, Debug, Default)]
pub struct MTInnerListAtom {
    pub inner_list: Option<MTMathListRef>,
}

#[derive(Clone, Debug, Default)]
pub struct MTMathSpace {
    /// The amount of space represented by this object in mu units.
    pub space: f64,
}

#[derive(Clone, Debug, Default)]
pub struct MTMathStyle {
    pub style: MTLineStyle,
}

/// `MTMathColor`, `MTMathTextColor`, `MTMathColorbox`.
#[derive(Clone, Debug, Default)]
pub struct MTColorAtom {
    pub color_string: String,
    pub inner_list: Option<MTMathListRef>,
}

#[derive(Clone, Debug, Default)]
pub struct MTMathTable {
    /// The alignment for each column (left, right, center).
    pub alignments: Vec<MTColumnAlignment>,
    /// The cells in the table as a two dimensional array.
    pub cells: Vec<Vec<MTMathListRef>>,
    /// The name of the environment that this table denotes.
    pub environment: String,
    /// Spacing between each column in mu units.
    pub inter_column_spacing: f64,
    /// Additional spacing between rows in jots (one jot is 0.3 times font size).
    pub inter_row_additional_spacing: f64,
}

impl MTMathTable {
    /// Set the value of a given cell. The table is automatically resized to contain this cell.
    pub fn set_cell(&mut self, list: MTMathListRef, row: usize, column: usize) {
        if self.cells.len() <= row {
            for _ in self.cells.len()..=row {
                self.cells.push(Vec::new());
            }
        }
        let rows = self.cells[row].len();
        if rows <= column {
            for _ in rows..=column {
                self.cells[row].push(MTMathList::new());
            }
        }
        self.cells[row][column] = list;
    }

    /// Set the alignment of a particular column; new columns are centred.
    pub fn set_alignment(&mut self, alignment: MTColumnAlignment, column: usize) {
        if self.alignments.len() <= column {
            for _ in self.alignments.len()..=column {
                self.alignments.push(MTColumnAlignment::Center);
            }
        }
        self.alignments[column] = alignment;
    }

    /// Gets the alignment for a given column, centre when unspecified.
    pub fn get_alignment_for_column(&self, column: usize) -> MTColumnAlignment {
        if self.alignments.len() <= column {
            MTColumnAlignment::Center
        } else {
            self.alignments[column]
        }
    }

    pub fn num_columns(&self) -> usize {
        let mut number_of_cols = 0;
        for row in &self.cells {
            number_of_cols = number_of_cols.max(row.len());
        }
        number_of_cols
    }

    pub fn num_rows(&self) -> usize {
        self.cells.len()
    }
}

/// The Swift class of an atom.
#[derive(Clone, Debug, Default)]
pub enum AtomKind {
    /// Plain `MTMathAtom`.
    #[default]
    Atom,
    Fraction(MTFraction),
    Radical(MTRadical),
    LargeOperator(MTLargeOperator),
    Inner(MTInner),
    OverLine(MTInnerListAtom),
    UnderLine(MTInnerListAtom),
    Accent(MTInnerListAtom),
    Space(MTMathSpace),
    Style(MTMathStyle),
    Color(MTColorAtom),
    TextColor(MTColorAtom),
    Colorbox(MTColorAtom),
    Table(MTMathTable),
}

impl AtomKind {
    /// The Swift class name.
    pub fn class_name(&self) -> &'static str {
        match self {
            AtomKind::Atom => "MTMathAtom",
            AtomKind::Fraction(_) => "MTFraction",
            AtomKind::Radical(_) => "MTRadical",
            AtomKind::LargeOperator(_) => "MTLargeOperator",
            AtomKind::Inner(_) => "MTInner",
            AtomKind::OverLine(_) => "MTOverLine",
            AtomKind::UnderLine(_) => "MTUnderLine",
            AtomKind::Accent(_) => "MTAccent",
            AtomKind::Space(_) => "MTMathSpace",
            AtomKind::Style(_) => "MTMathStyle",
            AtomKind::Color(_) => "MTMathColor",
            AtomKind::TextColor(_) => "MTMathTextColor",
            AtomKind::Colorbox(_) => "MTMathColorbox",
            AtomKind::Table(_) => "MTMathTable",
        }
    }
}

// MARK: - MTMathAtom

/// The basic unit of a math list.
#[derive(Debug)]
pub struct MTMathAtom {
    /// The type of the atom.
    pub type_: MTMathAtomType,
    sub_script: Option<MTMathListRef>,
    super_script: Option<MTMathListRef>,
    /// The nucleus of the atom.
    pub nucleus: String,
    /// The index range in the MTMathList this MTMathAtom tracks.
    pub index_range: NSRange,
    /// The font style to be used for the atom.
    pub font_style: MTFontStyle,
    /// If this atom was formed by fusion of multiple atoms, the atoms fused to create this one.
    pub fused_atoms: Vec<MTMathAtomRef>,
    /// The Swift subclass and its fields.
    pub kind: AtomKind,
}

impl Default for MTMathAtom {
    /// `MTMathAtom()`.
    fn default() -> Self {
        MTMathAtom {
            type_: MTMathAtomType::Ordinary,
            sub_script: None,
            super_script: None,
            nucleus: String::new(),
            index_range: NSRange::new(0, 0),
            font_style: MTFontStyle::DefaultStyle,
            fused_atoms: Vec::new(),
            kind: AtomKind::Atom,
        }
    }
}

fn wrap(atom: MTMathAtom) -> MTMathAtomRef {
    Rc::new(RefCell::new(atom))
}

impl MTMathAtom {
    /// `MTMathAtom()` behind a shared reference.
    pub fn new() -> MTMathAtomRef {
        wrap(MTMathAtom::default())
    }

    /// `MTMathAtom(type:value:)`. The value is ignored for radicals.
    pub fn with_type(type_: MTMathAtomType, value: &str) -> MTMathAtomRef {
        wrap(Self::plain(type_, value))
    }

    pub(crate) fn plain(type_: MTMathAtomType, value: &str) -> MTMathAtom {
        MTMathAtom {
            type_,
            nucleus: if type_ == MTMathAtomType::Radical {
                String::new()
            } else {
                value.to_owned()
            },
            ..MTMathAtom::default()
        }
    }

    /// `MTMathAtom(_ atom:)`: the fields of `MTMathAtom` itself, scripts
    /// deep-copied, fused atoms shared. `nil` gives a default atom.
    fn base_copy(atom: Option<&MTMathAtom>) -> MTMathAtom {
        let Some(atom) = atom else {
            return MTMathAtom::default();
        };
        MTMathAtom {
            type_: atom.type_,
            nucleus: atom.nucleus.clone(),
            sub_script: MTMathList::copy_of(atom.sub_script.as_ref()),
            super_script: MTMathList::copy_of(atom.super_script.as_ref()),
            index_range: atom.index_range,
            font_style: atom.font_style,
            fused_atoms: atom.fused_atoms.clone(),
            kind: AtomKind::Atom,
        }
    }

    /// `MTMathAtom(_ atom:)` behind a shared reference.
    pub fn copy_of(atom: Option<&MTMathAtom>) -> MTMathAtomRef {
        wrap(Self::base_copy(atom))
    }

    /// `MTFraction(hasRule:)`.
    pub fn fraction(has_rule: bool) -> MTMathAtomRef {
        wrap(MTMathAtom {
            type_: MTMathAtomType::Fraction,
            kind: AtomKind::Fraction(MTFraction {
                has_rule,
                ..MTFraction::default()
            }),
            ..MTMathAtom::default()
        })
    }

    /// `MTRadical()`.
    pub fn radical() -> MTMathAtomRef {
        wrap(MTMathAtom {
            type_: MTMathAtomType::Radical,
            kind: AtomKind::Radical(MTRadical::default()),
            ..MTMathAtom::default()
        })
    }

    /// `MTLargeOperator(value:limits:)`.
    pub fn large_operator(value: &str, limits: bool) -> MTMathAtomRef {
        wrap(Self::plain_large_operator(value, limits))
    }

    pub(crate) fn plain_large_operator(value: &str, limits: bool) -> MTMathAtom {
        MTMathAtom {
            kind: AtomKind::LargeOperator(MTLargeOperator { limits }),
            ..Self::plain(MTMathAtomType::LargeOperator, value)
        }
    }

    /// `MTInner()`.
    pub fn inner() -> MTMathAtomRef {
        wrap(MTMathAtom {
            type_: MTMathAtomType::Inner,
            kind: AtomKind::Inner(MTInner::default()),
            ..MTMathAtom::default()
        })
    }

    /// `MTOverLine()`.
    pub fn over_line() -> MTMathAtomRef {
        wrap(MTMathAtom {
            type_: MTMathAtomType::Overline,
            kind: AtomKind::OverLine(MTInnerListAtom::default()),
            ..MTMathAtom::default()
        })
    }

    /// `MTUnderLine()`.
    pub fn under_line() -> MTMathAtomRef {
        wrap(MTMathAtom {
            type_: MTMathAtomType::Underline,
            kind: AtomKind::UnderLine(MTInnerListAtom::default()),
            ..MTMathAtom::default()
        })
    }

    /// `MTAccent(value:)`.
    pub fn accent(value: &str) -> MTMathAtomRef {
        wrap(MTMathAtom {
            type_: MTMathAtomType::Accent,
            nucleus: value.to_owned(),
            kind: AtomKind::Accent(MTInnerListAtom::default()),
            ..MTMathAtom::default()
        })
    }

    /// `MTMathSpace(space:)`.
    pub fn space(space: f64) -> MTMathAtomRef {
        wrap(Self::plain_space(space))
    }

    pub(crate) fn plain_space(space: f64) -> MTMathAtom {
        MTMathAtom {
            type_: MTMathAtomType::Space,
            kind: AtomKind::Space(MTMathSpace { space }),
            ..MTMathAtom::default()
        }
    }

    /// `MTMathStyle(style:)`.
    pub fn style(style: MTLineStyle) -> MTMathAtomRef {
        wrap(Self::plain_style(style))
    }

    pub(crate) fn plain_style(style: MTLineStyle) -> MTMathAtom {
        MTMathAtom {
            type_: MTMathAtomType::Style,
            kind: AtomKind::Style(MTMathStyle { style }),
            ..MTMathAtom::default()
        }
    }

    /// `MTMathColor()`.
    pub fn color() -> MTMathAtomRef {
        wrap(MTMathAtom {
            type_: MTMathAtomType::Color,
            kind: AtomKind::Color(MTColorAtom::default()),
            ..MTMathAtom::default()
        })
    }

    /// `MTMathTextColor()`.
    pub fn text_color() -> MTMathAtomRef {
        wrap(MTMathAtom {
            type_: MTMathAtomType::Textcolor,
            kind: AtomKind::TextColor(MTColorAtom::default()),
            ..MTMathAtom::default()
        })
    }

    /// `MTMathColorbox()`.
    pub fn colorbox() -> MTMathAtomRef {
        wrap(MTMathAtom {
            type_: MTMathAtomType::ColorBox,
            kind: AtomKind::Colorbox(MTColorAtom::default()),
            ..MTMathAtom::default()
        })
    }

    /// `MTMathTable(environment:)`.
    pub fn table(environment: Option<&str>) -> MTMathAtomRef {
        wrap(MTMathAtom {
            type_: MTMathAtomType::Table,
            kind: AtomKind::Table(MTMathTable {
                environment: environment.unwrap_or("").to_owned(),
                ..MTMathTable::default()
            }),
            ..MTMathAtom::default()
        })
    }

    // MARK: Scripts

    /// An optional subscript.
    pub fn sub_script(&self) -> Option<&MTMathListRef> {
        self.sub_script.as_ref()
    }

    /// An optional superscript.
    pub fn super_script(&self) -> Option<&MTMathListRef> {
        self.super_script.as_ref()
    }

    /// The `subScript` setter: a script on an atom that allows none raises.
    pub fn set_sub_script(&mut self, list: Option<MTMathListRef>) {
        if list.is_some() && !self.is_script_allowed() {
            panic!("Subscripts not allowed for atom of type {}", self.type_);
        }
        self.sub_script = list;
    }

    /// The `superScript` setter: a script on an atom that allows none raises.
    pub fn set_super_script(&mut self, list: Option<MTMathListRef>) {
        if list.is_some() && !self.is_script_allowed() {
            panic!("Superscripts not allowed for atom of type {}", self.type_);
        }
        self.super_script = list;
    }

    /// Returns true if this atom allows scripts (sub or super).
    pub fn is_script_allowed(&self) -> bool {
        self.type_.is_script_allowed()
    }

    pub fn is_not_binary_operator(&self) -> bool {
        self.type_.is_not_binary_operator()
    }

    // MARK: Subclass accessors

    pub fn as_fraction(&self) -> Option<&MTFraction> {
        match &self.kind {
            AtomKind::Fraction(fraction) => Some(fraction),
            _ => None,
        }
    }

    pub fn as_fraction_mut(&mut self) -> Option<&mut MTFraction> {
        match &mut self.kind {
            AtomKind::Fraction(fraction) => Some(fraction),
            _ => None,
        }
    }

    pub fn as_radical(&self) -> Option<&MTRadical> {
        match &self.kind {
            AtomKind::Radical(radical) => Some(radical),
            _ => None,
        }
    }

    pub fn as_radical_mut(&mut self) -> Option<&mut MTRadical> {
        match &mut self.kind {
            AtomKind::Radical(radical) => Some(radical),
            _ => None,
        }
    }

    pub fn as_large_operator(&self) -> Option<&MTLargeOperator> {
        match &self.kind {
            AtomKind::LargeOperator(op) => Some(op),
            _ => None,
        }
    }

    pub fn as_large_operator_mut(&mut self) -> Option<&mut MTLargeOperator> {
        match &mut self.kind {
            AtomKind::LargeOperator(op) => Some(op),
            _ => None,
        }
    }

    pub fn as_inner(&self) -> Option<&MTInner> {
        match &self.kind {
            AtomKind::Inner(inner) => Some(inner),
            _ => None,
        }
    }

    pub fn as_inner_mut(&mut self) -> Option<&mut MTInner> {
        match &mut self.kind {
            AtomKind::Inner(inner) => Some(inner),
            _ => None,
        }
    }

    pub fn as_table(&self) -> Option<&MTMathTable> {
        match &self.kind {
            AtomKind::Table(table) => Some(table),
            _ => None,
        }
    }

    pub fn as_table_mut(&mut self) -> Option<&mut MTMathTable> {
        match &mut self.kind {
            AtomKind::Table(table) => Some(table),
            _ => None,
        }
    }

    pub fn as_space(&self) -> Option<&MTMathSpace> {
        match &self.kind {
            AtomKind::Space(space) => Some(space),
            _ => None,
        }
    }

    pub fn as_style(&self) -> Option<&MTMathStyle> {
        match &self.kind {
            AtomKind::Style(style) => Some(style),
            _ => None,
        }
    }

    /// The color string of a `\color`, `\textcolor` or `\colorbox` atom.
    pub fn as_color(&self) -> Option<&MTColorAtom> {
        match &self.kind {
            AtomKind::Color(color) | AtomKind::TextColor(color) | AtomKind::Colorbox(color) => {
                Some(color)
            }
            _ => None,
        }
    }

    pub fn as_color_mut(&mut self) -> Option<&mut MTColorAtom> {
        match &mut self.kind {
            AtomKind::Color(color) | AtomKind::TextColor(color) | AtomKind::Colorbox(color) => {
                Some(color)
            }
            _ => None,
        }
    }

    /// The `innerList` of an inner, overline, underline, accent or colour atom.
    pub fn inner_list(&self) -> Option<&MTMathListRef> {
        match &self.kind {
            AtomKind::Inner(inner) => inner.inner_list.as_ref(),
            AtomKind::OverLine(atom) | AtomKind::UnderLine(atom) | AtomKind::Accent(atom) => {
                atom.inner_list.as_ref()
            }
            AtomKind::Color(color) | AtomKind::TextColor(color) | AtomKind::Colorbox(color) => {
                color.inner_list.as_ref()
            }
            _ => None,
        }
    }

    /// Sets the `innerList` of an atom class that has one.
    pub fn set_inner_list(&mut self, list: Option<MTMathListRef>) {
        match &mut self.kind {
            AtomKind::Inner(inner) => inner.inner_list = list,
            AtomKind::OverLine(atom) | AtomKind::UnderLine(atom) | AtomKind::Accent(atom) => {
                atom.inner_list = list
            }
            AtomKind::Color(color) | AtomKind::TextColor(color) | AtomKind::Colorbox(color) => {
                color.inner_list = list
            }
            _ => panic!("{} has no innerList", self.kind.class_name()),
        }
    }

    // MARK: Copying

    /// `copy()`: dispatches on the atom's type.
    pub fn copy(&self) -> MTMathAtomRef {
        wrap(self.copy_value())
    }

    fn copy_value(&self) -> MTMathAtom {
        let this = Some(self);
        match self.type_ {
            MTMathAtomType::LargeOperator => {
                let AtomKind::LargeOperator(op) = &self.kind else {
                    panic!(
                        "MTLargeOperator(nil): atom of type {} is not a MTLargeOperator",
                        self.type_
                    )
                };
                MTMathAtom {
                    type_: MTMathAtomType::LargeOperator,
                    kind: AtomKind::LargeOperator(MTLargeOperator { limits: op.limits }),
                    ..Self::base_copy(this)
                }
            }
            MTMathAtomType::Fraction => {
                let (base, fraction) = match &self.kind {
                    AtomKind::Fraction(fraction) => (
                        Self::base_copy(this),
                        MTFraction {
                            numerator: MTMathList::copy_of(fraction.numerator.as_ref()),
                            denominator: MTMathList::copy_of(fraction.denominator.as_ref()),
                            has_rule: fraction.has_rule,
                            left_delimiter: fraction.left_delimiter.clone(),
                            right_delimiter: fraction.right_delimiter.clone(),
                        },
                    ),
                    _ => (Self::base_copy(None), MTFraction::default()),
                };
                MTMathAtom {
                    type_: MTMathAtomType::Fraction,
                    kind: AtomKind::Fraction(fraction),
                    ..base
                }
            }
            MTMathAtomType::Radical => {
                let (base, radical) = match &self.kind {
                    AtomKind::Radical(radical) => (
                        Self::base_copy(this),
                        MTRadical {
                            radicand: MTMathList::copy_of(radical.radicand.as_ref()),
                            degree: MTMathList::copy_of(radical.degree.as_ref()),
                        },
                    ),
                    _ => (Self::base_copy(None), MTRadical::default()),
                };
                MTMathAtom {
                    type_: MTMathAtomType::Radical,
                    nucleus: String::new(),
                    kind: AtomKind::Radical(radical),
                    ..base
                }
            }
            MTMathAtomType::Style => {
                let AtomKind::Style(style) = &self.kind else {
                    panic!(
                        "MTMathStyle(nil): atom of type {} is not a MTMathStyle",
                        self.type_
                    )
                };
                MTMathAtom {
                    type_: MTMathAtomType::Style,
                    kind: AtomKind::Style(style.clone()),
                    ..Self::base_copy(this)
                }
            }
            MTMathAtomType::Inner => {
                let (base, inner) = match &self.kind {
                    AtomKind::Inner(inner) => (
                        Self::base_copy(this),
                        MTInner {
                            inner_list: MTMathList::copy_of(inner.inner_list.as_ref()),
                            // `MTMathAtom(inner?.leftBoundary)` is never nil: a
                            // missing boundary copies as an empty ordinary atom.
                            left_boundary: Some(Self::copy_of(
                                inner.left_boundary.as_ref().map(|b| b.borrow()).as_deref(),
                            )),
                            right_boundary: Some(Self::copy_of(
                                inner.right_boundary.as_ref().map(|b| b.borrow()).as_deref(),
                            )),
                        },
                    ),
                    _ => (
                        Self::base_copy(None),
                        MTInner {
                            inner_list: None,
                            left_boundary: Some(Self::copy_of(None)),
                            right_boundary: Some(Self::copy_of(None)),
                        },
                    ),
                };
                MTMathAtom {
                    type_: MTMathAtomType::Inner,
                    kind: AtomKind::Inner(inner),
                    ..base
                }
            }
            MTMathAtomType::Underline => {
                let (base, inner) = match &self.kind {
                    AtomKind::UnderLine(under) => (
                        Self::base_copy(this),
                        MTInnerListAtom {
                            inner_list: MTMathList::copy_of(under.inner_list.as_ref()),
                        },
                    ),
                    _ => (Self::base_copy(None), MTInnerListAtom::default()),
                };
                MTMathAtom {
                    type_: MTMathAtomType::Underline,
                    kind: AtomKind::UnderLine(inner),
                    ..base
                }
            }
            MTMathAtomType::Overline => self.overline_copy(),
            MTMathAtomType::Accent => {
                let (base, inner) = match &self.kind {
                    AtomKind::Accent(accent) => (
                        Self::base_copy(this),
                        MTInnerListAtom {
                            inner_list: MTMathList::copy_of(accent.inner_list.as_ref()),
                        },
                    ),
                    _ => (Self::base_copy(None), MTInnerListAtom::default()),
                };
                MTMathAtom {
                    type_: MTMathAtomType::Accent,
                    kind: AtomKind::Accent(inner),
                    ..base
                }
            }
            MTMathAtomType::Space => {
                let (base, space) = match &self.kind {
                    AtomKind::Space(space) => (Self::base_copy(this), space.space),
                    _ => (Self::base_copy(None), 0.0),
                };
                MTMathAtom {
                    type_: MTMathAtomType::Space,
                    kind: AtomKind::Space(MTMathSpace { space }),
                    ..base
                }
            }
            MTMathAtomType::Color | MTMathAtomType::Textcolor | MTMathAtomType::ColorBox => {
                let matches_class = matches!(
                    (&self.kind, self.type_),
                    (AtomKind::Color(_), MTMathAtomType::Color)
                        | (AtomKind::TextColor(_), MTMathAtomType::Textcolor)
                        | (AtomKind::Colorbox(_), MTMathAtomType::ColorBox)
                );
                let (base, color) = match (&self.kind, matches_class) {
                    (
                        AtomKind::Color(color)
                        | AtomKind::TextColor(color)
                        | AtomKind::Colorbox(color),
                        true,
                    ) => (
                        Self::base_copy(this),
                        MTColorAtom {
                            color_string: color.color_string.clone(),
                            inner_list: MTMathList::copy_of(color.inner_list.as_ref()),
                        },
                    ),
                    _ => (Self::base_copy(None), MTColorAtom::default()),
                };
                let kind = match self.type_ {
                    MTMathAtomType::Color => AtomKind::Color(color),
                    MTMathAtomType::Textcolor => AtomKind::TextColor(color),
                    _ => AtomKind::Colorbox(color),
                };
                MTMathAtom {
                    type_: self.type_,
                    kind,
                    ..base
                }
            }
            MTMathAtomType::Table => {
                let AtomKind::Table(table) = &self.kind else {
                    panic!(
                        "MTMathTable(self as! MTMathTable): atom of type {} is not a MTMathTable",
                        self.type_
                    )
                };
                let cells = table
                    .cells
                    .iter()
                    .map(|row| {
                        row.iter()
                            .map(|col| MTMathList::copy_of(Some(col)).unwrap())
                            .collect()
                    })
                    .collect();
                MTMathAtom {
                    type_: MTMathAtomType::Table,
                    kind: AtomKind::Table(MTMathTable {
                        alignments: table.alignments.clone(),
                        cells,
                        environment: table.environment.clone(),
                        inter_column_spacing: table.inter_column_spacing,
                        inter_row_additional_spacing: table.inter_row_additional_spacing,
                    }),
                    ..Self::base_copy(this)
                }
            }
            _ => Self::base_copy(this),
        }
    }

    /// `MTOverLine(_ over:)`.
    fn overline_copy(&self) -> MTMathAtom {
        let AtomKind::OverLine(over) = &self.kind else {
            panic!(
                "MTOverLine(nil): atom of type {} is not a MTOverLine",
                self.type_
            )
        };
        MTMathAtom {
            type_: MTMathAtomType::Overline,
            kind: AtomKind::OverLine(MTInnerListAtom {
                inner_list: MTMathList::copy_of(over.inner_list.as_ref()),
            }),
            ..Self::base_copy(Some(self))
        }
    }

    /// `finalized`: dispatches on the atom's class.
    pub fn finalized(&self) -> MTMathAtomRef {
        if let AtomKind::OverLine(_) = self.kind {
            // MTOverLine does not call super: its scripts are copied, not finalized.
            let mut new_overline = self.overline_copy();
            let inner = new_overline
                .inner_list()
                .map(|list| list.borrow().finalized());
            new_overline.set_inner_list(inner);
            return wrap(new_overline);
        }
        let finalized = self.copy();
        {
            let mut atom = finalized.borrow_mut();
            let super_script = atom
                .super_script
                .as_ref()
                .map(|list| list.borrow().finalized());
            atom.set_super_script(super_script);
            let sub_script = atom
                .sub_script
                .as_ref()
                .map(|list| list.borrow().finalized());
            atom.set_sub_script(sub_script);
        }
        {
            let mut atom = finalized.borrow_mut();
            // The copy's class follows the type; when the two disagree the
            // subclass's `super.finalized as! Subclass` traps.
            if std::mem::discriminant(&self.kind) != std::mem::discriminant(&atom.kind)
                && !matches!(self.kind, AtomKind::Atom)
            {
                panic!(
                    "finalized copy of {} is not a {}",
                    self.kind.class_name(),
                    self.kind.class_name()
                );
            }
            match &mut atom.kind {
                AtomKind::Fraction(fraction) => {
                    fraction.numerator = fraction
                        .numerator
                        .as_ref()
                        .map(|list| list.borrow().finalized());
                    fraction.denominator = fraction
                        .denominator
                        .as_ref()
                        .map(|list| list.borrow().finalized());
                }
                AtomKind::Radical(radical) => {
                    radical.radicand = radical
                        .radicand
                        .as_ref()
                        .map(|list| list.borrow().finalized());
                    radical.degree = radical
                        .degree
                        .as_ref()
                        .map(|list| list.borrow().finalized());
                }
                AtomKind::Inner(inner) => {
                    inner.inner_list = inner
                        .inner_list
                        .as_ref()
                        .map(|list| list.borrow().finalized());
                }
                AtomKind::UnderLine(inner) | AtomKind::Accent(inner) => {
                    inner.inner_list = inner
                        .inner_list
                        .as_ref()
                        .map(|list| list.borrow().finalized());
                }
                AtomKind::Color(color) | AtomKind::TextColor(color) | AtomKind::Colorbox(color) => {
                    color.inner_list = color
                        .inner_list
                        .as_ref()
                        .map(|list| list.borrow().finalized());
                }
                // MTMathTable.finalized finalizes each cell into a copy of the
                // row array, which it then drops: the cells stay as copied.
                _ => {}
            }
        }
        finalized
    }

    /// `fuse(with:)`: fuse `atom` into `this` by combining their nuclei.
    pub fn fuse(this: &MTMathAtomRef, atom: &MTMathAtomRef) {
        let fusible = {
            let target = this.borrow();
            let other = atom.borrow();
            target.sub_script.is_none()
                && target.super_script.is_none()
                && target.type_ == other.type_
        };
        if !fusible {
            println!("Can't fuse these 2 atoms");
            return;
        }
        let mut target = this.borrow_mut();
        if target.fused_atoms.is_empty() {
            let copy = Self::copy_of(Some(&*target));
            target.fused_atoms.push(copy);
        }
        let other = atom.borrow();
        if !other.fused_atoms.is_empty() {
            target.fused_atoms.extend(other.fused_atoms.iter().cloned());
        } else {
            target.fused_atoms.push(atom.clone());
        }
        target.nucleus.push_str(&other.nucleus);
        target.index_range.length += other.index_range.length;
        let super_script = other.super_script.clone();
        let sub_script = other.sub_script.clone();
        target.set_super_script(super_script);
        target.set_sub_script(sub_script);
    }

    // MARK: Descriptions

    fn scripts_description(&self, out: &mut String) {
        if let Some(super_script) = &self.super_script {
            out.push_str(&format!("^{{{}}}", super_script.borrow().description()));
        }
        if let Some(sub_script) = &self.sub_script {
            out.push_str(&format!("_{{{}}}", sub_script.borrow().description()));
        }
    }

    /// `description`.
    pub fn description(&self) -> String {
        match &self.kind {
            AtomKind::Fraction(fraction) => {
                let mut string = if fraction.has_rule {
                    "\\frac".to_owned()
                } else {
                    "\\atop".to_owned()
                };
                if !fraction.left_delimiter.is_empty() {
                    string += &format!("[{}]", fraction.left_delimiter);
                }
                if !fraction.right_delimiter.is_empty() {
                    string += &format!("[{}]", fraction.right_delimiter);
                }
                let numerator = fraction
                    .numerator
                    .as_ref()
                    .map_or("placeholder".to_owned(), |l| l.borrow().description());
                let denominator = fraction
                    .denominator
                    .as_ref()
                    .map_or("placeholder".to_owned(), |l| l.borrow().description());
                string += &format!("{{{numerator}}}{{{denominator}}}");
                self.scripts_description(&mut string);
                string
            }
            AtomKind::Radical(radical) => {
                let mut string = "\\sqrt".to_owned();
                if let Some(degree) = &radical.degree {
                    string += &format!("[{}]", degree.borrow().description());
                }
                if let Some(radicand) = &radical.radicand {
                    string += &format!("{{{}}}", radicand.borrow().description());
                }
                self.scripts_description(&mut string);
                string
            }
            AtomKind::Inner(inner) => {
                let mut string = "\\inner".to_owned();
                if let Some(left) = &inner.left_boundary {
                    string += &format!("[{}]", left.borrow().nucleus);
                }
                string += &format!(
                    "{{{}}}",
                    inner
                        .inner_list
                        .as_ref()
                        .expect("innerList")
                        .borrow()
                        .description()
                );
                if let Some(right) = &inner.right_boundary {
                    string += &format!("[{}]", right.borrow().nucleus);
                }
                self.scripts_description(&mut string);
                string
            }
            _ => {
                let mut string = self.nucleus.clone();
                self.scripts_description(&mut string);
                string
            }
        }
    }

    /// `string`.
    pub fn string(&self) -> String {
        match &self.kind {
            AtomKind::Color(color) => {
                format!(
                    "\\color{{{}}}{{{}}}",
                    color.color_string,
                    color
                        .inner_list
                        .as_ref()
                        .expect("innerList")
                        .borrow()
                        .string()
                )
            }
            AtomKind::TextColor(color) => format!(
                "\\textcolor{{{}}}{{{}}}",
                color.color_string,
                color
                    .inner_list
                    .as_ref()
                    .expect("innerList")
                    .borrow()
                    .string()
            ),
            AtomKind::Colorbox(color) => format!(
                "\\colorbox{{{}}}{{{}}}",
                color.color_string,
                color
                    .inner_list
                    .as_ref()
                    .expect("innerList")
                    .borrow()
                    .string()
            ),
            _ => {
                let mut string = self.nucleus.clone();
                if let Some(super_script) = &self.super_script {
                    string += &format!("^{{{}}}", super_script.borrow().string());
                }
                if let Some(sub_script) = &self.sub_script {
                    string += &format!("_{{{}}}", sub_script.borrow().string());
                }
                string
            }
        }
    }
}

/// `isNotBinaryOperator(_ prevNode:)`.
pub fn is_not_binary_operator(prev_node: Option<&MTMathAtomRef>) -> bool {
    match prev_node {
        None => true,
        Some(prev) => prev.borrow().type_.is_not_binary_operator(),
    }
}

// MARK: - Detached copies

/// A structural copy of an atom graph with fresh cells throughout: nothing in
/// the result is shared with the source, while sharing *inside* the source
/// (one style atom in every matrix cell, fused atoms) is kept.
#[derive(Default)]
pub(crate) struct Detacher {
    atoms: HashMap<*const RefCell<MTMathAtom>, MTMathAtomRef>,
    lists: HashMap<*const RefCell<MTMathList>, MTMathListRef>,
}

impl Detacher {
    pub(crate) fn atom(&mut self, source: &MTMathAtomRef) -> MTMathAtomRef {
        let key = Rc::as_ptr(source);
        if let Some(done) = self.atoms.get(&key) {
            return done.clone();
        }
        let copy = MTMathAtom::new();
        self.atoms.insert(key, copy.clone());
        let source = source.borrow();
        let value = MTMathAtom {
            type_: source.type_,
            sub_script: source.sub_script.as_ref().map(|list| self.list(list)),
            super_script: source.super_script.as_ref().map(|list| self.list(list)),
            nucleus: source.nucleus.clone(),
            index_range: source.index_range,
            font_style: source.font_style,
            fused_atoms: source
                .fused_atoms
                .iter()
                .map(|atom| self.atom(atom))
                .collect(),
            kind: self.kind(&source.kind),
        };
        *copy.borrow_mut() = value;
        copy
    }

    fn list(&mut self, source: &MTMathListRef) -> MTMathListRef {
        let key = Rc::as_ptr(source);
        if let Some(done) = self.lists.get(&key) {
            return done.clone();
        }
        let copy = MTMathList::new();
        self.lists.insert(key, copy.clone());
        let atoms = source
            .borrow()
            .atoms
            .iter()
            .map(|atom| self.atom(atom))
            .collect();
        copy.borrow_mut().atoms = atoms;
        copy
    }

    fn optional_list(&mut self, source: &Option<MTMathListRef>) -> Option<MTMathListRef> {
        source.as_ref().map(|list| self.list(list))
    }

    fn kind(&mut self, kind: &AtomKind) -> AtomKind {
        match kind {
            AtomKind::Atom => AtomKind::Atom,
            AtomKind::Fraction(fraction) => AtomKind::Fraction(MTFraction {
                has_rule: fraction.has_rule,
                left_delimiter: fraction.left_delimiter.clone(),
                right_delimiter: fraction.right_delimiter.clone(),
                numerator: self.optional_list(&fraction.numerator),
                denominator: self.optional_list(&fraction.denominator),
            }),
            AtomKind::Radical(radical) => AtomKind::Radical(MTRadical {
                radicand: self.optional_list(&radical.radicand),
                degree: self.optional_list(&radical.degree),
            }),
            AtomKind::LargeOperator(op) => AtomKind::LargeOperator(op.clone()),
            AtomKind::Inner(inner) => AtomKind::Inner(MTInner {
                inner_list: self.optional_list(&inner.inner_list),
                left_boundary: inner.left_boundary.as_ref().map(|atom| self.atom(atom)),
                right_boundary: inner.right_boundary.as_ref().map(|atom| self.atom(atom)),
            }),
            AtomKind::OverLine(atom) => AtomKind::OverLine(MTInnerListAtom {
                inner_list: self.optional_list(&atom.inner_list),
            }),
            AtomKind::UnderLine(atom) => AtomKind::UnderLine(MTInnerListAtom {
                inner_list: self.optional_list(&atom.inner_list),
            }),
            AtomKind::Accent(atom) => AtomKind::Accent(MTInnerListAtom {
                inner_list: self.optional_list(&atom.inner_list),
            }),
            AtomKind::Space(space) => AtomKind::Space(space.clone()),
            AtomKind::Style(style) => AtomKind::Style(style.clone()),
            AtomKind::Color(color) => AtomKind::Color(self.color(color)),
            AtomKind::TextColor(color) => AtomKind::TextColor(self.color(color)),
            AtomKind::Colorbox(color) => AtomKind::Colorbox(self.color(color)),
            AtomKind::Table(table) => AtomKind::Table(MTMathTable {
                alignments: table.alignments.clone(),
                cells: table
                    .cells
                    .iter()
                    .map(|row| row.iter().map(|cell| self.list(cell)).collect())
                    .collect(),
                environment: table.environment.clone(),
                inter_column_spacing: table.inter_column_spacing,
                inter_row_additional_spacing: table.inter_row_additional_spacing,
            }),
        }
    }

    fn color(&mut self, color: &MTColorAtom) -> MTColorAtom {
        MTColorAtom {
            color_string: color.color_string.clone(),
            inner_list: self.optional_list(&color.inner_list),
        }
    }
}

// MARK: - MTMathList

/// A representation of a list of math objects.
#[derive(Debug, Default)]
pub struct MTMathList {
    /// A list of MathAtoms
    pub atoms: Vec<MTMathAtomRef>,
}

impl MTMathList {
    /// `MTMathList()`.
    pub fn new() -> MTMathListRef {
        Rc::new(RefCell::new(MTMathList::default()))
    }

    /// `MTMathList(atoms:)`.
    pub fn with_atoms(atoms: Vec<MTMathAtomRef>) -> MTMathListRef {
        Rc::new(RefCell::new(MTMathList { atoms }))
    }

    /// `MTMathList(atom:)`.
    pub fn with_atom(atom: MTMathAtomRef) -> MTMathListRef {
        Self::with_atoms(vec![atom])
    }

    /// `MTMathList(_ list:)`: a copy of every atom, or nil.
    pub fn copy_of(list: Option<&MTMathListRef>) -> Option<MTMathListRef> {
        let list = list?.borrow();
        Some(Self::with_atoms(
            list.atoms.iter().map(|atom| atom.borrow().copy()).collect(),
        ))
    }

    /// `finalized`: combines like atoms and converts unary operators to
    /// binary operators, in a new list.
    pub fn finalized(&self) -> MTMathListRef {
        let finalized_list = MTMathList::new();
        let zero_range = NSRange::new(0, 0);

        let mut prev_node: Option<MTMathAtomRef> = None;
        for atom in &self.atoms {
            let new_node = atom.borrow().finalized();

            if zero_range == atom.borrow().index_range {
                let index = match &prev_node {
                    None => 0,
                    Some(prev) => {
                        let prev = prev.borrow();
                        prev.index_range.location + prev.index_range.length
                    }
                };
                new_node.borrow_mut().index_range = NSRange::new(index, 1);
            }

            let new_type = new_node.borrow().type_;
            match new_type {
                MTMathAtomType::BinaryOperator => {
                    if is_not_binary_operator(prev_node.as_ref()) {
                        new_node.borrow_mut().type_ = MTMathAtomType::UnaryOperator;
                    }
                }
                MTMathAtomType::Relation | MTMathAtomType::Punctuation | MTMathAtomType::Close => {
                    if let Some(prev) = &prev_node
                        && prev.borrow().type_ == MTMathAtomType::BinaryOperator
                    {
                        prev.borrow_mut().type_ = MTMathAtomType::UnaryOperator;
                    }
                }
                MTMathAtomType::Number => {
                    if let Some(prev) = &prev_node {
                        let fuse = {
                            let prev = prev.borrow();
                            prev.type_ == MTMathAtomType::Number
                                && prev.sub_script.is_none()
                                && prev.super_script.is_none()
                        };
                        if fuse {
                            MTMathAtom::fuse(prev, &new_node);
                            continue; // skip the current node, we are done here.
                        }
                    }
                }
                _ => {}
            }
            finalized_list.borrow_mut().add(Some(new_node.clone()));
            prev_node = Some(new_node);
        }
        if let Some(prev) = &prev_node
            && prev.borrow().type_ == MTMathAtomType::BinaryOperator
        {
            prev.borrow_mut().type_ = MTMathAtomType::UnaryOperator;
        }
        finalized_list
    }

    /// Add an atom to the end of the list. A boundary atom raises.
    pub fn add(&mut self, atom: Option<MTMathAtomRef>) {
        let Some(atom) = atom else { return };
        if Self::is_atom_allowed(&atom) {
            self.atoms.push(atom);
        } else {
            panic!(
                "Cannot add atom of type {} into mathlist",
                atom.borrow().type_.raw_value()
            );
        }
    }

    /// Inserts an atom at the given index; an index past the end is ignored.
    pub fn insert(&mut self, atom: Option<MTMathAtomRef>, index: usize) {
        let Some(atom) = atom else { return };
        if index > self.atoms.len() {
            return;
        }
        if Self::is_atom_allowed(&atom) {
            self.atoms.insert(index, atom);
        } else {
            panic!(
                "Cannot add atom of type {} into mathlist",
                atom.borrow().type_.raw_value()
            );
        }
    }

    /// Append the given list to the end of the current list.
    pub fn append(&mut self, list: Option<&MTMathListRef>) {
        let Some(list) = list else { return };
        let atoms: Vec<MTMathAtomRef> = list.borrow().atoms.clone();
        self.atoms.extend(atoms);
    }

    /// Removes the last atom from the math list, if any.
    pub fn remove_last_atom(&mut self) {
        self.atoms.pop();
    }

    /// Removes the atom at the given index. An index out of bounds raises.
    pub fn remove_atom(&mut self, index: usize) {
        if index >= self.atoms.len() {
            panic!("Index {index} out of bounds");
        }
        self.atoms.remove(index);
    }

    /// Removes all the atoms within the given closed range.
    pub fn remove_atoms(&mut self, range: std::ops::RangeInclusive<usize>) {
        if *range.start() >= self.atoms.len() {
            panic!("Index {} out of bounds", range.start());
        }
        if *range.end() >= self.atoms.len() {
            panic!("Index {} out of bounds", range.end());
        }
        self.atoms.drain(range);
    }

    fn is_atom_allowed(atom: &MTMathAtomRef) -> bool {
        atom.borrow().type_ != MTMathAtomType::Boundary
    }

    /// `description`: Swift's array description of the atoms.
    pub fn description(&self) -> String {
        let parts: Vec<String> = self
            .atoms
            .iter()
            .map(|atom| atom.borrow().description())
            .collect();
        format!("[{}]", parts.join(", "))
    }

    /// `string`: the same as `description` (this is not LaTeX).
    pub fn string(&self) -> String {
        self.description()
    }
}
