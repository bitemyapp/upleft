//! Swift's `Foundation.URL` for `file:` URLs, as swift-foundation implements
//! it on macOS 26. Every Downright path that reaches a file name, a hash, a
//! JSON file or the terminal goes through `URL`, so the port needs its exact
//! behaviour, which is not `NSURL`'s in every case.
//!
//! Recorded from Swift 6.4 on macOS 26 (`tests/url.rs` replays the probe):
//!
//! * `URL(fileURLWithPath:)` expands a leading `~` (`NSURL` does not),
//!   resolves a relative path against the current directory and removes its
//!   dot segments (RFC 3986 reference resolution; an absolute path keeps them),
//!   converts the path to its file-system representation (`é` becomes
//!   `e` + U+0301, so `.path` returns decomposed text), and, when the path
//!   does not end in `/`, asks the file system whether it names a directory,
//!   in which case the URL gets a directory path (trailing `/`).
//! * `appendingPathComponent(_:)` drops the component's leading `/`, treats an
//!   empty component as "make this a directory path", converts to the file
//!   system representation, and asks the file system about directories too.
//! * `deletingLastPathComponent()` leaves `/` and a trailing `..` alone and
//!   collapses the slashes it exposes (`NSURL` appends `../` instead).
//! * The directory checks use `lstat`: a symbolic link to a directory is not
//!   a directory path. A last component of `.` or `..` always is.
//! * A URL made from a relative path stays relative to the current directory:
//!   appending `..` or `.` to it resolves the dot segment away.
//! * `standardizedFileURL`, `resolvingSymlinksInPath()` and `pathExtension`
//!   compute the same path as `NSURL`, so `NSURL` computes it; the result keeps
//!   the receiver's directory flag (`NSURL` re-checks and follows links).
//! * `absoluteString` percent-encodes everything but RFC 3986 `pchar`s and `/`
//!   (`NSURL` also encodes `;`).
//!
//! Not reproduced: `appendingPathExtension` on a URL made from the relative
//! path `""`, `.` or `..` (Swift appends to the relative string, giving
//! `/cwd/..md`). `tests/url.rs` skips exactly these cases.
//!
//! [`FileUrl`] stores the URL's path exactly as the URL spells it (decoded,
//! trailing slashes kept), so equality and hashing follow Swift's `URL ==`,
//! which compares the whole URL string: `/a/dir` and `/a/dir/` differ.

use objc2::rc::Retained;
use objc2_foundation::{NSFileManager, NSString, NSURL};

/// A `file:` URL with Swift `URL` semantics.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileUrl {
    /// The URL's path component, percent-decoded, trailing slashes kept.
    url_path: String,
    /// Made from a relative path: Swift keeps such a URL relative to the
    /// current directory, so a component appended later (`..`, `.`) is
    /// resolved away rather than kept.
    relative: bool,
}

/// `(path as NSString).fileSystemRepresentation`, back as a `String`.
pub fn file_system_representation(path: &str) -> String {
    if path.is_ascii() {
        return path.to_owned();
    }
    let string = NSString::from_str(path);
    let pointer = string.fileSystemRepresentation();
    unsafe { std::ffi::CStr::from_ptr(pointer.as_ptr()) }.to_string_lossy().into_owned()
}

/// `(path as NSString).expandingTildeInPath`.
pub fn expanding_tilde_in_path(path: &str) -> String {
    NSString::from_str(path).stringByExpandingTildeInPath().to_string()
}

/// `FileManager.default.currentDirectoryPath`.
pub fn current_directory_path() -> String {
    NSFileManager::defaultManager().currentDirectoryPath().to_string()
}

/// Whether `path` names a directory, as `URL` asks: `lstat`, so a symbolic
/// link to a directory is not one.
fn is_directory(path: &str) -> bool {
    std::fs::symlink_metadata(path).map(|metadata| metadata.is_dir()).unwrap_or(false)
}

