//! Rust side of the `down-cli` suite; mirrors
//! `oracle/app/Sources/downright-app-oracle/DownCLIDump.swift`, which
//! documents the scenario format and the dump.
//!
//! Runs Upleft's `down` (`target/release/down`) on one scenario in a fresh
//! sandbox under `target/down-cli-sandboxes`, exactly as the Swift side runs
//! Downright's: through a `bin/down` symbolic link, with the same argv
//! (argv[0] included), environment, working directory, stdin, SIGPIPE at its
//! default action and an empty signal mask. No field is read from the clock.

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Map, Value};
use upleft_foundation::foundation_io;
use upleft_foundation::json_serialization::{self, AnyJson, ReadingOptions};

use super::{Failure, Request, json};

pub fn run(request: &Request) -> Result<(), Failure> {
    let text = std::fs::read_to_string(&request.input)?;
    let scenario: Value = serde_json::from_str(&text).map_err(|error| Failure::Error(format!("scenario: {error}")))?;
    let scenario = scenario.as_object().ok_or_else(|| Failure::Error("a scenario is a JSON object".into()))?;
    let root = repository_root().ok_or_else(|| Failure::Error("cannot find the repository root".into()))?;
    let binary = root.join("target/release/down");
    if !binary.exists() {
        return Err(Failure::Error(format!("{} is missing; run `cargo build --release -p upleft-cli`", binary.display())));
    }
    let value = run_scenario(scenario, &root, &binary, "rust")?;
    Ok(json::write(&value, &request.output)?)
}

/// The directory holding `Cargo.toml` and `vendor/downright`, found upward
/// from this binary.
fn repository_root() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?.canonicalize().ok()?;
    let mut directory = executable.parent()?.to_path_buf();
    loop {
        if directory.join("Cargo.toml").exists() && directory.join("vendor/downright").exists() {
            return Some(directory);
        }
        directory = directory.parent()?.to_path_buf();
    }
}

fn run_scenario(scenario: &Map<String, Value>, root: &Path, binary: &Path, side: &str) -> Result<Value, Failure> {
    let parent = root.join("target/down-cli-sandboxes");
    std::fs::create_dir_all(&parent)?;
    let sandbox = parent.join(format!("{side}-{}", foundation_io::uuid_string()));
    std::fs::create_dir_all(&sandbox)?;
    let result = run_in(scenario, &sandbox, binary);
    remove_tree(&sandbox);
    result
}

