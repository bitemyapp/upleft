//! Compatibility/RenderTarget.swift — renderer capabilities and profiles.
//!
//! Swift makes `MarkdownCapability`, `MarkdownCapabilities`,
//! `BuiltInRenderTarget` and `RenderTargetProfile` `Codable`, but nothing in
//! MarkdownCore (or Downright's app targets) encodes them; only the tests
//! round-trip them through JSON. The encoding is not ported. For reference,
//! `MarkdownCapabilities` encodes as `{"capabilities": [rawValue, …]}` in
//! `ALL_CASES` order, and the rest use the synthesized keyed shape.

/// Markdown features whose spelling or semantics vary between renderers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MarkdownCapability {
    Tables,
    TaskLists,
    Strikethrough,
    Footnotes,
    Math,
    Mermaid,
    CalloutsAlerts,
    Wikilinks,
    FrontMatter,
    RawHTML,
    HeadingAttributes,
}

impl MarkdownCapability {
    pub const ALL_CASES: [MarkdownCapability; 11] = [
        MarkdownCapability::Tables,
        MarkdownCapability::TaskLists,
        MarkdownCapability::Strikethrough,
        MarkdownCapability::Footnotes,
        MarkdownCapability::Math,
        MarkdownCapability::Mermaid,
        MarkdownCapability::CalloutsAlerts,
        MarkdownCapability::Wikilinks,
        MarkdownCapability::FrontMatter,
        MarkdownCapability::RawHTML,
        MarkdownCapability::HeadingAttributes,
    ];

    pub fn raw_value(&self) -> &'static str {
        match self {
            MarkdownCapability::Tables => "tables",
            MarkdownCapability::TaskLists => "taskLists",
            MarkdownCapability::Strikethrough => "strikethrough",
            MarkdownCapability::Footnotes => "footnotes",
            MarkdownCapability::Math => "math",
            MarkdownCapability::Mermaid => "mermaid",
            MarkdownCapability::CalloutsAlerts => "calloutsAlerts",
            MarkdownCapability::Wikilinks => "wikilinks",
            MarkdownCapability::FrontMatter => "frontMatter",
            MarkdownCapability::RawHTML => "rawHTML",
            MarkdownCapability::HeadingAttributes => "headingAttributes",
        }
    }

    pub fn from_raw_value(raw: &str) -> Option<MarkdownCapability> {
        MarkdownCapability::ALL_CASES.into_iter().find(|capability| capability.raw_value() == raw)
    }
}

/// A stable set of renderer capabilities (a Swift `OptionSet` over `UInt64`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct MarkdownCapabilities(pub u64);

impl MarkdownCapabilities {
    pub const TABLES: MarkdownCapabilities = MarkdownCapabilities(1 << 0);
    pub const TASK_LISTS: MarkdownCapabilities = MarkdownCapabilities(1 << 1);
    pub const STRIKETHROUGH: MarkdownCapabilities = MarkdownCapabilities(1 << 2);
    pub const FOOTNOTES: MarkdownCapabilities = MarkdownCapabilities(1 << 3);
    pub const MATH: MarkdownCapabilities = MarkdownCapabilities(1 << 4);
    pub const MERMAID: MarkdownCapabilities = MarkdownCapabilities(1 << 5);
    pub const CALLOUTS_ALERTS: MarkdownCapabilities = MarkdownCapabilities(1 << 6);
    pub const WIKILINKS: MarkdownCapabilities = MarkdownCapabilities(1 << 7);
    pub const FRONT_MATTER: MarkdownCapabilities = MarkdownCapabilities(1 << 8);
    pub const RAW_HTML: MarkdownCapabilities = MarkdownCapabilities(1 << 9);
    pub const HEADING_ATTRIBUTES: MarkdownCapabilities = MarkdownCapabilities(1 << 10);

    /// The empty set (`Self()` / `[]`).
    pub const EMPTY: MarkdownCapabilities = MarkdownCapabilities(0);

