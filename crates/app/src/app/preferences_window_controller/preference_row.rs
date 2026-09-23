//! `PreferenceRow` and `PreferenceRowFilter` (PreferencesWindowController.swift,
//! "Form rows").

use std::ops::RangeInclusive;
use std::rc::Rc;

use upleft_swift_text as swift_text;

/// `PreferenceRow.ChoiceSelection`: what a popup should show.
///
/// A stored value that is no longer in the list — a theme whose file was
/// deleted, an editor that was uninstalled — must stay visible. Falling back
/// to the first item makes the popup disagree with the setting behind it, and
/// the user has no way to tell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChoiceSelection {
    Index(isize),
    Missing(String),
}

/// `PreferenceRow`: a row in a settings pane. Keeping the panes declarative
/// means a new preference is one line in `PreferencesForms`, not a layout
/// exercise — and it is what lets the search filter them.
///
/// Swift's closures are `Rc<dyn Fn>` here, so a row is as cheap to copy as the
/// Swift enum.
#[derive(Clone)]
pub enum PreferenceRow {
    /// `.toggle(_:help:get:set:)`.
    Toggle { title: String, help: Option<String>, get: Rc<dyn Fn() -> bool>, set: Rc<dyn Fn(bool)> },
    /// `.stepper(_:help:range:step:get:set:)`.
    Stepper {
        title: String,
        help: Option<String>,
        range: RangeInclusive<f64>,
        step: f64,
        get: Rc<dyn Fn() -> f64>,
        set: Rc<dyn Fn(f64)>,
    },
    /// `.choice(_:help:options:get:set:)`.
    Choice {
        title: String,
        help: Option<String>,
        options: Vec<String>,
        get: Rc<dyn Fn() -> ChoiceSelection>,
        set: Rc<dyn Fn(isize)>,
    },
    /// `.text(_:help:get:set:)`.
    Text { title: String, help: Option<String>, get: Rc<dyn Fn() -> String>, set: Rc<dyn Fn(String)> },
    /// `.button(_:_:)`.
    Button(String, Rc<dyn Fn()>),
    /// `.section(_:)`.
    Section(String),
    /// `.note(_:)`.
    Note(String),
    /// `.themePreview`.
    ThemePreview,
}

impl std::fmt::Debug for PreferenceRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PreferenceRow::Toggle { title, help, .. } => write!(f, "toggle({title:?}, help: {help:?})"),
            PreferenceRow::Stepper { title, help, range, step, .. } => {
                write!(f, "stepper({title:?}, help: {help:?}, range: {range:?}, step: {step})")
            }
            PreferenceRow::Choice { title, help, options, .. } => {
                write!(f, "choice({title:?}, help: {help:?}, options: {options:?})")
            }
            PreferenceRow::Text { title, help, .. } => write!(f, "text({title:?}, help: {help:?})"),
            PreferenceRow::Button(title, _) => write!(f, "button({title:?})"),
            PreferenceRow::Section(title) => write!(f, "section({title:?})"),
            PreferenceRow::Note(text) => write!(f, "note({text:?})"),
            PreferenceRow::ThemePreview => write!(f, "themePreview"),
        }
    }
}

impl PreferenceRow {
    /// `.toggle(title, help:, get:, set:)`.
    pub fn toggle(
        title: impl Into<String>,
        help: Option<&str>,
        get: impl Fn() -> bool + 'static,
        set: impl Fn(bool) + 'static,
    ) -> PreferenceRow {
        PreferenceRow::Toggle { title: title.into(), help: help.map(str::to_owned), get: Rc::new(get), set: Rc::new(set) }
    }

    /// `.stepper(title, help:, range:, step:, get:, set:)`.
    pub fn stepper(
        title: impl Into<String>,
        help: Option<&str>,
        range: RangeInclusive<f64>,
        step: f64,
        get: impl Fn() -> f64 + 'static,
        set: impl Fn(f64) + 'static,
    ) -> PreferenceRow {
        PreferenceRow::Stepper {
            title: title.into(),
            help: help.map(str::to_owned),
            range,
            step,
            get: Rc::new(get),
            set: Rc::new(set),
        }
    }