fn run_in(scenario: &Map<String, Value>, sandbox: &Path, binary: &Path) -> Result<Value, Failure> {
    let sandbox_text = sandbox.to_str().ok_or_else(|| Failure::Error("sandbox path is not UTF-8".into()))?.to_owned();
    for directory in ["bin", "home", "work", "tmp"] {
        std::fs::create_dir_all(sandbox.join(directory))?;
    }
    std::os::unix::fs::symlink(binary, sandbox.join("bin/down"))?;
    let substitute = |text: &str| text.replace("$SANDBOX", &sandbox_text);

    let files = scenario.get("files").and_then(Value::as_array).cloned().unwrap_or_default();
    let root = repository_root().ok_or_else(|| Failure::Error("cannot find the repository root".into()))?;
    let mut copied = json::Object::new();
    let mut any_copied = false;
    for file in &files {
        let relative = file.get("path").and_then(Value::as_str).ok_or_else(|| Failure::Error("file without path".into()))?;
        let path = PathBuf::from(format!("{sandbox_text}/{}", substitute(relative)));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if file.get("directory").and_then(Value::as_bool) == Some(true) {
            std::fs::create_dir_all(&path)?;
        } else if let Some(source) = file.get("copyTree").and_then(Value::as_str) {
            let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
            copy_tree(&root.join(source), &path, &mut hash, "")?;
            copied = copied.with(relative, json::hex(hash));
            any_copied = true;
        } else if let Some(target) = file.get("symlink").and_then(Value::as_str) {
            std::os::unix::fs::symlink(substitute(target), &path)?;
        } else {
            std::fs::write(&path, bytes(file, &substitute)?)?;
        }
    }
    for file in &files {
        let (Some(mode), Some(relative)) =
            (file.get("mode").and_then(Value::as_str), file.get("path").and_then(Value::as_str))
        else {
            continue;
        };
        let mode = u32::from_str_radix(mode, 8).map_err(|_| Failure::Error(format!("bad mode {mode}")))?;
        let path = format!("{sandbox_text}/{}", substitute(relative));
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode));
    }

    let cwd = format!("{sandbox_text}/{}", substitute(scenario.get("cwd").and_then(Value::as_str).unwrap_or("work")));
    let argv0 = scenario
        .get("argv0")
        .and_then(Value::as_str)
        .map(|argv0| substitute(argv0))
        .unwrap_or_else(|| format!("{sandbox_text}/bin/down"));
    let arguments: Vec<String> = scenario
        .get("argv")
        .and_then(Value::as_array)
        .map(|argv| argv.iter().filter_map(Value::as_str).map(|argument| substitute(argument)).collect())
        .unwrap_or_default();
    let mut environment: Vec<(String, String)> = vec![
        ("HOME".into(), format!("{sandbox_text}/home")),
        ("CFFIXED_USER_HOME".into(), format!("{sandbox_text}/home")),
        ("PATH".into(), "/usr/bin:/bin:/usr/sbin:/sbin".into()),
        ("TMPDIR".into(), format!("{sandbox_text}/tmp/")),
    ];
    if let Some(env) = scenario.get("env").and_then(Value::as_object) {
        for (key, value) in env {
            let value = substitute(value.as_str().unwrap_or(""));
            match environment.iter_mut().find(|(k, _)| k == key) {
                Some(slot) => slot.1 = value,
                None => environment.push((key.clone(), value)),
            }
        }
    }
    let stdin = match scenario.get("stdin") {
        Some(spec) => Some(bytes(spec, &substitute)?),
        None => None,
    };

    let collect_temp = scenario.get("tempFiles").and_then(Value::as_bool) == Some(true);
    let temp_directory = foundation_io::temporary_directory().appending_path_component("Upleft").path();
    let temp_before = directory_names(Path::new(&temp_directory));

    let (status, stdout, stderr) = spawn_and_wait(
        &format!("{sandbox_text}/bin/down"),
        &argv0,
        &arguments,
        &environment,
        &cwd,
        stdin,
    )?;

    let masked = |data: &[u8]| mask(data, &sandbox_text);
    let mut object = json::Object::new().with("status", status);
    object = if scenario.get("stdoutFormat").and_then(Value::as_str) == Some("json-lines") {
        object.with("stdout", json_lines(&masked(&stdout)))
    } else {
        object.with("stdout", text_or_hex(&masked(&stdout)))
    };
    object = object.with("stderr", text_or_hex(&masked(&stderr)));
    if any_copied {
        object = object.with("copied", copied.build());
    }
    let ignored: Vec<String> = scenario
        .get("ignore")
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default();
    let items: Vec<Value> = snapshot(sandbox, &ignored)
        .into_iter()
        .map(|item| {
            let mut entry = json::Object::new().with("path", item.path).with("type", item.kind).with("mode", item.mode);
            if let Some(target) = item.target {
                entry = entry.with("target", text_or_hex(&masked(&target)));
            }
            if let Some(contents) = item.contents {
                entry = entry.with("contents", if contents.len() > 1_048_576 { digest(&contents) } else { text_or_hex(&masked(&contents)) });
            }
            entry.build()
        })
        .collect();
    object = object.with("files", Value::Array(items));
    if collect_temp {
        let mut temp = Vec::new();
        for name in directory_names(Path::new(&temp_directory)) {
            if temp_before.contains(&name) || !name.starts_with("stdin-") {
                continue;
            }
            let path = Path::new(&temp_directory).join(&name);
            let contents = std::fs::read(&path).unwrap_or_default();
            temp.push(json::Object::new().with("name", mask_uuid(&name)).with("contents", text_or_hex(&masked(&contents))).build());
            let _ = std::fs::remove_file(&path);
        }
        object = object.with("tempFiles", Value::Array(temp));
    }
    Ok(object.build())
}

