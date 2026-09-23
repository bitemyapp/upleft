//! The Foundation file-system calls Downright's command-line tools and app
//! integrations make, called through objc2 so their behaviour and, above
//! all, their error text (`error.localizedDescription`, which `down` prints)
//! are Foundation's own.
//!
//! Where Swift's own implementation differs from the `NSString`/`NSData`
//! method a Rust caller could reach, the difference is reproduced here and
//! the probe that recorded it is quoted.

use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::AnyObject;
use objc2_foundation::{
    NSCocoaErrorDomain, NSData, NSDataWritingOptions, NSDate, NSDictionary, NSError, NSFileHandle, NSFileManager,
    NSFileModificationDate, NSFileSize, NSFileType, NSNumber, NSString, NSUUID,
};

use crate::url::FileUrl;

/// An `NSError` as a Swift `catch` sees it: its domain, code and
/// `localizedDescription`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoundationError {
    pub domain: String,
    pub code: isize,
    pub description: String,
}

impl FoundationError {
    pub fn from_ns(error: &NSError) -> FoundationError {
        FoundationError {
            domain: error.domain().to_string(),
            code: error.code(),
            description: error.localizedDescription().to_string(),
        }
    }

    /// `CocoaError(code)` with no user info, as Swift throws it.
    pub fn cocoa(code: isize) -> FoundationError {
        autoreleasepool(|_| {
            let error = unsafe { NSError::errorWithDomain_code_userInfo(NSCocoaErrorDomain, code, None) };
            FoundationError::from_ns(&error)
        })
    }

    /// `error as? CocoaError` with this code.
    pub fn is_cocoa(&self, code: isize) -> bool {
        self.domain == "NSCocoaErrorDomain" && self.code == code
    }
}

/// `CocoaError.Code` values the ports test for.
pub mod cocoa_code {
    pub const FILE_READ_NO_SUCH_FILE: isize = 260;
    pub const FILE_READ_CORRUPT_FILE: isize = 259;
    pub const FILE_READ_UNSUPPORTED_SCHEME: isize = 262;
}

/// `NSHomeDirectory()`: honours `CFFIXED_USER_HOME`, ignores `HOME`.
pub fn ns_home_directory() -> String {
    objc2_foundation::NSHomeDirectory().to_string()
}

/// `FileManager.default.homeDirectoryForCurrentUser`.
pub fn home_directory_for_current_user() -> FileUrl {
    let url = NSFileManager::defaultManager().homeDirectoryForCurrentUser();
    FileUrl::from_nsurl(&url).unwrap_or_else(|| FileUrl::from_path(&ns_home_directory()))
}

/// `FileManager.default.temporaryDirectory` (which ignores `TMPDIR`).
pub fn temporary_directory() -> FileUrl {
    let url = NSFileManager::defaultManager().temporaryDirectory();
    FileUrl::from_nsurl(&url).unwrap_or_else(|| FileUrl::from_path_is_directory("/tmp", true))
}

/// `UUID().uuidString`.
pub fn uuid_string() -> String {
    NSUUID::UUID().UUIDString().to_string()
}

/// `FileManager.default.fileExists(atPath:)`.
pub fn file_exists(path: &str) -> bool {
    NSFileManager::defaultManager().fileExistsAtPath(&NSString::from_str(path))
}

/// `FileManager.default.fileExists(atPath:isDirectory:)`: `(exists, isDirectory)`.
pub fn file_exists_is_directory(path: &str) -> (bool, bool) {
    let mut directory = objc2::runtime::Bool::NO;
    let exists = unsafe {
        NSFileManager::defaultManager().fileExistsAtPath_isDirectory(&NSString::from_str(path), &mut directory)
    };
    (exists, directory.as_bool())
}

/// `FileManager.default.isExecutableFile(atPath:)`.
pub fn is_executable_file(path: &str) -> bool {
    NSFileManager::defaultManager().isExecutableFileAtPath(&NSString::from_str(path))
}

