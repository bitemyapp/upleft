//! DocumentIO.swift — byte-faithful reading and writing (§3.1).
//!
//! "An agent-written file that passes through this app is unchanged,
//! character for character, including its odd spacing and its trailing
//! newline." `read` normalises line endings **only when the file uses one
//! ending consistently**, and records what it did in `ByteFidelity`. A file
//! with mixed endings is handed back verbatim with `.lf` recorded, so `write`
//! does no conversion and the stray `\r`s survive. The invariant is that
//! `write(read(x)) == x` for every input.
//!
//! Foundation facts this module reproduces (probed on macOS 26 / Swift 6.4;
//! each is pinned by a unit test below):
//!
//! * `String(data:encoding:)` — see [`string_from_data`].
//! * `String.data(using:)` — see [`data_using`].
//! * `Data.write(to:options:)` — see [`write_data_atomically`] and
//!   [`write_data_without_overwriting`].

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::model::{ByteFidelity, LineEnding, TextEncodingKind};
use crate::swift_text;

/// Swift's untyped `throws`: a [`DocumentIOError`], a [`PosixError`], or an
/// `io::Error` from the filesystem (Foundation's `NSCocoaErrorDomain` errors).
pub type DynError = Box<dyn std::error::Error + Send + Sync + 'static>;

#[derive(Debug)]
pub enum DocumentIOError {
    Undecodable(PathBuf),
    Unencodable(TextEncodingKind),
    TargetChanged { url: PathBuf, displaced: Vec<Vec<u8>> },
    /// The first swap succeeded, but the displaced generation could not be
    /// read back. `recovery_url` is deliberately part of the error: the
    /// caller must never mistake the public path (which now contains our
    /// bytes) for the external generation that was preserved beside it.
    DisplacedGenerationUnreadable { url: PathBuf, recovery_url: PathBuf, underlying: io::Error },
}

impl fmt::Display for DocumentIOError {
    /// `errorDescription`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DocumentIOError::Undecodable(url) => {
                write!(f, "Could not decode {} as text.", last_path_component(url))
            }
            DocumentIOError::Unencodable(encoding) => write!(
                f,
                "The document contains characters that cannot be written as {}.",
                encoding.raw_value()
            ),
            DocumentIOError::TargetChanged { url, .. } => write!(
                f,
                "{} changed while it was being saved. The external bytes were restored.",
                last_path_component(url)
            ),
            DocumentIOError::DisplacedGenerationUnreadable { url, recovery_url, .. } => write!(
                f,
                "{} could not be reconciled while it was being saved. The external bytes were preserved at {}.",
                last_path_component(url),
                last_path_component(recovery_url)
            ),
        }
    }
}

impl std::error::Error for DocumentIOError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DocumentIOError::DisplacedGenerationUnreadable { underlying, .. } => Some(underlying),
            _ => None,
        }
    }
}

/// `NSError(domain: NSPOSIXErrorDomain, code: errno, userInfo:
/// [NSFilePathErrorKey: path])`, as `posixRenameError(path:)` builds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PosixError {
    pub code: i32,
    pub path: PathBuf,
}

impl fmt::Display for PosixError {
    /// The error's `localizedDescription`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "The operation couldn\u{2019}t be completed. {}", strerror(self.code))
    }
}

impl std::error::Error for PosixError {}

pub struct DocumentIO;

impl DocumentIO {
    pub fn read(url: &Path) -> Result<(String, ByteFidelity), DynError> {
        let (text, fidelity, _) = Self::read_snapshot(url)?;
        Ok((text, fidelity))
    }

    /// One read, used by save reconciliation so decoded text, byte fidelity,
    /// and the generation token always describe the same filesystem snapshot.
    pub fn read_snapshot(url: &Path) -> Result<(String, ByteFidelity, Vec<u8>), DynError> {
        let data = fs::read(url)?;
        let (text, fidelity) = Self::decode(&data, url)?;
        Ok((text, fidelity, data))
    }

    /// Decodes bytes already captured by the guarded save protocol without
    /// reopening a path that may now name a different generation.
    pub fn decode_snapshot(data: &[u8], source_url: &Path) -> Result<(String, ByteFidelity), DocumentIOError> {
        Self::decode(data, source_url)
    }

    /// Reads at most `limit` bytes from the head of the file — a bounded read
    /// for surfaces that must never load a huge file whole. A truncated read
    /// can split a multi-byte UTF-8 scalar, so trailing bytes are trimmed
    /// until the head decodes. `None` when the file cannot be opened or its
    /// head cannot be decoded at all. Encoding detection matches `decode`.
    pub fn read_head(url: &Path, limit: isize) -> Option<String> {
        let file = File::open(url).ok()?;
        let data = read_up_to_count(file, limit).ok()??;
        if data.is_empty() {
            return None;
        }

        let bom = Self::detect_bom(&data);
        let body = &data[bom.map_or(0, |bom| bom.length)..];
        let encoding = if let Some(bom) = bom {
            bom.encoding
        } else if let Some(guessed) = Self::sniff_bomless_utf16_or_32(body) {
            guessed
        } else if string_from_data(body, TextEncodingKind::Utf8).is_some() {
            TextEncodingKind::Utf8
        } else {
            TextEncodingKind::Latin1
        };
        if encoding.code_unit_width() > 1 {
            if let Some(text) = string_from_data(body, encoding) {
                return Some(text);
            }
            let trimmed = Self::adjust_truncation(body, encoding);
            if let Some(text) = string_from_data(trimmed, encoding) {
                return Some(text);
            }
        } else {
            // A torn multi-byte UTF-8 tail: trim up to three bytes before the
            // never-failing Latin-1 fallback hides the problem.
            for drop in 0..=3usize {
                if (drop == 0 || data.len() > drop)
                    && let Some(text) = string_from_data(&data[..data.len() - drop], TextEncodingKind::Utf8)
                {
                    return Some(text);
                }
            }
            return string_from_data(body, TextEncodingKind::Latin1);
        }
        // Corrupt beyond either repair path.
        string_from_data(body, TextEncodingKind::Latin1)
    }

