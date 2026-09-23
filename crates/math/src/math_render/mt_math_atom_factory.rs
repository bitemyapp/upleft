//! `MTMathAtomFactory.swift`: the symbol tables and atom constructors.

use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

use super::mt_math_list::{
    AtomKind, MTColumnAlignment, MTFontStyle, MTLineStyle, MTMathAtom, MTMathAtomRef,
    MTMathAtomType, MTMathList, MTMathListRef,
};
use super::mt_math_list_builder::{MTParseError, MTParseErrors};
use super::mt_unicode::unicode_symbol;
use crate::swift;

use MTMathAtomType as T;

pub const ALIASES: &[(&str, &str)] = &[
    ("lnot", "neg"),
    ("land", "wedge"),
    ("lor", "vee"),
    ("ne", "neq"),
    ("le", "leq"),
    ("ge", "geq"),
    ("lbrace", "{"),
    ("rbrace", "}"),
    ("Vert", "|"),
    ("gets", "leftarrow"),
    ("to", "rightarrow"),
    ("iff", "Longleftrightarrow"),
    ("AA", "angstrom"),
];

pub const DELIMITERS: &[(&str, &str)] = &[
    (".", ""), // . means no delimiter
    ("(", "("),
    (")", ")"),
    ("[", "["),
    ("]", "]"),
    ("<", "\u{2329}"),
    (">", "\u{232A}"),
    ("/", "/"),
    ("\\", "\\"),
    ("|", "|"),
    ("lgroup", "\u{27EE}"),
    ("rgroup", "\u{27EF}"),
    ("||", "\u{2016}"),
    ("Vert", "\u{2016}"),
    ("vert", "|"),
    ("uparrow", "\u{2191}"),
    ("downarrow", "\u{2193}"),
    ("updownarrow", "\u{2195}"),
    ("Uparrow", "\u{21D1}"),
    ("Downarrow", "\u{21D3}"),
    ("Updownarrow", "\u{21D5}"),
    ("backslash", "\\"),
    ("rangle", "\u{232A}"),
    ("langle", "\u{2329}"),
    ("rbrace", "}"),
    ("}", "}"),
    ("{", "{"),
    ("lbrace", "{"),
    ("lceil", "\u{2308}"),
    ("rceil", "\u{2309}"),
    ("lfloor", "\u{230A}"),
    ("rfloor", "\u{230B}"),
];

pub const ACCENTS: &[(&str, &str)] = &[
    ("grave", "\u{0300}"),
    ("acute", "\u{0301}"),
    ("hat", "\u{0302}"), // In our implementation hat and widehat behave the same.
    ("tilde", "\u{0303}"), // In our implementation tilde and widetilde behave the same.
    ("bar", "\u{0304}"),
    ("breve", "\u{0306}"),
    ("dot", "\u{0307}"),
    ("ddot", "\u{0308}"),
    ("check", "\u{030C}"),
    ("vec", "\u{20D7}"),
    ("widehat", "\u{0302}"),
    ("widetilde", "\u{0303}"),
];

fn lookup<'a>(table: &'a [(&'a str, &'a str)], key: &str) -> Option<&'a str> {
    table
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, value)| *value)
}

/// The reverse of a name → value table: for each value, the shortest name,
/// ties broken by `compare` (the alphabetically first).
fn reverse(pairs: impl Iterator<Item = (String, String)>) -> HashMap<String, String> {
    let mut output: HashMap<String, String> = HashMap::new();
    for (key, value) in pairs {
        if let Some(existing) = output.get(&value) {
            let (key_count, existing_count) = (swift::count(&key), swift::count(existing));
            if key_count > existing_count {
                continue;
            } else if key_count == existing_count
                && swift::compare(&key, existing) == std::cmp::Ordering::Greater
            {
                continue;
            }
        }
        output.insert(value, key);
    }
    output
}

static DELIM_VALUE_TO_NAME: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    reverse(
        DELIMITERS
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned())),
    )
});

static ACCENT_VALUE_TO_NAME: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    reverse(
        ACCENTS
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned())),
    )
});

/// What `supportedLatexSymbols` stores. `atom(forLatexSymbol:)` hands out a
/// `copy()` of the stored atom; every stored atom is a fresh one of these.
#[derive(Clone, Debug)]
pub enum SymbolTemplate {
    Atom {
        type_: MTMathAtomType,
        nucleus: String,
    },
    LargeOperator {
        nucleus: String,
        limits: bool,
    },
    Space(f64),
    Style(MTLineStyle),
}

impl SymbolTemplate {
    fn nucleus(&self) -> &str {
        match self {
            SymbolTemplate::Atom { nucleus, .. }
            | SymbolTemplate::LargeOperator { nucleus, .. } => nucleus,
            SymbolTemplate::Space(_) | SymbolTemplate::Style(_) => "",
        }
    }

    fn instantiate(&self) -> MTMathAtomRef {
        match self {
            SymbolTemplate::Atom { type_, nucleus } => MTMathAtom::with_type(*type_, nucleus),
            SymbolTemplate::LargeOperator { nucleus, limits } => {
                MTMathAtom::large_operator(nucleus, *limits)
            }
            SymbolTemplate::Space(space) => MTMathAtom::space(*space),
            SymbolTemplate::Style(style) => MTMathAtom::style(*style),
        }
    }

