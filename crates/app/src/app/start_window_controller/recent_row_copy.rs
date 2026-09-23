//! `RecentRowCopy` (the `// MARK: - Recent row` enum of
//! `App/StartWindowController.swift`): what a recent row says — its title,
//! its second line and its timestamp.

use objc2::rc::Retained;
use objc2_foundation::{
    NSCalendar, NSCalendarUnit, NSDate, NSDateFormatter, NSLocale, NSNotFound, NSRange, NSString,
    NSStringCompareOptions,
};
use upleft_foundation::url::FileUrl;
use upleft_render::appkit_compat::ns_string;
use upleft_swift_text as swift_text;

use super::START_RECENT_DISPLAY_LIMIT;
use crate::ai::document_state_store::RecentDocument;

/// `RecentRowCopy`.
pub struct RecentRowCopy;

thread_local! {
    /// `monthDayFormatter`.
    static MONTH_DAY_FORMATTER: Retained<NSDateFormatter> = date_formatter("MMM d");
    /// `monthDayYearFormatter`.
    static MONTH_DAY_YEAR_FORMATTER: Retained<NSDateFormatter> = date_formatter("MMM d yyyy");
}

/// `DateFormatter()`, `locale = .current`,
/// `setLocalizedDateFormatFromTemplate(template)`.
fn date_formatter(template: &str) -> Retained<NSDateFormatter> {
    let formatter = NSDateFormatter::new();
    formatter.setLocale(Some(&NSLocale::currentLocale()));
    formatter.setLocalizedDateFormatFromTemplate(&NSString::from_str(template));
    formatter
}

/// `genericHeadings`.
const GENERIC_HEADINGS: [&str; 5] = ["title", "untitled", "heading", "document", "readme"];

fn ns_date(date: upleft_foundation::date::Date) -> Retained<NSDate> {
    NSDate::dateWithTimeIntervalSinceReferenceDate(date.time_interval_since_reference_date)
}

/// `string.range(of: pattern, options: .regularExpression)`: Foundation's
/// regular-expression search over the bridged `NSString`, with the match in
/// UTF-16 offsets.
fn regular_expression_range(text: &str, pattern: &str) -> Option<(Retained<NSString>, NSRange)> {
    let string = ns_string(text);
    let range =
        string.rangeOfString_options(&NSString::from_str(pattern), NSStringCompareOptions::RegularExpressionSearch);
    (range.location != NSNotFound as usize).then_some((string, range))
}

/// `Dictionary(grouping: titles, by: { $0 }).mapValues(\.count)[title]`:
/// how many titles equal `title` (Swift `String` equality, canonical
/// equivalence).
fn occurrences(titles: &[String], title: &str) -> usize {
    titles.iter().filter(|other| swift_text::str_eq(other, title)).count()
}

/// `URL(fileURLWithPath: path).deletingLastPathComponent().lastPathComponent`.
fn parent_folder_name(path: &str) -> String {
    FileUrl::from_path(path).deleting_last_path_component().last_path_component()
}

impl RecentRowCopy {
    /// `preferredTitle(for:)`.
    pub fn preferred_title(recent: &RecentDocument) -> String {
        let heading = swift_text::trim_whitespaces_and_newlines(&recent.first_heading);
        let lowered = swift_text::lowercased(heading);
        let heading_is_useful =
            !heading.is_empty() && !GENERIC_HEADINGS.iter().any(|generic| swift_text::str_eq(generic, &lowered));
        if Self::looks_machine_generated(&recent.display_name) {
            if heading_is_useful {
                return heading.to_owned();
            }
            return Self::strip_uuid_suffix(&recent.display_name);
        }
        recent.display_name.clone()
    }

    /// `disambiguatedTitles(for:)`.
    pub fn disambiguated_titles(recents: &[RecentDocument]) -> Vec<String> {
        let limited = &recents[..recents.len().min(START_RECENT_DISPLAY_LIMIT)];
        let mut titles: Vec<String> = limited.iter().map(Self::preferred_title).collect();

        // Pass 1: same title → append parent folder when folders differ.
        let counts: Vec<usize> = titles.iter().map(|title| occurrences(&titles, title)).collect();
        titles = limited
            .iter()
            .zip(titles.iter())
            .zip(counts)
            .map(|((recent, title), count)| {
                if count <= 1 {
                    return title.clone();
                }
                let folder = parent_folder_name(&recent.path);
                if folder.is_empty() || folder == "/" {
                    return title.clone();
                }
                format!("{title} ({folder})")
            })
            .collect();

        // Pass 2: still colliding (same folder) → short unique id from the
        // file name.
        let counts: Vec<usize> = titles.iter().map(|title| occurrences(&titles, title)).collect();
        titles = limited
            .iter()
            .zip(titles.iter())
            .zip(counts)
            .map(|((recent, title), count)| {
                if count <= 1 {
                    return title.clone();
                }
                let base = Self::preferred_title(recent);
                let id = Self::unique_fragment(&recent.display_name);
                format!("{base} · {id}")
            })
            .collect();
        titles
    }