/// Percent-encodes a URL path as Swift's `URL` does: RFC 3986 `pchar`s and
/// `/` stay, everything else is `%XX` over UTF-8 with upper-case hex. (`NSURL`
/// also encodes `;`.)
fn percent_encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for byte in path.bytes() {
        let keep = byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'-' | b'.' | b'_' | b'~' | b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b',' | b';' | b'='
                    | b':' | b'@' | b'/'
            );
        if keep {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Whether a path's last component is `.` or `..`, which `URL` always
/// treats as a directory.
fn ends_in_dot_component(path: &str) -> bool {
    let last = path.rsplit('/').next().unwrap_or("");
    last == "." || last == ".."
}

/// RFC 3986 `remove_dot_segments`.
fn remove_dot_segments(path: &str) -> String {
    let mut input = path.to_owned();
    let mut output = String::new();
    while !input.is_empty() {
        if let Some(rest) = input.strip_prefix("../") {
            input = rest.to_owned();
        } else if let Some(rest) = input.strip_prefix("./") {
            input = rest.to_owned();
        } else if input.starts_with("/./") {
            input = input[2..].to_owned();
        } else if input == "/." {
            input = "/".into();
        } else if input.starts_with("/../") || input == "/.." {
            input = if input == "/.." { "/".into() } else { input[3..].to_owned() };
            if let Some(index) = output.rfind('/') {
                output.truncate(index);
            } else {
                output.clear();
            }
        } else if input == "." || input == ".." {
            input.clear();
        } else {
            let start = usize::from(input.starts_with('/'));
            let end = input[start..].find('/').map_or(input.len(), |index| index + start);
            output.push_str(&input[..end]);
            input = input[end..].to_owned();
        }
    }
    output
}

impl FileUrl {
    /// `URL(fileURLWithPath:)`.
    pub fn from_path(path: &str) -> FileUrl {
        let mut url_path = Self::absolute(path);
        if !url_path.ends_with('/') && is_directory(&url_path) {
            url_path.push('/');
        }
        FileUrl { url_path, relative: Self::is_relative(path) }
    }

    fn is_relative(path: &str) -> bool {
        !path.starts_with('/') && !path.starts_with('~')
    }

    /// `URL(fileURLWithPath:isDirectory:)`: no file-system check.
    pub fn from_path_is_directory(path: &str, directory: bool) -> FileUrl {
        let mut url_path = Self::absolute(path);
        if directory {
            if !url_path.ends_with('/') {
                url_path.push('/');
            }
        } else if Self::is_relative(path) && (path.is_empty() || ends_in_dot_component(path)) {
            // A relative `""`, `.` or `..` still names a directory.
        } else if url_path.len() > 1 {
            let trimmed = url_path.trim_end_matches('/');
            url_path = if trimmed.is_empty() { "/".into() } else { trimmed.to_owned() };
        }
        FileUrl { url_path, relative: Self::is_relative(path) }
    }

    fn absolute(path: &str) -> String {
        let expanded = if path.starts_with('~') { expanding_tilde_in_path(path) } else { path.to_owned() };
        let absolute = if expanded.starts_with('/') {
            expanded
        } else {
            let mut base = current_directory_path();
            if !base.ends_with('/') {
                base.push('/');
            }
            let joined = format!("{base}{expanded}");
            let mut resolved = remove_dot_segments(&joined);
            if expanded.is_empty() || expanded == "." || expanded.ends_with("/.") || expanded == ".." || expanded.ends_with("/..") {
                if !resolved.ends_with('/') {
                    resolved.push('/');
                }
            }
            resolved
        };
        file_system_representation(&absolute)
    }

    /// `URL(fileURLWithPath:relativeTo:)`, for the path it names.
    ///
    /// Recorded from Swift 6.4 on macOS 26: a leading `~` is expanded, an
    /// absolute path ignores the base, and a relative one is merged with the
    /// base as RFC 3986 does (so a base without a trailing slash loses its
    /// last segment: `bin/down` against `/tmp` is `/bin/down`), with dot
    /// segments removed and the result in its file-system representation.
    /// The result is stored as an absolute URL; `path`, `standardizedFileURL`
    /// and `resolvingSymlinksInPath()` are what callers read from it.
    pub fn from_path_relative_to(path: &str, base: &FileUrl) -> FileUrl {
        let expanded = if path.starts_with('~') { expanding_tilde_in_path(path) } else { path.to_owned() };
        if expanded.starts_with('/') {
            let mut url = FileUrl::from_path(&expanded);
            url.relative = false;
            return url;
        }
        let directory = match base.url_path.rfind('/') {
            Some(index) => &base.url_path[..=index],
            None => "/",
        };
        let mut url_path = file_system_representation(&remove_dot_segments(&format!("{directory}{expanded}")));
        if !url_path.ends_with('/') && is_directory(&url_path) {
            url_path.push('/');
        }
        FileUrl { url_path, relative: false }
    }

    /// Wraps an `NSURL` that Foundation handed back (a `file:` URL).
    pub fn from_nsurl(url: &NSURL) -> Option<FileUrl> {
        let path = url.path()?.to_string();
        let mut url_path = path;
        if url.hasDirectoryPath() && !url_path.ends_with('/') {
            url_path.push('/');
        }
        Some(FileUrl { url_path, relative: false })
    }

    /// The equivalent `NSURL`, for Foundation APIs that take one.
    pub fn to_nsurl(&self) -> Retained<NSURL> {
        NSURL::fileURLWithPath_isDirectory(&NSString::from_str(&self.path()), self.has_directory_path())
    }

    /// `url.path`: decoded, without trailing slashes (except the root).
    pub fn path(&self) -> String {
        let trimmed = self.url_path.trim_end_matches('/');
        if trimmed.is_empty() { "/".into() } else { trimmed.to_owned() }
    }

    /// The path as the URL spells it, trailing slash included.
    pub fn url_path(&self) -> &str {
        &self.url_path
    }

    /// `url.hasDirectoryPath`.
    pub fn has_directory_path(&self) -> bool {
        self.url_path.ends_with('/') || ends_in_dot_component(&self.url_path)
    }

    /// `url.absoluteString`.
    pub fn absolute_string(&self) -> String {
        format!("file://{}", percent_encode_path(&self.url_path))
    }

    /// `url.lastPathComponent`.
    pub fn last_path_component(&self) -> String {
        let path = self.path();
        if path == "/" {
            return "/".into();
        }
        path.rsplit('/').next().unwrap_or("").to_owned()
    }

    /// `url.pathComponents`.
    pub fn path_components(&self) -> Vec<String> {
        let mut components = vec!["/".to_owned()];
        components.extend(self.path().split('/').filter(|part| !part.is_empty()).map(str::to_owned));
        components
    }

    /// `url.pathExtension`.
    pub fn path_extension(&self) -> String {
        NSString::from_str(&self.last_path_component()).pathExtension().to_string()
    }

    /// `url.appendingPathComponent(_:)`.
    pub fn appending_path_component(&self, component: &str) -> FileUrl {
        let mut url = self.appending(component);
        if !url.url_path.ends_with('/') && is_directory(&url.url_path) {
            url.url_path.push('/');
        }
        url
    }

    /// `url.appendingPathComponent(_:isDirectory:)`.
    pub fn appending_path_component_is_directory(&self, component: &str, directory: bool) -> FileUrl {
        let mut url = self.appending(component);
        if directory && !url.url_path.ends_with('/') {
            url.url_path.push('/');
        }
        url
    }

    fn appending(&self, component: &str) -> FileUrl {
        let mut url_path = self.url_path.clone();
        if !url_path.ends_with('/') {
            url_path.push('/');
        }
        url_path.push_str(&file_system_representation(component.trim_start_matches('/')));
        if self.relative && ends_in_dot_component(&url_path) {
            url_path = remove_dot_segments(&url_path);
            if !url_path.ends_with('/') {
                url_path.push('/');
            }
        }
        FileUrl { url_path, relative: self.relative }
    }

    /// `url.deletingLastPathComponent()`.
    pub fn deleting_last_path_component(&self) -> FileUrl {
        let trimmed = self.url_path.trim_end_matches('/');
        if trimmed.is_empty() {
            return FileUrl { url_path: "/".into(), relative: self.relative };
        }
        let last = trimmed.rsplit('/').next().unwrap_or("");
        if last == ".." {
            return self.clone();
        }
        let parent = trimmed[..trimmed.len() - last.len()].trim_end_matches('/');
        FileUrl { url_path: format!("{parent}/"), relative: self.relative }
    }

    /// `url.deletingPathExtension()`.
    pub fn deleting_path_extension(&self) -> FileUrl {
        let extension = self.path_extension();
        if extension.is_empty() {
            return self.clone();
        }
        let directory = self.has_directory_path();
        let path = self.path();
        let mut url_path = path[..path.len() - extension.len() - 1].to_owned();
        if directory {
            url_path.push('/');
        }
        FileUrl { url_path, relative: false }
    }

    /// `url.appendingPathExtension(_:)`.
    pub fn appending_path_extension(&self, extension: &str) -> FileUrl {
        let directory = self.has_directory_path() && self.url_path != "/";
        let mut url_path = format!("{}.{}", self.path(), file_system_representation(extension));
        if directory {
            url_path.push('/');
        }
        FileUrl { url_path, relative: false }
    }

    /// `url.standardizedFileURL` (`standardized` for a file URL).
    ///
    /// The path comes from `NSURL`, which agrees with Swift on it; the
    /// result keeps this URL's directory flag, as Swift's does (`NSURL`
    /// re-asks the file system and follows symbolic links).
    pub fn standardized_file_url(&self) -> FileUrl {
        match self.to_nsurl().URLByStandardizingPath().and_then(|url| url.path()) {
            Some(path) => self.with_path_keeping_flag(&path.to_string()),
            None => self.clone(),
        }
    }

    /// `url.resolvingSymlinksInPath()`; see [`FileUrl::standardized_file_url`].
    pub fn resolving_symlinks_in_path(&self) -> FileUrl {
        match self.to_nsurl().URLByResolvingSymlinksInPath().and_then(|url| url.path()) {
            Some(path) => self.with_path_keeping_flag(&path.to_string()),
            None => self.clone(),
        }
    }

    fn with_path_keeping_flag(&self, path: &str) -> FileUrl {
        let mut url_path = path.to_owned();
        if self.has_directory_path() && !url_path.ends_with('/') && !ends_in_dot_component(&url_path) {
            url_path.push('/');
        }
        FileUrl { url_path, relative: false }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dot_segments() {
        assert_eq!(remove_dot_segments("/private/tmp/a/../b"), "/private/tmp/b");
        assert_eq!(remove_dot_segments("/private/tmp/./c"), "/private/tmp/c");
        assert_eq!(remove_dot_segments("/a/b/../../.."), "/");
    }

    #[test]
    fn deleting_last_component_matches_swift() {
        let url = |path: &str| FileUrl { url_path: path.to_owned(), relative: false };
        assert_eq!(url("/tmp/urlprobe//double//slash.md").deleting_last_path_component().url_path(), "/tmp/urlprobe//double/");
        assert_eq!(url("/").deleting_last_path_component().url_path(), "/");
        assert_eq!(url("/tmp/urlprobe/../").deleting_last_path_component().url_path(), "/tmp/urlprobe/../");
        assert_eq!(url("/a/b/c.md").deleting_last_path_component().url_path(), "/a/b/");
        let mut root = url("/a/b/c.md");
        for _ in 0..4 {
            root = root.deleting_last_path_component();
        }
        assert_eq!(root.url_path(), "/");
    }

    #[test]
    fn appending_matches_swift() {
        let base = FileUrl::from_path_is_directory("/nonexistent-upleft/base", false);
        assert_eq!(base.appending_path_component("/lead").url_path(), "/nonexistent-upleft/base/lead");
        assert_eq!(base.appending_path_component("").url_path(), "/nonexistent-upleft/base/");
        assert_eq!(base.appending_path_component("a/b.md").path(), "/nonexistent-upleft/base/a/b.md");
        assert_eq!(base.appending_path_component("\u{e9}.md").path(), "/nonexistent-upleft/base/e\u{301}.md");
        assert_eq!(
            FileUrl::from_path("/a/b.tar.gz").appending_path_extension("md").path(),
            "/a/b.tar.gz.md"
        );
        assert_eq!(
            FileUrl::from_path_is_directory("/a/b", true).appending_path_extension("md").url_path(),
            "/a/b.md/"
        );
        assert_eq!(FileUrl::from_path("/a/b/c.md").path_components(), vec!["/", "a", "b", "c.md"]);
        assert_eq!(FileUrl::from_path("/tmp/x/file.tar.gz").path_extension(), "gz");
        assert_eq!(FileUrl::from_path("/tmp/x/.hidden").path_extension(), "");
    }
}
