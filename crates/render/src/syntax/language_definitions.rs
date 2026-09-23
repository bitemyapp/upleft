//! Port of `Syntax/LanguageDefinitions.swift`: one `LanguageSpec` per
//! language, read by the one scanner in `GenericLexer`.
//!
//! Word lists are the *reserved* spellings only. Anything a program names
//! itself is classified structurally.

use super::language_spec::{BlockCommentSpec, LanguageSpec, RawStringStyle, StringPrefixSpec, StringSpec};
use super::scanner_core::of;
use super::syntax_contracts::SyntaxToken;
use super::word_table::WordTable;

use SyntaxToken as T;

pub fn spec(name: &str) -> Option<LanguageSpec> {
    match name {
        "swift" => Some(swift()),
        "typescript" | "tsx" => Some(typescript(name)),
        "javascript" | "jsx" => Some(javascript(name)),
        "python" => Some(python()),
        "rust" => Some(rust()),
        "go" => Some(go()),
        "ruby" => Some(ruby()),
        "java" => Some(java()),
        "c" => Some(c()),
        "cpp" => Some(cpp()),
        "objc" => Some(objc()),
        "bash" => Some(bash()),
        "json" => Some(json()),
        "yaml" => Some(yaml()),
        "toml" => Some(toml()),
        "sql" => Some(sql()),
        "css" => Some(css()),
        _ => None,
    }
}

fn line(marker: &str) -> Vec<u8> {
    marker.as_bytes().to_vec()
}

// MARK: - Swift

fn swift() -> LanguageSpec {
    LanguageSpec {
        words: WordTable::new(
            false,
            &[
                (
                    T::Keyword,
                    &[
                        "actor", "associatedtype", "async", "await", "borrowing", "break", "case", "catch", "class",
                        "consuming", "continue", "convenience", "default", "defer", "deinit", "didSet", "do",
                        "dynamic", "each", "else", "enum", "extension", "fallthrough", "fileprivate", "final",
                        "for", "func", "get", "guard", "if", "import", "in", "indirect", "infix", "init", "inout",
                        "internal", "is", "lazy", "let", "macro", "mutating", "nonisolated", "nonmutating", "open",
                        "operator", "optional", "override", "package", "postfix", "precedencegroup", "prefix",
                        "private", "protocol", "public", "repeat", "required", "rethrows", "return", "set", "some",
                        "static", "struct", "subscript", "super", "switch", "throw", "throws", "try", "typealias",
                        "unowned", "var", "weak", "where", "while", "willSet", "as", "any",
                    ],
                ),
                (T::Constant, &["true", "false", "nil", "self", "Self"]),
            ],
        ),
        capitalised_are_types: true,
        calls_are_functions: true,
        line_comments: vec![line("//")],
        block_comments: vec![BlockCommentSpec::with("/*", "*/", true, false)],
        strings: vec![StringSpec::spanning("\"\"\""), StringSpec::new("\"")],
        raw_strings: vec![RawStringStyle::SwiftHash],
        attribute_sigils: vec![of('@')],
        hash_directive_token: Some(T::Attribute),
        ..LanguageSpec::named("swift")
    }
}

// MARK: - TypeScript / JavaScript

const ECMA_KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "case", "catch", "class", "const", "continue", "debugger", "default", "delete",
    "do", "else", "export", "extends", "finally", "for", "from", "function", "get", "if", "import", "in",
    "instanceof", "let", "new", "of", "return", "set", "static", "super", "switch", "this", "throw", "try",
    "typeof", "var", "void", "while", "with", "yield",
];

const TYPESCRIPT_KEYWORDS: &[&str] = &[
    "abstract", "accessor", "asserts", "declare", "enum", "implements", "infer", "interface", "is", "keyof",
    "namespace", "module", "out", "override", "private", "protected", "public", "readonly", "satisfies", "type",
    "unique", "using",
];

fn javascript(name: &str) -> LanguageSpec {
    ecma_spec(name, &[], &[])
}

fn typescript(name: &str) -> LanguageSpec {
    ecma_spec(
        name,
        TYPESCRIPT_KEYWORDS,
        &["any", "bigint", "boolean", "never", "number", "object", "string", "symbol", "unknown"],
    )
}

