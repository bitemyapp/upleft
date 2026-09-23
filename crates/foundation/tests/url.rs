//! Replays `data/url.json`, Swift's own `URL` results recorded by
//! `url_probe.swift` (Swift 6.4, macOS 26), against `FileUrl`.
//!
//! The probe ran with `CFFIXED_USER_HOME=/tmp/upleft-url-home` and the
//! current directory at the fixture root; this test re-runs itself in a child
//! process with the same environment, since Foundation reads the home
//! directory once per process.

use std::process::Command;

use serde_json::Value;
use upleft_foundation::url::FileUrl;

const ROOT: &str = "/tmp/upleft-url-fixture";
const HOME: &str = "/tmp/upleft-url-home";

fn build_fixture() {
    let _ = std::fs::remove_dir_all(ROOT);
    for dir in ["dir", "dir/sub", "caf\u{e9}", "sp ace", "Docs.md"] {
        std::fs::create_dir_all(format!("{ROOT}/{dir}")).unwrap();
    }
    for file in ["file.md", "dir/a.md", "caf\u{e9}/n\u{f6}te.md", "sp ace/x y.markdown", "archive.tar.gz", ".hidden", "noext"] {
        std::fs::write(format!("{ROOT}/{file}"), "x").unwrap();
    }
    std::os::unix::fs::symlink(format!("{ROOT}/dir"), format!("{ROOT}/link")).unwrap();
}

fn describe(url: &FileUrl) -> Value {
    serde_json::json!({"abs": url.absolute_string(), "path": url.path(), "dir": url.has_directory_path()})
}

#[test]
fn url_matches_swift() {
    if std::env::var("CFFIXED_USER_HOME").as_deref() != Ok(HOME) {
        let status = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "url_matches_swift", "--test-threads", "1", "--nocapture"])
            .env("CFFIXED_USER_HOME", HOME)
            .status()
            .unwrap();
        assert!(status.success(), "child test failed");
        return;
    }
    build_fixture();
    std::env::set_current_dir(ROOT).unwrap();
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/url.json")).unwrap();
    let cases: Vec<Value> = serde_json::from_str(&text).unwrap();
    let mut failures = Vec::new();
    for case in &cases {
        let input = case["input"].as_str().unwrap();
        let url = FileUrl::from_path(input);
        let mut check = |name: &str, got: Value| {
            if case[name] != got {
                failures.push(format!("{input:?} {name}: swift {} rust {}", case[name], got));
            }
        };
        check("init", describe(&url));
        check("initDir", describe(&FileUrl::from_path_is_directory(input, true)));
        check("initFile", describe(&FileUrl::from_path_is_directory(input, false)));
        check("last", Value::from(url.last_path_component()));
        check("ext", Value::from(url.path_extension()));
        check("components", Value::from(url.path_components()));
        check("deleteLast", describe(&url.deleting_last_path_component()));
        check("deleteExt", describe(&url.deleting_path_extension()));
        // Not reproduced (see url.rs): the extension goes onto the relative
        // string of a URL made from "", "." or "..".
        if !matches!(input, "" | "." | "..") {
            check("appendExt", describe(&url.appending_path_extension("md")));
        }
        check("standardized", describe(&url.standardized_file_url()));
        check("resolved", describe(&url.resolving_symlinks_in_path()));
        for appended in case["append"].as_array().unwrap() {
            let component = appended["component"].as_str().unwrap();
            for (name, got) in [
                ("plain", url.appending_path_component(component)),
                ("dir", url.appending_path_component_is_directory(component, true)),
                ("file", url.appending_path_component_is_directory(component, false)),
            ] {
                if appended[name] != describe(&got) {
                    failures.push(format!(
                        "{input:?} append {component:?} {name}: swift {} rust {}",
                        appended[name],
                        describe(&got)
                    ));
                }
            }
        }
    }
    for failure in failures.iter().take(60) {
        eprintln!("{failure}");
    }
    assert!(failures.is_empty(), "{} differences from Swift", failures.len());
}
