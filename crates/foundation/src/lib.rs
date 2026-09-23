//! `upleft-foundation`: the Foundation behaviours Downright's app layer and
//! its command-line tools depend on byte for byte, shared by `upleft-cli`,
//! `upleft-app` and the conformance oracle.
//!
//! * [`json_encoder`] reproduces Swift's `JSONEncoder` output (swift-foundation,
//!   macOS 14 and later): formatting, key order under `.sortedKeys`, string
//!   escaping, number text and the `.iso8601` date strategy. `JSONEncoder` is
//!   Swift-only, so it cannot be called through objc2.
//! * [`json_decoder`] reproduces the input side of Swift's `JSONDecoder`: its
//!   scanner's acceptance rules (encodings, trailing commas, lazy numbers and
//!   strings, first duplicate wins) and the `NSError` code each typed
//!   `decode` throws.
//! * [`json_serialization`] calls `NSJSONSerialization` itself through objc2,
//!   so `JSONSerialization` call sites get Foundation's own output.
//! * [`url`] reproduces Swift's `URL` for `file:` URLs, which differs from
//!   `NSURL` (tilde expansion, file-system representation, some edge cases).
//! * [`date`] holds the `Date` arithmetic both need.
//! * [`decodable`] reproduces what `JSONDecoder` does with a parsed value:
//!   keyed-container rules, value conversions, `.iso8601` dates, `UUID`.
//! * [`file_manager`] makes the `FileManager` and `Data` calls the stores
//!   make, through Foundation.
//! * [`foundation_io`] makes the `FileManager`, `FileHandle`, `Data` and
//!   `String(contentsOf:)` calls the command-line tools make, with
//!   Foundation's own error text.
//!
//! Unlike the other crates this one has no Swift file of its own to mirror;
//! each function names the Foundation API it reproduces.

pub mod date;
pub mod decodable;
pub mod file_manager;
pub mod json_decoder;
pub mod foundation_io;
pub mod json_encoder;
pub mod json_serialization;
pub mod url;
