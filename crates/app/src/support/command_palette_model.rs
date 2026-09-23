//! Port of `Sources/DownrightApp/Support/CommandPaletteModel.swift`.
//!
//! Pure search and selection state for the command palette, the synonyms
//! each command may be found by, and the `UserDefaults` store of recently
//! run commands.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_foundation::{NSArray, NSString, NSUserDefaults};
use upleft_swift_text as swift_text;

use super::commands::{Command, CommandScope, KeyBinding};
use super::keybindings::KeybindingStore;
use super::quick_open_providers::{
    QuickOpenAction, QuickOpenFilter, QuickOpenProvider, QuickOpenProviderKind, QuickOpenQuery, QuickOpenResult,
};
use crate::panels::fuzzy_matcher::FuzzyMatcher;

/// A small, typed description of one command shown by the palette.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandPaletteEntry {
    pub command: Command,
    pub title: String,
    pub synonyms: Vec<String>,
    pub binding: Option<String>,
    pub scopes: Vec<CommandScope>,
}

impl CommandPaletteEntry {
    pub fn id(&self) -> Command {
        self.command
    }

    pub fn scope_label(&self) -> String {
        let mut seen = HashSet::new();
        let titles: Vec<&str> =
            self.scopes.iter().map(|scope| scope.palette_title()).filter(|title| seen.insert(*title)).collect();
        titles.join(" / ")
    }

    /// Everything a query is allowed to match. The rendered subtitle
    /// ("⌘⇧K  ·  Document") is deliberately absent: searching it made `doc`
    /// match nearly every command in the app.
    pub fn search_candidates(&self) -> Vec<String> {
        std::iter::once(self.title.clone()).chain(self.synonyms.iter().cloned()).collect()
    }
}

impl CommandScope {
    /// `CommandScope.paletteTitle`.
    pub fn palette_title(self) -> &'static str {
        match self {
            CommandScope::Read | CommandScope::Live => "Document",
            CommandScope::Source => "Source",
        }
    }
}

/// A persistence seam for command history. The palette does not know where
/// history lives, which keeps tests deterministic and avoids disk work while
/// the user types.
pub trait CommandPaletteRecentStore {
    fn recent_commands(&self) -> Vec<Command>;
    fn record(&self, command: Command);
}

/// `UserDefaultsCommandPaletteRecentStore`: an array of raw values under
/// `commandPalette.recentCommands`, newest first, at most `limit` long.
pub struct UserDefaultsCommandPaletteRecentStore {
    defaults: Retained<NSUserDefaults>,
    key: String,
    limit: usize,
}

impl UserDefaultsCommandPaletteRecentStore {
    pub const DEFAULT_KEY: &'static str = "commandPalette.recentCommands";
    pub const DEFAULT_LIMIT: isize = 12;

    /// `init(defaults:key:limit:)`.
    pub fn new(defaults: Retained<NSUserDefaults>, key: &str, limit: isize) -> UserDefaultsCommandPaletteRecentStore {
        UserDefaultsCommandPaletteRecentStore { defaults, key: key.to_owned(), limit: limit.max(1) as usize }
    }

    /// `init()`: `UserDefaults.standard`, the default key and limit.
    pub fn standard() -> UserDefaultsCommandPaletteRecentStore {
        Self::new(NSUserDefaults::standardUserDefaults(), Self::DEFAULT_KEY, Self::DEFAULT_LIMIT)
    }
}

impl CommandPaletteRecentStore for UserDefaultsCommandPaletteRecentStore {
    /// `defaults.array(forKey:) as? [String]`, then `compactMap`: an array
    /// holding anything but strings reads as empty.
    fn recent_commands(&self) -> Vec<Command> {
        objc2::rc::autoreleasepool(|_| {
            let Some(values) = self.defaults.arrayForKey(&NSString::from_str(&self.key)) else { return Vec::new() };
            let mut strings = Vec::with_capacity(values.len());
            for value in values.iter() {
                let Some(string) = value.downcast_ref::<NSString>() else { return Vec::new() };
                strings.push(swift_text::ns::foundation::to_string(string));
            }
            strings.iter().filter_map(|raw| Command::from_raw_value(raw)).collect()
        })
    }

    fn record(&self, command: Command) {
        let mut values: Vec<Command> = self.recent_commands().into_iter().filter(|value| *value != command).collect();
        values.insert(0, command);
        values.truncate(self.limit);
        objc2::rc::autoreleasepool(|_| {
            let strings: Vec<Retained<NSString>> =
                values.iter().map(|value| NSString::from_str(value.raw_value())).collect();
            let array = NSArray::from_retained_slice(&strings);
            let object: &AnyObject = &array;
            unsafe { self.defaults.setObject_forKey(Some(object), &NSString::from_str(&self.key)) };
        });
    }
}

