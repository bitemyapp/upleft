//! Upleft's port of Downright's `DownrightSpotlightMetadata` target: the one
//! parser-backed definition of the Spotlight attributes that both the app
//! (`upleft-app` `integrations::spotlight_metadata`) and the filesystem
//! importer derive from a Markdown file. Like the Swift target it links no
//! AppKit, so an importer process can load it without starting the app.

pub mod spotlight_metadata;
