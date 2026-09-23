//! Port of `Mermaid/src_xychart_parser.swift` (from `original/src/xychart/parser.ts`).

use super::src_xychart_types::*;
use crate::swift;

/// The text between the first `"` and the last `"`, when they differ.
fn quoted(text: &str) -> Option<&str> {
    let q_start = swift::first_index_of(text, '"')?;
    let q_end = swift::last_index_of(text, '"')?;
    if q_start == q_end {
        return None;
    }
    // `text[text.index(after: qStart)..<qEnd]` (traps if reversed, which a
    // first/last pair never is).
    Some(&text[swift::index_after(text, q_start)..q_end])
}

/// `parseXYChart(_:)`.
pub fn parse_xy_chart(lines: &[&str]) -> XYChart {
    let mut x_axis = XYAxis::default();
    let mut y_axis = XYAxis::default();
    let mut series: Vec<XYChartSeries> = Vec::new();
    let mut title: Option<String> = None;
    let mut horizontal = false;

    for &line in lines {
        // Header line — detect horizontal
        if swift::regex_test(r"^xychart(-beta)?\b", line, true) {
            if swift::regex_test(r"\bhorizontal\b", line, true) {
                horizontal = true;
            }
            continue;
        }

        // Title
        if let Some(title_str) = swift::regex_find(r#"^title\s+"([^"]+)""#, line, false) {
            if let Some(t) = quoted(title_str) {
                title = Some(t.to_owned());
            }
            continue;
        }

        // x-axis with categories: x-axis "Title" [a, b, c] or x-axis [a, b, c]
        if let Some((t, categories)) = match_x_axis_categories(line) {
            if let Some(t) = t {
                x_axis.title = Some(t);
            }
            x_axis.categories = Some(categories);
            continue;
        }

        // x-axis with range: x-axis "Title" min --> max or x-axis min --> max
        if let Some((t, min, max)) = match_axis_range(line, "x-axis") {
            if let Some(t) = t {
                x_axis.title = Some(t);
            }
            x_axis.range = Some((min, max));
            continue;
        }

        // y-axis with range
        if let Some((t, min, max)) = match_axis_range(line, "y-axis") {
            if let Some(t) = t {
                y_axis.title = Some(t);
            }
            y_axis.range = Some((min, max));
            continue;
        }

        // y-axis with just title
        if let Some(sub) = swift::regex_find(r#"^y-axis\s+"([^"]+)"\s*$"#, line, false) {
            if let Some(t) = quoted(sub) {
                y_axis.title = Some(t.to_owned());
            }
            continue;
        }

        // bar [...]
        if let Some(sub) = swift::regex_find(r"^bar\s+\[([^\]]+)\]", line, false) {
            if let Some(nums) = bracketed_numbers(sub) {
                series.push(XYChartSeries { r#type: XYSeriesType::Bar, data: nums });
            }
            continue;
        }

        // line [...]
        if let Some(sub) = swift::regex_find(r"^line\s+\[([^\]]+)\]", line, false) {
            if let Some(nums) = bracketed_numbers(sub) {
                series.push(XYChartSeries { r#type: XYSeriesType::Line, data: nums });
            }
            continue;
        }
    }

    // Auto-derive y-axis range from data if not specified
    if y_axis.range.is_none() && !series.is_empty() {
        let all_values: Vec<f64> = series.iter().flat_map(|s| s.data.iter().copied()).collect();
        let mut min_val = swift::seq_min(all_values.iter().copied()).unwrap_or(0.0);
        let mut max_val = swift::seq_max(all_values.iter().copied()).unwrap_or(0.0);
        let span = if max_val - min_val == 0.0 { 1.0 } else { max_val - min_val };
        min_val -= span * 0.1;
        max_val += span * 0.1;
        if min_val > 0.0 && min_val < span * 0.5 {
            min_val = 0.0;
        }
        y_axis.range = Some((min_val, max_val));
    }

    // Fallback y-axis range
    if y_axis.range.is_none() {
        y_axis.range = Some((0.0, 100.0));
    }

    XYChart { title, horizontal, x_axis, y_axis, series }
}

fn bracketed_numbers(sub: &str) -> Option<Vec<f64>> {
    let bracket_start = swift::first_index_of(sub, '[')?;
    let bracket_end = swift::first_index_of(sub, ']')?;
    let start = swift::index_after(sub, bracket_start);
    assert!(start <= bracket_end, "Range requires lowerBound <= upperBound");
    Some(parse_numeric_array(&sub[start..bracket_end]))
}

fn parse_numeric_array(text: &str) -> Vec<f64> {
    swift::split_character(text, ',')
        .into_iter()
        .filter_map(|p| swift::parse_double(swift::trim_whitespaces(p)))
        .collect()
}

fn match_x_axis_categories(line: &str) -> Option<(Option<String>, Vec<String>)> {
    // x-axis "Title" [a, b, c] or x-axis [a, b, c]
    if !swift::has_prefix(line, "x-axis") {
        return None;
    }
    let bracket_start = swift::first_index_of(line, '[')?;
    let bracket_end = swift::first_index_of(line, ']')?;

    // `line[line.index(line.startIndex, offsetBy: 6)..<bracketStart]`: the
    // six characters of "x-axis" are six bytes.
    assert!(6 <= bracket_start, "Range requires lowerBound <= upperBound");
    let before_bracket = swift::trim_whitespaces(&line[6..bracket_start]);
    let title = quoted(before_bracket).map(str::to_owned);

    let start = swift::index_after(line, bracket_start);
    assert!(start <= bracket_end, "Range requires lowerBound <= upperBound");
    let cat_str = &line[start..bracket_end];
    let categories = swift::split_character(cat_str, ',')
        .into_iter()
        .map(|c| swift::trim_whitespaces(c).to_owned())
        .collect();

    Some((title, categories))
}

fn match_axis_range(line: &str, prefix: &str) -> Option<(Option<String>, f64, f64)> {
    if !swift::has_prefix(line, prefix) {
        return None;
    }
    if !swift::contains(line, "-->") {
        return None;
    }

    let after_prefix = swift::trim_whitespaces(swift::drop_first(line, swift::character_count(prefix)));

    let mut title: Option<String> = None;
    let mut num_part = after_prefix;

    if let (Some(q_start), Some(q_end)) = (swift::first_index_of(after_prefix, '"'), swift::last_index_of(after_prefix, '"')) {
        if q_start != q_end {
            title = Some(after_prefix[swift::index_after(after_prefix, q_start)..q_end].to_owned());
            num_part = swift::trim_whitespaces(&after_prefix[swift::index_after(after_prefix, q_end)..]);
        }
    }

    let parts: Vec<&str> = num_part.split("-->").collect();
    if parts.len() != 2 {
        return None;
    }
    let min_val = swift::parse_double(swift::trim_whitespaces(parts[0]))?;
    let max_val = swift::parse_double(swift::trim_whitespaces(parts[1]))?;

    Some((title, min_val, max_val))
}