    pub const ALL: MarkdownCapabilities = MarkdownCapabilities(
        Self::TABLES.0
            | Self::TASK_LISTS.0
            | Self::STRIKETHROUGH.0
            | Self::FOOTNOTES.0
            | Self::MATH.0
            | Self::MERMAID.0
            | Self::CALLOUTS_ALERTS.0
            | Self::WIKILINKS.0
            | Self::FRONT_MATTER.0
            | Self::RAW_HTML.0
            | Self::HEADING_ATTRIBUTES.0,
    );

    /// `init(rawValue:)`.
    pub const fn from_raw_value(raw_value: u64) -> MarkdownCapabilities {
        MarkdownCapabilities(raw_value)
    }

    pub const fn raw_value(&self) -> u64 {
        self.0
    }

    /// `init(_ capability:)`.
    pub const fn from_capability(capability: MarkdownCapability) -> MarkdownCapabilities {
        match capability {
            MarkdownCapability::Tables => Self::TABLES,
            MarkdownCapability::TaskLists => Self::TASK_LISTS,
            MarkdownCapability::Strikethrough => Self::STRIKETHROUGH,
            MarkdownCapability::Footnotes => Self::FOOTNOTES,
            MarkdownCapability::Math => Self::MATH,
            MarkdownCapability::Mermaid => Self::MERMAID,
            MarkdownCapability::CalloutsAlerts => Self::CALLOUTS_ALERTS,
            MarkdownCapability::Wikilinks => Self::WIKILINKS,
            MarkdownCapability::FrontMatter => Self::FRONT_MATTER,
            MarkdownCapability::RawHTML => Self::RAW_HTML,
            MarkdownCapability::HeadingAttributes => Self::HEADING_ATTRIBUTES,
        }
    }

    /// `OptionSet.contains(_:)`: every member of `other` is in `self`.
    pub const fn contains(&self, other: MarkdownCapabilities) -> bool {
        self.0 & other.0 == other.0
    }

    /// `contains(_ capability: MarkdownCapability)`.
    pub const fn contains_capability(&self, capability: MarkdownCapability) -> bool {
        self.contains(Self::from_capability(capability))
    }

    pub const fn is_empty(&self) -> bool {
        self.0 == 0
    }

    pub const fn union(&self, other: MarkdownCapabilities) -> MarkdownCapabilities {
        MarkdownCapabilities(self.0 | other.0)
    }

    pub const fn intersection(&self, other: MarkdownCapabilities) -> MarkdownCapabilities {
        MarkdownCapabilities(self.0 & other.0)
    }

    pub const fn subtracting(&self, other: MarkdownCapabilities) -> MarkdownCapabilities {
        MarkdownCapabilities(self.0 & !other.0)
    }

    pub const fn symmetric_difference(&self, other: MarkdownCapabilities) -> MarkdownCapabilities {
        MarkdownCapabilities(self.0 ^ other.0)
    }

    pub fn insert(&mut self, other: MarkdownCapabilities) {
        self.0 |= other.0;
    }

    pub fn remove(&mut self, other: MarkdownCapabilities) {
        self.0 &= !other.0;
    }

    /// The members, in `MarkdownCapability.allCases` order.
    pub fn capabilities(&self) -> Vec<MarkdownCapability> {
        MarkdownCapability::ALL_CASES.into_iter().filter(|&capability| self.contains_capability(capability)).collect()
    }
}

impl From<MarkdownCapability> for MarkdownCapabilities {
    fn from(capability: MarkdownCapability) -> Self {
        MarkdownCapabilities::from_capability(capability)
    }
}

impl std::ops::BitOr for MarkdownCapabilities {
    type Output = MarkdownCapabilities;
    fn bitor(self, rhs: MarkdownCapabilities) -> MarkdownCapabilities {
        self.union(rhs)
    }
}