fn ecma_spec(name: &str, extra_keywords: &[&str], extra_types: &[&str]) -> LanguageSpec {
    let keywords: Vec<&str> = ECMA_KEYWORDS.iter().chain(extra_keywords).copied().collect();
    let types: Vec<&str> = extra_types
        .iter()
        .copied()
        .chain(["Array", "Object", "Promise", "Map", "Set", "Date", "RegExp", "Error"])
        .collect();
    LanguageSpec {
        words: WordTable::new(
            false,
            &[
                (T::Constant, &["true", "false", "null", "undefined", "NaN", "Infinity"]),
                (T::Keyword, &keywords),
                (T::Type, &types),
            ],
        ),
        all_caps_are_constants: true,
        capitalised_are_types: true,
        calls_are_functions: true,
        line_comments: vec![line("//")],
        block_comments: vec![BlockCommentSpec::new("/*", "*/")],
        strings: vec![StringSpec::spanning("`"), StringSpec::new("\""), StringSpec::new("'")],
        attribute_sigils: vec![of('@')],
        ..LanguageSpec::named(name)
    }
}

// MARK: - Python

fn python() -> LanguageSpec {
    LanguageSpec {
        words: WordTable::new(
            false,
            &[
                (T::Constant, &["True", "False", "None", "self", "cls", "NotImplemented", "Ellipsis"]),
                (
                    T::Keyword,
                    &[
                        "and", "as", "assert", "async", "await", "break", "case", "class", "continue", "def", "del",
                        "elif", "else", "except", "finally", "for", "from", "global", "if", "import", "in", "is",
                        "lambda", "match", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while",
                        "with", "yield",
                    ],
                ),
                (
                    T::Type,
                    &[
                        "bool", "bytearray", "bytes", "complex", "dict", "float", "frozenset", "int", "list",
                        "object", "range", "set", "str", "tuple", "type",
                    ],
                ),
            ],
        ),
        all_caps_are_constants: true,
        capitalised_are_types: true,
        calls_are_functions: true,
        line_comments: vec![line("#")],
        strings: vec![
            StringSpec::spanning("\"\"\""),
            StringSpec::spanning("'''"),
            StringSpec::new("\""),
            StringSpec::new("'"),
        ],
        string_prefixes: vec![
            StringPrefixSpec::new("f"),
            StringPrefixSpec::new("b"),
            StringPrefixSpec::new("u"),
            StringPrefixSpec::new("fr"),
            StringPrefixSpec::new("bf"),
            StringPrefixSpec::with("r", false),
            StringPrefixSpec::with("rb", false),
            StringPrefixSpec::with("br", false),
            StringPrefixSpec::with("rf", false),
        ],
        attribute_sigils: vec![of('@')],
        ..LanguageSpec::named("python")
    }
}

// MARK: - Rust

fn rust() -> LanguageSpec {
    LanguageSpec {
        words: WordTable::new(
            false,
            &[
                (T::Constant, &["true", "false", "None", "Some", "Ok", "Err", "self", "Self"]),
                (
                    T::Keyword,
                    &[
                        "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
                        "extern", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut",
                        "pub", "ref", "return", "static", "struct", "super", "trait", "type", "union", "unsafe",
                        "use", "where", "while", "yield",
                    ],
                ),
                (
                    T::Type,
                    &[
                        "bool", "char", "f32", "f64", "i8", "i16", "i32", "i64", "i128", "isize", "str", "u8",
                        "u16", "u32", "u64", "u128", "usize",
                    ],
                ),
            ],
        ),
        all_caps_are_constants: true,
        capitalised_are_types: true,
        calls_are_functions: true,
        line_comments: vec![line("//")],
        block_comments: vec![BlockCommentSpec::with("/*", "*/", true, false)],
        // Rust string literals may contain raw newlines.
        strings: vec![StringSpec::spanning("\""), StringSpec::new("'")],
        string_prefixes: vec![StringPrefixSpec::new("b")],
        raw_strings: vec![RawStringStyle::RustHash],
        hash_attributes: true,
        has_lifetimes: true,
        ..LanguageSpec::named("rust")
    }
}

// MARK: - Go

