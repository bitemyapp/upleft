//! The `FileManager` and `Data` file calls Downright's stores make, through
//! Foundation itself wherever the outcome depends on it (atomic writes,
//! zlib compression, directory enumeration order), so the bytes on disk and
//! the order entries come back in are Foundation's.

use objc2::rc::Retained;
use objc2_foundation::{
    NSData, NSDataCompressionAlgorithm, NSDataWritingOptions, NSDirectoryEnumerationOptions, NSFileManager, NSString,
    NSURL,
};

use crate::date::Date;
use crate::url::FileUrl;

/// `NSTemporaryDirectory()`.
pub fn temporary_directory() -> String {
    objc2_foundation::NSTemporaryDirectory().to_string()
}

/// `Data(contentsOf: url)`: the file's bytes, or `None` where Swift throws
/// (missing, unreadable, a directory).
pub fn data_contents_of(url: &FileUrl) -> Option<Vec<u8>> {
    std::fs::read(url.path()).ok()
}

/// `data.write(to: url, options: .atomic)`, through `NSData`.
pub fn write_atomic(data: &[u8], url: &FileUrl) -> Result<(), String> {
    NSData::with_bytes(data)
        .writeToURL_options_error(&url.to_nsurl(), NSDataWritingOptions::Atomic)
        .map_err(|error| error.localizedDescription().to_string())
}

/// `data.write(to: url)` (no options), through `NSData`.
pub fn write(data: &[u8], url: &FileUrl) -> Result<(), String> {
    NSData::with_bytes(data)
        .writeToURL_options_error(&url.to_nsurl(), NSDataWritingOptions::empty())
        .map_err(|error| error.localizedDescription().to_string())
}

/// `FileManager.default.fileExists(atPath:)`.
pub fn file_exists(path: &str) -> bool {
    NSFileManager::defaultManager().fileExistsAtPath(&NSString::from_str(path))
}

/// `FileManager.default.fileExists(atPath:isDirectory:)`: `None` when
/// nothing is there, else whether it is a directory.
pub fn file_exists_is_directory(path: &str) -> Option<bool> {
    let mut is_directory = objc2::runtime::Bool::NO;
    let exists = unsafe {
        NSFileManager::defaultManager().fileExistsAtPath_isDirectory(&NSString::from_str(path), &mut is_directory)
    };
    exists.then(|| is_directory.as_bool())
}

/// `FileManager.default.isExecutableFile(atPath:)`.
pub fn is_executable_file(path: &str) -> bool {
    NSFileManager::defaultManager().isExecutableFileAtPath(&NSString::from_str(path))
}

/// `FileManager.default.removeItem(at:)`.
pub fn remove_item(url: &FileUrl) -> Result<(), String> {
    NSFileManager::defaultManager()
        .removeItemAtURL_error(&url.to_nsurl())
        .map_err(|error| error.localizedDescription().to_string())
}

/// `FileManager.default.moveItem(at:to:)`.
pub fn move_item(from: &FileUrl, to: &FileUrl) -> Result<(), String> {
    NSFileManager::defaultManager()
        .moveItemAtURL_toURL_error(&from.to_nsurl(), &to.to_nsurl())
        .map_err(|error| error.localizedDescription().to_string())
}

/// `FileManager.default.createDirectory(at:withIntermediateDirectories:)`.
pub fn create_directory(url: &FileUrl, with_intermediate_directories: bool) -> Result<(), String> {
    unsafe {
        NSFileManager::defaultManager().createDirectoryAtURL_withIntermediateDirectories_attributes_error(
            &url.to_nsurl(),
            with_intermediate_directories,
            None,
        )
    }
    .map_err(|error| error.localizedDescription().to_string())
}

fn wrap(url: &NSURL) -> Option<FileUrl> {
    FileUrl::from_nsurl(url)
}

/// `FileManager.default.contentsOfDirectory(at:includingPropertiesForKeys: nil)`,
/// in Foundation's order.
pub fn contents_of_directory(url: &FileUrl) -> Result<Vec<FileUrl>, String> {
    let urls = NSFileManager::defaultManager()
        .contentsOfDirectoryAtURL_includingPropertiesForKeys_options_error(
            &url.to_nsurl(),
            None,
            NSDirectoryEnumerationOptions::empty(),
        )
        .map_err(|error| error.localizedDescription().to_string())?;
    Ok(urls.iter().filter_map(|entry| wrap(&entry)).collect())
}