impl std::ops::BitOrAssign for MarkdownCapabilities {
    fn bitor_assign(&mut self, rhs: MarkdownCapabilities) {
        self.insert(rhs);
    }
}

impl FromIterator<MarkdownCapabilities> for MarkdownCapabilities {
    fn from_iter<I: IntoIterator<Item = MarkdownCapabilities>>(iter: I) -> Self {
        iter.into_iter().fold(MarkdownCapabilities::EMPTY, |set, member| set.union(member))
    }
}

/// The named renderer families shipped by Downright.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BuiltInRenderTarget {
    Downright,
    CommonMark,
    GitHub,
    Obsidian,
    Pandoc,
    MultiMarkdown,
    Jekyll,
    Hugo,
    Quarto,
}

impl BuiltInRenderTarget {
    pub const ALL_CASES: [BuiltInRenderTarget; 9] = [
        BuiltInRenderTarget::Downright,
        BuiltInRenderTarget::CommonMark,
        BuiltInRenderTarget::GitHub,
        BuiltInRenderTarget::Obsidian,
        BuiltInRenderTarget::Pandoc,
        BuiltInRenderTarget::MultiMarkdown,
        BuiltInRenderTarget::Jekyll,
        BuiltInRenderTarget::Hugo,
        BuiltInRenderTarget::Quarto,
    ];

    pub fn raw_value(&self) -> &'static str {
        match self {
            BuiltInRenderTarget::Downright => "downright",
            BuiltInRenderTarget::CommonMark => "commonMark",
            BuiltInRenderTarget::GitHub => "gitHub",
            BuiltInRenderTarget::Obsidian => "obsidian",
            BuiltInRenderTarget::Pandoc => "pandoc",
            BuiltInRenderTarget::MultiMarkdown => "multiMarkdown",
            BuiltInRenderTarget::Jekyll => "jekyll",
            BuiltInRenderTarget::Hugo => "hugo",
            BuiltInRenderTarget::Quarto => "quarto",
        }
    }

    pub fn from_raw_value(raw: &str) -> Option<BuiltInRenderTarget> {
        BuiltInRenderTarget::ALL_CASES.into_iter().find(|target| target.raw_value() == raw)
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            BuiltInRenderTarget::Downright => "Upleft",
            BuiltInRenderTarget::CommonMark => "CommonMark",
            BuiltInRenderTarget::GitHub => "GitHub",
            BuiltInRenderTarget::Obsidian => "Obsidian",
            BuiltInRenderTarget::Pandoc => "Pandoc",
            BuiltInRenderTarget::MultiMarkdown => "MultiMarkdown",
            BuiltInRenderTarget::Jekyll => "Jekyll",
            BuiltInRenderTarget::Hugo => "Hugo",
            BuiltInRenderTarget::Quarto => "Quarto",
        }
    }

    pub fn capabilities(&self) -> MarkdownCapabilities {
        type C = MarkdownCapabilities;
        match self {
            BuiltInRenderTarget::Downright => C::ALL,
            BuiltInRenderTarget::CommonMark => C::RAW_HTML,
            BuiltInRenderTarget::GitHub => {
                C::TABLES | C::TASK_LISTS | C::STRIKETHROUGH | C::FOOTNOTES | C::MATH | C::MERMAID | C::CALLOUTS_ALERTS | C::RAW_HTML
            }
            BuiltInRenderTarget::Obsidian => {
                C::TABLES
                    | C::TASK_LISTS
                    | C::STRIKETHROUGH
                    | C::FOOTNOTES
                    | C::MATH
                    | C::MERMAID
                    | C::CALLOUTS_ALERTS
                    | C::WIKILINKS
                    | C::FRONT_MATTER
                    | C::RAW_HTML
            }
            BuiltInRenderTarget::Pandoc => {
                C::TABLES
                    | C::TASK_LISTS
                    | C::STRIKETHROUGH
                    | C::FOOTNOTES
                    | C::MATH
                    | C::FRONT_MATTER
                    | C::RAW_HTML
                    | C::HEADING_ATTRIBUTES
            }
            BuiltInRenderTarget::MultiMarkdown => {
                C::TABLES | C::TASK_LISTS | C::STRIKETHROUGH | C::FOOTNOTES | C::MATH | C::FRONT_MATTER | C::RAW_HTML
            }
            BuiltInRenderTarget::Jekyll => {
                C::TABLES | C::TASK_LISTS | C::STRIKETHROUGH | C::FOOTNOTES | C::FRONT_MATTER | C::RAW_HTML | C::HEADING_ATTRIBUTES
            }
            BuiltInRenderTarget::Hugo => C::TABLES | C::TASK_LISTS | C::STRIKETHROUGH | C::FOOTNOTES | C::FRONT_MATTER | C::RAW_HTML,
            BuiltInRenderTarget::Quarto => {
                C::TABLES
                    | C::TASK_LISTS
                    | C::STRIKETHROUGH
                    | C::FOOTNOTES
                    | C::MATH
                    | C::MERMAID
                    | C::CALLOUTS_ALERTS
                    | C::FRONT_MATTER
                    | C::RAW_HTML
                    | C::HEADING_ATTRIBUTES
            }
        }
    }

    pub fn profile(&self) -> RenderTargetProfile {
        RenderTargetProfile::from_built_in(*self, Some(self.capabilities()))
    }
}

