//! Rust side of the `spotlight` suite; mirrors
//! `oracle/app/Sources/downright-app-oracle/SpotlightDump.swift`: the
//! Spotlight metadata one corpus document yields through
//! `SpotlightMetadataImporter` and through the C importer bridge. Nothing is
//! read from the clock or from file dates.

use std::ffi::c_void;

use objc2::rc::autoreleasepool;
use objc2::runtime::AnyObject;
use objc2_foundation::{NSArray, NSMutableDictionary, NSString};
use serde_json::Value;
use upleft_foundation::url::FileUrl;
use upleft_spotlight_metadata::spotlight_metadata::{
    AttributeValue, DownrightSpotlightPopulateMetadata, SpotlightMetadata, SpotlightMetadataImporter,
};

use super::{Failure, Request, json};

pub fn run(request: &Request) -> Result<(), Failure> {
    let path = request.input.to_str().ok_or_else(|| Failure::Error("input path is not UTF-8".into()))?;
    let input = FileUrl::from_path(path);
    let mut object = json::Object::new().with("accepts", SpotlightMetadataImporter::accepts(&input));
    object = match SpotlightMetadataImporter::metadata_at(&input) {
        Ok(metadata) => object.with("metadata", dump(&metadata)),
        Err(error) => object.with("error", error.code as i64),
    };

    let (returned, attributes) = autoreleasepool(|_| {
        let dictionary = NSMutableDictionary::<AnyObject, AnyObject>::new();
        let uti = NSString::from_str("net.daringfireball.markdown");
        let file = NSString::from_str(&input.path());
        let returned = unsafe {
            DownrightSpotlightPopulateMetadata(
                &*dictionary as *const NSMutableDictionary<AnyObject, AnyObject> as *mut c_void,
                &*uti as *const NSString as *const c_void,
                &*file as *const NSString as *const c_void,
            )
        };
        let mut members = Vec::new();
        for key in dictionary.allKeys() {
            let Some(name) = key.downcast_ref::<NSString>() else { continue };
            let Some(value) = dictionary.objectForKey(&key) else { continue };
            let value = if let Some(text) = value.downcast_ref::<NSString>() {
                AttributeValue::String(upleft_swift_text::ns::foundation::to_string(text))
            } else if let Some(array) = value.downcast_ref::<NSArray>() {
                AttributeValue::Strings(
                    array
                        .iter()
                        .filter_map(|item| item.downcast_ref::<NSString>().map(upleft_swift_text::ns::foundation::to_string))
                        .collect(),
                )
            } else {
                continue;
            };
            members.push((name.to_string(), value));
        }
        (returned, members)
    });
    object = object.with(
        "importer",
        json::Object::new().with("returned", returned).with("attributes", attributes_json(attributes)).build(),
    );

    let base = input.deleting_path_extension().last_path_component();
    let extensions = ["md", "MD", "markdown", "mdown", "mkd", "mdx", "mdc", "qmd", "rmd", "Rmd", "txt", ""];
    let content_types: Vec<Value> = extensions
        .iter()
        .map(|extension| {
            let name = if extension.is_empty() { base.clone() } else { format!("{base}.{extension}") };
            let url = FileUrl::from_path(&format!("/tmp/upleft-spotlight/{name}"));
            json::Object::new()
                .with("extension", *extension)
                .with("contentType", SpotlightMetadataImporter::content_type(&url))
                .with("accepts", SpotlightMetadataImporter::accepts(&url))
                .build()
        })
        .collect();
    object = object.with("contentTypes", Value::Array(content_types));
    Ok(json::write(&object.build(), &request.output)?)
}

fn dump(metadata: &SpotlightMetadata) -> Value {
    json::Object::new()
        .with("title", metadata.title.clone())
        .with("textContent", lines(&metadata.text_content))
        .with("keywords", Value::Array(metadata.keywords.iter().cloned().map(Value::String).collect()))
        .with("contentType", metadata.content_type.clone())
        .with(
            "attributes",
            attributes_json(metadata.attributes().into_iter().map(|(key, value)| (key.to_owned(), value)).collect()),
        )
        .build()
}

/// Members sorted by key; the text content as lines.
fn attributes_json(mut values: Vec<(String, AttributeValue)>) -> Value {
    values.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    let mut object = json::Object::new();
    for (key, value) in values {
        let value = match value {
            AttributeValue::String(text) if key == "kMDItemTextContent" => lines(&text),
            AttributeValue::String(text) => Value::String(text),
            AttributeValue::Strings(items) => Value::Array(items.into_iter().map(Value::String).collect()),
        };
        object = object.with(&key, value);
    }
    object.build()
}

fn lines(text: &str) -> Value {
    Value::Array(text.split('\n').map(|line| Value::String(line.into())).collect())
}
