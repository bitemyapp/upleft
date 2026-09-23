//! Swift standard-library behaviour that elk-swift relies on implicitly.
//!
//! Not a port of an elk-swift file: these are the Swift 6.4 stdlib algorithms
//! (from the SDK's `Swift.swiftinterface`, where they are `@inlinable`) that the
//! port must reproduce exactly, because ELK's comparators are not always
//! consistent and the result of a sort then depends on the algorithm.

/// `MutableCollection.sort(by:)` — Swift's stable merge sort
/// (`_stableSortImpl`): insertion sort for short inputs, otherwise natural runs
/// extended to a minimum length by insertion sort and merged with the
/// run-stack invariants below. Comparator calls happen in exactly the order
/// Swift makes them.
pub fn sort_by<T: Clone, F: FnMut(&T, &T) -> bool>(v: &mut [T], mut less: F) {
    let count = v.len();
    let minimum_run_length = minimum_merge_run_length(count);
    if count <= minimum_run_length {
        if count > 0 {
            insertion_sort(v, 0, count, 1, &mut less);
        }
        return;
    }
    let mut buffer: Vec<T> = Vec::with_capacity(count / 2 + 1);
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut start = 0;
    while start < count {
        let (mut end, descending) = find_next_run(v, start, &mut less);
        if descending {
            v[start..end].reverse();
        }
        if end < count && end - start < minimum_run_length {
            let new_end = count.min(start + minimum_run_length);
            insertion_sort(v, start, new_end, end, &mut less);
            end = new_end;
        }
        runs.push((start, end));
        merge_top_runs(v, &mut runs, &mut buffer, &mut less);
        start = end;
    }
    while runs.len() > 1 {
        let i = runs.len() - 1;
        merge_runs(v, &mut runs, i, &mut buffer, &mut less);
    }
}

/// `Sequence.sorted(by:)`.
pub fn sorted_by<T: Clone, F: FnMut(&T, &T) -> bool>(items: impl IntoIterator<Item = T>, less: F) -> Vec<T> {
    let mut v: Vec<T> = items.into_iter().collect();
    sort_by(&mut v, less);
    v
}

/// `sort()` on `Comparable` elements: `sort(by: <)`.
pub fn sort<T: Clone + PartialOrd>(v: &mut [T]) {
    sort_by(v, |a, b| a < b);
}

/// `sorted()` on `Comparable` elements.
pub fn sorted<T: Clone + PartialOrd>(items: impl IntoIterator<Item = T>) -> Vec<T> {
    sorted_by(items, |a, b| a < b)
}

fn minimum_merge_run_length(c: usize) -> usize {
    let bits_to_use = 6;
    if c < 1 << bits_to_use {
        return c;
    }
    let c = c as i64;
    let offset = (64 - bits_to_use) - c.leading_zeros() as i64;
    let mask = (1i64 << offset) - 1;
    ((c >> offset) + if c & mask == 0 { 0 } else { 1 }) as usize
}

fn insertion_sort<T, F: FnMut(&T, &T) -> bool>(v: &mut [T], lower: usize, upper: usize, sorted_end: usize, less: &mut F) {
    let mut sorted_end = sorted_end;
    while sorted_end != upper {
        let mut i = sorted_end;
        loop {
            let j = i - 1;
            if !less(&v[i], &v[j]) {
                break;
            }
            v.swap(i, j);
            i = j;
            if i == lower {
                break;
            }
        }
        sorted_end += 1;
    }
}

fn find_next_run<T, F: FnMut(&T, &T) -> bool>(v: &[T], start: usize, less: &mut F) -> (usize, bool) {
    let mut previous = start;
    let mut current = start + 1;
    if current >= v.len() {
        return (current, false);
    }
    let is_descending = less(&v[current], &v[previous]);
    loop {
        previous = current;
        current += 1;
        if !(current < v.len() && is_descending == less(&v[current], &v[previous])) {
            break;
        }
    }
    (current, is_descending)
}