fn bytes(spec: &Value, substitute: &dyn Fn(&str) -> String) -> Result<Vec<u8>, Failure> {
    if let Some(text) = spec.get("text").and_then(Value::as_str) {
        return Ok(substitute(text).into_bytes());
    }
    if let Some(hex) = spec.get("hex").and_then(Value::as_str) {
        let digits: Vec<u8> = hex.bytes().filter(|&b| b != b' ').collect();
        if digits.len() % 2 != 0 {
            return Err(Failure::Error("odd hex".into()));
        }
        return digits
            .chunks(2)
            .map(|pair| {
                u8::from_str_radix(std::str::from_utf8(pair).unwrap_or("zz"), 16).map_err(|_| Failure::Error("bad hex".into()))
            })
            .collect();
    }
    if let (Some(unit), Some(count)) = (spec.get("repeat").and_then(Value::as_str), spec.get("count").and_then(Value::as_u64)) {
        let mut out = substitute(spec.get("prefix").and_then(Value::as_str).unwrap_or("")).into_bytes();
        out.extend(unit.repeat(count as usize).into_bytes());
        out.extend(substitute(spec.get("suffix").and_then(Value::as_str).unwrap_or("")).into_bytes());
        return Ok(out);
    }
    Ok(Vec::new())
}

fn directory_names(path: &Path) -> BTreeSet<String> {
    std::fs::read_dir(path)
        .map(|entries| entries.flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default()
}

/// Copies the regular files and directories under `source` to
/// `destination`, in byte order of their names, folding each relative path
/// and its contents into a 64-bit FNV-1a hash (as the Swift side does).
fn copy_tree(source: &Path, destination: &Path, hash: &mut u64, relative: &str) -> Result<(), Failure> {
    fn fold(hash: &mut u64, bytes: &[u8]) {
        for &byte in bytes {
            *hash ^= byte as u64;
            *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    std::fs::create_dir_all(destination)?;
    let mut names: Vec<std::ffi::OsString> =
        std::fs::read_dir(source)?.flatten().map(|entry| entry.file_name()).collect();
    names.sort_by(|a, b| a.as_encoded_bytes().cmp(b.as_encoded_bytes()));
    for name in names {
        let from = source.join(&name);
        let Ok(metadata) = std::fs::symlink_metadata(&from) else { continue };
        let name_text = name.to_string_lossy();
        let path = if relative.is_empty() { name_text.into_owned() } else { format!("{relative}/{name_text}") };
        if metadata.is_dir() {
            copy_tree(&from, &destination.join(&name), hash, &path)?;
        } else if metadata.is_file() {
            let contents = std::fs::read(&from)?;
            std::fs::write(destination.join(&name), &contents)?;
            fold(hash, path.as_bytes());
            fold(hash, &[0]);
            fold(hash, &contents);
            fold(hash, &[0]);
        }
    }
    Ok(())
}

/// Deletes a tree, first making every directory in it writable and searchable.
fn remove_tree(path: &Path) {
    let Ok(metadata) = std::fs::symlink_metadata(path) else { return };
    if metadata.is_dir() {
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700));
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                remove_tree(&entry.path());
            }
        }
        let _ = std::fs::remove_dir(path);
    } else {
        let _ = std::fs::remove_file(path);
    }
}

struct SnapshotItem {
    path: String,
    kind: &'static str,
    mode: String,
    target: Option<Vec<u8>>,
    contents: Option<Vec<u8>>,
}

