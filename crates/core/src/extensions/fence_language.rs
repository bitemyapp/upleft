//! Extensions/FenceLanguage.swift — fenced language flags (§4.1).
//!
//! `mermaid` becomes a diagram and `math`/`latex` a display formula;
//! everything else stays a code block.

use crate::swift_text;

pub struct FenceLanguage;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FenceLanguageKind {
    Mermaid,
    Math,
    Code,
}

impl FenceLanguage {
    pub const MERMAID_ALIASES: &'static [&'static str] = &["mermaid"];
    pub const MATH_ALIASES: &'static [&'static str] = &["math", "latex", "tex", "katex"];

    fn alias_set_contains(set: &[&str], value: &str) -> bool {
        // `Set<String>.contains`: canonical-equivalence equality.
        set.iter().any(|alias| swift_text::str_eq(alias, value))
    }

    /// `kind(for:)`.
    pub fn kind(language: Option<&str>) -> FenceLanguageKind {
        let Some(language) = language else { return FenceLanguageKind::Code };
        let lowered = swift_text::lowercased(swift_text::trim_whitespaces(language));
        let Some(first) = swift_text::split_default(&lowered, ' ').first().map(|s| s.to_string()) else {
            return FenceLanguageKind::Code;
        };
        if Self::alias_set_contains(Self::MERMAID_ALIASES, &first) {
            return FenceLanguageKind::Mermaid;
        }
        if Self::alias_set_contains(Self::MATH_ALIASES, &first) {
            return FenceLanguageKind::Math;
        }
        FenceLanguageKind::Code
    }

    /// Guesses a language for §9.1's `codeFenceLanguages` rule.
    pub fn guess(code: &str) -> Option<String> {
        let lines: Vec<&str> = swift_text::split(code, '\n', usize::MAX, true);
        if lines.is_empty() {
            return None;
        }
        let body = code;

        if let Some(first) = lines.first()
            && swift_text::has_prefix(first, "#!")
        {
            let lower = swift_text::lowercased(first);
            if swift_text::contains(&lower, "python") {
                return Some("python".into());
            }
            if swift_text::contains(&lower, "node") {
                return Some("javascript".into());
            }
            if swift_text::contains(&lower, "ruby") {
                return Some("ruby".into());
            }
            if swift_text::contains(&lower, "bash") || swift_text::contains(&lower, "/sh") || swift_text::contains(&lower, "zsh") {
                return Some("bash".into());
            }
        }

        // Every line a shell prompt is unambiguous; a lone `$` is not.
        let command_lines: Vec<&&str> = lines.iter().filter(|line| !swift_text::trim_whitespaces(line).is_empty()).collect();
        if !command_lines.is_empty()
            && command_lines.iter().all(|line| swift_text::has_prefix(line, "$ ") || swift_text::has_prefix(line, "% "))
        {
            return Some("bash".into());
        }

        if Self::contains_any(body, &["func ", "let ", "var ", "guard ", "@objc", "import Foundation"])
            && Self::contains_any(body, &["func ", "guard ", "-> ", "@objc"])
        {
            return Some("swift".into());
        }
        if Self::contains_any(body, &["def ", "import ", "from "]) && Self::contains_any(body, &["def ", "self.", "elif ", "__init__"]) {
            return Some("python".into());
        }
        if Self::contains_any(body, &["const ", "let ", "function ", "=> "]) && Self::contains_any(body, &["{", ";"]) {
            return Some("javascript".into());
        }
        if swift_text::contains(body, "<")
            && swift_text::contains(body, ">")
            && Self::contains_any(body, &["<html", "<div", "<span", "<p>", "<!DOCTYPE", "</"])
        {
            return Some("html".into());
        }
        // Leading whitespace is legal in JSON documents, so trim first.
        let trimmed_body = swift_text::trim_whitespaces_and_newlines(body);
        if (swift_text::has_prefix(trimmed_body, "{") || swift_text::has_prefix(trimmed_body, "["))
            && Self::contains_any(body, &["\": ", "\":"])
        {
            return Some("json".into());
        }
        None
    }

    fn contains_any(haystack: &str, needles: &[&str]) -> bool {
        needles.iter().any(|needle| swift_text::contains(haystack, needle))
    }
}