    /// The template for an atom handed to `add(latexSymbol:value:)`.
    fn of(atom: &MTMathAtom) -> SymbolTemplate {
        match &atom.kind {
            AtomKind::Atom => SymbolTemplate::Atom {
                type_: atom.type_,
                nucleus: atom.nucleus.clone(),
            },
            AtomKind::LargeOperator(op) => SymbolTemplate::LargeOperator {
                nucleus: atom.nucleus.clone(),
                limits: op.limits,
            },
            AtomKind::Space(space) => SymbolTemplate::Space(space.space),
            AtomKind::Style(style) => SymbolTemplate::Style(style.style),
            other => panic!(
                "add(latexSymbol:value:) supports plain atoms, operators, spaces and styles, not {}",
                other.class_name()
            ),
        }
    }
}

fn atom(type_: MTMathAtomType, nucleus: &str) -> SymbolTemplate {
    SymbolTemplate::Atom {
        type_,
        nucleus: nucleus.to_owned(),
    }
}

fn op(nucleus: &str, limits: bool) -> SymbolTemplate {
    SymbolTemplate::LargeOperator {
        nucleus: nucleus.to_owned(),
        limits,
    }
}

fn supported_latex_symbols() -> HashMap<String, SymbolTemplate> {
    let entries: Vec<(&str, SymbolTemplate)> = vec![
        ("square", atom(T::Placeholder, unicode_symbol::WHITE_SQUARE)),
        // Greek characters
        ("alpha", atom(T::Variable, "\u{03B1}")),
        ("beta", atom(T::Variable, "\u{03B2}")),
        ("gamma", atom(T::Variable, "\u{03B3}")),
        ("delta", atom(T::Variable, "\u{03B4}")),
        ("varepsilon", atom(T::Variable, "\u{03B5}")),
        ("zeta", atom(T::Variable, "\u{03B6}")),
        ("eta", atom(T::Variable, "\u{03B7}")),
        ("theta", atom(T::Variable, "\u{03B8}")),
        ("iota", atom(T::Variable, "\u{03B9}")),
        ("kappa", atom(T::Variable, "\u{03BA}")),
        ("lambda", atom(T::Variable, "\u{03BB}")),
        ("mu", atom(T::Variable, "\u{03BC}")),
        ("nu", atom(T::Variable, "\u{03BD}")),
        ("xi", atom(T::Variable, "\u{03BE}")),
        ("omicron", atom(T::Variable, "\u{03BF}")),
        ("pi", atom(T::Variable, "\u{03C0}")),
        ("rho", atom(T::Variable, "\u{03C1}")),
        ("varsigma", atom(T::Variable, "\u{03C1}")),
        ("sigma", atom(T::Variable, "\u{03C3}")),
        ("tau", atom(T::Variable, "\u{03C4}")),
        ("upsilon", atom(T::Variable, "\u{03C5}")),
        ("varphi", atom(T::Variable, "\u{03C6}")),
        ("chi", atom(T::Variable, "\u{03C7}")),
        ("psi", atom(T::Variable, "\u{03C8}")),
        ("omega", atom(T::Variable, "\u{03C9}")),
        // We mark the following greek chars as ordinary so that we don't try
        // to automatically italicize them as we do with variables.
        ("epsilon", atom(T::Ordinary, "\u{1D716}")),
        ("vartheta", atom(T::Ordinary, "\u{1D717}")),
        ("phi", atom(T::Ordinary, "\u{1D719}")),
        ("varrho", atom(T::Ordinary, "\u{1D71A}")),
        ("varpi", atom(T::Ordinary, "\u{1D71B}")),
        // Capital greek characters
        ("Gamma", atom(T::Variable, "\u{0393}")),
        ("Delta", atom(T::Variable, "\u{0394}")),
        ("Theta", atom(T::Variable, "\u{0398}")),
        ("Lambda", atom(T::Variable, "\u{039B}")),
        ("Xi", atom(T::Variable, "\u{039E}")),
        ("Pi", atom(T::Variable, "\u{03A0}")),
        ("Sigma", atom(T::Variable, "\u{03A3}")),
        ("Upsilon", atom(T::Variable, "\u{03A5}")),
        ("Phi", atom(T::Variable, "\u{03A6}")),
        ("Psi", atom(T::Variable, "\u{03A8}")),
        ("Omega", atom(T::Variable, "\u{03A9}")),
        // Open
        ("lceil", atom(T::Open, "\u{2308}")),
        ("lfloor", atom(T::Open, "\u{230A}")),
        ("langle", atom(T::Open, "\u{27E8}")),
        ("lgroup", atom(T::Open, "\u{27EE}")),
        // Close
        ("rceil", atom(T::Close, "\u{2309}")),
        ("rfloor", atom(T::Close, "\u{230B}")),
        ("rangle", atom(T::Close, "\u{27E9}")),
        ("rgroup", atom(T::Close, "\u{27EF}")),
        // Arrows
        ("leftarrow", atom(T::Relation, "\u{2190}")),
        ("uparrow", atom(T::Relation, "\u{2191}")),
        ("rightarrow", atom(T::Relation, "\u{2192}")),
        ("downarrow", atom(T::Relation, "\u{2193}")),
        ("leftrightarrow", atom(T::Relation, "\u{2194}")),
        ("updownarrow", atom(T::Relation, "\u{2195}")),
        ("nwarrow", atom(T::Relation, "\u{2196}")),
        ("nearrow", atom(T::Relation, "\u{2197}")),
        ("searrow", atom(T::Relation, "\u{2198}")),
        ("swarrow", atom(T::Relation, "\u{2199}")),
        ("mapsto", atom(T::Relation, "\u{21A6}")),
        ("Leftarrow", atom(T::Relation, "\u{21D0}")),
        ("Uparrow", atom(T::Relation, "\u{21D1}")),
        ("Rightarrow", atom(T::Relation, "\u{21D2}")),
        ("Downarrow", atom(T::Relation, "\u{21D3}")),
        ("Leftrightarrow", atom(T::Relation, "\u{21D4}")),
        ("Updownarrow", atom(T::Relation, "\u{21D5}")),
        ("longleftarrow", atom(T::Relation, "\u{27F5}")),
        ("longrightarrow", atom(T::Relation, "\u{27F6}")),
        ("longleftrightarrow", atom(T::Relation, "\u{27F7}")),
        ("Longleftarrow", atom(T::Relation, "\u{27F8}")),
        ("Longrightarrow", atom(T::Relation, "\u{27F9}")),
        ("Longleftrightarrow", atom(T::Relation, "\u{27FA}")),
        // Relations
        ("leq", atom(T::Relation, unicode_symbol::LESS_EQUAL)),
        ("geq", atom(T::Relation, unicode_symbol::GREATER_EQUAL)),
        ("neq", atom(T::Relation, unicode_symbol::NOT_EQUAL)),
        ("in", atom(T::Relation, "\u{2208}")),
        ("notin", atom(T::Relation, "\u{2209}")),
        ("ni", atom(T::Relation, "\u{220B}")),
        ("propto", atom(T::Relation, "\u{221D}")),
        ("mid", atom(T::Relation, "\u{2223}")),
        ("parallel", atom(T::Relation, "\u{2225}")),
        ("sim", atom(T::Relation, "\u{223C}")),
        ("simeq", atom(T::Relation, "\u{2243}")),
        ("cong", atom(T::Relation, "\u{2245}")),
        ("approx", atom(T::Relation, "\u{2248}")),
        ("asymp", atom(T::Relation, "\u{224D}")),
        ("doteq", atom(T::Relation, "\u{2250}")),
        ("equiv", atom(T::Relation, "\u{2261}")),
        ("gg", atom(T::Relation, "\u{226B}")),
        ("ll", atom(T::Relation, "\u{226A}")),
        ("prec", atom(T::Relation, "\u{227A}")),
        ("succ", atom(T::Relation, "\u{227B}")),
        ("subset", atom(T::Relation, "\u{2282}")),
        ("supset", atom(T::Relation, "\u{2283}")),
        ("subseteq", atom(T::Relation, "\u{2286}")),
        ("supseteq", atom(T::Relation, "\u{2287}")),
        ("sqsubset", atom(T::Relation, "\u{228F}")),
        ("sqsupset", atom(T::Relation, "\u{2290}")),
        ("sqsubseteq", atom(T::Relation, "\u{2291}")),
        ("sqsupseteq", atom(T::Relation, "\u{2292}")),
        ("models", atom(T::Relation, "\u{22A7}")),
        ("perp", atom(T::Relation, "\u{27C2}")),
        // operators
        (
            "times",
            atom(T::BinaryOperator, unicode_symbol::MULTIPLICATION),
        ),
        ("div", atom(T::BinaryOperator, unicode_symbol::DIVISION)),
        ("pm", atom(T::BinaryOperator, "\u{00B1}")),
        ("dagger", atom(T::BinaryOperator, "\u{2020}")),
        ("ddagger", atom(T::BinaryOperator, "\u{2021}")),
        ("mp", atom(T::BinaryOperator, "\u{2213}")),
        ("setminus", atom(T::BinaryOperator, "\u{2216}")),
        ("ast", atom(T::BinaryOperator, "\u{2217}")),
        ("circ", atom(T::BinaryOperator, "\u{2218}")),
        ("bullet", atom(T::BinaryOperator, "\u{2219}")),
        ("wedge", atom(T::BinaryOperator, "\u{2227}")),
        ("vee", atom(T::BinaryOperator, "\u{2228}")),
        ("cap", atom(T::BinaryOperator, "\u{2229}")),
        ("cup", atom(T::BinaryOperator, "\u{222A}")),
        ("wr", atom(T::BinaryOperator, "\u{2240}")),
        ("uplus", atom(T::BinaryOperator, "\u{228E}")),
        ("sqcap", atom(T::BinaryOperator, "\u{2293}")),
        ("sqcup", atom(T::BinaryOperator, "\u{2294}")),
        ("oplus", atom(T::BinaryOperator, "\u{2295}")),
        ("ominus", atom(T::BinaryOperator, "\u{2296}")),
        ("otimes", atom(T::BinaryOperator, "\u{2297}")),
        ("oslash", atom(T::BinaryOperator, "\u{2298}")),
        ("odot", atom(T::BinaryOperator, "\u{2299}")),
        ("star", atom(T::BinaryOperator, "\u{22C6}")),
        ("cdot", atom(T::BinaryOperator, "\u{22C5}")),
        ("amalg", atom(T::BinaryOperator, "\u{2A3F}")),
        // No limit operators
        ("log", op("log", false)),
        ("lg", op("lg", false)),
        ("ln", op("ln", false)),
        ("sin", op("sin", false)),
        ("arcsin", op("arcsin", false)),
        ("sinh", op("sinh", false)),
        ("cos", op("cos", false)),
        ("arccos", op("arccos", false)),
        ("cosh", op("cosh", false)),
        ("tan", op("tan", false)),
        ("arctan", op("arctan", false)),
        ("tanh", op("tanh", false)),
        ("cot", op("cot", false)),
        ("coth", op("coth", false)),
        ("sec", op("sec", false)),
        ("csc", op("csc", false)),
        ("arg", op("arg", false)),
        ("ker", op("ker", false)),
        ("dim", op("dim", false)),
        ("hom", op("hom", false)),
        ("exp", op("exp", false)),
        ("deg", op("deg", false)),
        // Limit operators
        ("lim", op("lim", true)),
        ("limsup", op("lim sup", true)),
        ("liminf", op("lim inf", true)),
        ("max", op("max", true)),
        ("min", op("min", true)),
        ("sup", op("sup", true)),
        ("inf", op("inf", true)),
        ("det", op("det", true)),
        ("Pr", op("Pr", true)),
        ("gcd", op("gcd", true)),
        // Large operators
        ("prod", op("\u{220F}", true)),
        ("coprod", op("\u{2210}", true)),
        ("sum", op("\u{2211}", true)),
        ("int", op("\u{222B}", false)),
        ("oint", op("\u{222E}", false)),
        ("bigwedge", op("\u{22C0}", true)),
        ("bigvee", op("\u{22C1}", true)),
        ("bigcap", op("\u{22C2}", true)),
        ("bigcup", op("\u{22C3}", true)),
        ("bigodot", op("\u{2A00}", true)),
        ("bigoplus", op("\u{2A01}", true)),
        ("bigotimes", op("\u{2A02}", true)),
        ("biguplus", op("\u{2A04}", true)),
        ("bigsqcup", op("\u{2A06}", true)),
        // Latex command characters
        ("{", atom(T::Open, "{")),
        ("}", atom(T::Close, "}")),
        ("$", atom(T::Ordinary, "$")),
        ("&", atom(T::Ordinary, "&")),
        ("#", atom(T::Ordinary, "#")),
        ("%", atom(T::Ordinary, "%")),
        ("_", atom(T::Ordinary, "_")),
        (" ", atom(T::Ordinary, " ")),
        ("backslash", atom(T::Ordinary, "\\")),
        // Punctuation
        // Note: \colon is different from : which is a relation
        ("colon", atom(T::Punctuation, ":")),
        ("cdotp", atom(T::Punctuation, "\u{00B7}")),
        // Other symbols
        ("degree", atom(T::Ordinary, "\u{00B0}")),
        ("neg", atom(T::Ordinary, "\u{00AC}")),
        ("angstrom", atom(T::Ordinary, "\u{00C5}")),
        ("aa", atom(T::Ordinary, "\u{00E5}")),
        ("ae", atom(T::Ordinary, "\u{00E6}")),
        ("o", atom(T::Ordinary, "\u{00F8}")),
        ("oe", atom(T::Ordinary, "\u{0153}")),
        ("ss", atom(T::Ordinary, "\u{00DF}")),
        ("cc", atom(T::Ordinary, "\u{00E7}")),
        ("CC", atom(T::Ordinary, "\u{00C7}")),
        ("O", atom(T::Ordinary, "\u{00D8}")),
        ("AE", atom(T::Ordinary, "\u{00C6}")),
        ("OE", atom(T::Ordinary, "\u{0152}")),
        ("|", atom(T::Ordinary, "\u{2016}")),
        ("vert", atom(T::Ordinary, "|")),
        ("ldots", atom(T::Ordinary, "\u{2026}")),
        ("prime", atom(T::Ordinary, "\u{2032}")),
        ("hbar", atom(T::Ordinary, "\u{210F}")),
        ("lbar", atom(T::Ordinary, "\u{019B}")),
        ("Im", atom(T::Ordinary, "\u{2111}")),
        ("ell", atom(T::Ordinary, "\u{2113}")),
        ("wp", atom(T::Ordinary, "\u{2118}")),
        ("Re", atom(T::Ordinary, "\u{211C}")),
        ("mho", atom(T::Ordinary, "\u{2127}")),
        ("aleph", atom(T::Ordinary, "\u{2135}")),
        ("forall", atom(T::Ordinary, "\u{2200}")),
        ("exists", atom(T::Ordinary, "\u{2203}")),
        ("emptyset", atom(T::Ordinary, "\u{2205}")),
        ("nabla", atom(T::Ordinary, "\u{2207}")),
        ("infty", atom(T::Ordinary, "\u{221E}")),
        ("angle", atom(T::Ordinary, "\u{2220}")),
        ("top", atom(T::Ordinary, "\u{22A4}")),
        ("bot", atom(T::Ordinary, "\u{22A5}")),
        ("vdots", atom(T::Ordinary, "\u{22EE}")),
        ("cdots", atom(T::Ordinary, "\u{22EF}")),
        ("ddots", atom(T::Ordinary, "\u{22F1}")),
        ("triangle", atom(T::Ordinary, "\u{25B3}")),
        ("imath", atom(T::Ordinary, "\u{1D6A4}")),
        ("jmath", atom(T::Ordinary, "\u{1D6A5}")),
        ("upquote", atom(T::Ordinary, "\u{0027}")),
        ("partial", atom(T::Ordinary, "\u{1D715}")),
        // Spacing
        (",", SymbolTemplate::Space(3.0)),
        (">", SymbolTemplate::Space(4.0)),
        (";", SymbolTemplate::Space(5.0)),
        ("!", SymbolTemplate::Space(-3.0)),
        ("quad", SymbolTemplate::Space(18.0)), // quad = 1em = 18mu
        ("qquad", SymbolTemplate::Space(36.0)), // qquad = 2em
        // Style
        ("displaystyle", SymbolTemplate::Style(MTLineStyle::Display)),
        ("textstyle", SymbolTemplate::Style(MTLineStyle::Text)),
        ("scriptstyle", SymbolTemplate::Style(MTLineStyle::Script)),
        (
            "scriptscriptstyle",
            SymbolTemplate::Style(MTLineStyle::ScriptOfScript),
        ),
    ];
    let mut table = HashMap::with_capacity(entries.len());
    for (name, template) in entries {
        let previous = table.insert(name.to_owned(), template);
        assert!(
            previous.is_none(),
            "Dictionary literal contains duplicate keys"
        );
    }
    table
}