    pub fn write(text: &str, url: &Path, fidelity: ByteFidelity) -> Result<(), DynError> {
        let data = Self::encoded_data(text, fidelity)?;
        write_data_atomically(&data, url)?;
        Ok(())
    }

    /// Publishes a complete new file only if the destination is still
    /// absent. Recovery must not overwrite an external writer that recreated
    /// the path after the document inspected it.
    pub fn create_atomically(data: &[u8], url: &Path) -> Result<(), DynError> {
        let temporary = save_temporary_path(url);
        let _cleanup = RemoveOnDrop { path: &temporary, armed: true };
        write_data_without_overwriting(data, &temporary)?;
        Self::sync_temporary_to_stable_storage(&temporary)?;
        if renamex(&temporary, url, libc::RENAME_EXCL) != 0 {
            return Err(Self::posix_rename_error(url));
        }
        Ok(())
    }

    /// Replaces an existing path without a check/write gap. `RENAME_SWAP`
    /// moves the exact displaced generation to the temporary path
    /// atomically; if it is not the generation the caller inspected, a
    /// second swap puts those external bytes back and the save fails closed.
    /// A concurrently deleted target makes the first swap fail, so this never
    /// recreates a missing file.
    pub fn replace_existing_atomically(data: &[u8], url: &Path, expected: &[u8]) -> Result<(), DynError> {
        Self::replace_existing_atomically_impl(data, url, expected, None, None)
    }

    /// Deterministic seam for the two-swap conflict rollback.
    pub fn replace_existing_atomically_for_testing(
        data: &[u8],
        url: &Path,
        expected: &[u8],
        after_displaced_read: &mut dyn FnMut(),
        after_swap: Option<&mut dyn FnMut(&Path)>,
    ) -> Result<(), DynError> {
        Self::replace_existing_atomically_impl(data, url, expected, Some(after_displaced_read), after_swap)
    }

    fn replace_existing_atomically_impl(
        data: &[u8],
        url: &Path,
        expected: &[u8],
        after_displaced_read: Option<&mut dyn FnMut()>,
        after_swap: Option<&mut dyn FnMut(&Path)>,
    ) -> Result<(), DynError> {
        let temporary = save_temporary_path(url);
        // Once the first swap succeeds, this path owns the displaced
        // generation. Keep it until that generation has been read and the
        // two-swap reconciliation has completed; a read failure must leave a
        // recoverable path rather than silently deleting the only external
        // bytes.
        let mut cleanup = RemoveOnDrop { path: &temporary, armed: false };
        write_data_without_overwriting(data, &temporary)?;

        // `attributesOfItem(atPath:)[.posixPermissions]` (lstat, 0o7777
        // bits), applied with `setAttributes`; both failures are ignored.
        if let Ok(metadata) = fs::symlink_metadata(url) {
            let permissions = metadata.permissions().mode() & 0o7777;
            let _ = fs::set_permissions(&temporary, fs::Permissions::from_mode(permissions));
        }

        // The swap publishes a name, not durability: flush the payload first
        // so a crash right after the swap cannot leave a truncated or empty
        // generation at the document path.
        Self::sync_temporary_to_stable_storage(&temporary)?;

        if renamex(&temporary, url, libc::RENAME_SWAP) != 0 {
            return Err(Self::posix_rename_error(url));
        }
        if let Some(after_swap) = after_swap {
            after_swap(&temporary);
        }
        let displaced = match fs::read(&temporary) {
            Ok(displaced) => displaced,
            Err(error) => {
                return Err(Box::new(DocumentIOError::DisplacedGenerationUnreadable {
                    url: url.to_path_buf(),
                    recovery_url: temporary.clone(),
                    underlying: error,
                }));
            }
        };
        if displaced != expected {
            if let Some(after_displaced_read) = after_displaced_read {
                after_displaced_read();
            }
            // Swap, rather than replace, so a writer that landed after our
            // first swap is retained at `temporary` instead of being erased.
            if renamex(&temporary, url, libc::RENAME_SWAP) != 0 {
                return Err(Self::posix_rename_error(url));
            }
            let post_swap_temporary = match fs::read(&temporary) {
                Ok(bytes) => bytes,
                Err(error) => {
                    // The public path now holds the external generation and
                    // the temporary path our attempted payload; keep both.
                    return Err(Box::new(DocumentIOError::DisplacedGenerationUnreadable {
                        url: url.to_path_buf(),
                        recovery_url: url.to_path_buf(),
                        underlying: error,
                    }));
                }
            };
            if post_swap_temporary != data {
                // Another writer replaced our payload between the two swaps.
                // Put that newest external generation back at the public
                // path; both external generations travel in the error.
                if renamex(&temporary, url, libc::RENAME_SWAP) != 0 {
                    return Err(Self::posix_rename_error(url));
                }
                cleanup.armed = true;
                return Err(Box::new(DocumentIOError::TargetChanged {
                    url: url.to_path_buf(),
                    displaced: vec![displaced, post_swap_temporary],
                }));
            }
            cleanup.armed = true;
            return Err(Box::new(DocumentIOError::TargetChanged { url: url.to_path_buf(), displaced: vec![displaced] }));
        }
        // The inspected generation matched and has been read safely.
        cleanup.armed = true;
        Ok(())
    }

    /// `posixRenameError(path:)`: the current `errno`.
    fn posix_rename_error(path: &Path) -> DynError {
        Box::new(PosixError { code: errno(), path: path.to_path_buf() })
    }

    /// Flushes a just-written file's contents to stable storage before its
    /// name is published. `F_FULLFSYNC` waits for the drive itself; where it
    /// is unsupported, fall back to `fsync`, and only a failure of both is a
    /// save error.
    fn sync_temporary_to_stable_storage(url: &Path) -> Result<(), DynError> {
        let handle = File::open(url)?;
        let fd = handle.as_raw_fd();
        if unsafe { libc::fcntl(fd, libc::F_FULLFSYNC) } == 0 {
            return Ok(());
        }
        if unsafe { libc::fsync(fd) } != 0 {
            return Err(Self::posix_rename_error(url));
        }
        Ok(())
    }

