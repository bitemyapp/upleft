//! DocumentIOTests.swift — §3.1 is the app's central guarantee, so it is
//! tested against real files on disk: read → parse → write must return the
//! identical bytes.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use common::corpus;
use upleft_core::document_io::{DocumentIO, DocumentIOError, data_using, write_data_atomically};
use upleft_core::model::{LineEnding, TextEncodingKind};
use upleft_core::parser::MarkdownParser;

/// `temporaryDirectory()` plus its `defer { removeItem }`.
struct TemporaryDirectory(PathBuf);

impl TemporaryDirectory {
    fn new() -> TemporaryDirectory {
        let url = std::env::temp_dir().join(format!("MarkdownCoreTests-{}", uuid::Uuid::new_v4().hyphenated().to_string().to_uppercase()));
        fs::create_dir_all(&url).unwrap();
        TemporaryDirectory(url)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The complete pipeline: bytes → read → parse → write → bytes.
fn assert_round_trip(data: &[u8], label: &str) {
    let directory = TemporaryDirectory::new();
    let url = directory.path().join("doc.md");
    fs::write(&url, data).unwrap();

    let (text, fidelity) = DocumentIO::read(&url).unwrap();
    let parsed = MarkdownParser::parse(&text);
    assert_eq!(parsed.text, text, "{label}: parse must not touch the text");

    let output = directory.path().join("out.md");
    DocumentIO::write(&parsed.text, &output, fidelity).unwrap();
    let written = fs::read(&output).unwrap();
    assert!(written == data, "{label}: round trip changed {} bytes into {}", data.len(), written.len());
}

/// `assert_round_trip` without the parse step (extra: exercises the IO half
/// until the parser lands).
fn assert_io_round_trip(data: &[u8], label: &str) {
    let directory = TemporaryDirectory::new();
    let url = directory.path().join("doc.md");
    fs::write(&url, data).unwrap();

    let (text, fidelity) = DocumentIO::read(&url).unwrap();
    let output = directory.path().join("out.md");
    DocumentIO::write(&text, &output, fidelity).unwrap();
    let written = fs::read(&output).unwrap();
    assert!(written == data, "{label}: round trip changed {} bytes into {}", data.len(), written.len());
}

fn concat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

#[test]
#[ignore = "needs parser (upleft-markup)"]
fn round_trips_every_corpus_document() {
    for (name, text) in corpus::ALL {
        assert_round_trip(text.as_bytes(), name);
    }
}

#[test]
#[ignore = "needs parser (upleft-markup)"]
fn round_trips_crlf_and_cr_files() {
    assert_round_trip(b"# A\r\n\r\nB\r\n", "crlf");
    assert_round_trip(b"# A\rB\r", "cr");
    // Mixed endings are left alone on purpose — there is no `.mixed` case.
    assert_round_trip(b"a\nb\r\nc\rd\n", "mixed");
}

#[test]
#[ignore = "needs parser (upleft-markup)"]
fn round_trips_without_trailing_newline() {
    assert_round_trip(corpus::NO_TRAILING_NEWLINE.as_bytes(), "noTrailingNewline");
    assert_round_trip(b"x", "single character");
}

#[test]
#[ignore = "needs parser (upleft-markup)"]
fn round_trips_bom_and_utf16() {
    let bom: &[u8] = &[0xEF, 0xBB, 0xBF];
    assert_round_trip(&concat(&[bom, b"# Title\n\nBody.\n"]), "utf8 BOM");

    let utf16 = concat(&[&[0xFF, 0xFE], &data_using("# Title\n\nBody with ü.\n", TextEncodingKind::Utf16LE).unwrap()]);
    assert_round_trip(&utf16, "utf16 LE BOM");
}

#[test]
#[ignore = "needs parser (upleft-markup)"]
fn round_trips_tabs_and_trailing_spaces() {
    assert_round_trip(b"\t\tdeep\n  \n trailing   \n\n\n", "whitespace");
}

#[test]
fn fidelity_records_what_it_saw() {
    let directory = TemporaryDirectory::new();
    let url = directory.path().join("doc.md");
    fs::write(&url, b"a\r\nb\r\n").unwrap();

    let (text, fidelity) = DocumentIO::read(&url).unwrap();
    assert_eq!(text, "a\nb\n");
    assert_eq!(fidelity.line_ending, LineEnding::Crlf);
    assert!(fidelity.has_trailing_newline);
    assert!(!fidelity.has_bom);
    assert_eq!(fidelity.encoding, TextEncodingKind::Utf8);
}

#[test]
fn mixed_endings_are_not_normalised() {
    assert_eq!(DocumentIO::dominant_line_ending("a\nb\r\n"), LineEnding::Lf);
    assert_eq!(DocumentIO::dominant_line_ending("a\r\nb\r\n"), LineEnding::Crlf);
    assert_eq!(DocumentIO::dominant_line_ending("a\rb\r"), LineEnding::Cr);
    assert_eq!(DocumentIO::dominant_line_ending("no breaks"), LineEnding::Lf);
}

#[test]
#[ignore = "needs parser (upleft-markup)"]
fn latin1_falls_back_when_utf8_fails() {
    let directory = TemporaryDirectory::new();
    let url = directory.path().join("doc.md");
    // 0xE9 is `é` in Latin-1 and an invalid lone byte in UTF-8.
    let data = [0x63, 0x61, 0x66, 0xE9, 0x0A];
    fs::write(&url, data).unwrap();

    let (text, fidelity) = DocumentIO::read(&url).unwrap();
    assert_eq!(fidelity.encoding, TextEncodingKind::Latin1);
    assert_eq!(text, "café\n");
    assert_round_trip(&data, "latin1");
}

#[test]
fn content_hash_is_stable_and_sensitive() {
    let a = DocumentIO::content_hash("hello");
    assert_eq!(a, DocumentIO::content_hash("hello"));
    assert_ne!(a, DocumentIO::content_hash("hello "));
    assert_eq!(upleft_core::swift_text::count(&a), 64);
    assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    // Known SHA-256 of "hello", so a change of algorithm is caught.
    assert_eq!(a, "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824");
}

#[test]
fn read_head_reads_only_the_requested_bytes() {
    let directory = TemporaryDirectory::new();
    let url = directory.path().join("big.md");
    // 4MB, far beyond the thumbnail's 64KB head.
    fs::write(&url, vec![0x61u8; 4 * 1024 * 1024]).unwrap();

    let head = DocumentIO::read_head(&url, 64 * 1024);
    assert!(head.is_some());
    let head = head.unwrap();
    assert!(head.len() <= 64 * 1024);
    assert_eq!(head, "a".repeat(64 * 1024));
}

#[test]
fn read_head_trims_a_split_multibyte_scalar() {
    let directory = TemporaryDirectory::new();
    let url = directory.path().join("split.md");
    // "🌊" is four UTF-8 bytes; a limit that splits it mid-scalar must not
    // return an undecodable head.
    let text = "🌊".repeat(20) + "tail";
    fs::write(&url, text.as_bytes()).unwrap();

    let head = DocumentIO::read_head(&url, 30);
    let expected = "🌊".repeat(7); // 28 bytes
    assert_eq!(head.as_deref(), Some(expected.as_str()));
    assert_eq!(head.map(|h| h.len()), Some(28));
}

/// The bounded head must agree with a full decode about the encoding.
#[test]
fn read_head_matches_full_decode_for_wide_encodings() {
    let directory = TemporaryDirectory::new();
    let text = "# Título\n\nContenido — con acentos.\n";
    // `text.data(using: .utf16)` is `FF FE` + little-endian units (probed).
    let utf16_le = data_using(text, TextEncodingKind::Utf16LE).unwrap();
    let cases: [(&str, Vec<u8>); 4] = [
        ("bom16.md", concat(&[&[0xFF, 0xFE], &utf16_le])),
        ("bomless16.md", utf16_le.clone()),
        ("bom8.md", concat(&[&[0xEF, 0xBB, 0xBF], text.as_bytes()])),
        ("plain8.md", text.as_bytes().to_vec()),
    ];
    for (name, data) in cases {
        let url = directory.path().join(name);
        fs::write(&url, &data).unwrap();
        let full = DocumentIO::decode_snapshot(&data, &url).unwrap().0;
        let head = DocumentIO::read_head(&url, 64 * 1024).expect("head");
        assert_eq!(head, full, "{name}: head must decode like a full read");
    }
}

#[test]
fn read_head_returns_nil_for_missing_file() {
    let path = PathBuf::from(format!("/nonexistent/{}.md", uuid::Uuid::new_v4().hyphenated().to_string().to_uppercase()));
    assert_eq!(DocumentIO::read_head(&path, 1024), None);
}

#[test]
#[ignore = "needs parser (upleft-markup)"]
fn round_trips_utf32_bom() {
    let directory = TemporaryDirectory::new();
    let url = directory.path().join("doc.md");

    let le = concat(&[&[0xFF, 0xFE, 0x00, 0x00], &data_using("# Title\n\nBody.\n", TextEncodingKind::Utf32LE).unwrap()]);
    fs::write(&url, &le).unwrap();
    let (text_le, fidelity_le) = DocumentIO::read(&url).unwrap();
    assert_eq!(text_le, "# Title\n\nBody.\n");
    assert_eq!(fidelity_le.encoding, TextEncodingKind::Utf32LE);
    assert!(fidelity_le.has_bom);
    assert_round_trip(&le, "utf32 LE BOM");

    let be = concat(&[&[0x00, 0x00, 0xFE, 0xFF], &data_using("# Title\n", TextEncodingKind::Utf32BE).unwrap()]);
    fs::write(&url, &be).unwrap();
    let (text_be, _) = DocumentIO::read(&url).unwrap();
    assert_eq!(text_be, "# Title\n");
    assert_round_trip(&be, "utf32 BE BOM");
}

#[test]
fn reads_truncated_utf16_and_32() {
    let directory = TemporaryDirectory::new();
    let url = directory.path().join("doc.md");

    // UTF-16 LE body with one torn trailing byte: must decode up to the last
    // whole code unit instead of throwing.
    let le = concat(&[&[0xFF, 0xFE], &data_using("# Abc\n", TextEncodingKind::Utf16LE).unwrap(), &[0x41]]);
    fs::write(&url, &le).unwrap();
    let (text, _) = DocumentIO::read(&url).unwrap();
    assert_eq!(text, "# Abc\n");

    // UTF-32 LE body with three stray trailing bytes.
    let utf32 = concat(&[&[0xFF, 0xFE, 0x00, 0x00], &data_using("Hi\n", TextEncodingKind::Utf32LE).unwrap(), &[0x01, 0x02]]);
    fs::write(&url, &utf32).unwrap();
    let (text32, _) = DocumentIO::read(&url).unwrap();
    assert_eq!(text32, "Hi\n");
}

#[test]
#[ignore = "needs parser (upleft-markup)"]
fn reads_bomless_utf16() {
    let directory = TemporaryDirectory::new();
    let url = directory.path().join("doc.md");

    let le_body = data_using("one\ntwo\n", TextEncodingKind::Utf16LE).unwrap();
    fs::write(&url, &le_body).unwrap();
    let (text_le, fidelity_le) = DocumentIO::read(&url).unwrap();
    assert_eq!(text_le, "one\ntwo\n");
    assert_eq!(fidelity_le.encoding, TextEncodingKind::Utf16LE);
    assert!(!fidelity_le.has_bom);
    assert_round_trip(&le_body, "utf16 LE no BOM");

    let be_body = data_using("one\n", TextEncodingKind::Utf16BE).unwrap();
    fs::write(&url, &be_body).unwrap();
    let (text_be, fidelity_be) = DocumentIO::read(&url).unwrap();
    assert_eq!(text_be, "one\n");
    assert_eq!(fidelity_be.encoding, TextEncodingKind::Utf16BE);
    assert_round_trip(&be_body, "utf16 BE no BOM");
}

#[test]
fn guarded_replace_restores_racing_generation() {
    let directory = TemporaryDirectory::new();
    let url = directory.path().join("doc.md");
    let opened = b"opened\n".to_vec();
    let first_external = b"external-one\n".to_vec();
    let newest_external = b"external-two\n".to_vec();
    fs::write(&url, &first_external).unwrap();

    let result = DocumentIO::replace_existing_atomically_for_testing(
        b"mine\n",
        &url,
        &opened,
        &mut || write_data_atomically(&newest_external, &url).unwrap(),
        None,
    );
    match result {
        Ok(()) => panic!("a mismatched generation must fail closed"),
        Err(error) => match error.downcast_ref::<DocumentIOError>() {
            Some(DocumentIOError::TargetChanged { displaced, .. }) => {
                assert!(displaced.contains(&first_external));
                assert!(displaced.contains(&newest_external));
            }
            _ => panic!("unexpected error: {error}"),
        },
    }
    assert_eq!(fs::read(&url).unwrap(), newest_external);
}

#[test]
fn failed_rollback_never_deletes_displaced_external_bytes() {
    let directory = TemporaryDirectory::new();
    let url = directory.path().join("doc.md");
    let external = b"external\n".to_vec();
    fs::write(&url, &external).unwrap();

    let result = DocumentIO::replace_existing_atomically_for_testing(
        b"mine\n",
        &url,
        b"opened\n",
        &mut || fs::remove_file(&url).unwrap(),
        None,
    );
    assert!(result.is_err());
    let recovery_files: Vec<PathBuf> = fs::read_dir(directory.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.file_name().unwrap().to_string_lossy().starts_with(".downright-save-"))
        .collect();
    assert_eq!(recovery_files.len(), 1);
    assert_eq!(fs::read(&recovery_files[0]).unwrap(), external);
}

#[test]
fn displaced_read_failure_leaves_recoverable_external_generation() {
    let directory = TemporaryDirectory::new();
    let url = directory.path().join("doc.md");
    let external = b"external-before-read-failure\n".to_vec();
    let mine = b"mine\n".to_vec();
    fs::write(&url, &external).unwrap();

    let result = DocumentIO::replace_existing_atomically_for_testing(
        &mine,
        &url,
        &external,
        &mut || {},
        Some(&mut |temporary: &Path| fs::set_permissions(temporary, fs::Permissions::from_mode(0o000)).unwrap()),
    );
    match result {
        Ok(()) => panic!("a displaced-generation read failure must fail closed"),
        Err(error) => match error.downcast_ref::<DocumentIOError>() {
            Some(DocumentIOError::DisplacedGenerationUnreadable { recovery_url, .. }) => {
                assert!(recovery_url.exists());
                fs::set_permissions(recovery_url, fs::Permissions::from_mode(0o600)).unwrap();
                assert_eq!(fs::read(recovery_url).unwrap(), external);
            }
            _ => panic!("unexpected error: {error}"),
        },
    }

    // The public path contains Downright's candidate, while the external
    // generation remains available at the recovery URL from the error.
    assert_eq!(fs::read(&url).unwrap(), mine);
}

// MARK: - Extras (not in DocumentIOTests.swift)

/// The parse-free half of every ignored round-trip test above.
#[test]
fn io_round_trips_without_the_parser() {
    for (name, text) in corpus::ALL {
        assert_io_round_trip(text.as_bytes(), name);
    }
    assert_io_round_trip(b"# A\r\n\r\nB\r\n", "crlf");
    assert_io_round_trip(b"# A\rB\r", "cr");
    assert_io_round_trip(b"a\nb\r\nc\rd\n", "mixed");
    assert_io_round_trip(b"x", "single character");
    assert_io_round_trip(b"\t\tdeep\n  \n trailing   \n\n\n", "whitespace");
    assert_io_round_trip(&concat(&[&[0xEF, 0xBB, 0xBF], b"# Title\n\nBody.\n"]), "utf8 BOM");
    assert_io_round_trip(
        &concat(&[&[0xFF, 0xFE], &data_using("# Title\n\nBody with ü.\n", TextEncodingKind::Utf16LE).unwrap()]),
        "utf16 LE BOM",
    );
    assert_io_round_trip(&[0x63, 0x61, 0x66, 0xE9, 0x0A], "latin1");
    assert_io_round_trip(
        &concat(&[&[0xFF, 0xFE, 0x00, 0x00], &data_using("# Title\n\nBody.\n", TextEncodingKind::Utf32LE).unwrap()]),
        "utf32 LE BOM",
    );
    assert_io_round_trip(
        &concat(&[&[0x00, 0x00, 0xFE, 0xFF], &data_using("# Title\n", TextEncodingKind::Utf32BE).unwrap()]),
        "utf32 BE BOM",
    );
    assert_io_round_trip(&data_using("one\ntwo\n", TextEncodingKind::Utf16LE).unwrap(), "utf16 LE no BOM");
    assert_io_round_trip(&data_using("one\n", TextEncodingKind::Utf16BE).unwrap(), "utf16 BE no BOM");
}

/// `Data.write(to:options: .atomic)` keeps an existing file's permission bits
/// and gives a new file 0666 & ~umask (probed).
#[test]
fn atomic_write_preserves_permissions() {
    let directory = TemporaryDirectory::new();
    let url = directory.path().join("doc.md");
    fs::write(&url, b"a").unwrap();
    fs::set_permissions(&url, fs::Permissions::from_mode(0o600)).unwrap();
    write_data_atomically(b"b", &url).unwrap();
    assert_eq!(fs::symlink_metadata(&url).unwrap().permissions().mode() & 0o7777, 0o600);
    assert_eq!(fs::read(&url).unwrap(), b"b");
    let leftovers = fs::read_dir(directory.path()).unwrap().count();
    assert_eq!(leftovers, 1);
}

#[test]
fn create_atomically_never_overwrites() {
    let directory = TemporaryDirectory::new();
    let url = directory.path().join("doc.md");
    DocumentIO::create_atomically(b"first", &url).unwrap();
    assert_eq!(fs::read(&url).unwrap(), b"first");
    let error = DocumentIO::create_atomically(b"second", &url).unwrap_err();
    assert_eq!(error.to_string(), "The operation couldn\u{2019}t be completed. File exists");
    assert_eq!(fs::read(&url).unwrap(), b"first");
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn guarded_replace_succeeds_on_the_expected_generation() {
    let directory = TemporaryDirectory::new();
    let url = directory.path().join("doc.md");
    fs::write(&url, b"opened\n").unwrap();
    fs::set_permissions(&url, fs::Permissions::from_mode(0o640)).unwrap();
    DocumentIO::replace_existing_atomically(b"mine\n", &url, b"opened\n").unwrap();
    assert_eq!(fs::read(&url).unwrap(), b"mine\n");
    assert_eq!(fs::symlink_metadata(&url).unwrap().permissions().mode() & 0o7777, 0o640);
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);

    // A missing target is never recreated.
    let missing = directory.path().join("missing.md");
    assert!(DocumentIO::replace_existing_atomically(b"x", &missing, b"").is_err());
    assert!(!missing.exists());
}

#[test]
fn unencodable_latin1_text_is_an_error() {
    let directory = TemporaryDirectory::new();
    let url = directory.path().join("doc.md");
    fs::write(&url, [0x63, 0x61, 0x66, 0xE9, 0x0A]).unwrap();
    let (text, fidelity) = DocumentIO::read(&url).unwrap();
    let edited = text + "€";
    let error = DocumentIO::write(&edited, &url, fidelity).unwrap_err();
    assert_eq!(error.to_string(), "The document contains characters that cannot be written as latin1.");
    // A decomposed é folds into Latin-1's precomposed byte, as Foundation does.
    DocumentIO::write("cafe\u{301}\n", &url, fidelity).unwrap();
    assert_eq!(fs::read(&url).unwrap(), [0x63, 0x61, 0x66, 0xE9, 0x0A]);
}