fn merge<T: Clone, F: FnMut(&T, &T) -> bool>(v: &mut [T], low: usize, mid: usize, high: usize, buffer: &mut Vec<T>, less: &mut F) {
    let low_count = mid - low;
    let high_count = high - mid;
    buffer.clear();
    if low_count < high_count {
        buffer.extend_from_slice(&v[low..mid]);
        let mut buffer_low = 0;
        let buffer_high = low_count;
        let mut src_low = mid;
        let mut dest_low = low;
        while buffer_low < buffer_high && src_low < high {
            if less(&v[src_low], &buffer[buffer_low]) {
                v[dest_low] = v[src_low].clone();
                src_low += 1;
            } else {
                v[dest_low] = buffer[buffer_low].clone();
                buffer_low += 1;
            }
            dest_low += 1;
        }
        for k in buffer_low..buffer_high {
            v[dest_low] = buffer[k].clone();
            dest_low += 1;
        }
    } else {
        buffer.extend_from_slice(&v[mid..high]);
        let buffer_low = 0;
        let mut buffer_high = high_count;
        let mut dest_high = high;
        let mut src_high = mid;
        let mut dest_low = mid;
        while buffer_high > buffer_low && src_high > low {
            dest_high -= 1;
            if less(&buffer[buffer_high - 1], &v[src_high - 1]) {
                src_high -= 1;
                v[dest_high] = v[src_high].clone();
                dest_low -= 1;
            } else {
                buffer_high -= 1;
                v[dest_high] = buffer[buffer_high].clone();
            }
        }
        for k in buffer_low..buffer_high {
            v[dest_low] = buffer[k].clone();
            dest_low += 1;
        }
    }
}

fn merge_runs<T: Clone, F: FnMut(&T, &T) -> bool>(v: &mut [T], runs: &mut Vec<(usize, usize)>, i: usize, buffer: &mut Vec<T>, less: &mut F) {
    let low = runs[i - 1].0;
    let middle = runs[i].0;
    let high = runs[i].1;
    merge(v, low, middle, high, buffer, less);
    runs[i - 1] = (low, high);
    runs.remove(i);
}

fn merge_top_runs<T: Clone, F: FnMut(&T, &T) -> bool>(v: &mut [T], runs: &mut Vec<(usize, usize)>, buffer: &mut Vec<T>, less: &mut F) {
    let count = |r: (usize, usize)| r.1 - r.0;
    while runs.len() > 1 {
        let mut last_index = runs.len() - 1;
        if last_index >= 3 && count(runs[last_index - 3]) <= count(runs[last_index - 2]) + count(runs[last_index - 1]) {
            if count(runs[last_index - 2]) < count(runs[last_index]) {
                last_index -= 1;
            }
        } else if last_index >= 2 && count(runs[last_index - 2]) <= count(runs[last_index - 1]) + count(runs[last_index]) {
            if count(runs[last_index - 2]) < count(runs[last_index]) {
                last_index -= 1;
            }
        } else if count(runs[last_index - 1]) <= count(runs[last_index]) {
            // merge Y and Z below
        } else {
            break;
        }
        merge_runs(v, runs, last_index, buffer, less);
    }
}

/// `Swift.min(x, y)`: `y < x ? y : x`. Differs from `f64::min` for NaN and
/// signed zeros, so every `min` on doubles goes through here.
#[inline]
pub fn min<T: PartialOrd>(x: T, y: T) -> T {
    if y < x { y } else { x }
}

/// `Swift.max(x, y)`: `y >= x ? y : x`.
#[inline]
pub fn max<T: PartialOrd>(x: T, y: T) -> T {
    if y >= x { y } else { x }
}

/// `Swift.min(x, y, z, rest...)`: `min(min(x, y), z)`, then any smaller rest.
pub fn min_of<T: PartialOrd + Copy>(values: &[T]) -> T {
    let mut min_value = if values.len() >= 3 { min(min(values[0], values[1]), values[2]) } else if values.len() == 2 { min(values[0], values[1]) } else { values[0] };
    for &value in values.iter().skip(3) {
        if value < min_value {
            min_value = value;
        }
    }
    min_value
}

/// `Swift.max(x, y, z, rest...)`: `max(max(x, y), z)`, then any `>=` rest.
pub fn max_of<T: PartialOrd + Copy>(values: &[T]) -> T {
    let mut max_value = if values.len() >= 3 { max(max(values[0], values[1]), values[2]) } else if values.len() == 2 { max(values[0], values[1]) } else { values[0] };
    for &value in values.iter().skip(3) {
        if value >= max_value {
            max_value = value;
        }
    }
    max_value
}

/// `Sequence.min()`: the first element no later element is `<`.
pub fn seq_min<T: PartialOrd + Copy>(items: impl IntoIterator<Item = T>) -> Option<T> {
    let mut iter = items.into_iter();
    let mut result = iter.next()?;
    for e in iter {
        if e < result {
            result = e;
        }
    }
    Some(result)
}

/// `Sequence.max()`: replaces the running result whenever `result < e`.
pub fn seq_max<T: PartialOrd + Copy>(items: impl IntoIterator<Item = T>) -> Option<T> {
    let mut iter = items.into_iter();
    let mut result = iter.next()?;
    for e in iter {
        if result < e {
            result = e;
        }
    }
    Some(result)
}

/// `Sequence.min(by:)`.
pub fn seq_min_by<T, F: FnMut(&T, &T) -> bool>(items: impl IntoIterator<Item = T>, mut less: F) -> Option<T> {
    let mut iter = items.into_iter();
    let mut result = iter.next()?;
    for e in iter {
        if less(&e, &result) {
            result = e;
        }
    }
    Some(result)
}