struct SymbolTables {
    symbols: HashMap<String, SymbolTemplate>,
    text_to_latex: HashMap<String, String>,
}

static SYMBOLS: LazyLock<RwLock<SymbolTables>> = LazyLock::new(|| {
    let symbols = supported_latex_symbols();
    let text_to_latex = reverse(
        symbols
            .iter()
            .filter(|(_, atom)| swift::count(atom.nucleus()) != 0)
            .map(|(key, atom)| (key.clone(), atom.nucleus().to_owned())),
    );
    RwLock::new(SymbolTables {
        symbols,
        text_to_latex,
    })
});

/// `supportedAccentedCharacters`, keyed by NFC (a `Character` key matches
/// canonically equivalent characters).
const SUPPORTED_ACCENTED_CHARACTERS: &[(&str, (&str, &str))] = &[
    // Acute accents
    ("á", ("acute", "a")),
    ("é", ("acute", "e")),
    ("í", ("acute", "i")),
    ("ó", ("acute", "o")),
    ("ú", ("acute", "u")),
    ("ý", ("acute", "y")),
    // Grave accents
    ("à", ("grave", "a")),
    ("è", ("grave", "e")),
    ("ì", ("grave", "i")),
    ("ò", ("grave", "o")),
    ("ù", ("grave", "u")),
    // Circumflex
    ("â", ("hat", "a")),
    ("ê", ("hat", "e")),
    ("î", ("hat", "i")),
    ("ô", ("hat", "o")),
    ("û", ("hat", "u")),
    // Umlaut/dieresis
    ("ä", ("ddot", "a")),
    ("ë", ("ddot", "e")),
    ("ï", ("ddot", "i")),
    ("ö", ("ddot", "o")),
    ("ü", ("ddot", "u")),
    ("ÿ", ("ddot", "y")),
    // Tilde
    ("ã", ("tilde", "a")),
    ("ñ", ("tilde", "n")),
    ("õ", ("tilde", "o")),
    // Special characters
    ("ç", ("cc", "")),
    ("ø", ("o", "")),
    ("å", ("aa", "")),
    ("æ", ("ae", "")),
    ("œ", ("oe", "")),
    ("ß", ("ss", "")),
    ("'", ("upquote", "")), // this may be dangerous in math mode
    // Upper case variants
    ("Á", ("acute", "A")),
    ("É", ("acute", "E")),
    ("Í", ("acute", "I")),
    ("Ó", ("acute", "O")),
    ("Ú", ("acute", "U")),
    ("Ý", ("acute", "Y")),
    ("À", ("grave", "A")),
    ("È", ("grave", "E")),
    ("Ì", ("grave", "I")),
    ("Ò", ("grave", "O")),
    ("Ù", ("grave", "U")),
    ("Â", ("hat", "A")),
    ("Ê", ("hat", "E")),
    ("Î", ("hat", "I")),
    ("Ô", ("hat", "O")),
    ("Û", ("hat", "U")),
    ("Ä", ("ddot", "A")),
    ("Ë", ("ddot", "E")),
    ("Ï", ("ddot", "I")),
    ("Ö", ("ddot", "O")),
    ("Ü", ("ddot", "U")),
    ("Ã", ("tilde", "A")),
    ("Ñ", ("tilde", "N")),
    ("Õ", ("tilde", "O")),
    ("Ç", ("CC", "")),
    ("Ø", ("O", "")),
    ("Å", ("AA", "")),
    ("Æ", ("AE", "")),
    ("Œ", ("OE", "")),
];

