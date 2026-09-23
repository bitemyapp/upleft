//! Port of `Sources/DownrightApp/Integrations/SpotlightMetadata.swift`.
//!
//! Keeps the app-facing names stable for integrations and tests while the
//! parser-backed implementation lives in the platform-neutral crate shared
//! with the mdimporter bundle (`upleft-spotlight-metadata`).
//!
//! [`SpotlightIndexer`] adds documents that the user opens to the local Core
//! Spotlight index. The filesystem importer covers unopened Markdown; this
//! path gives an opened document immediate results before the system index
//! catches up. Core Spotlight is reached through the Objective-C runtime
//! (there is no objc2 binding crate for it here), with the same calls in the
//! same order; the item it would index is computed by
//! [`SpotlightIndexer::item`] so it can be checked without touching the
//! user's index.

use objc2::msg_send;
use objc2::rc::{Allocated, Retained, autoreleasepool};
use objc2::runtime::{AnyClass, AnyObject};
use objc2_foundation::{NSArray, NSString};
use objc2_uniform_type_identifiers::UTType;
use upleft_foundation::url::FileUrl;

pub use upleft_spotlight_metadata::spotlight_metadata::{
    AttributeValue, SpotlightMetadata, SpotlightMetadataImporter, SpotlightMetadataKey,
};

#[link(name = "CoreSpotlight", kind = "framework")]
unsafe extern "C" {}

/// What `indexOpenedDocument(at:)` hands to Core Spotlight.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexedItem {
    /// `url.standardizedFileURL.path`.
    pub unique_identifier: String,
    pub domain_identifier: &'static str,
    /// The identifier `CSSearchableItemAttributeSet(contentType:)` gets.
    pub content_type: String,
    pub title: String,
    pub text_content: String,
    pub keywords: Vec<String>,
    pub content_url: FileUrl,
    pub kind: &'static str,
}

/// `SpotlightIndexer`.
pub struct SpotlightIndexer;

impl SpotlightIndexer {
    pub const DOMAIN_IDENTIFIER: &'static str = "com.bitemyapp.upleft.documents";

    /// The item `indexOpenedDocument(at:)` indexes, or `None` where Swift's
    /// `guard` returns: the metadata cannot be read, or `UTType(contentType)`
    /// is `nil`.
    pub fn item(url: &FileUrl) -> Option<IndexedItem> {
        let metadata = SpotlightMetadataImporter::metadata_at(url).ok()?;
        UTType::typeWithIdentifier(&NSString::from_str(&metadata.content_type))?;
        Some(IndexedItem {
            unique_identifier: url.standardized_file_url().path(),
            domain_identifier: Self::DOMAIN_IDENTIFIER,
            content_type: metadata.content_type,
            title: metadata.title,
            text_content: metadata.text_content,
            keywords: metadata.keywords,
            content_url: url.clone(),
            kind: "Markdown document",
        })
    }

    /// `indexOpenedDocument(at:)`: reads and indexes on a `.utility` global
    /// queue, never on the caller's thread.
    pub fn index_opened_document(url: &FileUrl) {
        let url = url.clone();
        dispatch2::DispatchQueue::global_queue(dispatch2::GlobalQueueIdentifier::QualityOfService(
            dispatch2::DispatchQoS::Utility,
        ))
        .exec_async(move || {
            let Some(item) = Self::item(&url) else { return };
            autoreleasepool(|_| index(&item));
        });
    }
}

/// `CSSearchableIndex.default().indexSearchableItems([item])`.
fn index(item: &IndexedItem) {
    let (Some(attribute_set_class), Some(item_class), Some(index_class)) = (
        AnyClass::get(c"CSSearchableItemAttributeSet"),
        AnyClass::get(c"CSSearchableItem"),
        AnyClass::get(c"CSSearchableIndex"),
    ) else {
        return;
    };
    let Some(content_type) = UTType::typeWithIdentifier(&NSString::from_str(&item.content_type)) else { return };
    unsafe {
        let allocated: Allocated<AnyObject> = msg_send![attribute_set_class, alloc];
        let attributes: Option<Retained<AnyObject>> = msg_send![allocated, initWithContentType: &*content_type];
        let Some(attributes) = attributes else { return };
        let _: () = msg_send![&*attributes, setTitle: &*NSString::from_str(&item.title)];
        let _: () = msg_send![&*attributes, setTextContent: &*NSString::from_str(&item.text_content)];
        let keywords: Vec<Retained<NSString>> = item.keywords.iter().map(|keyword| NSString::from_str(keyword)).collect();
        let keywords = NSArray::from_retained_slice(&keywords);
        let _: () = msg_send![&*attributes, setKeywords: &*keywords];
        let _: () = msg_send![&*attributes, setContentURL: &*item.content_url.to_nsurl()];
        let _: () = msg_send![&*attributes, setKind: &*NSString::from_str(item.kind)];

        let allocated: Allocated<AnyObject> = msg_send![item_class, alloc];
        let searchable: Option<Retained<AnyObject>> = msg_send![
            allocated,
            initWithUniqueIdentifier: &*NSString::from_str(&item.unique_identifier),
            domainIdentifier: &*NSString::from_str(item.domain_identifier),
            attributeSet: &*attributes
        ];
        let Some(searchable) = searchable else { return };
        let items = NSArray::from_retained_slice(&[searchable]);
        let default_index: Option<Retained<AnyObject>> = msg_send![index_class, defaultSearchableIndex];
        let Some(default_index) = default_index else { return };
        let no_handler: Option<&block2::DynBlock<dyn Fn(*mut AnyObject)>> = None;
        let _: () = msg_send![&*default_index, indexSearchableItems: &*items, completionHandler: no_handler];
    }
}