/// `Sequence.max(by:)`.
pub fn seq_max_by<T, F: FnMut(&T, &T) -> bool>(items: impl IntoIterator<Item = T>, mut less: F) -> Option<T> {
    let mut iter = items.into_iter();
    let mut result = iter.next()?;
    for e in iter {
        if less(&result, &e) {
            result = e;
        }
    }
    Some(result)
}

/// Swift's `Double.description` (`"\(value)"`): shortest round-trip digits,
/// `.0` on integral values, exponent form below 1e-4 or from 2^53 up.
pub fn describe_double(value: f64) -> String {
    if value.is_nan() {
        return "nan".into();
    }
    if value.is_infinite() {
        return if value < 0.0 { "-inf".into() } else { "inf".into() };
    }
    let magnitude = value.abs();
    if magnitude != 0.0 && (magnitude < 1e-4 || magnitude >= 9007199254740992.0) {
        let s = format!("{:e}", value);
        let (mantissa, exponent) = s.split_once('e').unwrap();
        let exponent: i32 = exponent.parse().unwrap();
        let sign = if exponent < 0 { '-' } else { '+' };
        return format!("{mantissa}e{sign}{:02}", exponent.abs());
    }
    let s = format!("{:?}", value);
    s
}

/// `Double(_: String)` (`LosslessStringConvertible`): the whole string must be
/// a decimal or hexadecimal floating-point literal, `inf`/`infinity`, or `nan`,
/// with an optional sign and no surrounding whitespace.
pub fn parse_double(s: &str) -> Option<f64> {
    if s.is_empty() || s.starts_with(char::is_whitespace) || s.ends_with(char::is_whitespace) {
        return None;
    }
    let (negative, body) = match s.as_bytes()[0] {
        b'-' => (true, &s[1..]),
        b'+' => (false, &s[1..]),
        _ => (false, s),
    };
    let lower = body.to_ascii_lowercase();
    let value = if lower == "inf" || lower == "infinity" {
        f64::INFINITY
    } else if lower == "nan" {
        f64::NAN
    } else if lower.starts_with("0x") {
        parse_hex_float(&lower[2..])?
    } else {
        if body.is_empty() || !body.bytes().all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-')) {
            return None;
        }
        body.parse::<f64>().ok()?
    };
    Some(if negative { -value } else { value })
}

fn parse_hex_float(s: &str) -> Option<f64> {
    let (mantissa, exponent) = match s.find('p') {
        Some(i) => (&s[..i], s[i + 1..].parse::<i32>().ok()?),
        None => (s, 0),
    };
    let (int_part, frac_part) = match mantissa.find('.') {
        Some(i) => (&mantissa[..i], &mantissa[i + 1..]),
        None => (mantissa, ""),
    };
    if int_part.is_empty() && frac_part.is_empty() {
        return None;
    }
    let mut value = 0f64;
    for c in int_part.chars() {
        value = value * 16.0 + c.to_digit(16)? as f64;
    }
    let mut scale = 1.0 / 16.0;
    for c in frac_part.chars() {
        value += c.to_digit(16)? as f64 * scale;
        scale /= 16.0;
    }
    Some(value * 2f64.powi(exponent))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sort_is_stable_and_sorts() {
        for n in [0usize, 1, 2, 5, 63, 64, 65, 100, 1000, 5000] {
            let mut v: Vec<(u32, usize)> = (0..n).map(|i| (((i * 7919) % 31) as u32, i)).collect();
            sort_by(&mut v, |a, b| a.0 < b.0);
            for w in v.windows(2) {
                assert!(w[0].0 < w[1].0 || (w[0].0 == w[1].0 && w[0].1 < w[1].1));
            }
        }
    }

    #[test]
    fn minimum_run_length_matches_swift() {
        assert_eq!(minimum_merge_run_length(10), 10);
        assert_eq!(minimum_merge_run_length(64), 32);
        assert_eq!(minimum_merge_run_length(65), 33);
        assert_eq!(minimum_merge_run_length(1000), 63);
    }

    #[test]
    fn double_parsing() {
        assert_eq!(parse_double("28"), Some(28.0));
        assert_eq!(parse_double("-1.5e2"), Some(-150.0));
        assert_eq!(parse_double(" 1"), None);
        assert_eq!(parse_double("true"), None);
        assert_eq!(parse_double("0x10"), Some(16.0));
        assert!(parse_double("nan").unwrap().is_nan());
        assert_eq!(parse_double("1."), Some(1.0));
        assert_eq!(parse_double(".5"), Some(0.5));
        assert_eq!(parse_double(""), None);
    }
}