/// `try? FileManager.default.destinationOfSymbolicLink(atPath:)`.
pub fn destination_of_symbolic_link(path: &str) -> Option<String> {
    NSFileManager::defaultManager()
        .destinationOfSymbolicLinkAtPath_error(&NSString::from_str(path))
        .ok()
        .map(|destination| destination.to_string())
}

/// The members of `FileManager.attributesOfItem(atPath:)` the ports read.
#[derive(Clone, Debug, PartialEq)]
pub struct ItemAttributes {
    /// `attributes[.type]`, e.g. `NSFileTypeRegular`.
    pub file_type: Option<String>,
    /// `attributes[.size] as? NSNumber`, as `uint64Value`.
    pub size: Option<u64>,
    /// `attributes[.size] as? Int`.
    pub size_int: Option<i64>,
    /// `attributes[.modificationDate] as? Date`, seconds since 2001.
    pub modification_date: Option<f64>,
}

/// `FileManager.default.attributesOfItem(atPath:)` (which does not follow a
/// final symbolic link).
pub fn attributes_of_item(path: &str) -> Result<ItemAttributes, FoundationError> {
    autoreleasepool(|_| {
        let attributes = NSFileManager::defaultManager()
            .attributesOfItemAtPath_error(&NSString::from_str(path))
            .map_err(|error| FoundationError::from_ns(&error))?;
        let get = |key: &NSString| -> Option<Retained<AnyObject>> { attributes.objectForKey(key) };
        let file_type = get(unsafe { NSFileType }).and_then(|value| value.downcast::<NSString>().ok()).map(|s| s.to_string());
        let number = get(unsafe { NSFileSize }).and_then(|value| value.downcast::<NSNumber>().ok());
        let size = number.as_ref().map(|number| number.unsignedLongLongValue());
        let size_int = number.as_ref().map(|number| number.longLongValue());
        let modification_date = get(unsafe { NSFileModificationDate })
            .and_then(|value| value.downcast::<NSDate>().ok())
            .map(|date| date.timeIntervalSinceReferenceDate());
        Ok(ItemAttributes { file_type, size, size_int, modification_date })
    })
}

/// `NSFileTypeRegular`.
pub fn file_type_regular() -> String {
    unsafe { objc2_foundation::NSFileTypeRegular }.to_string()
}

/// `FileHandle(forReadingFrom:)` then `read(upToCount:)`, the way
/// `MarkdownCLI.loadSettings` reads a bounded file. The error of whichever
/// call failed is returned; the handle is closed either way (`try? close()`).
pub fn read_up_to_count(url: &FileUrl, count: usize) -> Result<Vec<u8>, FoundationError> {
    autoreleasepool(|_| {
        let handle = NSFileHandle::fileHandleForReadingFromURL_error(&url.to_nsurl())
            .map_err(|error| FoundationError::from_ns(&error))?;
        let result = handle.readDataUpToLength_error(count).map(|data| data.to_vec());
        let _ = handle.closeAndReturnError();
        result.map_err(|error| FoundationError::from_ns(&error))
    })
}

/// `data.write(to: url, options: .atomic)`.
pub fn write_atomically(bytes: &[u8], url: &FileUrl) -> Result<(), FoundationError> {
    autoreleasepool(|_| {
        NSData::with_bytes(bytes)
            .writeToURL_options_error(&url.to_nsurl(), NSDataWritingOptions::Atomic)
            .map_err(|error| FoundationError::from_ns(&error))
    })
}

/// `FileManager.default.createDirectory(at:withIntermediateDirectories:)`.
pub fn create_directory(url: &FileUrl, intermediates: bool) -> Result<(), FoundationError> {
    autoreleasepool(|_| unsafe {
        NSFileManager::defaultManager()
            .createDirectoryAtURL_withIntermediateDirectories_attributes_error(&url.to_nsurl(), intermediates, None)
            .map_err(|error| FoundationError::from_ns(&error))
    })
}

