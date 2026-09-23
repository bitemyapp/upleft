//! Myers.swift — one O(ND) diff, shared by the AST diff (over subtree hashes)
//! and the text diff (over line and word hashes).
//!
//! §12's keystroke budget is 8ms p95 on a 5k-line document, so this stays
//! linear in the *edit distance* rather than in the document size.
//!
//! `max_distance` is the escape hatch: two unrelated documents have an edit
//! distance near N+M, and the caller would rather have "everything changed"
//! instantly than an exact script eventually. The worst case is bounded twice
//! over, because the classic Myers trace is O(D²) memory:
//!
//! 1. **Edge trimming.** Shared leading and trailing runs are peeled off
//!    before the grid is built.
//! 2. **Provable lower-bound bail.** When both sides are large, count how many
//!    new elements can possibly match (presence in the old set). That yields a
//!    lower bound on the edit distance; when it already exceeds
//!    `max_distance`, return `None` *before* allocating the trace.

use std::collections::HashSet;
use std::hash::{BuildHasherDefault, Hasher};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Step {
    Equal { old_index: isize, new_index: isize },
    Delete { old_index: isize },
    Insert { new_index: isize },
}

pub struct Myers;

impl Myers {
    /// Swift's default `maxDistance`.
    pub const DEFAULT_MAX_DISTANCE: isize = 4096;

    /// `diff(_:_:)` with the default `maxDistance` of 4096.
    #[inline]
    pub fn diff_default(old: &[u64], new: &[u64]) -> Option<Vec<Step>> {
        Self::diff(old, new, Self::DEFAULT_MAX_DISTANCE)
    }

    /// Edit script transforming `old` into `new`, or `None` when the distance
    /// exceeds `max_distance`.
    pub fn diff(old: &[u64], new: &[u64], max_distance: isize) -> Option<Vec<Step>> {
        let n = old.len() as isize;
        let m = new.len() as isize;
        if n == 0 {
            return Some((0..m).map(|i| Step::Insert { new_index: i }).collect());
        }
        if m == 0 {
            return Some((0..n).map(|i| Step::Delete { old_index: i }).collect());
        }

        // Shared edges are free. Peel them off so the grid only spans the
        // genuinely different middle; the script is reassembled around it.
        let mut prefix: isize = 0;
        while prefix < n && prefix < m && old[prefix as usize] == new[prefix as usize] {
            prefix += 1;
        }
        let mut hi_old = n;
        let mut hi_new = m;
        while hi_old > prefix && hi_new > prefix && old[(hi_old - 1) as usize] == new[(hi_new - 1) as usize] {
            hi_old -= 1;
            hi_new -= 1;
        }

        // With the edges gone, a lower bound on the distance is N+M-2K where
        // K is the number of elements one side can match in the other. If
        // that bound already blows the cap the search is provably doomed.
        // (The Set is built from the smaller side, and counting stops as soon
        // as the bound drops under the cap. The `- 4` margin catches a pair
        // that sits just under the cap.)
        let smaller = n.min(m);
        if smaller >= 256 && n + m >= max_distance - 4 {
            let small = if n <= m { old } else { new };
            let large = if n <= m { new } else { old };
            let mut present: HashSet<u64, BuildHasherDefault<U64Hasher>> =
                HashSet::with_capacity_and_hasher(smaller as usize, BuildHasherDefault::default());
            for &hash in small {
                present.insert(hash);
            }
            // Enough matches that N+M-2K ≤ maxDistance proves success is
            // still possible; below that the search is provably doomed.
            let match_target = (n + m - max_distance + 1) / 2;
            let mut matched: isize = 0;
            for hash in large {
                if present.contains(hash) {
                    matched += 1;
                    if matched >= match_target {
                        break;
                    }
                }
            }
            if matched < match_target {
                return None;
            }
        }

        let mut steps: Vec<Step> = Vec::with_capacity((n + m) as usize);
        if prefix > 0 {
            steps.extend((0..prefix).map(|i| Step::Equal { old_index: i, new_index: i }));
        }

        let mid_old = hi_old - prefix;
        let mid_new = hi_new - prefix;
        if mid_old == 0 {
            steps.extend((prefix..hi_new).map(|i| Step::Insert { new_index: i }));
        } else if mid_new == 0 {
            steps.extend((prefix..hi_old).map(|i| Step::Delete { old_index: i }));
        } else if !Self::diff_core(
            &old[prefix as usize..hi_old as usize],
            &new[prefix as usize..hi_new as usize],
            prefix,
            max_distance,
            &mut steps,
        ) {
            return None;
        }

        let suffix_length = n - hi_old; // == m - hi_new after the trim loop
        for offset in 0..suffix_length {
            steps.push(Step::Equal { old_index: hi_old + offset, new_index: hi_new + offset });
        }
        Some(steps)
    }