static ACCENTED_CHARACTERS: LazyLock<HashMap<&'static str, (&'static str, &'static str)>> =
    LazyLock::new(|| SUPPORTED_ACCENTED_CHARACTERS.iter().copied().collect());

fn accented_character(ch: &str) -> Option<(&'static str, &'static str)> {
    // Only characters that are not plain ASCII (or are `'`) can be keys.
    if ch.is_ascii() && ch != "'" {
        return None;
    }
    ACCENTED_CHARACTERS.get(&*swift::nfc(ch)).copied()
}

const FONT_STYLES: &[(&str, MTFontStyle)] = &[
    ("mathnormal", MTFontStyle::DefaultStyle),
    ("mathrm", MTFontStyle::Roman),
    ("textrm", MTFontStyle::Roman),
    ("rm", MTFontStyle::Roman),
    ("mathbf", MTFontStyle::Bold),
    ("bf", MTFontStyle::Bold),
    ("textbf", MTFontStyle::Bold),
    ("mathcal", MTFontStyle::Caligraphic),
    ("cal", MTFontStyle::Caligraphic),
    ("mathtt", MTFontStyle::Typewriter),
    ("texttt", MTFontStyle::Typewriter),
    ("mathit", MTFontStyle::Italic),
    ("textit", MTFontStyle::Italic),
    ("mit", MTFontStyle::Italic),
    ("mathsf", MTFontStyle::SansSerif),
    ("textsf", MTFontStyle::SansSerif),
    ("mathfrak", MTFontStyle::Fraktur),
    ("frak", MTFontStyle::Fraktur),
    ("mathbb", MTFontStyle::Blackboard),
    ("mathbfit", MTFontStyle::BoldItalic),
    ("bm", MTFontStyle::BoldItalic),
    ("text", MTFontStyle::Roman),
];

