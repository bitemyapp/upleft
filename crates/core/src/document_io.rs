//! DocumentIO.swift — byte-faithful reading and writing (§3.1).
//!
//! `read` normalises line endings only when the file uses one ending
//! consistently and records what it did in `ByteFidelity`, so
//! `write(read(x)) == x` for every input.

use crate::model::LineEnding;

pub struct DocumentIO;

impl DocumentIO {
    /// `.crlf` or `.cr` only when *every* line break in the file agrees. A
    /// mixed file reports `.lf`, which makes `write` a no-op on line endings.
    pub fn dominant_line_ending(text: &str) -> LineEnding {
        let (mut saw_lf, mut saw_crlf, mut saw_cr) = (false, false, false);
        let mut previous_was_cr = false;
        for scalar in text.chars() {
            if previous_was_cr {
                if scalar == '\n' {
                    saw_crlf = true;
                } else {
                    saw_cr = true;
                }
                previous_was_cr = false;
                if scalar == '\r' {
                    previous_was_cr = true;
                }
                continue;
            }
            if scalar == '\r' {
                previous_was_cr = true;
            } else if scalar == '\n' {
                saw_lf = true;
            }
        }
        if previous_was_cr {
            saw_cr = true;
        }

        if saw_crlf && !saw_lf && !saw_cr {
            return LineEnding::Crlf;
        }
        if saw_cr && !saw_lf && !saw_crlf {
            return LineEnding::Cr;
        }
        LineEnding::Lf
    }
}