    /// Myers over the trimmed middle, appending steps whose indices are
    /// offset back into the original arrays by `base`. `false` is Swift's
    /// `nil`.
    fn diff_core(old: &[u64], new: &[u64], base: isize, max_distance: isize, out: &mut Vec<Step>) -> bool {
        let n = old.len() as isize;
        let m = new.len() as isize;

        let max = (n + m).min(max_distance);
        let offset = max;
        let mut v: Vec<isize> = vec![0; (2 * max + 1) as usize];
        // The trace is Myers' O(D²) memory. Each row is stored compactly, at
        // only the width the level touches (k ∈ [-d, d]), so row `d` holds
        // 2d+1 entries. Rows are laid end to end: row `d` starts at d².
        let mut trace: Vec<isize> = Vec::new();

        for d in 0..=max {
            let used = offset - d;
            trace.extend_from_slice(&v[used as usize..=(used + 2 * d) as usize]);
            let mut k = -d;
            while k <= d {
                let mut x = if k == -d || (k != d && v[(k - 1 + offset) as usize] < v[(k + 1 + offset) as usize]) {
                    v[(k + 1 + offset) as usize]
                } else {
                    v[(k - 1 + offset) as usize] + 1
                };
                let mut y = x - k;
                while x < n && y < m && old[x as usize] == new[y as usize] {
                    x += 1;
                    y += 1;
                }
                v[(k + offset) as usize] = x;
                if x >= n && y >= m {
                    Self::backtrack(&trace, d, n, m, base, out);
                    return true;
                }
                k += 2;
            }
        }
        false
    }

    fn backtrack(trace: &[isize], final_d: isize, n: isize, m: isize, base: isize, out: &mut Vec<Step>) {
        // Built backwards, then reversed in place onto the caller's steps.
        let start = out.len();
        let mut x = n;
        let mut y = m;
        let mut d = final_d;
        while d > 0 {
            // Row `d` holds k ∈ [-d, d] at local index k + d.
            let row = &trace[(d * d) as usize..(d * d + 2 * d + 1) as usize];
            let k = x - y;
            let previous_k = if k == -d || (k != d && row[(k - 1 + d) as usize] < row[(k + 1 + d) as usize]) {
                k + 1
            } else {
                k - 1
            };
            let previous_x = row[(previous_k + d) as usize];
            let previous_y = previous_x - previous_k;

            while x > previous_x && y > previous_y {
                x -= 1;
                y -= 1;
                out.push(Step::Equal { old_index: x + base, new_index: y + base });
            }
            if x > previous_x {
                x -= 1;
                out.push(Step::Delete { old_index: x + base });
            } else if y > previous_y {
                y -= 1;
                out.push(Step::Insert { new_index: y + base });
            }
            d -= 1;
        }
        while x > 0 && y > 0 {
            x -= 1;
            y -= 1;
            out.push(Step::Equal { old_index: x + base, new_index: y + base });
        }
        out[start..].reverse();
    }
}

/// Multiplicative (Fx-style) hasher for the membership set. Only membership
/// is observed, so the hash function cannot change the result.
#[derive(Default)]
struct U64Hasher(u64);

impl Hasher for U64Hasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.write_u64(byte as u64);
        }
    }

    #[inline]
    fn write_u64(&mut self, value: u64) {
        self.0 = (self.0.rotate_left(5) ^ value).wrapping_mul(0x517c_c1b7_2722_0a95);
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn replay(old: &[u64], new: &[u64], script: &[Step]) {
        let (mut oi, mut ni) = (0isize, 0isize);
        for step in script {
            match *step {
                Step::Equal { old_index, new_index } => {
                    assert_eq!(old[old_index as usize], new[new_index as usize]);
                    assert_eq!((old_index, new_index), (oi, ni));
                    oi += 1;
                    ni += 1;
                }
                Step::Delete { old_index } => {
                    assert_eq!(old_index, oi);
                    oi += 1;
                }
                Step::Insert { new_index } => {
                    assert_eq!(new_index, ni);
                    ni += 1;
                }
            }
        }
        assert_eq!((oi, ni), (old.len() as isize, new.len() as isize));
    }

    #[test]
    fn scripts_replay_in_document_order() {
        let cases: &[(&[u64], &[u64])] = &[
            (&[1, 2, 3], &[3, 2, 1]),
            (&[1, 2, 3, 4, 5], &[1, 9, 3, 9, 5]),
            (&[7, 7, 7], &[7, 7]),
            (&[1], &[2]),
            (&[1, 2], &[2, 1, 2]),
        ];
        for (old, new) in cases {
            let script = Myers::diff_default(old, new).unwrap();
            replay(old, new, &script);
        }
    }
}