/// A renderer configuration. Custom profiles are data-only so they can be
/// saved and loaded without coupling compatibility to the app.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RenderTargetProfile {
    pub id: String,
    pub name: String,
    pub capabilities: MarkdownCapabilities,
    pub built_in: Option<BuiltInRenderTarget>,
}

impl RenderTargetProfile {
    /// `init(name:capabilities:)`: a custom profile.
    pub fn new(name: impl Into<String>, capabilities: MarkdownCapabilities) -> RenderTargetProfile {
        let name = name.into();
        RenderTargetProfile { id: format!("custom:{name}"), name, capabilities, built_in: None }
    }

    /// `init(builtIn:capabilities:)`.
    pub fn from_built_in(built_in: BuiltInRenderTarget, capabilities: Option<MarkdownCapabilities>) -> RenderTargetProfile {
        RenderTargetProfile {
            id: built_in.raw_value().to_owned(),
            name: built_in.display_name().to_owned(),
            capabilities: capabilities.unwrap_or_else(|| built_in.capabilities()),
            built_in: Some(built_in),
        }
    }

    pub fn downright() -> RenderTargetProfile {
        BuiltInRenderTarget::Downright.profile()
    }

    pub fn common_mark() -> RenderTargetProfile {
        BuiltInRenderTarget::CommonMark.profile()
    }

    pub fn git_hub() -> RenderTargetProfile {
        BuiltInRenderTarget::GitHub.profile()
    }

    pub fn obsidian() -> RenderTargetProfile {
        BuiltInRenderTarget::Obsidian.profile()
    }

    pub fn pandoc() -> RenderTargetProfile {
        BuiltInRenderTarget::Pandoc.profile()
    }

    pub fn multi_markdown() -> RenderTargetProfile {
        BuiltInRenderTarget::MultiMarkdown.profile()
    }

    pub fn jekyll() -> RenderTargetProfile {
        BuiltInRenderTarget::Jekyll.profile()
    }

    pub fn hugo() -> RenderTargetProfile {
        BuiltInRenderTarget::Hugo.profile()
    }

    pub fn quarto() -> RenderTargetProfile {
        BuiltInRenderTarget::Quarto.profile()
    }

    pub fn custom(name: impl Into<String>, capabilities: MarkdownCapabilities) -> RenderTargetProfile {
        RenderTargetProfile::new(name, capabilities)
    }

    pub fn built_ins() -> Vec<RenderTargetProfile> {
        BuiltInRenderTarget::ALL_CASES.iter().map(BuiltInRenderTarget::profile).collect()
    }
}