    /// The exact bytes `write` will place on disk.
    pub fn encoded_data(text: &str, fidelity: ByteFidelity) -> Result<Vec<u8>, DocumentIOError> {
        Self::encode(text, fidelity)
    }

    /// Hex SHA-256 of the text's UTF-8 bytes (§8.2, §8.3).
    pub fn content_hash(text: &str) -> String {
        Self::content_hash_data(text.as_bytes())
    }

    /// Hex SHA-256 of raw bytes (`contentHash(_: Data)`).
    pub fn content_hash_data(data: &[u8]) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let digest = Sha256::digest(data);
        let mut out = String::with_capacity(64);
        for byte in digest {
            out.push(HEX[(byte >> 4) as usize] as char);
            out.push(HEX[(byte & 0x0F) as usize] as char);
        }
        out
    }

    // MARK: Decoding

    pub fn decode(data: &[u8], url: &Path) -> Result<(String, ByteFidelity), DocumentIOError> {
        let bom = Self::detect_bom(data);
        let has_bom = bom.is_some();
        let body = &data[bom.map_or(0, |bom| bom.length)..];

        // The UTF-8 probe's result is the decode itself when UTF-8 wins.
        let mut decoded: Option<String> = None;
        let encoding = if let Some(bom) = bom {
            bom.encoding
        } else if let Some(guessed) = Self::sniff_bomless_utf16_or_32(body) {
            // A NUL-padded UTF-16/32 stream is also *valid* UTF-8, so it is
            // checked first; the heuristic only fires on a clearly
            // NUL-structured body.
            guessed
        } else if let Some(text) = string_from_data(body, TextEncodingKind::Utf8) {
            decoded = Some(text);
            TextEncodingKind::Utf8
        } else {
            // Latin-1 never fails, which is why it is the fallback and never
            // a guess made ahead of UTF-8.
            TextEncodingKind::Latin1
        };

        let raw = if let Some(text) = decoded.or_else(|| string_from_data(body, encoding)) {
            text
        } else if encoding.code_unit_width() > 1 {
            // A file truncated mid-code-unit is read up to the last whole
            // code unit rather than rejected outright.
            let trimmed = Self::adjust_truncation(body, encoding);
            match string_from_data(trimmed, encoding) {
                Some(recovered) => recovered,
                None => return Err(DocumentIOError::Undecodable(url.to_path_buf())),
            }
        } else {
            return Err(DocumentIOError::Undecodable(url.to_path_buf()));
        };

        let ending = Self::dominant_line_ending(&raw);
        let text = match ending {
            LineEnding::Lf => raw,
            LineEnding::Crlf => swift_text::replacing_occurrences(&raw, "\r\n", "\n"),
            LineEnding::Cr => swift_text::replacing_occurrences(&raw, "\r", "\n"),
        };
        let has_trailing_newline = swift_text::has_suffix(&text, "\n");
        Ok((text, ByteFidelity::new(encoding, has_bom, ending, has_trailing_newline)))
    }

    /// Drops the trailing bytes that made a multi-byte decode fail: for
    /// UTF-16 a single torn stride byte, for UTF-32 anything up to a
    /// code-unit boundary.
    fn adjust_truncation(body: &[u8], encoding: TextEncodingKind) -> &[u8] {
        let width = encoding.code_unit_width();
        if width <= 1 {
            return body;
        }
        let excess = body.len() % width;
        if excess == 0 {
            return body;
        }
        &body[..body.len() - excess]
    }

    /// Heuristic for a BOM-less UTF-16/32 file: an 8-bit file with enough NUL
    /// bytes to look like a packed UTF-16/32 code-unit stream.
    fn sniff_bomless_utf16_or_32(body: &[u8]) -> Option<TextEncodingKind> {
        if body.len() < 4 {
            return None;
        }
        let bytes = &body[..body.len().min(256)];
        let (mut even, mut odd) = (0usize, 0usize);
        for (index, &byte) in bytes.iter().enumerate() {
            if byte == 0 {
                if index % 2 == 0 {
                    even += 1;
                } else {
                    odd += 1;
                }
            }
        }
        let total = bytes.len();
        let even_ratio = even as f64 / total as f64;
        let odd_ratio = odd as f64 / total as f64;
        if even_ratio >= 0.3 && even_ratio > odd_ratio * 2.0 {
            return Some(TextEncodingKind::Utf16BE);
        }
        if odd_ratio >= 0.3 && odd_ratio > even_ratio * 2.0 {
            return Some(TextEncodingKind::Utf16LE);
        }
        // A UTF-32 stream is densely NUL at both parities for ASCII content.
        // The first non-NUL byte's position in its 4-byte word tells the
        // byte order (LE: `XX 00 00 00`, BE: `00 00 00 XX`).
        if even_ratio >= 0.45
            && odd_ratio >= 0.45
            && let Some(first_non_zero) = bytes.iter().position(|&byte| byte != 0)
        {
            return Some(if first_non_zero % 4 == 3 { TextEncodingKind::Utf32BE } else { TextEncodingKind::Utf32LE });
        }
        None
    }

    pub fn encode(text: &str, fidelity: ByteFidelity) -> Result<Vec<u8>, DocumentIOError> {
        // The text buffer owns whether a final newline exists. Fidelity owns
        // how that newline is encoded.
        let converted;
        let body = match fidelity.line_ending {
            LineEnding::Lf => text,
            LineEnding::Crlf => {
                converted = swift_text::replacing_occurrences(text, "\n", "\r\n");
                converted.as_str()
            }
            LineEnding::Cr => {
                converted = swift_text::replacing_occurrences(text, "\n", "\r");
                converted.as_str()
            }
        };

        let Some(data) = data_using(body, fidelity.encoding) else {
            return Err(DocumentIOError::Unencodable(fidelity.encoding));
        };
        if fidelity.has_bom
            && let Some(bom) = Self::bom_bytes(fidelity.encoding)
        {
            let mut out = Vec::with_capacity(bom.len() + data.len());
            out.extend_from_slice(bom);
            out.extend_from_slice(&data);
            return Ok(out);
        }
        Ok(data)
    }

    // MARK: Byte-level facts

    fn detect_bom(data: &[u8]) -> Option<Bom> {
        let bytes = &data[..data.len().min(4)];
        // 4-byte BOMs first: a UTF-32LE BOM starts `FF FE`, which a 2-byte
        // check would misread as UTF-16LE, and UTF-32BE is `00 00 FE FF`.
        if bytes.len() >= 4 && bytes[0] == 0xFF && bytes[1] == 0xFE && bytes[2] == 0x00 && bytes[3] == 0x00 {
            return Some(Bom { encoding: TextEncodingKind::Utf32LE, length: 4 });
        }
        if bytes.len() >= 4 && bytes[0] == 0x00 && bytes[1] == 0x00 && bytes[2] == 0xFE && bytes[3] == 0xFF {
            return Some(Bom { encoding: TextEncodingKind::Utf32BE, length: 4 });
        }
        if bytes.len() >= 3 && bytes[0] == 0xEF && bytes[1] == 0xBB && bytes[2] == 0xBF {
            return Some(Bom { encoding: TextEncodingKind::Utf8, length: 3 });
        }
        if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
            return Some(Bom { encoding: TextEncodingKind::Utf16LE, length: 2 });
        }
        if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
            return Some(Bom { encoding: TextEncodingKind::Utf16BE, length: 2 });
        }
        None
    }

    fn bom_bytes(encoding: TextEncodingKind) -> Option<&'static [u8]> {
        match encoding {
            TextEncodingKind::Utf8 => Some(&[0xEF, 0xBB, 0xBF]),
            TextEncodingKind::Utf16LE => Some(&[0xFF, 0xFE]),
            TextEncodingKind::Utf16BE => Some(&[0xFE, 0xFF]),
            TextEncodingKind::Utf32LE => Some(&[0xFF, 0xFE, 0x00, 0x00]),
            TextEncodingKind::Utf32BE => Some(&[0x00, 0x00, 0xFE, 0xFF]),
            TextEncodingKind::Latin1 => None,
        }
    }

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