fn go() -> LanguageSpec {
    LanguageSpec {
        words: WordTable::new(
            false,
            &[
                (T::Constant, &["true", "false", "nil", "iota"]),
                (
                    T::Keyword,
                    &[
                        "break", "case", "chan", "const", "continue", "default", "defer", "else", "fallthrough",
                        "for", "func", "go", "goto", "if", "import", "interface", "map", "package", "range",
                        "return", "select", "struct", "switch", "type", "var",
                    ],
                ),
                (
                    T::Type,
                    &[
                        "any", "bool", "byte", "comparable", "complex64", "complex128", "error", "float32",
                        "float64", "int", "int8", "int16", "int32", "int64", "rune", "string", "uint", "uint8",
                        "uint16", "uint32", "uint64", "uintptr",
                    ],
                ),
            ],
        ),
        all_caps_are_constants: true,
        capitalised_are_types: true,
        calls_are_functions: true,
        line_comments: vec![line("//")],
        block_comments: vec![BlockCommentSpec::new("/*", "*/")],
        strings: vec![StringSpec::with("`", None, None, true), StringSpec::new("\""), StringSpec::new("'")],
        ..LanguageSpec::named("go")
    }
}

// MARK: - Ruby

fn ruby() -> LanguageSpec {
    LanguageSpec {
        words: WordTable::new(
            false,
            &[
                (T::Constant, &["true", "false", "nil", "self", "__FILE__", "__LINE__"]),
                (
                    T::Keyword,
                    &[
                        "BEGIN", "END", "alias", "and", "begin", "break", "case", "class", "def", "do", "else",
                        "elsif", "end", "ensure", "extend", "for", "if", "in", "include", "lambda", "module",
                        "next", "not", "or", "proc", "redo", "require", "require_relative", "rescue", "retry",
                        "return", "super", "then", "undef", "unless", "until", "when", "while", "yield",
                    ],
                ),
                (T::Function, &["attr_accessor", "attr_reader", "attr_writer", "puts", "print", "raise", "new"]),
            ],
        ),
        all_caps_are_constants: true,
        capitalised_are_types: true,
        calls_are_functions: true,
        line_comments: vec![line("#")],
        block_comments: vec![BlockCommentSpec::with("=begin", "=end", false, true)],
        strings: vec![StringSpec::spanning("\""), StringSpec::with("'", None, None, true)],
        variable_sigils: vec![of('@'), of('$')],
        has_symbols: true,
        ..LanguageSpec::named("ruby")
    }
}

// MARK: - Java

fn java() -> LanguageSpec {
    LanguageSpec {
        words: WordTable::new(
            false,
            &[
                (T::Constant, &["true", "false", "null", "this", "super"]),
                (
                    T::Keyword,
                    &[
                        "abstract", "assert", "break", "case", "catch", "class", "const", "continue", "default",
                        "do", "else", "enum", "extends", "final", "finally", "for", "goto", "if", "implements",
                        "import", "instanceof", "interface", "native", "new", "package", "permits", "private",
                        "protected", "public", "record", "return", "sealed", "static", "strictfp", "switch",
                        "synchronized", "throw", "throws", "transient", "try", "var", "volatile", "while", "yield",
                    ],
                ),
                (T::Type, &["boolean", "byte", "char", "double", "float", "int", "long", "short", "void"]),
            ],
        ),
        all_caps_are_constants: true,
        capitalised_are_types: true,
        calls_are_functions: true,
        line_comments: vec![line("//")],
        block_comments: vec![BlockCommentSpec::new("/*", "*/")],
        strings: vec![StringSpec::spanning("\"\"\""), StringSpec::new("\""), StringSpec::new("'")],
        attribute_sigils: vec![of('@')],
        ..LanguageSpec::named("java")
    }
}

// MARK: - C family

const C_KEYWORDS: &[&str] = &[
    "auto", "break", "case", "const", "continue", "default", "do", "else", "enum", "extern", "for", "goto", "if",
    "inline", "register", "restrict", "return", "sizeof", "static", "struct", "switch", "typedef", "union",
    "volatile", "while", "_Atomic", "_Bool", "_Generic", "_Static_assert", "_Thread_local",
];

const C_TYPES: &[&str] = &[
    "bool", "char", "double", "float", "int", "int8_t", "int16_t", "int32_t", "int64_t", "long", "ptrdiff_t",
    "short", "signed", "size_t", "ssize_t", "uint8_t", "uint16_t", "uint32_t", "uint64_t", "unsigned", "void",
    "wchar_t",
];

fn concat(a: &[&'static str], b: &[&'static str]) -> Vec<&'static str> {
    a.iter().chain(b).copied().collect()
}