/// Every item under the sandbox, by relative path in UTF-8 byte order.
fn snapshot(sandbox: &Path, ignored: &[String]) -> Vec<SnapshotItem> {
    fn visit(sandbox: &Path, ignored: &[String], relative: &str, items: &mut Vec<SnapshotItem>) {
        if relative == "bin"
            || relative == "home/Library"
            || relative.starts_with("tmp/xcrun_db")
            || ignored.iter().any(|path| path == relative)
        {
            return;
        }
        let path = if relative.is_empty() { sandbox.to_path_buf() } else { sandbox.join(relative) };
        let Ok(metadata) = std::fs::symlink_metadata(&path) else { return };
        let permissions = metadata.mode() & 0o7777;
        let mode = format!("{permissions:o}");
        let file_type = metadata.file_type();
        if file_type.is_dir() {
            if !relative.is_empty() {
                items.push(SnapshotItem { path: relative.into(), kind: "directory", mode, target: None, contents: None });
            }
            let searchable = permissions & 0o500 == 0o500;
            if !searchable {
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(permissions | 0o500));
            }
            let mut names: Vec<String> = std::fs::read_dir(&path)
                .map(|entries| entries.flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).collect())
                .unwrap_or_default();
            if !searchable {
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(permissions));
            }
            names.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
            for name in names {
                let child = if relative.is_empty() { name } else { format!("{relative}/{name}") };
                visit(sandbox, ignored, &child, items);
            }
        } else if file_type.is_symlink() {
            let target = std::fs::read_link(&path)
                .map(|target| target.as_os_str().as_encoded_bytes().to_vec())
                .unwrap_or_default();
            items.push(SnapshotItem { path: relative.into(), kind: "symlink", mode, target: Some(target), contents: None });
        } else if file_type.is_file() {
            let readable = permissions & 0o400 != 0;
            if !readable {
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(permissions | 0o400));
            }
            let contents = std::fs::read(&path).unwrap_or_default();
            if !readable {
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(permissions));
            }
            items.push(SnapshotItem { path: relative.into(), kind: "file", mode, target: None, contents: Some(contents) });
        } else {
            items.push(SnapshotItem { path: relative.into(), kind: "other", mode, target: None, contents: None });
        }
    }
    let mut items = Vec::new();
    visit(sandbox, ignored, "", &mut items);
    items
}

/// Spawns `executable` with an explicit argv[0], a cleared environment and
/// pipes; kills it after 60 seconds. Returns the status as the dump spells
/// it, stdout and stderr.
fn spawn_and_wait(
    executable: &str,
    argv0: &str,
    arguments: &[String],
    environment: &[(String, String)],
    cwd: &str,
    stdin: Option<Vec<u8>>,
) -> Result<(Value, Vec<u8>, Vec<u8>), Failure> {
    let mut command = Command::new(executable);
    command
        .arg0(argv0)
        .args(arguments)
        .env_clear()
        .envs(environment.iter().map(|(key, value)| (key.as_str(), value.as_str())))
        .current_dir(cwd)
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let writer = child.stdin.take().map(|mut pipe| {
        let data = stdin.unwrap_or_default();
        std::thread::spawn(move || {
            let _ = pipe.write_all(&data);
        })
    });
    let mut out_pipe = child.stdout.take().expect("stdout");
    let mut err_pipe = child.stderr.take().expect("stderr");
    let out_reader = std::thread::spawn(move || {
        let mut data = Vec::new();
        let _ = out_pipe.read_to_end(&mut data);
        data
    });
    let err_reader = std::thread::spawn(move || {
        let mut data = Vec::new();
        let _ = err_pipe.read_to_end(&mut data);
        data
    });
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    if let Some(writer) = writer {
        let _ = writer.join();
    }
    let stdout = out_reader.join().unwrap_or_default();
    let stderr = err_reader.join().unwrap_or_default();
    let status = match status {
        None => Value::String("timeout".into()),
        Some(status) => match (status.code(), status.signal()) {
            (Some(code), _) => json::Object::new().with("exit", code).build(),
            (None, Some(signal)) => json::Object::new().with("signal", signal).build(),
            (None, None) => Value::String("unknown".into()),
        },
    };
    Ok((status, stdout, stderr))
}