    /// `timestamp(for:)`.
    pub fn timestamp(recent: &RecentDocument) -> String {
        let calendar = NSCalendar::currentCalendar();
        let last_opened = ns_date(recent.last_opened);
        if calendar.isDateInToday(&last_opened) {
            return "Today".to_owned();
        }
        if calendar.isDateInYesterday(&last_opened) {
            return "Yesterday".to_owned();
        }
        let now = NSDate::new();
        let year = calendar.component_fromDate(NSCalendarUnit::Year, &last_opened);
        let current_year = calendar.component_fromDate(NSCalendarUnit::Year, &now);
        let formatter = if year == current_year { &MONTH_DAY_FORMATTER } else { &MONTH_DAY_YEAR_FORMATTER };
        formatter.with(|formatter| swift_text::ns::foundation::to_string(&formatter.stringFromDate(&last_opened)))
    }

    /// `folder(for:)`.
    pub fn folder(recent: &RecentDocument) -> String {
        let name = parent_folder_name(&recent.path);
        if name == "/" { String::new() } else { name }
    }

    /// `subtitle(for:title:)`: the row's second line, what the document is
    /// *about*.
    ///
    /// The first heading is the point — twelve files called `plan.md` are
    /// twelve identical rows without it, and that is the exact problem this
    /// app exists to solve. It is skipped only when it would repeat the
    /// title back, in which case the folder is the more informative thing to
    /// say.
    pub fn subtitle(recent: &RecentDocument, title: &str) -> String {
        let heading = Self::cleaned_heading(&recent.first_heading);
        let place = Self::folder(recent);
        if heading.is_empty() || Self::echoes(&heading, title) {
            return place;
        }
        // `README` in `Downright/` under the heading "Upleft" would otherwise
        // render as "Upleft · Upleft"; saying it once is the whole point.
        if place.is_empty() || Self::echoes(&heading, &place) {
            return heading;
        }
        // Both, when the folder is what disambiguates two same-named
        // documents and the heading is what explains them.
        format!("{heading}  ·  {place}")
    }

    /// `cleanedHeading(_:)`: a malformed editor write once persisted a
    /// partial prose prefix directly against a capitalized heading
    /// (`ThiDownright …`). Keep that stale metadata from leaking into the
    /// welcome surface while leaving ordinary headings, including `This …`,
    /// untouched.
    fn cleaned_heading(raw_heading: &str) -> String {
        let heading = swift_text::trim_whitespaces_and_newlines(raw_heading);
        let rest = swift_text::drop_first(heading, 3);
        if !(swift_text::has_prefix(heading, "Thi") && swift_text::first(rest).is_some_and(swift_text::is_uppercase)) {
            return heading.to_owned();
        }
        rest.to_owned()
    }

    /// `echoes(_:of:)`: whether the heading would just restate the title.
    /// Compared without case or punctuation so "Release Plan" and
    /// "release-plan" count as the same thing being said twice.
    pub fn echoes(heading: &str, title: &str) -> bool {
        fn fold(text: &str) -> String {
            swift_text::filter(&swift_text::lowercased(text), |character| {
                swift_text::is_letter(character) || swift_text::is_number(character)
            })
        }
        let left = fold(heading);
        let right = fold(title);
        if left.is_empty() || right.is_empty() {
            return true;
        }
        swift_text::str_eq(&left, &right) || swift_text::has_prefix(&left, &right) || swift_text::has_prefix(&right, &left)
    }

    /// `looksMachineGenerated(_:)`.
    pub fn looks_machine_generated(name: &str) -> bool {
        if swift_text::count(name) > 36 {
            return true;
        }
        regular_expression_range(name, r"[0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-").is_some()
    }

    /// `stripUUIDSuffix(_:)`: `EditingKeyRepro-92C5F190-…` → `EditingKeyRepro`.
    pub fn strip_uuid_suffix(name: &str) -> String {
        let Some((string, range)) = regular_expression_range(name, r"-[0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-") else {
            return name.to_owned();
        };
        // `String(name[..<range.lowerBound])`.
        let trimmed = swift_text::ns::foundation::to_string(&string.substringToIndex(range.location));
        if trimmed.is_empty() { name.to_owned() } else { trimmed }
    }

    /// `uniqueFragment(from:)`: prefer the first UUID octet from generated
    /// names; otherwise a short tail.
    pub fn unique_fragment(display_name: &str) -> String {
        if let Some((string, range)) = regular_expression_range(display_name, r"[0-9A-Fa-f]{8}") {
            let matched = swift_text::ns::foundation::to_string(&string.substringWithRange(range));
            return swift_text::uppercased(&matched);
        }
        let tail = swift_text::suffix(display_name, 6);
        if tail.is_empty() { display_name.to_owned() } else { tail.to_owned() }
    }
}