#[derive(Clone, Copy, Debug)]
struct Bom {
    encoding: TextEncodingKind,
    length: usize,
}

// MARK: - Foundation conversions

/// `String(data:encoding: encoding.stringEncoding)`, as Foundation behaves on
/// macOS 26 (probed):
///
/// * UTF-8: strict validation (overlongs, encoded surrogates, > U+10FFFF and
///   torn sequences are `nil`); exactly **one** leading `EF BB BF` is
///   stripped, a second one stays as U+FEFF.
/// * UTF-16LE/BE: a trailing odd byte is silently dropped (not `nil`); any
///   unpaired surrogate is `nil`; a leading BOM of either order is **kept**
///   (as U+FEFF or U+FFFE).
/// * UTF-32LE/BE: trailing bytes short of a whole unit are dropped; a
///   surrogate or a value above U+10FFFF is `nil`; a BOM is kept.
/// * ISO Latin-1: every byte is its own scalar; never `nil`.
pub fn string_from_data(data: &[u8], encoding: TextEncodingKind) -> Option<String> {
    match encoding {
        TextEncodingKind::Utf8 => {
            let body = data.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(data);
            std::str::from_utf8(body).ok().map(str::to_owned)
        }
        TextEncodingKind::Utf16LE | TextEncodingKind::Utf16BE => {
            let little = encoding == TextEncodingKind::Utf16LE;
            let units = data.chunks_exact(2).map(|pair| {
                let bytes = [pair[0], pair[1]];
                if little { u16::from_le_bytes(bytes) } else { u16::from_be_bytes(bytes) }
            });
            let mut out = String::with_capacity(data.len() / 2);
            for decoded in char::decode_utf16(units) {
                out.push(decoded.ok()?);
            }
            Some(out)
        }
        TextEncodingKind::Utf32LE | TextEncodingKind::Utf32BE => {
            let little = encoding == TextEncodingKind::Utf32LE;
            let mut out = String::with_capacity(data.len() / 4);
            for word in data.chunks_exact(4) {
                let bytes = [word[0], word[1], word[2], word[3]];
                let value = if little { u32::from_le_bytes(bytes) } else { u32::from_be_bytes(bytes) };
                out.push(char::from_u32(value)?);
            }
            Some(out)
        }
        TextEncodingKind::Latin1 => Some(data.iter().map(|&byte| byte as char).collect()),
    }
}

/// `text.data(using: encoding.stringEncoding)` (probed): UTF-8/16/32 never
/// fail and never add a BOM (a U+FEFF already in the text is encoded like any
/// other scalar). ISO Latin-1 is `nil` for a character it cannot hold, but
/// Foundation first precomposes a base letter with following combining marks
/// (`e` + U+0301 → `E9`, while U+212B ANGSTROM SIGN is `nil`), so anything
/// beyond U+00FF goes through `-[NSString dataUsingEncoding:]` itself.
///
/// These are the semantics of a *native* Swift `String`. A `String` bridged
/// from an `NSString` (text-view storage, some Foundation results) takes the
/// CFString path, which drops a leading U+FEFF instead of failing; that
/// provenance is not modelled.
pub fn data_using(text: &str, encoding: TextEncodingKind) -> Option<Vec<u8>> {
    match encoding {
        TextEncodingKind::Utf8 => Some(text.as_bytes().to_vec()),
        TextEncodingKind::Utf16LE => Some(text.encode_utf16().flat_map(u16::to_le_bytes).collect()),
        TextEncodingKind::Utf16BE => Some(text.encode_utf16().flat_map(u16::to_be_bytes).collect()),
        TextEncodingKind::Utf32LE => Some(text.chars().flat_map(|c| (c as u32).to_le_bytes()).collect()),
        TextEncodingKind::Utf32BE => Some(text.chars().flat_map(|c| (c as u32).to_be_bytes()).collect()),
        TextEncodingKind::Latin1 => {
            if text.chars().all(|c| (c as u32) <= 0xFF) {
                return Some(text.chars().map(|c| c as u32 as u8).collect());
            }
            foundation_latin1(text)
        }
    }
}