/// Byte-level replacement (no Unicode semantics).
fn replace_bytes(data: &[u8], target: &[u8], replacement: &[u8]) -> Vec<u8> {
    if target.is_empty() || data.len() < target.len() {
        return data.to_vec();
    }
    let mut out = Vec::with_capacity(data.len());
    let mut index = 0;
    while index < data.len() {
        if data[index..].starts_with(target) {
            out.extend_from_slice(replacement);
            index += target.len();
        } else {
            out.push(data[index]);
            index += 1;
        }
    }
    out
}

fn mask(data: &[u8], sandbox: &str) -> Vec<u8> {
    let escaped = sandbox.replace('/', "\\/");
    replace_bytes(&replace_bytes(data, sandbox.as_bytes(), b"$SANDBOX"), escaped.as_bytes(), b"$SANDBOX")
}

/// `stdin-<UUID>.md` → `stdin-$UUID.md`.
fn mask_uuid(name: &str) -> String {
    if name.starts_with("stdin-") && name.ends_with(".md") && name.len() == 6 + 36 + 3 {
        "stdin-$UUID.md".into()
    } else {
        name.into()
    }
}

/// A file over 1 MiB is dumped as its size and 64-bit FNV-1a hash.
fn digest(data: &[u8]) -> Value {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    json::Object::new().with("size", data.len()).with("fnv1a64", json::hex(hash)).build()
}

/// Lines split on `\n` when the bytes are UTF-8, else `{"hex": …}`.
fn text_or_hex(data: &[u8]) -> Value {
    match std::str::from_utf8(data) {
        Ok(text) => Value::Array(text.split('\n').map(|line| Value::String(line.into())).collect()),
        Err(_) => {
            let hex: String = data.iter().map(|byte| format!("{byte:02x}")).collect();
            json::Object::new().with("hex", hex).build()
        }
    }
}

/// Each line parsed as JSON by `JSONSerialization` (compared structurally),
/// else kept as text.
fn json_lines(data: &[u8]) -> Value {
    Value::Array(
        data.split(|&byte| byte == b'\n')
            .map(|line| {
                if !line.is_empty()
                    && let Ok(value) = json_serialization::json_object(line, ReadingOptions { fragments_allowed: true })
                {
                    return json::Object::new().with("json", any_json(&value)).build();
                }
                Value::String(String::from_utf8_lossy(line).into_owned())
            })
            .collect(),
    )
}

/// A parsed JSON value with object members in UTF-8 byte order of their keys.
fn any_json(value: &AnyJson) -> Value {
    match value {
        AnyJson::Null => Value::Null,
        AnyJson::Bool(flag) => Value::Bool(*flag),
        AnyJson::Int(value) => (*value).into(),
        AnyJson::Double(value) => json::double(*value),
        AnyJson::Number(number) => {
            let kind = json_serialization::number_objc_type(number);
            if matches!(kind.as_str(), "d" | "f") {
                json::double(number.doubleValue())
            } else {
                (number.longLongValue()).into()
            }
        }
        AnyJson::String(text) => Value::String(text.clone()),
        AnyJson::Array(values) => Value::Array(values.iter().map(any_json).collect()),
        AnyJson::Object(members) => {
            let mut sorted: Vec<&(String, AnyJson)> = members.iter().collect();
            sorted.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
            let mut object = json::Object::new();
            for (key, value) in sorted {
                object = object.with(key, any_json(value));
            }
            object.build()
        }
    }
}