/// Every URL `FileManager.default.enumerator(at:includingPropertiesForKeys:)`
/// yields, in its order, or `None` when Swift gets `nil`.
pub fn enumerate(url: &FileUrl) -> Option<Vec<FileUrl>> {
    let enumerator: Retained<_> = NSFileManager::defaultManager().enumeratorAtURL_includingPropertiesForKeys_options_errorHandler(
        &url.to_nsurl(),
        None,
        NSDirectoryEnumerationOptions::empty(),
        None,
    )?;
    let mut out = Vec::new();
    while let Some(entry) = enumerator.nextObject() {
        if let Some(entry) = wrap(&entry) {
            out.push(entry);
        }
    }
    Some(out)
}

/// The resource values the stores read (`.fileSizeKey`,
/// `.contentModificationDateKey`, `.isRegularFileKey`), for the item itself
/// (a symbolic link is not followed).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResourceValues {
    pub is_regular_file: bool,
    /// `nil` for a directory.
    pub file_size: Option<i64>,
    pub content_modification_date: Option<Date>,
}

/// `url.resourceValues(forKeys:)`, or `None` where Swift throws.
pub fn resource_values(url: &FileUrl) -> Option<ResourceValues> {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::symlink_metadata(url.path()).ok()?;
    let is_directory = metadata.is_dir();
    // `NSDate` from a `timespec`: seconds since the reference date plus
    // nanoseconds scaled, as CoreFoundation computes it.
    let modified = (metadata.mtime() - 978_307_200) as f64 + metadata.mtime_nsec() as f64 * 1.0e-9;
    Some(ResourceValues {
        is_regular_file: metadata.file_type().is_file(),
        file_size: if is_directory { None } else { Some(metadata.len() as i64) },
        content_modification_date: Some(Date::from_reference(modified)),
    })
}

/// `(data as NSData).compressed(using: .zlib)`.
pub fn compressed_zlib(data: &[u8]) -> Option<Vec<u8>> {
    NSData::with_bytes(data).compressedDataUsingAlgorithm_error(NSDataCompressionAlgorithm::Zlib).ok().map(|out| out.to_vec())
}

/// `(data as NSData).decompressed(using: .zlib)`.
pub fn decompressed_zlib(data: &[u8]) -> Option<Vec<u8>> {
    NSData::with_bytes(data).decompressedDataUsingAlgorithm_error(NSDataCompressionAlgorithm::Zlib).ok().map(|out| out.to_vec())
}

/// `String(data: data, encoding: .utf8)`: `nil` for invalid UTF-8, and one
/// leading byte-order mark dropped (probed: `EF BB BF EF BB BF 61` reads as
/// `"\u{FEFF}a"`).
pub fn string_from_utf8_data(data: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(data).ok()?;
    Some(text.strip_prefix('\u{FEFF}').unwrap_or(text).to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Swift 6.4, macOS 26: `(Data("hello hello hello".utf8) as NSData).compressed(using: .zlib)`.
    #[test]
    fn zlib_matches_foundation() {
        let hex = |bytes: Vec<u8>| bytes.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
        assert_eq!(hex(compressed_zlib(b"hello hello hello").unwrap()), "cb48cdc9c957c8409000");
        assert_eq!(hex(compressed_zlib(b"").unwrap()), "0300");
        assert_eq!(decompressed_zlib(b""), None);
        assert_eq!(decompressed_zlib(&compressed_zlib(b"abc").unwrap()).unwrap(), b"abc");
    }

    #[test]
    fn utf8_strings_drop_one_byte_order_mark() {
        assert_eq!(string_from_utf8_data(&[0xEF, 0xBB, 0xBF, 0x61]).as_deref(), Some("a"));
        assert_eq!(string_from_utf8_data(&[0xEF, 0xBB, 0xBF, 0xEF, 0xBB, 0xBF, 0x61]).as_deref(), Some("\u{FEFF}a"));
        assert_eq!(string_from_utf8_data(&[0x61, 0xEF, 0xBB, 0xBF]).as_deref(), Some("a\u{FEFF}"));
        assert_eq!(string_from_utf8_data(&[0xFF]), None);
        assert_eq!(string_from_utf8_data(&[0xED, 0xA0, 0x80]), None);
    }
}