/// Fuzzy search for the palette.
///
/// There is one matcher in the app: [`FuzzyMatcher`], the dynamic program the
/// outline panel already uses, which scores word boundaries, prefixes, and
/// consecutive runs and reports the positions it matched. This wrapper only
/// decides *which strings* a query is allowed to see and how several terms
/// combine.
pub struct PaletteSearch;

impl PaletteSearch {
    /// Best score across `candidates`; `None` when the query matches none of
    /// them. An empty query matches everything with score 0.
    pub fn score(query: &str, candidates: &[String]) -> Option<isize> {
        let lowered = swift_text::lowercased(query);
        let mut terms: Vec<&str> = Vec::new();
        let mut start = 0;
        let mut offset = 0;
        for character in swift_text::graphemes(&lowered) {
            if swift_text::char_is(character, ' ') || swift_text::char_is(character, '\t') {
                if start < offset {
                    terms.push(&lowered[start..offset]);
                }
                start = offset + character.len();
            }
            offset += character.len();
        }
        if start < lowered.len() {
            terms.push(&lowered[start..]);
        }
        if terms.is_empty() {
            return Some(0);
        }
        let haystacks: Vec<&String> = candidates.iter().filter(|candidate| !candidate.is_empty()).collect();
        if haystacks.is_empty() {
            return None;
        }

        // Every term must land somewhere, so `edit cells` finds the command
        // whose title and synonyms together cover both words.
        let mut total = 0;
        for term in terms {
            let best = haystacks.iter().filter_map(|haystack| FuzzyMatcher::r#match(term, haystack)).map(|m| m.score).max()?;
            total += best;
        }
        Some(total)
    }
}

#[derive(Default)]
struct PaletteResultCache {
    results_query: Option<String>,
    results: Option<Vec<CommandPaletteEntry>>,
    quick_query: Option<String>,
    quick_results: Option<Vec<QuickOpenResult>>,
}

impl PaletteResultCache {
    fn invalidate(&mut self) {
        self.results_query = None;
        self.results = None;
        self.quick_query = None;
        self.quick_results = None;
    }
}

/// A candidate and the numbers it is ordered by. One ordering serves both
/// result lists, so the palette cannot rank one way and the quick-open list
/// another.
#[derive(Clone)]
struct Ranked<Value> {
    value: Value,
    score: isize,
    command: Option<Command>,
    title: String,
}

/// Pure search and selection state for the command palette.
///
/// Cloning shares the result cache, as copying the Swift struct shares its
/// cache object.
#[derive(Clone)]
pub struct CommandPaletteModel {
    cache: Rc<RefCell<PaletteResultCache>>,
    entries: Vec<CommandPaletteEntry>,
    recent_commands: Vec<Command>,
    providers: Vec<Rc<dyn QuickOpenProvider>>,
    selected_index: isize,
    query: String,
}

impl CommandPaletteModel {
    /// `init(entries:recentCommands:providers:)`.
    pub fn new(
        entries: Vec<CommandPaletteEntry>,
        recent_commands: Vec<Command>,
        providers: Vec<Rc<dyn QuickOpenProvider>>,
    ) -> CommandPaletteModel {
        CommandPaletteModel {
            cache: Rc::new(RefCell::new(PaletteResultCache::default())),
            entries,
            recent_commands: Self::unique(&recent_commands),
            providers,
            selected_index: 0,
            query: String::new(),
        }
    }

    /// `init(commands:bindings:recentCommands:providers:)`. Swift's defaults
    /// are `Command.allCases` and [`Self::store_bindings`].
    pub fn with_commands(
        commands: &[Command],
        bindings: impl Fn(Command) -> Vec<KeyBinding>,
        recent_commands: Vec<Command>,
        providers: Vec<Rc<dyn QuickOpenProvider>>,
    ) -> CommandPaletteModel {
        let entries = commands
            .iter()
            .map(|&command| CommandPaletteEntry {
                command,
                title: command.title().to_owned(),
                synonyms: synonyms(command).iter().map(|synonym| (*synonym).to_owned()).collect(),
                binding: bindings(command).first().map(KeyBinding::display_string),
                scopes: CommandScope::ALL_CASES.into_iter().filter(|scope| command.scopes().contains(scope)).collect(),
            })
            .collect();
        Self::new(entries, recent_commands, providers)
    }

    /// The default `bindings:` argument: `KeybindingStore.shared.bindings(for:)`.
    pub fn store_bindings(command: Command) -> Vec<KeyBinding> {
        KeybindingStore::shared().bindings(command)
    }

    pub fn entries(&self) -> &[CommandPaletteEntry] {
        &self.entries
    }

    pub fn recent_commands(&self) -> &[Command] {
        &self.recent_commands
    }

    pub fn providers(&self) -> &[Rc<dyn QuickOpenProvider>] {
        &self.providers
    }