fn c() -> LanguageSpec {
    LanguageSpec {
        words: WordTable::new(
            false,
            &[(T::Constant, &["NULL", "true", "false"]), (T::Keyword, C_KEYWORDS), (T::Type, C_TYPES)],
        ),
        all_caps_are_constants: true,
        calls_are_functions: true,
        line_comments: vec![line("//")],
        block_comments: vec![BlockCommentSpec::new("/*", "*/")],
        strings: vec![StringSpec::new("\""), StringSpec::new("'")],
        hash_directive_token: Some(T::Attribute),
        ..LanguageSpec::named("c")
    }
}

fn cpp() -> LanguageSpec {
    let keywords = concat(
        C_KEYWORDS,
        &[
            "alignas", "alignof", "asm", "catch", "class", "co_await", "co_return", "co_yield", "concept",
            "consteval", "constexpr", "constinit", "const_cast", "decltype", "delete", "dynamic_cast", "explicit",
            "export", "friend", "mutable", "namespace", "new", "noexcept", "operator", "private", "protected",
            "public", "reinterpret_cast", "requires", "static_assert", "static_cast", "template", "thread_local",
            "throw", "try", "typeid", "typename", "using", "virtual",
        ],
    );
    let types = concat(
        C_TYPES,
        &[
            "char8_t", "char16_t", "char32_t", "map", "optional", "set", "shared_ptr", "string", "string_view",
            "unique_ptr", "unordered_map", "unordered_set", "vector",
        ],
    );
    LanguageSpec {
        words: WordTable::new(
            false,
            &[
                (T::Constant, &["NULL", "nullptr", "true", "false", "this"]),
                (T::Keyword, &keywords),
                (T::Type, &types),
            ],
        ),
        all_caps_are_constants: true,
        calls_are_functions: true,
        line_comments: vec![line("//")],
        block_comments: vec![BlockCommentSpec::new("/*", "*/")],
        strings: vec![StringSpec::new("\""), StringSpec::new("'")],
        raw_strings: vec![RawStringStyle::CppDelimited],
        hash_directive_token: Some(T::Attribute),
        ..LanguageSpec::named("cpp")
    }
}

fn objc() -> LanguageSpec {
    let keywords = concat(
        C_KEYWORDS,
        &[
            "assign", "atomic", "copy", "nonatomic", "nonnull", "nullable", "readonly", "readwrite", "retain",
            "strong", "unsafe_unretained", "weak",
        ],
    );
    let types = concat(C_TYPES, &["BOOL", "IBAction", "IBOutlet", "id", "instancetype", "SEL", "IMP", "Class"]);
    LanguageSpec {
        words: WordTable::new(
            false,
            &[
                (T::Constant, &["NULL", "nil", "Nil", "YES", "NO", "true", "false", "self", "super"]),
                (T::Keyword, &keywords),
                (T::Type, &types),
            ],
        ),
        all_caps_are_constants: true,
        capitalised_are_types: true,
        calls_are_functions: true,
        line_comments: vec![line("//")],
        block_comments: vec![BlockCommentSpec::new("/*", "*/")],
        strings: vec![StringSpec::new("\""), StringSpec::new("'")],
        attribute_sigils: vec![of('@')],
        objc_string_sigil: true,
        hash_directive_token: Some(T::Attribute),
        ..LanguageSpec::named("objc")
    }
}

// MARK: - Shell

fn bash() -> LanguageSpec {
    LanguageSpec {
        words: WordTable::new(
            false,
            &[
                (
                    T::Keyword,
                    &[
                        "case", "coproc", "do", "done", "elif", "else", "esac", "fi", "for", "function", "if", "in",
                        "select", "then", "time", "until", "while",
                    ],
                ),
                (T::Constant, &["true", "false"]),
                (
                    T::Function,
                    &[
                        "alias", "break", "cd", "command", "continue", "declare", "echo", "eval", "exec", "exit",
                        "export", "getopts", "kill", "let", "local", "popd", "printf", "pushd", "read", "readonly",
                        "return", "set", "shift", "source", "test", "trap", "type", "typeset", "ulimit", "umask",
                        "unalias", "unset", "wait",
                    ],
                ),
            ],
        ),
        all_caps_are_constants: true,
        line_comments: vec![line("#")],
        line_comment_needs_word_start: true,
        strings: vec![StringSpec::spanning("\""), StringSpec::with("'", None, None, true)],
        variable_sigils: vec![of('$')],
        ..LanguageSpec::named("bash")
    }
}

// MARK: - Data formats

