//! Rust side of the `workspace` suite; mirrors
//! `oracle/app/Sources/downright-app-oracle/WorkspaceDump.swift` (see there
//! for the input and output formats).

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use serde_json::Value;
use upleft_app::workspace::workspace_index::{
    WorkspaceIndex, WorkspaceIndexPolicy, WorkspaceIndexPolicyInit, WorkspaceIndexSnapshot,
};
use upleft_app::workspace::workspace_link_graph::{StringMap, WorkspaceLinkGraphBuilder, WorkspaceLinkTarget};
use upleft_app::workspace::workspace_search::{WorkspaceSearch, WorkspaceSearchMention, WorkspaceSearchQuery};
use upleft_foundation::url::FileUrl;

use super::json::{self, Object};
use super::parse::{range, string};
use super::{Failure, Request};

pub fn policy(object: &Value) -> WorkspaceIndexPolicy {
    let defaults = WorkspaceIndexPolicyInit::default();
    if !object.is_object() {
        return WorkspaceIndexPolicy::new(defaults);
    }
    let strings = |key: &str| -> Option<Vec<String>> {
        object[key].as_array().map(|items| items.iter().filter_map(|item| item.as_str().map(str::to_owned)).collect())
    };
    WorkspaceIndexPolicy::new(WorkspaceIndexPolicyInit {
        markdown_extensions: strings("markdownExtensions").unwrap_or(defaults.markdown_extensions),
        ignored_directory_names: strings("ignoredDirectoryNames").unwrap_or(defaults.ignored_directory_names),
        ignores_hidden_directories: object["ignoresHiddenDirectories"]
            .as_bool()
            .unwrap_or(defaults.ignores_hidden_directories),
        maximum_files: object["maximumFiles"].as_i64().map_or(defaults.maximum_files, |value| value as isize),
        maximum_bytes_per_file: object["maximumBytesPerFile"].as_i64().unwrap_or(defaults.maximum_bytes_per_file),
        maximum_total_bytes: object["maximumTotalBytes"].as_i64().unwrap_or(defaults.maximum_total_bytes),
        read_concurrency: object["readConcurrency"].as_i64().map_or(defaults.read_concurrency, |value| value as isize),
    })
}

pub fn query(object: &Value) -> WorkspaceSearchQuery {
    let flag = |key: &str| object[key].as_bool().unwrap_or(false);
    WorkspaceSearchQuery {
        text: object["text"].as_str().unwrap_or("").to_owned(),
        is_regex: flag("isRegex"),
        case_sensitive: flag("caseSensitive"),
        whole_word: flag("wholeWord"),
    }
}

unsafe extern "C" {
    fn CFRunLoopRunInMode(mode: *const std::ffi::c_void, seconds: f64, return_after_source_handled: u8) -> i32;
    static kCFRunLoopDefaultMode: *const std::ffi::c_void;
}

/// Runs the index on the main thread, turning the run loop until the scan is
/// published; every published snapshot, in order.
pub fn index(root: &FileUrl, policy: WorkspaceIndexPolicy) -> Result<Vec<WorkspaceIndexSnapshot>, Failure> {
    let index = WorkspaceIndex::new(policy);
    let updates = Rc::new(RefCell::new(Vec::new()));
    let sink = updates.clone();
    index.set_on_update(Some(Box::new(move |snapshot: &WorkspaceIndexSnapshot| sink.borrow_mut().push(snapshot.clone()))));
    index.reroot(root);
    let deadline = Instant::now() + Duration::from_secs(120);
    while updates.borrow().len() < 2 && Instant::now() < deadline {
        // SAFETY: runs the current (main) thread's run loop briefly.
        unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.01, 0) };
    }
    let updates = updates.borrow().clone();
    if updates.len() != 2 {
        return Err(Failure::Error("the index never published a scan".into()));
    }
    Ok(updates)
}