const MATRIX_ENVS: &[(&str, &[&str])] = &[
    ("matrix", &[]),
    ("pmatrix", &["(", ")"]),
    ("bmatrix", &["[", "]"]),
    ("Bmatrix", &["{", "}"]),
    ("vmatrix", &["vert", "vert"]),
    ("Vmatrix", &["Vert", "Vert"]),
];

/// A factory to create commonly used MTMathAtoms.
pub struct MTMathAtomFactory;

impl MTMathAtomFactory {
    pub fn delim_value_to_name() -> &'static HashMap<String, String> {
        &DELIM_VALUE_TO_NAME
    }

    pub fn accent_value_to_name() -> &'static HashMap<String, String> {
        &ACCENT_VALUE_TO_NAME
    }

    pub fn supported_latex_symbol_names() -> Vec<String> {
        SYMBOLS.read().unwrap().symbols.keys().cloned().collect()
    }

    /// `textToLatexSymbolName[text]`.
    pub fn text_to_latex_symbol_name(text: &str) -> Option<String> {
        SYMBOLS.read().unwrap().text_to_latex.get(text).cloned()
    }

    pub fn font_style_with_name(font_name: &str) -> Option<MTFontStyle> {
        FONT_STYLES
            .iter()
            .find(|(name, _)| *name == font_name)
            .map(|(_, style)| *style)
    }

    pub fn font_name_for_style(font_style: MTFontStyle) -> &'static str {
        match font_style {
            MTFontStyle::DefaultStyle => "mathnormal",
            MTFontStyle::Roman => "mathrm",
            MTFontStyle::Bold => "mathbf",
            MTFontStyle::Fraktur => "mathfrak",
            MTFontStyle::Caligraphic => "mathcal",
            MTFontStyle::Italic => "mathit",
            MTFontStyle::SansSerif => "mathsf",
            MTFontStyle::Blackboard => "mathbb",
            MTFontStyle::Typewriter => "mathtt",
            MTFontStyle::BoldItalic => "bm",
        }
    }

    /// Returns an atom for the multiplication sign (i.e., \times or "*")
    pub fn times() -> MTMathAtomRef {
        MTMathAtom::with_type(T::BinaryOperator, unicode_symbol::MULTIPLICATION)
    }

    /// Returns an atom for the division sign (i.e., \div or "/")
    pub fn divide() -> MTMathAtomRef {
        MTMathAtom::with_type(T::BinaryOperator, unicode_symbol::DIVISION)
    }

    /// Returns an atom which is a placeholder square
    pub fn placeholder() -> MTMathAtomRef {
        MTMathAtom::with_type(T::Placeholder, unicode_symbol::WHITE_SQUARE)
    }

    /// Returns a fraction with a placeholder for the numerator and denominator
    pub fn placeholder_fraction() -> MTMathAtomRef {
        let frac = MTMathAtom::fraction(true);
        {
            let mut atom = frac.borrow_mut();
            let fraction = atom.as_fraction_mut().unwrap();
            let numerator = MTMathList::new();
            numerator.borrow_mut().add(Some(Self::placeholder()));
            fraction.numerator = Some(numerator);
            let denominator = MTMathList::new();
            denominator.borrow_mut().add(Some(Self::placeholder()));
            fraction.denominator = Some(denominator);
        }
        frac
    }

    /// Returns a square root with a placeholder as the radicand.
    pub fn placeholder_square_root() -> MTMathAtomRef {
        let rad = MTMathAtom::radical();
        {
            let mut atom = rad.borrow_mut();
            let radical = atom.as_radical_mut().unwrap();
            let radicand = MTMathList::new();
            radicand.borrow_mut().add(Some(Self::placeholder()));
            radical.radicand = Some(radicand);
        }
        rad
    }

    /// Returns a radical with a placeholder as the radicand.
    pub fn placeholder_radical() -> MTMathAtomRef {
        let rad = MTMathAtom::radical();
        {
            let mut atom = rad.borrow_mut();
            let radical = atom.as_radical_mut().unwrap();
            let radicand = MTMathList::new();
            let degree = MTMathList::new();
            radicand.borrow_mut().add(Some(Self::placeholder()));
            degree.borrow_mut().add(Some(Self::placeholder()));
            radical.radicand = Some(radicand);
            radical.degree = Some(degree);
        }
        rad
    }

    pub fn atom_from_accented_character(ch: &str) -> Option<MTMathAtomRef> {
        if let Some((name, base)) = accented_character(ch) {
            // first handle any special characters
            if let Some(atom) = Self::atom_for_latex_symbol(name) {
                return Some(atom);
            }
            if let Some(accent) = Self::accent_with_name(name) {
                // The command is an accent
                let list = MTMathList::new();
                let ch = swift::first_character(base).expect("Array(symbol.1)[0]");
                list.borrow_mut().add(Self::atom_for_character(ch));
                accent.borrow_mut().set_inner_list(Some(list));
                return Some(accent);
            }
        }
        None
    }

    /// Gets the atom with the right type for the given character, following
    /// LaTeX conventions; nil for characters it does not support.
    pub fn atom_for_character(ch: &str) -> Option<MTMathAtomRef> {
        let ch_str = ch;
        if !ch_str.is_ascii() && swift::in_closed_range(ch_str, "\u{0410}", "\u{044F}") {
            // Cyrillic alphabet
            return Some(MTMathAtom::with_type(T::Ordinary, ch_str));
        }
        if accented_character(ch).is_some() {
            return Self::atom_from_accented_character(ch);
        }
        let utf32 = swift::utf32_char(ch);
        if !(0x0021..=0x007E).contains(&utf32) {
            return None;
        }
        // Only a single printable ASCII character is left.
        match ch_str {
            "$" | "%" | "#" | "&" | "~" | "'" | "^" | "_" | "{" | "}" | "\\" => None,
            "(" | "[" => Some(MTMathAtom::with_type(T::Open, ch_str)),
            ")" | "]" | "!" | "?" => Some(MTMathAtom::with_type(T::Close, ch_str)),
            "," | ";" => Some(MTMathAtom::with_type(T::Punctuation, ch_str)),
            "=" | ">" | "<" => Some(MTMathAtom::with_type(T::Relation, ch_str)),
            // Math colon is ratio. Regular colon is \colon
            ":" => Some(MTMathAtom::with_type(T::Relation, "\u{2236}")),
            "-" => Some(MTMathAtom::with_type(T::BinaryOperator, "\u{2212}")),
            "+" | "*" => Some(MTMathAtom::with_type(T::BinaryOperator, ch_str)),
            "." => Some(MTMathAtom::with_type(T::Number, ch_str)),
            _ if swift::in_closed_range(ch_str, "0", "9") => {
                Some(MTMathAtom::with_type(T::Number, ch_str))
            }
            _ if swift::in_closed_range(ch_str, "a", "z")
                || swift::in_closed_range(ch_str, "A", "Z") =>
            {
                Some(MTMathAtom::with_type(T::Variable, ch_str))
            }
            "\"" | "/" | "@" | "`" | "|" => Some(MTMathAtom::with_type(T::Ordinary, ch_str)),
            // assertionFailure: a no-op in release builds.
            _ => None,
        }
    }

    /// One atom per character; characters that cannot be converted are ignored.
    pub fn atom_list_for(string: &str) -> MTMathListRef {
        let list = MTMathList::new();
        for character in swift::characters(string) {
            if let Some(new_atom) = Self::atom_for_character(character) {
                list.borrow_mut().add(Some(new_atom));
            }
        }
        list
    }

    /// Returns an atom for a latex symbol (e.g. theta), following aliases.
    pub fn atom_for_latex_symbol(name: &str) -> Option<MTMathAtomRef> {
        let name = lookup(ALIASES, name).unwrap_or(name);
        SYMBOLS
            .read()
            .unwrap()
            .symbols
            .get(name)
            .map(SymbolTemplate::instantiate)
    }

    /// The LaTeX symbol name for the given atom, if any.
    pub fn latex_symbol_name(atom: &MTMathAtom) -> Option<String> {
        if atom.nucleus.is_empty() {
            return None;
        }
        Self::text_to_latex_symbol_name(&atom.nucleus)
    }

    /// Define a latex symbol for rendering.
    pub fn add_latex_symbol(name: &str, value: &MTMathAtom) {
        let mut tables = SYMBOLS.write().unwrap();
        tables
            .symbols
            .insert(name.to_owned(), SymbolTemplate::of(value));
        tables
            .text_to_latex
            .insert(value.nucleus.clone(), name.to_owned());
    }

    /// Returns a large operator for the given name.
    pub fn operator_with_name(name: &str, limits: bool) -> MTMathAtomRef {
        MTMathAtom::large_operator(name, limits)
    }

    /// Returns an accent with the given name (`grave`, `hat`…), or nil.
    pub fn accent_with_name(name: &str) -> Option<MTMathAtomRef> {
        lookup(ACCENTS, name).map(MTMathAtom::accent)
    }

    /// Returns the accent name for the given accent.
    pub fn accent_name(accent: &MTMathAtom) -> Option<String> {
        ACCENT_VALUE_TO_NAME.get(&accent.nucleus).cloned()
    }

    /// Creates a new boundary atom for the given delimiter name, or nil.
    pub fn boundary_for_delimiter(name: &str) -> Option<MTMathAtomRef> {
        lookup(DELIMITERS, name).map(|value| MTMathAtom::with_type(T::Boundary, value))
    }

    /// Returns the delimiter name for a boundary atom.
    pub fn get_delimiter_name(boundary: &MTMathAtom) -> Option<String> {
        if boundary.type_ != T::Boundary {
            return None;
        }
        DELIM_VALUE_TO_NAME.get(&boundary.nucleus).cloned()
    }

    /// Returns a fraction with the given numerator and denominator.
    pub fn fraction_with(numerator: MTMathListRef, denominator: MTMathListRef) -> MTMathAtomRef {
        let frac = MTMathAtom::fraction(true);
        {
            let mut atom = frac.borrow_mut();
            let fraction = atom.as_fraction_mut().unwrap();
            fraction.numerator = Some(numerator);
            fraction.denominator = Some(denominator);
        }
        frac
    }

    pub fn math_list_for_characters(chars: &str) -> Option<MTMathListRef> {
        let list = MTMathList::new();
        for ch in swift::characters(chars) {
            if let Some(atom) = Self::atom_for_character(ch) {
                list.borrow_mut().add(Some(atom));
            }
        }
        Some(list)
    }

    /// `fraction(withNumeratorString:denominatorString:)`.
    pub fn fraction_with_strings(numerator: &str, denominator: &str) -> MTMathAtomRef {
        let num = Self::atom_list_for(numerator);
        let denom = Self::atom_list_for(denominator);
        Self::fraction_with(num, denom)
    }

    /// Builds a table for a given environment with the given rows.
    pub fn table(
        env: Option<&str>,
        rows: &[Vec<MTMathListRef>],
        error: &mut Option<MTParseError>,
    ) -> Option<MTMathAtomRef> {
        let table_atom = MTMathAtom::table(env);
        {
            let mut atom = table_atom.borrow_mut();
            let table = atom.as_table_mut().unwrap();
            for (i, row) in rows.iter().enumerate() {
                for (j, cell) in row.iter().enumerate() {
                    table.set_cell(cell.clone(), i, j);
                }
            }
        }

        let Some(env) = env else {
            let mut atom = table_atom.borrow_mut();
            let table = atom.as_table_mut().unwrap();
            table.inter_column_spacing = 0.0;
            table.inter_row_additional_spacing = 1.0;
            for i in 0..table.num_columns() {
                table.set_alignment(MTColumnAlignment::Left, i);
            }
            drop(atom);
            return Some(table_atom);
        };

        let column_error = |error: &mut Option<MTParseError>, message: String| {
            if error.is_none() {
                *error = Some(MTParseError::new(MTParseErrors::InvalidNumColumns, message));
            }
        };

        if let Some((_, delims)) = MATRIX_ENVS.iter().find(|(name, _)| *name == env) {
            {
                let mut atom = table_atom.borrow_mut();
                let table = atom.as_table_mut().unwrap();
                table.environment = "matrix".to_owned();
                table.inter_row_additional_spacing = 0.0;
                table.inter_column_spacing = 18.0;

                let style = MTMathAtom::style(MTLineStyle::Text);
                for row in &table.cells {
                    for cell in row {
                        cell.borrow_mut().insert(Some(style.clone()), 0);
                    }
                }
            }
            if delims.len() == 2 {
                let inner = MTMathAtom::inner();
                {
                    let mut atom = inner.borrow_mut();
                    let data = atom.as_inner_mut().unwrap();
                    data.set_left_boundary(Self::boundary_for_delimiter(delims[0]));
                    data.set_right_boundary(Self::boundary_for_delimiter(delims[1]));
                    data.inner_list = Some(MTMathList::with_atoms(vec![table_atom]));
                }
                return Some(inner);
            }
            return Some(table_atom);
        }
        if env == "eqalign" || env == "split" || env == "aligned" {
            let mut atom = table_atom.borrow_mut();
            let table = atom.as_table_mut().unwrap();
            if table.num_columns() != 2 {
                column_error(error, format!("{env} environment can only have 2 columns"));
                return None;
            }
            let spacer = MTMathAtom::with_type(T::Ordinary, "");
            for row in &table.cells {
                if row.len() >= 2 {
                    row[1].borrow_mut().insert(Some(spacer.clone()), 0);
                }
            }
            table.inter_row_additional_spacing = 1.0;
            table.inter_column_spacing = 0.0;
            table.set_alignment(MTColumnAlignment::Right, 0);
            table.set_alignment(MTColumnAlignment::Left, 1);
            drop(atom);
            return Some(table_atom);
        }
        if env == "displaylines" || env == "gather" {
            let mut atom = table_atom.borrow_mut();
            let table = atom.as_table_mut().unwrap();
            if table.num_columns() != 1 {
                column_error(error, format!("{env} environment can only have 1 column"));
                return None;
            }
            table.inter_row_additional_spacing = 1.0;
            table.inter_column_spacing = 0.0;
            table.set_alignment(MTColumnAlignment::Center, 0);
            drop(atom);
            return Some(table_atom);
        }
        if env == "eqnarray" {
            let mut atom = table_atom.borrow_mut();
            let table = atom.as_table_mut().unwrap();
            if table.num_columns() != 3 {
                column_error(error, format!("{env} environment can only have 3 columns"));
                return None;
            }
            table.inter_row_additional_spacing = 1.0;
            table.inter_column_spacing = 18.0;
            table.set_alignment(MTColumnAlignment::Right, 0);
            table.set_alignment(MTColumnAlignment::Center, 1);
            table.set_alignment(MTColumnAlignment::Left, 2);
            drop(atom);
            return Some(table_atom);
        }
        if env == "cases" {
            {
                let mut atom = table_atom.borrow_mut();
                let table = atom.as_table_mut().unwrap();
                if table.num_columns() != 2 {
                    column_error(
                        error,
                        "cases environment can only have 2 columns".to_owned(),
                    );
                    return None;
                }
                table.inter_row_additional_spacing = 0.0;
                table.inter_column_spacing = 18.0;
                table.set_alignment(MTColumnAlignment::Left, 0);
                table.set_alignment(MTColumnAlignment::Left, 1);

                let style = MTMathAtom::style(MTLineStyle::Text);
                for row in &table.cells {
                    for cell in row {
                        cell.borrow_mut().insert(Some(style.clone()), 0);
                    }
                }
            }
            let inner = MTMathAtom::inner();
            {
                let mut atom = inner.borrow_mut();
                let data = atom.as_inner_mut().unwrap();
                data.set_left_boundary(Self::boundary_for_delimiter("{"));
                data.set_right_boundary(Self::boundary_for_delimiter("."));
                let space = Self::atom_for_latex_symbol(",").unwrap();
                data.inner_list = Some(MTMathList::with_atoms(vec![space, table_atom]));
            }
            return Some(inner);
        }
        *error = Some(MTParseError::new(
            MTParseErrors::InvalidEnv,
            format!("Unknown environment {env}"),
        ));
        None
    }
}