// `//` and `/* */` are accepted because JSONC is what tooling and agents
// actually emit; strict JSON never contains them.
fn json() -> LanguageSpec {
    LanguageSpec {
        words: WordTable::new(false, &[(T::Constant, &["true", "false", "null"])]),
        line_comments: vec![line("//")],
        block_comments: vec![BlockCommentSpec::new("/*", "*/")],
        strings: vec![StringSpec::new("\"")],
        key_terminators: vec![of(':')],
        keys_from_strings: true,
        ..LanguageSpec::named("json")
    }
}

fn yaml() -> LanguageSpec {
    LanguageSpec {
        words: WordTable::new(true, &[(T::Constant, &["true", "false", "null", "yes", "no", "on", "off", "~"])]),
        line_comments: vec![line("#")],
        strings: vec![StringSpec::new("\""), StringSpec::with("'", None, None, false)],
        identifier_extra_continues: vec![of('-'), of('.')],
        key_terminators: vec![of(':')],
        keys_from_strings: true,
        keys_from_identifiers: true,
        key_terminator_needs_space: true,
        ..LanguageSpec::named("yaml")
    }
}

fn toml() -> LanguageSpec {
    LanguageSpec {
        words: WordTable::new(false, &[(T::Constant, &["true", "false"])]),
        line_comments: vec![line("#")],
        strings: vec![
            StringSpec::spanning("\"\"\""),
            StringSpec::with("'''", None, None, true),
            StringSpec::new("\""),
            StringSpec::with("'", None, None, false),
        ],
        identifier_extra_continues: vec![of('-'), of('.')],
        key_terminators: vec![of('=')],
        keys_from_strings: true,
        keys_from_identifiers: true,
        bracket_section_headers: true,
        ..LanguageSpec::named("toml")
    }
}

fn sql() -> LanguageSpec {
    LanguageSpec {
        words: WordTable::new(
            true,
            &[
                (T::Constant, &["true", "false", "null", "current_date", "current_time", "current_timestamp"]),
                (
                    T::Keyword,
                    &[
                        "all", "alter", "analyze", "and", "as", "asc", "begin", "between", "by", "cascade", "case",
                        "check", "commit", "constraint", "create", "cross", "delete", "desc", "distinct", "drop",
                        "else", "end", "exists", "explain", "foreign", "from", "full", "grant", "group", "having",
                        "in", "index", "inner", "insert", "into", "is", "join", "key", "left", "like", "limit",
                        "not", "offset", "on", "or", "order", "outer", "over", "partition", "primary", "recursive",
                        "references", "returning", "revoke", "right", "rollback", "select", "set", "table", "then",
                        "transaction", "truncate", "union", "unique", "update", "using", "values", "view", "when",
                        "where", "window", "with",
                    ],
                ),
                (
                    T::Type,
                    &[
                        "array", "bigint", "boolean", "bytea", "char", "date", "decimal", "double", "float", "int",
                        "integer", "json", "jsonb", "numeric", "precision", "real", "serial", "smallint", "text",
                        "time", "timestamp", "timestamptz", "uuid", "varchar",
                    ],
                ),
            ],
        ),
        calls_are_functions: true,
        line_comments: vec![line("--")],
        block_comments: vec![BlockCommentSpec::new("/*", "*/")],
        // `''` inside a literal reads as close-then-open; the two runs are
        // adjacent and merge, so the doubled-quote escape needs no special case.
        strings: vec![StringSpec::with("'", None, None, false), StringSpec::with("\"", None, None, false)],
        ..LanguageSpec::named("sql")
    }
}

fn css() -> LanguageSpec {
    LanguageSpec {
        words: WordTable::new(
            true,
            &[
                (
                    T::Constant,
                    &["auto", "inherit", "initial", "none", "revert", "unset", "currentColor", "transparent"],
                ),
                (T::Keyword, &["and", "from", "important", "not", "only", "to"]),
            ],
        ),
        calls_are_functions: true,
        block_comments: vec![BlockCommentSpec::new("/*", "*/")],
        strings: vec![StringSpec::new("\""), StringSpec::new("'")],
        identifier_extra_starts: vec![of('-')],
        identifier_extra_continues: vec![of('-')],
        attribute_sigils: vec![of('@')],
        // `#fff` and `#main` are both literals in CSS, not directives.
        hash_directive_token: Some(T::Constant),
        key_terminators: vec![of(':')],
        keys_from_identifiers: true,
        key_terminator_needs_space: true,
        ..LanguageSpec::named("css")
    }
}