pub fn run(request: &Request) -> Result<(), Failure> {
    let object: Value = serde_json::from_slice(&std::fs::read(&request.input)?)
        .map_err(|error| Failure::Error(format!("{}: {error}", request.input.display())))?;
    let Some(root_path) = object["root"].as_str() else {
        return Err(Failure::Error(format!("{}: expected {{root, …}}", request.input.display())));
    };
    let root = FileUrl::from_path(&request.input.to_string_lossy())
        .deleting_last_path_component()
        .appending_path_component(root_path)
        .standardized_file_url();
    let searches = object["searches"].as_array().cloned().unwrap_or_default();
    let policy = policy(&object["policy"]);
    let output_path = std::path::absolute(&request.output)?;
    std::env::set_current_dir("/").map_err(|error| Failure::Error(format!("cannot move to /: {error}")))?;
    let updates = index(&root, policy)?;
    let snapshot = &updates[1];

    let root_path = snapshot.root_url.path();
    let root_prefix = if root_path.ends_with('/') { root_path } else { root_path + "/" };
    let relative = |path: &str| -> Value {
        match path.strip_prefix(root_prefix.as_str()) {
            Some(rest) => Value::String(rest.to_owned()),
            None => Value::String(path.to_owned()),
        }
    };

    let entries: Vec<Value> = snapshot
        .entries
        .iter()
        .map(|entry| {
            Object::new()
                .with("id", relative(&entry.id))
                .with("url", relative(&entry.url.path()))
                .with("relativePath", entry.relative_path.clone())
                .with("text", entry.text.clone())
                .with("byteCount", entry.byte_count)
                .with(
                    "headings",
                    entry
                        .headings
                        .iter()
                        .map(|heading| {
                            Object::new()
                                .with("title", heading.title.clone())
                                .with("range", range(heading.range))
                                .with("level", heading.level as i64)
                                .build()
                        })
                        .collect::<Vec<_>>(),
                )
                .with(
                    "frontMatter",
                    entry
                        .front_matter
                        .iter()
                        .map(|field| {
                            Object::new()
                                .with("key", field.key.clone())
                                .with("value", field.value.clone())
                                .with("range", range(field.range))
                                .build()
                        })
                        .collect::<Vec<_>>(),
                )
                .with(
                    "links",
                    entry
                        .links
                        .iter()
                        .map(|link| {
                            Object::new()
                                .with("destination", link.destination.clone())
                                .with("range", range(link.range))
                                .with("kind", link.kind.raw_value())
                                .build()
                        })
                        .collect::<Vec<_>>(),
                )
                .build()
        })
        .collect();

    let target = |link: &WorkspaceLinkTarget| -> Value {
        Object::new()
            .with("sourceFile", relative(&link.source_file))
            .with("sourceRange", range(link.source_range))
            .with("destination", link.destination.clone())
            .with("targetFile", link.target_file.as_deref().map_or(Value::Null, &relative))
            .build()
    };
    let mentions = |mentions: &StringMap<Vec<WorkspaceSearchMention>>| -> Value {
        Value::Array(
            snapshot
                .entries
                .iter()
                .filter_map(|entry| {
                    mentions.get(&entry.id).map(|list| {
                        Object::new()
                            .with("target", relative(&entry.id))
                            .with(
                                "mentions",
                                list.iter()
                                    .map(|mention| {
                                        Object::new()
                                            .with("fileID", relative(&mention.file_id))
                                            .with("range", range(mention.range))
                                            .with("target", mention.target.clone())
                                            .build()
                                    })
                                    .collect::<Vec<_>>(),
                            )
                            .build()
                    })
                })
                .collect(),
        )
    };

    let graph = WorkspaceLinkGraphBuilder::build_with(snapshot, true);
    let graph_json = Object::new()
        .with(
            "outgoing",
            snapshot
                .entries
                .iter()
                .filter_map(|entry| {
                    graph.outgoing.get(&entry.id).map(|links| {
                        Object::new()
                            .with("source", relative(&entry.id))
                            .with("links", links.iter().map(target).collect::<Vec<_>>())
                            .build()
                    })
                })
                .collect::<Vec<_>>(),
        )
        .with(
            "backlinks",
            snapshot
                .entries
                .iter()
                .filter_map(|entry| {
                    graph.backlinks.get(&entry.id).map(|list| {
                        Object::new()
                            .with("target", relative(&entry.id))
                            .with(
                                "links",
                                list.iter()
                                    .map(|link| {
                                        Object::new()
                                            .with("sourceFile", relative(&link.source_file))
                                            .with("sourceRange", range(link.source_range))
                                            .with("targetFile", relative(&link.target_file))
                                            .with("destination", link.destination.clone())
                                            .build()
                                    })
                                    .collect::<Vec<_>>(),
                            )
                            .with("linksTo", graph.links_to(&entry.id).len() as i64)
                            .build()
                    })
                })
                .collect::<Vec<_>>(),
        )
        .with("unresolved", graph.unresolved.iter().map(target).collect::<Vec<_>>())
        .with("unlinkedMentions", mentions(&graph.unlinked_mentions))
        .with("unlinkedMentionsGivenGraph", mentions(&WorkspaceSearch::unlinked_mentions(snapshot, Some(&graph))))
        .build();

    let search_json: Vec<Value> = searches
        .iter()
        .map(|object| {
            let query = query(object);
            let limit = object["limitPerFile"].as_i64().unwrap_or(100) as usize;
            let results = WorkspaceSearch::search_limited(&query, snapshot, limit);
            Object::new()
                .with("isValid", WorkspaceSearch::is_valid(&query))
                .with(
                    "results",
                    results
                        .iter()
                        .map(|result| {
                            Object::new()
                                .with("fileID", relative(&result.file_id))
                                .with("url", relative(&result.url.path()))
                                .with("relativePath", result.relative_path.clone())
                                .with("range", range(result.range))
                                .with("contextRange", range(result.context_range))
                                .with("contextText", result.context_text.clone())
                                .with("line", result.line as i64)
                                .with("heading", string(result.heading.as_deref()))
                                .build()
                        })
                        .collect::<Vec<_>>(),
                )
                .build()
        })
        .collect();

    let output = Object::new()
        .with(
            "updates",
            updates
                .iter()
                .map(|update| {
                    Object::new()
                        .with("revision", update.revision as i64)
                        .with("entries", update.entries.len() as i64)
                        .with("skippedFiles", update.skipped_files as i64)
                        .with("root", relative(&(update.root_url.path() + "/")))
                        .build()
                })
                .collect::<Vec<_>>(),
        )
        .with("skippedFiles", snapshot.skipped_files as i64)
        .with("entries", entries)
        .with("graph", graph_json)
        .with("searches", search_json)
        .build();
    json::write(&output, &output_path)?;
    Ok(())
}