    pub fn selected_index(&self) -> isize {
        self.selected_index
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    // MARK: - Ranking

    /// Score, then recency, then title.
    fn ordered<Value: Clone>(items: Vec<Ranked<Value>>, recents: &[Command]) -> Vec<Value> {
        let recency = |command: Option<Command>| {
            command.and_then(|command| recents.iter().position(|recent| *recent == command)).unwrap_or(usize::MAX)
        };
        swift_text::sort::sorted_by(items, |lhs, rhs| {
            if lhs.score != rhs.score {
                return lhs.score > rhs.score;
            }
            let left = recency(lhs.command);
            let right = recency(rhs.command);
            if left != right {
                return left < right;
            }
            swift_text::ns::foundation::localized_standard_compare(&lhs.title, &rhs.title) == std::cmp::Ordering::Less
        })
        .into_iter()
        .map(|ranked| ranked.value)
        .collect()
    }

    fn ranked_entries(entries: &[CommandPaletteEntry], query: &str) -> Vec<Ranked<CommandPaletteEntry>> {
        let trimmed = swift_text::trim_whitespaces_and_newlines(query);
        entries
            .iter()
            .filter_map(|entry| {
                let score = PaletteSearch::score(trimmed, &entry.search_candidates())?;
                Some(Ranked { value: entry.clone(), score, command: Some(entry.command), title: entry.title.clone() })
            })
            .collect()
    }

    // MARK: - Results

    pub fn results(&self) -> Vec<CommandPaletteEntry> {
        let mut cache = self.cache.borrow_mut();
        let stale = cache.results.is_none()
            || !cache.results_query.as_deref().is_some_and(|query| swift_text::str_eq(query, &self.query));
        if stale {
            cache.results = Some(Self::compute_results(&self.entries, &self.recent_commands, &self.query));
            cache.results_query = Some(self.query.clone());
        }
        cache.results.clone().unwrap_or_default()
    }

    fn compute_results(entries: &[CommandPaletteEntry], recent_commands: &[Command], query: &str) -> Vec<CommandPaletteEntry> {
        Self::ordered(Self::ranked_entries(entries, query), recent_commands)
    }

    pub fn selected_entry(&self) -> Option<CommandPaletteEntry> {
        let values = self.results();
        usize::try_from(self.selected_index).ok().and_then(|index| values.get(index).cloned())
    }

    /// One ranked list for commands and every injected Quick Open provider.
    pub fn quick_results(&self) -> Vec<QuickOpenResult> {
        let mut cache = self.cache.borrow_mut();
        let stale = cache.quick_results.is_none()
            || !cache.quick_query.as_deref().is_some_and(|query| swift_text::str_eq(query, &self.query));
        if stale {
            cache.quick_results =
                Some(Self::compute_quick_results(&self.entries, &self.recent_commands, &self.providers, &self.query));
            cache.quick_query = Some(self.query.clone());
        }
        cache.quick_results.clone().unwrap_or_default()
    }

    /// `quickResults` read in place: the cached list, computed if stale,
    /// handed to `body` without copying it (the palette reads its count and
    /// one row at a time). `body` must not call back into the model.
    pub fn with_quick_results<R>(&self, body: impl FnOnce(&[QuickOpenResult]) -> R) -> R {
        let mut cache = self.cache.borrow_mut();
        let stale = cache.quick_results.is_none()
            || !cache.quick_query.as_deref().is_some_and(|query| swift_text::str_eq(query, &self.query));
        if stale {
            cache.quick_results =
                Some(Self::compute_quick_results(&self.entries, &self.recent_commands, &self.providers, &self.query));
            cache.quick_query = Some(self.query.clone());
        }
        body(cache.quick_results.as_deref().unwrap_or(&[]))
    }

    fn compute_quick_results(
        entries: &[CommandPaletteEntry],
        recent_commands: &[Command],
        providers: &[Rc<dyn QuickOpenProvider>],
        query: &str,
    ) -> Vec<QuickOpenResult> {
        let parsed = QuickOpenQuery::new(query);
        let mut candidates: Vec<Ranked<QuickOpenResult>> = Vec::new();

        if matches!(parsed.filter, QuickOpenFilter::All | QuickOpenFilter::Commands) {
            // Commands keep the score the entry matcher computed, synonyms and
            // all; re-scoring them against rendered text would throw it away.
            for ranked in Self::ranked_entries(entries, &parsed.terms) {
                let entry = &ranked.value;
                let subtitle = entry.binding.iter().cloned().chain(std::iter::once(entry.scope_label())).collect::<Vec<_>>().join("  ·  ");
                let result = QuickOpenResult {
                    id: format!("command:{}", entry.command.raw_value()),
                    kind: QuickOpenProviderKind::Command,
                    title: entry.title.clone(),
                    subtitle,
                    search_text: entry.synonyms.join(" "),
                    action: QuickOpenAction::Command(entry.command),
                    score: ranked.score,
                };
                candidates.push(Ranked { value: result, score: ranked.score, command: Some(entry.command), title: entry.title.clone() });
            }
        }

        // Providers list; they do not rank. One function scores everything so
        // a heading and a command compete on the same scale.
        for result in providers.iter().flat_map(|provider| provider.results(&parsed)) {
            let Some(score) = PaletteSearch::score(&parsed.terms, &[result.title.clone(), result.search_text.clone()]) else {
                continue;
            };
            let total = score + result.score;
            let title = result.title.clone();
            candidates.push(Ranked { value: result.scored(total), score: total, command: None, title });
        }

        let ranked = Self::ordered(candidates, recent_commands);
        if !(parsed.terms.is_empty() && parsed.filter == QuickOpenFilter::All) {
            return ranked;
        }
        let recent_rank: HashMap<String, usize> = recent_commands
            .iter()
            .enumerate()
            .map(|(offset, command)| (format!("command:{}", command.raw_value()), offset))
            .collect();
        swift_text::sort::sorted_by(ranked, |lhs, rhs| {
            let left_recent = recent_rank.get(&lhs.id).copied().unwrap_or(usize::MAX);
            let right_recent = recent_rank.get(&rhs.id).copied().unwrap_or(usize::MAX);
            if left_recent != right_recent {
                return left_recent < right_recent;
            }
            let left_kind = lhs.kind.index();
            let right_kind = rhs.kind.index();
            if left_kind != right_kind {
                return left_kind < right_kind;
            }
            swift_text::ns::foundation::localized_standard_compare(&lhs.title, &rhs.title) == std::cmp::Ordering::Less
        })
    }

    pub fn selected_result(&self) -> Option<QuickOpenResult> {
        let values = self.quick_results();
        usize::try_from(self.selected_index).ok().and_then(|index| values.get(index).cloned())
    }

    // MARK: - Selection

    pub fn update_query(&mut self, value: &str) {
        if swift_text::str_eq(&self.query, value) {
            return;
        }
        self.query = value.to_owned();
        self.selected_index = 0;
        self.cache.borrow_mut().invalidate();
    }

    pub fn move_selection(&mut self, offset: isize) {
        let count = self.quick_results().len() as isize;
        if count <= 0 {
            self.selected_index = 0;
            return;
        }
        self.selected_index = modulo(self.selected_index + offset, count);
    }

    pub fn select(&mut self, index: isize) {
        if index < 0 || index >= self.quick_results().len() as isize {
            return;
        }
        self.selected_index = index;
    }

    pub fn record(&mut self, command: Command) {
        self.recent_commands.retain(|recent| *recent != command);
        self.recent_commands.insert(0, command);
        self.cache.borrow_mut().invalidate();
    }

    fn unique(commands: &[Command]) -> Vec<Command> {
        let mut seen = HashSet::new();
        commands.iter().copied().filter(|command| seen.insert(*command)).collect()
    }
}

/// `Int.modulo(_:)`: a non-negative remainder.
fn modulo(value: isize, divisor: isize) -> isize {
    let remainder = value % divisor;
    if remainder >= 0 { remainder } else { remainder + divisor }
}

/// `CommandPaletteSynonyms.values[command] ?? []`.
pub fn synonyms(command: Command) -> &'static [&'static str] {
    match command {
        Command::SourceMode => &["markdown", "raw", "editor", "edit source", "full source"],
        Command::DocumentLens => &["contents", "outline", "headings", "document lens", "table of contents"],
        Command::TaskPanel => &["tasks", "todo", "checkbox", "checklist"],
        Command::ToggleTaskAtCaret => &["check", "tick", "checkbox", "done"],
        Command::FrontMatterEditor => &["metadata", "yaml", "toml", "properties"],
        Command::TableEditor => &["tables", "grid", "cells", "rows", "columns"],
        Command::AssetDoctor => &["images", "links", "missing files", "media"],
        Command::Find => &["search", "locate"],
        Command::FindReplace => &["search and replace", "substitute"],
        Command::TidyDocument => &["format", "clean", "lint", "fix"],
        Command::CopyAsMarkdown => &["copy source", "markdown"],
        Command::CopyAsRichText => &["copy formatted", "rich text"],
        Command::CopyAsPlainText => &["copy text", "plain"],
        Command::RevealInFinder => &["show file", "folder", "locate"],
        Command::VersionTimeline => &["history", "versions", "snapshots", "revert"],
        Command::Preferences => &["settings", "options", "configuration"],
        Command::ShowKeybindings => &["shortcuts", "keys", "keyboard"],
        Command::CheckForUpdates => &["update", "updates", "upgrade", "refresh"],
        _ => &[],
    }
}
