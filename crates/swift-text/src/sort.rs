//! Swift's `sort(by:)` and `sorted(by:)`, comparator call for comparator call.
//!
//! Swift 6.4's `MutableCollection.sort(by:)` is a stable merge sort
//! (`_stableSortImpl`, `@inlinable` in the SDK's `Swift.swiftinterface`):
//! insertion sort for short inputs, otherwise natural runs extended to a
//! minimum length by insertion sort and merged under the run-stack
//! invariants below. A Rust stable sort gives the same result for a strict
//! weak order; this port also gives Swift's result when the comparator is not
//! one (Swift `String <` on non-NFC text, `localizedStandardCompare`), because
//! it makes the same comparator calls in the same order.
//!
//! Copied from `upleft-elk`'s `swift.rs`, where it was first ported.

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