    /// `.choice(title, help:, options:, get:, set:)`.
    pub fn choice(
        title: impl Into<String>,
        help: Option<&str>,
        options: Vec<String>,
        get: impl Fn() -> ChoiceSelection + 'static,
        set: impl Fn(isize) + 'static,
    ) -> PreferenceRow {
        PreferenceRow::Choice {
            title: title.into(),
            help: help.map(str::to_owned),
            options,
            get: Rc::new(get),
            set: Rc::new(set),
        }
    }

    /// `.text(title, help:, get:, set:)`.
    pub fn text(
        title: impl Into<String>,
        help: Option<&str>,
        get: impl Fn() -> String + 'static,
        set: impl Fn(String) + 'static,
    ) -> PreferenceRow {
        PreferenceRow::Text { title: title.into(), help: help.map(str::to_owned), get: Rc::new(get), set: Rc::new(set) }
    }

    /// `.button(title) { … }`.
    pub fn button(title: impl Into<String>, action: impl Fn() + 'static) -> PreferenceRow {
        PreferenceRow::Button(title.into(), Rc::new(action))
    }

    /// `.section(title)`.
    pub fn section(title: impl Into<String>) -> PreferenceRow {
        PreferenceRow::Section(title.into())
    }

    /// `.note(text)`.
    pub fn note(text: impl Into<String>) -> PreferenceRow {
        PreferenceRow::Note(text.into())
    }

    /// Header rows carry no control of their own and survive a search only
    /// when something under them does.
    pub fn is_section(&self) -> bool {
        matches!(self, PreferenceRow::Section(_))
    }

    /// Every word the user might type to find this row.
    pub fn searchable_text(&self) -> String {
        match self {
            PreferenceRow::Toggle { title, help, .. } | PreferenceRow::Text { title, help, .. } => {
                [title.as_str(), help.as_deref().unwrap_or("")].join(" ")
            }
            PreferenceRow::Stepper { title, help, .. } => [title.as_str(), help.as_deref().unwrap_or("")].join(" "),
            PreferenceRow::Choice { title, help, options, .. } => {
                let mut parts: Vec<&str> = vec![title.as_str(), help.as_deref().unwrap_or("")];
                parts.extend(options.iter().map(String::as_str));
                parts.join(" ")
            }
            PreferenceRow::Button(title, _) => title.clone(),
            PreferenceRow::Section(title) => title.clone(),
            PreferenceRow::Note(text) => text.clone(),
            PreferenceRow::ThemePreview => "theme preview colors typography sample".to_owned(),
        }
    }
}

/// `PreferenceRowFilter`: pure filtering, so the search behaviour is testable
/// without a window.
pub struct PreferenceRowFilter;

impl PreferenceRowFilter {
    /// Keeps rows matching every word of the query, then drops section
    /// headers that ended up with nothing beneath them. A query that matches
    /// a section title keeps that whole section.
    pub fn apply(rows: &[PreferenceRow], query: &str) -> Vec<PreferenceRow> {
        let lowered = swift_text::lowercased(query);
        let words: Vec<&str> = swift_text::split_default(&lowered, ' ');
        if words.is_empty() {
            return rows.to_vec();
        }

        let matches = |row: &PreferenceRow| {
            let text = swift_text::lowercased(&row.searchable_text());
            words.iter().all(|word| swift_text::contains(&text, word))
        };

        let mut kept: Vec<PreferenceRow> = Vec::new();
        let mut section_matched = false;
        for row in rows {
            if row.is_section() {
                section_matched = matches(row);
                kept.push(row.clone());
                continue;
            }
            if section_matched || matches(row) {
                kept.push(row.clone());
            }
        }
        // Trailing pass: a header with no rows under it is noise.
        let mut pruned: Vec<PreferenceRow> = Vec::new();
        for (index, row) in kept.iter().enumerate() {
            if row.is_section() {
                let has_content = kept[(index + 1)..].iter().take_while(|row| !row.is_section()).next().is_some();
                if !has_content {
                    continue;
                }
            }
            pruned.push(row.clone());
        }
        pruned
    }
}