/// `String(contentsOf: url, encoding: .utf8)`.
///
/// Swift 6.4 on macOS 26 reads the file itself (swift-foundation) and, unlike
/// `NSString(contentsOf:encoding:)`, reports undecodable bytes as a bare
/// `CocoaError(.fileReadCorruptFile)` ("The file couldn’t be opened because
/// it isn’t in the correct format.", code 259, no user info) where `NSString`
/// says code 261 and names the file. A read failure (a directory, no
/// permission) is the same error from both, so it is taken from `NSString`.
/// A leading UTF-8 byte-order mark is dropped.
pub fn string_contents_of_utf8(url: &FileUrl) -> Result<String, FoundationError> {
    match std::fs::read(url.path()) {
        Ok(data) => {
            let body = data.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(&data);
            String::from_utf8(body.to_vec()).map_err(|_| FoundationError::cocoa(cocoa_code::FILE_READ_CORRUPT_FILE))
        }
        Err(_) => autoreleasepool(|_| {
            let result =
                NSString::stringWithContentsOfURL_encoding_error(&url.to_nsurl(), objc2_foundation::NSUTF8StringEncoding);
            match result {
                Ok(string) => Ok(string.to_string()),
                Err(error) => Err(FoundationError::from_ns(&error)),
            }
        }),
    }
}

/// `try? Data(contentsOf: url)`.
pub fn data_contents_of(url: &FileUrl) -> Option<Vec<u8>> {
    std::fs::read(url.path()).ok()
}

/// `FileManager.default.enumerator(atPath:)`, driven the way Downright
/// drives it: `visit` receives each relative path in enumeration order and
/// returns `true` to `skipDescendants()`. `None` when Foundation returns no
/// enumerator.
pub fn enumerate_directory(path: &str, mut visit: impl FnMut(&str) -> bool) -> Option<()> {
    autoreleasepool(|_| {
        let enumerator = NSFileManager::defaultManager().enumeratorAtPath(&NSString::from_str(path))?;
        loop {
            let item = autoreleasepool(|_| {
                let next = enumerator.nextObject()?;
                Some(next.to_string())
            });
            let Some(item) = item else { break };
            if visit(&item) {
                enumerator.skipDescendants();
            }
        }
        Some(())
    })
}

/// `NSDictionary` values reached by key, for `[String: Any]` casts on
/// property lists and JSON objects handed back by Foundation.
pub fn dictionary_get(dictionary: &NSDictionary<NSString, AnyObject>, key: &str) -> Option<Retained<AnyObject>> {
    dictionary.objectForKey(&NSString::from_str(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Swift 6.4, macOS 26:
    //   try String(contentsOf: fileWithBytes([0x61, 0xFF, 0x62]), encoding: .utf8)
    //   → NSCocoaErrorDomain 259, userInfo [], "The file couldn’t be opened
    //     because it isn’t in the correct format."
    #[test]
    fn undecodable_utf8_is_a_bare_corrupt_file_error() {
        let directory = std::env::temp_dir().join(format!("upleft-foundation-io-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("bad.md");
        std::fs::write(&path, [0x61, 0xFF, 0x62]).unwrap();
        let error = string_contents_of_utf8(&FileUrl::from_path(path.to_str().unwrap())).unwrap_err();
        assert_eq!(error.code, 259);
        assert_eq!(error.description, "The file couldn’t be opened because it isn’t in the correct format.");
        std::fs::write(&path, [0xEF, 0xBB, 0xBF, 0x61]).unwrap();
        assert_eq!(string_contents_of_utf8(&FileUrl::from_path(path.to_str().unwrap())).unwrap(), "a");
        let error = string_contents_of_utf8(&FileUrl::from_path(directory.to_str().unwrap())).unwrap_err();
        assert_eq!(error.code, 256);
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