/// A native Swift `String` and a CFString-backed `NSString` convert alike
/// (fuzzed), except that the CFString path skips a leading U+FEFF where the
/// native one fails; `NSString::from_str` is CFString-backed, so that case is
/// answered here.
fn foundation_latin1(text: &str) -> Option<Vec<u8>> {
    use objc2_foundation::{NSISOLatin1StringEncoding, NSString};
    if text.starts_with('\u{FEFF}') {
        return None;
    }
    objc2::rc::autoreleasepool(|_| {
        NSString::from_str(text)
            .dataUsingEncoding_allowLossyConversion(NSISOLatin1StringEncoding, false)
            .map(|data| data.to_vec())
    })
}

// MARK: - Filesystem helpers

/// `Data.write(to:options: .atomic)`, as Foundation performs it (traced on
/// macOS 26): a sibling temporary opened `O_RDWR|O_CREAT|O_EXCL` with mode
/// 0666, written, `fsync(2)`ed, and renamed over the destination. When the
/// destination existed, its `lstat` permission bits are then applied to the
/// new file (so a symlink is replaced by a regular file carrying the link's
/// mode, as Foundation does).
pub fn write_data_atomically(data: &[u8], url: &Path) -> io::Result<()> {
    let existing_mode = fs::symlink_metadata(url).ok().map(|metadata| metadata.permissions().mode() & 0o7777);
    let directory = parent_directory(url);
    let name = url.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();

    let (temporary, mut file) = loop {
        let candidate = directory.join(format!("{name}.sb-{}", random_suffix()));
        match OpenOptions::new().read(true).write(true).create_new(true).mode(0o666).open(&candidate) {
            Ok(file) => break (candidate, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    let published = (|| {
        file.write_all(data)?;
        if unsafe { libc::fsync(file.as_raw_fd()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        fs::rename(&temporary, url)
    })();
    if let Err(error) = published {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    if let Some(mode) = existing_mode {
        unsafe { libc::fchmod(file.as_raw_fd(), mode as libc::mode_t) };
    }
    Ok(())
}

/// `Data.write(to:options: .withoutOverwriting)` (traced):
/// `O_WRONLY|O_CREAT|O_EXCL|O_TRUNC` with mode 0666, written, `fsync(2)`ed.
pub fn write_data_without_overwriting(data: &[u8], url: &Path) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).truncate(true).mode(0o666).open(url)?;
    file.write_all(data)?;
    if unsafe { libc::fsync(file.as_raw_fd()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// `FileHandle.read(upToCount:)`: `None` (Swift's `nil`) for a zero count or
/// at end of file; a negative count reads to the end (probed).
fn read_up_to_count(file: File, limit: isize) -> io::Result<Option<Vec<u8>>> {
    if limit == 0 {
        return Ok(None);
    }
    let mut data = Vec::new();
    if limit < 0 {
        let mut file = file;
        file.read_to_end(&mut data)?;
    } else {
        file.take(limit as u64).read_to_end(&mut data)?;
    }
    Ok(if data.is_empty() { None } else { Some(data) })
}

/// `url.deletingLastPathComponent().appendingPathComponent(".downright-save-\(UUID().uuidString)")`.
fn save_temporary_path(url: &Path) -> PathBuf {
    let uuid = uuid::Uuid::new_v4().hyphenated().to_string().to_uppercase();
    parent_directory(url).join(format!(".downright-save-{uuid}"))
}

fn parent_directory(url: &Path) -> &Path {
    url.parent().unwrap_or(url)
}

fn random_suffix() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let bytes = *uuid::Uuid::new_v4().as_bytes();
    let tag = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let tail: String = bytes[4..10].iter().map(|&b| ALPHABET[b as usize % ALPHABET.len()] as char).collect();
    format!("{tag:08x}-{tail}")
}

/// `defer { try? FileManager.default.removeItem(at: temporary) }`, optionally
/// gated like `mayRemoveTemporary`.
struct RemoveOnDrop<'a> {
    path: &'a Path,
    armed: bool,
}

impl Drop for RemoveOnDrop<'_> {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(self.path);
        }
    }
}

fn renamex(from: &Path, to: &Path, flags: libc::c_uint) -> libc::c_int {
    let (Ok(from), Ok(to)) =
        (std::ffi::CString::new(from.as_os_str().as_bytes()), std::ffi::CString::new(to.as_os_str().as_bytes()))
    else {
        unsafe { *libc::__error() = libc::EINVAL };
        return -1;
    };
    unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), flags) }
}

