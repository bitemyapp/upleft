//! Rust side of the `find` suite; mirrors
//! `oracle/app/Sources/downright-app-oracle/FindDump.swift` (see there for
//! the input and output formats).

use std::path::Path;

use serde_json::Value;
use upleft_app::support::find_engine::{FindEngine, FindQuery, FindSession, SiblingSearch};
use upleft_core::document_io::DocumentIO;
use upleft_core::{NSRange, TextEdit};
use upleft_foundation::url::FileUrl;

use super::json::{self, Object};
use super::parse::{optional_range, range};
use super::{Failure, Request};

pub fn query(object: &Value) -> FindQuery {
    let flag = |key: &str| object[key].as_bool().unwrap_or(false);
    let scope = object["scope"]
        .as_array()
        .filter(|scope| scope.len() == 2)
        .and_then(|scope| Some(NSRange::new(scope[0].as_i64()? as isize, scope[1].as_i64()? as isize)));
    FindQuery {
        text: object["text"].as_str().unwrap_or("").to_owned(),
        is_regex: flag("isRegex"),
        case_sensitive: flag("caseSensitive"),
        whole_word: flag("wholeWord"),
        scope,
    }
}

fn edit(edit: Option<&TextEdit>) -> Value {
    edit.map_or(Value::Null, |edit| {
        Object::new().with("range", range(edit.range)).with("replacement", edit.replacement.clone()).build()
    })
}

fn optional_int(value: Option<usize>) -> Value {
    value.map_or(Value::Null, |value| (value as i64).into())
}

/// The documents an input names, relative to its directory, as the Swift
/// resolves them: `input.deletingLastPathComponent()
/// .appendingPathComponent(path).standardizedFileURL`.
pub fn document_urls(input: &Path, paths: &[String]) -> Vec<FileUrl> {
    let directory = FileUrl::from_path(&input.to_string_lossy()).deleting_last_path_component();
    paths.iter().map(|path| directory.appending_path_component(path).standardized_file_url()).collect()
}

pub fn run(request: &Request) -> Result<(), Failure> {
    let root: Value = serde_json::from_slice(&std::fs::read(&request.input)?)
        .map_err(|error| Failure::Error(format!("{}: {error}", request.input.display())))?;
    let (Some(document_paths), Some(query_objects)) = (root["documents"].as_array(), root["queries"].as_array()) else {
        return Err(Failure::Error(format!("{}: expected {{documents, queries}}", request.input.display())));
    };
    let document_paths: Vec<String> =
        document_paths.iter().filter_map(|path| path.as_str().map(str::to_owned)).collect();
    let urls = document_urls(&request.input, &document_paths);
    let mut texts = Vec::new();
    for url in &urls {
        let (text, _) = DocumentIO::read(Path::new(&url.path()))
            .map_err(|error| Failure::Error(format!("{}: {error}", url.path())))?;
        texts.push(text);
    }

    let mut documents = Vec::new();
    for (document_index, text) in texts.iter().enumerate() {
        let mut results = Vec::new();
        for object in query_objects {
            let query = query(object);
            let template = object["template"].as_str();
            let caret = object["caret"].as_i64().unwrap_or(0) as isize;
            let matches = FindEngine::matches(text, &query);
            let mut fields = Object::new()
                .with("isValid", FindEngine::is_valid(&query))
                .with("matches", matches.iter().map(|&m| range(m)).collect::<Vec<_>>());
            if let Some(template) = template {
                fields = fields
                    .with(
                        "replaceAll",
                        FindEngine::replace_all_edits(text, &query, template)
                            .iter()
                            .map(|e| edit(Some(e)))
                            .collect::<Vec<_>>(),
                    )
                    .with(
                        "replacement",
                        matches
                            .iter()
                            .take(3)
                            .map(|&m| Value::String(FindEngine::replacement(m, text, &query, template)))
                            .collect::<Vec<_>>(),
                    );
            }
            let mut session = FindSession::new();
            session.update(query.clone(), text, caret);
            let mut walk = vec![
                Object::new()
                    .with("status", session.status_text())
                    .with("index", optional_int(session.current_index()))
                    .with("current", optional_range(session.current_match()))
                    .with("count", session.count() as i64)
                    .build(),
            ];
            for forward in [true, true, false] {
                let next = session.advance(forward);
                walk.push(
                    Object::new()
                        .with("advanced", optional_range(next))
                        .with("status", session.status_text())
                        .with("index", optional_int(session.current_index()))
                        .build(),
                );
            }
            let replacement = session.replacement_edit(text, template.unwrap_or("X"), caret);
            session.clear();
            fields = fields.with(
                "session",
                Object::new()
                    .with("walk", walk)
                    .with("replacementEdit", edit(replacement.as_ref()))
                    .with("clearedStatus", session.status_text())
                    .with("clearedCount", session.count() as i64)
                    .build(),
            );
            results.push(fields.build());
        }
        documents.push(Object::new().with("document", document_index as i64).with("queries", results).build());
    }

    let mut output = Object::new()
        .with("documents", document_paths.iter().map(|path| Value::String(path.clone())).collect::<Vec<_>>())
        .with("results", documents);
    if root["siblings"].as_bool() == Some(true) {
        let siblings: Vec<Value> = query_objects
            .iter()
            .map(|object| {
                let hits = SiblingSearch::search_default(&query(object), &urls);
                Value::Array(
                    hits.iter()
                        .map(|hit| {
                            Object::new()
                                .with("document", optional_int(urls.iter().position(|url| *url == hit.url)))
                                .with("displayName", hit.display_name.clone())
                                .with("range", range(hit.range))
                                .with("contextRange", range(hit.context_range))
                                .with("contextText", hit.context_text.clone())
                                .with("headingTitle", super::parse::string(hit.heading_title.as_deref()))
                                .with("lineNumber", hit.line_number as i64)
                                .build()
                        })
                        .collect(),
                )
            })
            .collect();
        output = output.with("siblings", siblings);
    }
    json::write(&output.build(), &request.output)?;
    Ok(())
}