fn errno() -> i32 {
    io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

fn strerror(code: i32) -> String {
    let mut buffer = [0 as libc::c_char; 256];
    if unsafe { libc::strerror_r(code, buffer.as_mut_ptr(), buffer.len()) } != 0 {
        return format!("Unknown error: {code}");
    }
    unsafe { std::ffi::CStr::from_ptr(buffer.as_ptr()) }.to_string_lossy().into_owned()
}

/// `URL.lastPathComponent` for a file path.
fn last_path_component(path: &Path) -> String {
    path.components().next_back().map(|c| c.as_os_str().to_string_lossy().into_owned()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use TextEncodingKind::*;

    fn scalars(s: Option<String>) -> Option<Vec<u32>> {
        s.map(|s| s.chars().map(|c| c as u32).collect())
    }

    // Every expectation below was recorded from `String(data:encoding:)` on
    // macOS 26 / Swift 6.4.

    #[test]
    fn utf8_decoding_matches_foundation() {
        assert_eq!(scalars(string_from_data(&[], Utf8)), Some(vec![]));
        assert_eq!(scalars(string_from_data(&[0xEF, 0xBB, 0xBF], Utf8)), Some(vec![]));
        assert_eq!(scalars(string_from_data(&[0xEF, 0xBB, 0xBF, 0x61], Utf8)), Some(vec![0x61]));
        // Only one BOM is stripped.
        assert_eq!(
            scalars(string_from_data(&[0xEF, 0xBB, 0xBF, 0xEF, 0xBB, 0xBF, 0x61], Utf8)),
            Some(vec![0xFEFF, 0x61])
        );
        assert_eq!(scalars(string_from_data(&[0x61, 0xEF, 0xBB, 0xBF], Utf8)), Some(vec![0x61, 0xFEFF]));
        assert_eq!(scalars(string_from_data(&[0xEF, 0xBF, 0xBE], Utf8)), Some(vec![0xFFFE]));
        assert_eq!(scalars(string_from_data(&[0x61, 0x00, 0x62], Utf8)), Some(vec![0x61, 0, 0x62]));
        assert_eq!(scalars(string_from_data(&[0xF4, 0x8F, 0xBF, 0xBF], Utf8)), Some(vec![0x10FFFF]));
        for invalid in [
            &[0x63, 0xE9, 0x0A][..],
            &[0xC0, 0x80],
            &[0xED, 0xA0, 0x80],
            &[0xF4, 0x90, 0x80, 0x80],
            &[0x61, 0xF0, 0x9F, 0x8C],
            &[0xF5, 0x80, 0x80, 0x80],
            &[0xEF, 0xBB],
            &[0xEF, 0xBB, 0xBF, 0xFF],
            &[0xED, 0xA0, 0xBD, 0xED, 0xB8, 0x80],
            &[0xE0, 0x80, 0x80],
            &[0xF0, 0x80, 0x80, 0x80],
            &[0x80],
            &[0xFE],
            &[0xFF],
        ] {
            assert_eq!(string_from_data(invalid, Utf8), None, "{invalid:02X?}");
        }
    }

    #[test]
    fn utf16_decoding_matches_foundation() {
        assert_eq!(scalars(string_from_data(&[], Utf16LE)), Some(vec![]));
        assert_eq!(scalars(string_from_data(&[0x61, 0x00], Utf16LE)), Some(vec![0x61]));
        // A trailing odd byte is dropped, not an error.
        assert_eq!(scalars(string_from_data(&[0x61, 0x00, 0x62], Utf16LE)), Some(vec![0x61]));
        assert_eq!(scalars(string_from_data(&[0x61], Utf16LE)), Some(vec![]));
        assert_eq!(scalars(string_from_data(&[0x3D, 0xD8, 0x00, 0xDE, 0x41], Utf16LE)), Some(vec![0x1F600]));
        // BOMs are kept, in either byte order.
        assert_eq!(scalars(string_from_data(&[0xFF, 0xFE, 0x61, 0x00], Utf16LE)), Some(vec![0xFEFF, 0x61]));
        assert_eq!(
            scalars(string_from_data(&[0xFF, 0xFE, 0xFF, 0xFE, 0x61, 0x00], Utf16LE)),
            Some(vec![0xFEFF, 0xFEFF, 0x61])
        );
        assert_eq!(scalars(string_from_data(&[0xFE, 0xFF, 0x61, 0x00], Utf16LE)), Some(vec![0xFFFE, 0x61]));
        assert_eq!(scalars(string_from_data(&[0xFF, 0xFE], Utf16LE)), Some(vec![0xFEFF]));
        assert_eq!(scalars(string_from_data(&[0x00, 0x00], Utf16LE)), Some(vec![0]));
        assert_eq!(scalars(string_from_data(&[0xFF, 0xFF], Utf16LE)), Some(vec![0xFFFF]));
        for invalid in [
            &[0x61, 0x00, 0x3D, 0xD8][..],
            &[0x3D, 0xD8, 0x61, 0x00],
            &[0x00, 0xDC, 0x61, 0x00],
            &[0x00, 0xDC, 0x3D, 0xD8],
            &[0x3D, 0xD8, 0x00],
            &[0x61, 0x00, 0x3D, 0xD8, 0x00],
        ] {
            assert_eq!(string_from_data(invalid, Utf16LE), None, "{invalid:02X?}");
        }

        assert_eq!(scalars(string_from_data(&[0x00, 0x61], Utf16BE)), Some(vec![0x61]));
        assert_eq!(scalars(string_from_data(&[0x00, 0x61, 0x00], Utf16BE)), Some(vec![0x61]));
        assert_eq!(scalars(string_from_data(&[0xFE, 0xFF, 0x00, 0x61], Utf16BE)), Some(vec![0xFEFF, 0x61]));
        assert_eq!(scalars(string_from_data(&[0xFF, 0xFE, 0x00, 0x61], Utf16BE)), Some(vec![0xFFFE, 0x61]));
        assert_eq!(scalars(string_from_data(&[0xD8, 0x3D, 0xDE, 0x00], Utf16BE)), Some(vec![0x1F600]));
        assert_eq!(string_from_data(&[0x00, 0x61, 0xD8, 0x3D], Utf16BE), None);
        assert_eq!(string_from_data(&[0xDC, 0x00, 0x00, 0x61], Utf16BE), None);
        assert_eq!(string_from_data(&[0xD8, 0x3D, 0xDE], Utf16BE), None);
    }

    #[test]
    fn utf32_decoding_matches_foundation() {
        assert_eq!(scalars(string_from_data(&[], Utf32LE)), Some(vec![]));
        assert_eq!(scalars(string_from_data(&[0x61, 0, 0, 0], Utf32LE)), Some(vec![0x61]));
        assert_eq!(scalars(string_from_data(&[0x61, 0, 0, 0, 0x62], Utf32LE)), Some(vec![0x61]));
        assert_eq!(scalars(string_from_data(&[0x61, 0, 0], Utf32LE)), Some(vec![]));
        assert_eq!(scalars(string_from_data(&[0x61, 0, 0, 0, 0, 0, 0x11], Utf32LE)), Some(vec![0x61]));
        assert_eq!(scalars(string_from_data(&[0xFF, 0xFE, 0, 0, 0x61, 0, 0, 0], Utf32LE)), Some(vec![0xFEFF, 0x61]));
        assert_eq!(scalars(string_from_data(&[0x0A, 0xF3, 0x01, 0], Utf32LE)), Some(vec![0x1F30A]));
        assert_eq!(scalars(string_from_data(&[0xFF, 0xFF, 0x10, 0], Utf32LE)), Some(vec![0x10FFFF]));
        assert_eq!(scalars(string_from_data(&[0xFE, 0xFF, 0, 0], Utf32LE)), Some(vec![0xFFFE]));
        assert_eq!(scalars(string_from_data(&[0, 0, 0, 0], Utf32LE)), Some(vec![0]));
        for invalid in [
            &[0, 0, 0xFE, 0xFF, 0x61, 0, 0, 0][..],
            &[0, 0, 0x11, 0],
            &[0, 0xD8, 0, 0],
            &[0, 0xDC, 0, 0],
            &[0xFF, 0xFF, 0xFF, 0xFF],
            &[0, 0xD8, 0, 0, 1],
        ] {
            assert_eq!(string_from_data(invalid, Utf32LE), None, "{invalid:02X?}");
        }

        assert_eq!(scalars(string_from_data(&[0, 0, 0, 0x61], Utf32BE)), Some(vec![0x61]));
        assert_eq!(scalars(string_from_data(&[0, 0, 0, 0x61, 0], Utf32BE)), Some(vec![0x61]));
        assert_eq!(
            scalars(string_from_data(&[0, 0, 0xFE, 0xFF, 0, 0, 0xFE, 0xFF, 0, 0, 0, 0x61], Utf32BE)),
            Some(vec![0xFEFF, 0xFEFF, 0x61])
        );
        assert_eq!(string_from_data(&[0xFF, 0xFE, 0, 0, 0, 0, 0, 0x61], Utf32BE), None);
        assert_eq!(string_from_data(&[0, 0x11, 0, 0], Utf32BE), None);
        assert_eq!(string_from_data(&[0, 0, 0xD8, 0], Utf32BE), None);
    }

    #[test]
    fn latin1_decoding_is_the_identity() {
        let all: Vec<u8> = (0..=255).collect();
        assert_eq!(scalars(string_from_data(&all, Latin1)), Some((0..=255).collect()));
        assert_eq!(scalars(string_from_data(&[0xEF, 0xBB, 0xBF, 0x61], Latin1)), Some(vec![0xEF, 0xBB, 0xBF, 0x61]));
    }

    // Recorded from `String.data(using:)` on macOS 26 / Swift 6.4.
    #[test]
    fn encoding_matches_foundation() {
        assert_eq!(data_using("\u{FEFF}a", Utf8), Some(vec![0xEF, 0xBB, 0xBF, 0x61]));
        assert_eq!(data_using("\u{FEFF}a", Utf16LE), Some(vec![0xFF, 0xFE, 0x61, 0x00]));
        assert_eq!(data_using("a", Utf16LE), Some(vec![0x61, 0x00]));
        assert_eq!(data_using("a", Utf16BE), Some(vec![0x00, 0x61]));
        assert_eq!(data_using("😀", Utf16LE), Some(vec![0x3D, 0xD8, 0x00, 0xDE]));
        assert_eq!(data_using("😀", Utf16BE), Some(vec![0xD8, 0x3D, 0xDE, 0x00]));
        assert_eq!(data_using("😀", Utf32LE), Some(vec![0x00, 0xF6, 0x01, 0x00]));
        assert_eq!(data_using("😀", Utf32BE), Some(vec![0x00, 0x01, 0xF6, 0x00]));
        assert_eq!(data_using("", Utf32LE), Some(vec![]));
        assert_eq!(data_using("a\u{0}b", Latin1), Some(vec![0x61, 0x00, 0x62]));
        assert_eq!(data_using("é\u{85}ÿ\u{A0}", Latin1), Some(vec![0xE9, 0x85, 0xFF, 0xA0]));
        // Precomposition into Latin-1.
        assert_eq!(data_using("e\u{301}", Latin1), Some(vec![0xE9]));
        assert_eq!(data_using("A\u{30A}", Latin1), Some(vec![0xC5]));
        assert_eq!(data_using("a\u{308}", Latin1), Some(vec![0xE4]));
        assert_eq!(data_using("c\u{327}", Latin1), Some(vec![0xE7]));
        assert_eq!(data_using("y\u{308}", Latin1), Some(vec![0xFF]));
        assert_eq!(data_using("e\u{301}e\u{301}", Latin1), Some(vec![0xE9, 0xE9]));
        assert_eq!(data_using("e\u{301}\u{0}", Latin1), Some(vec![0xE9, 0x00]));
        for unencodable in [
            "\u{FEFF}a",
            "\u{FEFF}",
            "\u{FEFF}e\u{301}",
            "\u{FEFF}é",
            "a\u{FEFF}b",
            "\u{212B}",
            "\u{2126}",
            "Ω",
            "€",
            "\u{100}",
            "😀",
            "\u{212A}",
            "\u{FFFE}",
            "x\u{308}",
            "é\u{301}",
            "\u{FB01}",
            "\u{FF21}",
            "\u{2028}",
            "a\u{308}\u{301}",
            "\u{301}",
            "Y\u{308}",
            "a\u{323}\u{301}",
            "i\u{307}",
            "\u{C0}\u{300}",
            "a\u{30A}\u{30A}",
        ] {
            assert_eq!(data_using(unencodable, Latin1), None, "{unencodable:?}");
        }
    }

    #[test]
    fn bom_detection_prefers_four_byte_marks() {
        assert_eq!(DocumentIO::detect_bom(&[0xFF, 0xFE, 0, 0]).map(|b| (b.encoding, b.length)), Some((Utf32LE, 4)));
        assert_eq!(DocumentIO::detect_bom(&[0xFF, 0xFE, 0, 1]).map(|b| (b.encoding, b.length)), Some((Utf16LE, 2)));
        assert_eq!(DocumentIO::detect_bom(&[0, 0, 0xFE, 0xFF]).map(|b| (b.encoding, b.length)), Some((Utf32BE, 4)));
        assert_eq!(DocumentIO::detect_bom(&[0xEF, 0xBB, 0xBF]).map(|b| (b.encoding, b.length)), Some((Utf8, 3)));
        assert_eq!(DocumentIO::detect_bom(&[0xFE, 0xFF]).map(|b| (b.encoding, b.length)), Some((Utf16BE, 2)));
        assert!(DocumentIO::detect_bom(&[0xEF, 0xBB]).is_none());
    }

    /// A file with two UTF-8 BOMs: `detectBOM` takes the first and
    /// `String(data:encoding:)` silently strips the second.
    #[test]
    fn double_utf8_bom_loses_the_inner_mark() {
        let (text, fidelity) =
            DocumentIO::decode(&[0xEF, 0xBB, 0xBF, 0xEF, 0xBB, 0xBF, b'a', b'\n'], Path::new("/x/doc.md")).unwrap();
        assert_eq!(text, "a\n");
        assert!(fidelity.has_bom);
        assert_eq!(fidelity.encoding, Utf8);
    }

    /// A UTF-16LE file whose body starts with another `FF FE` keeps it.
    #[test]
    fn inner_utf16_bom_survives() {
        let (text, _) = DocumentIO::decode(&[0xFF, 0xFE, 0xFF, 0xFE, b'a', 0], Path::new("/x/doc.md")).unwrap();
        assert_eq!(text, "\u{FEFF}a");
    }

    #[test]
    fn character_wise_trailing_newline() {
        // A mixed file keeps its CR LF, which is one Character, not "\n".
        let (text, fidelity) = DocumentIO::decode(b"a\nb\r\n", Path::new("/x/doc.md")).unwrap();
        assert_eq!(text, "a\nb\r\n");
        assert_eq!(fidelity.line_ending, LineEnding::Lf);
        assert!(!fidelity.has_trailing_newline);
    }

    #[test]
    fn undecodable_utf16_is_an_error() {
        let error = DocumentIO::decode(&[0xFF, 0xFE, 0x3D, 0xD8], Path::new("/x/doc.md")).unwrap_err();
        assert_eq!(error.to_string(), "Could not decode doc.md as text.");
    }

    #[test]
    fn error_descriptions_match_swift() {
        assert_eq!(
            DocumentIOError::Unencodable(Latin1).to_string(),
            "The document contains characters that cannot be written as latin1."
        );
        assert_eq!(
            DocumentIOError::TargetChanged { url: PathBuf::from("/a/doc.md"), displaced: vec![] }.to_string(),
            "doc.md changed while it was being saved. The external bytes were restored."
        );
        assert_eq!(
            DocumentIOError::DisplacedGenerationUnreadable {
                url: PathBuf::from("/a/doc.md"),
                recovery_url: PathBuf::from("/a/.downright-save-X"),
                underlying: io::Error::other("x"),
            }
            .to_string(),
            "doc.md could not be reconciled while it was being saved. The external bytes were preserved at .downright-save-X."
        );
        // `NSError(domain: NSPOSIXErrorDomain, …).localizedDescription`.
        assert_eq!(
            PosixError { code: 2, path: PathBuf::from("/x/doc.md") }.to_string(),
            "The operation couldn\u{2019}t be completed. No such file or directory"
        );
        assert_eq!(
            PosixError { code: 17, path: PathBuf::from("/x/doc.md") }.to_string(),
            "The operation couldn\u{2019}t be completed. File exists"
        );
    }

    #[test]
    fn last_path_component_matches_url() {
        assert_eq!(last_path_component(Path::new("/")), "/");
        assert_eq!(last_path_component(Path::new("/a/b/")), "b");
        assert_eq!(last_path_component(Path::new("/a/..")), "..");
        assert_eq!(last_path_component(Path::new("/a/doc.md")), "doc.md");
    }

    #[test]
    fn sniffing_needs_a_nul_structure() {
        assert_eq!(DocumentIO::sniff_bomless_utf16_or_32(b"abc"), None);
        assert_eq!(DocumentIO::sniff_bomless_utf16_or_32(b"a\0b\0"), Some(Utf16LE));
        assert_eq!(DocumentIO::sniff_bomless_utf16_or_32(b"\0a\0b"), Some(Utf16BE));
        // ASCII UTF-32 has one parity at 25% NULs and the other at 50%: no
        // rule fires (0.5 is not > 2 × 0.25), exactly as in Downright.
        assert_eq!(DocumentIO::sniff_bomless_utf16_or_32(b"a\0\0\0b\0\0\0"), None);
        assert_eq!(DocumentIO::sniff_bomless_utf16_or_32(b"\0\0\0a\0\0\0b"), None);
        // Both parities dense: UTF-32, byte order from the first non-NUL.
        let mut be = [0u8; 32];
        be[3] = b'a';
        assert_eq!(DocumentIO::sniff_bomless_utf16_or_32(&be), Some(Utf32BE));
        let mut le = [0u8; 32];
        le[4] = b'a';
        assert_eq!(DocumentIO::sniff_bomless_utf16_or_32(&le), Some(Utf32LE));
        assert_eq!(DocumentIO::sniff_bomless_utf16_or_32(b"plain text"), None);
    }
}
