//! `conform` — runs every corpus document through `downright-oracle` (Swift)
//! and `upleft-oracle` (Rust) for each suite in `conformance/suites.json` and
//! compares the results exactly.
//!
//!   conform [--suite NAME]... [--filter SUBSTRING] [--limit N] [--jobs N]
//!           [--fail-fast] [--no-cache]
//!
//! Swift results are cached under `target/conform-cache`, keyed by the oracle
//! binary, the input bytes, and the arguments, since the Swift side only
//! changes when the submodule moves. Failures leave both outputs, a list of
//! differences, and (for images) a diff picture under `conformance-out/`.
//! `upleft-oracle` exits with status 3 for a command whose layer is not
//! ported yet; those cases are reported as "not ported", never as passes.

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::Value;
use upleft_conformance::{compare_images, compare_json, read_png};

const NOT_PORTED: i32 = 3;

struct Suite {
    name: String,
    command: String,
    /// "json" or "png".
    output: String,
    variants: Vec<Vec<String>>,
    /// Render suites launch an app per case; running them concurrently lets
    /// windows compete for activation and display cycles.
    serial: bool,
    /// Restricts the suite to corpus paths containing one of these.
    only: Vec<String>,
}

struct Options {
    suites: Vec<String>,
    filter: Option<String>,
    limit: Option<usize>,
    jobs: usize,
    fail_fast: bool,
    cache: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Pass,
    Fail,
    NotPorted,
    Error,
}

struct Case<'a> {
    suite: &'a Suite,
    input: PathBuf,
    variant: usize,
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn parse_options() -> Options {
    let mut options = Options {
        suites: Vec::new(),
        filter: None,
        limit: None,
        jobs: std::thread::available_parallelism().map_or(4, usize::from),
        fail_fast: false,
        cache: true,
    };
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        let mut value = || arguments.next().unwrap_or_else(|| usage(&format!("{argument} needs a value")));
        match argument.as_str() {
            "--suite" => options.suites.push(value()),
            "--filter" => options.filter = Some(value()),
            "--limit" => options.limit = Some(value().parse().unwrap_or_else(|_| usage("--limit takes a number"))),
            "--jobs" => options.jobs = value().parse().unwrap_or_else(|_| usage("--jobs takes a number")),
            "--fail-fast" => options.fail_fast = true,
            "--no-cache" => options.cache = false,
            "--help" | "-h" => usage(""),
            other => usage(&format!("unknown argument {other}")),
        }
    }
    options
}

fn usage(message: &str) -> ! {
    if !message.is_empty() {
        eprintln!("conform: {message}");
    }
    eprintln!(
        "usage: conform [--suite NAME]... [--filter SUBSTRING] [--limit N] [--jobs N] [--fail-fast] [--no-cache]"
    );
    std::process::exit(64)
}

fn load_suites(root: &Path) -> (Vec<PathBuf>, Vec<Suite>) {
    let path = root.join("conformance/suites.json");
    let text = fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let json: Value = serde_json::from_str(&text).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let strings = |value: &Value| -> Vec<String> {
        value.as_array().map_or_else(Vec::new, |array| {
            array.iter().map(|item| item.as_str().expect("string").to_owned()).collect()
        })
    };
    let corpus = strings(&json["corpus"]).into_iter().map(|dir| root.join(dir)).collect();
    let suites = json["suites"]
        .as_array()
        .expect("suites array")
        .iter()
        .map(|suite| Suite {
            name: suite["name"].as_str().expect("name").to_owned(),
            command: suite["command"].as_str().expect("command").to_owned(),
            output: suite["output"].as_str().unwrap_or("json").to_owned(),
            variants: suite["variants"]
                .as_array()
                .map_or_else(|| vec![Vec::new()], |variants| variants.iter().map(strings).collect()),
            serial: suite["serial"].as_bool().unwrap_or(false),
            only: strings(&suite["only"]),
        })
        .collect();
    (corpus, suites)
}

fn corpus_files(directories: &[PathBuf]) -> Vec<PathBuf> {
    fn visit(directory: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(directory) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                visit(&path, out);
            } else if path.extension().is_some_and(|extension| extension == "md") {
                out.push(path);
            }
        }
    }
    let mut files = Vec::new();
    for directory in directories {
        visit(directory, &mut files);
    }
    files.sort();
    files
}

fn binary_stamp(path: &Path) -> u64 {
    let metadata = fs::metadata(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let mut hasher = DefaultHasher::new();
    metadata.len().hash(&mut hasher);
    metadata.modified().ok().hash(&mut hasher);
    hasher.finish()
}

/// Runs one oracle. Returns its exit code (or None if killed) and stderr.
fn run_oracle(binary: &Path, home: &Path, command: &str, input: &Path, output: &Path, flags: &[String]) -> (Option<i32>, String) {
    let result = Command::new(binary)
        .env("HOME", home)
        .arg(command)
        .arg(input)
        .arg(output)
        .args(flags)
        .output();
    match result {
        Ok(result) => (result.status.code(), String::from_utf8_lossy(&result.stderr).into_owned()),
        Err(error) => (None, format!("could not run {}: {error}", binary.display())),
    }
}

fn case_directory(out: &Path, case: &Case, root: &Path) -> PathBuf {
    let relative = case.input.strip_prefix(root).unwrap_or(&case.input);
    let slug = relative.to_string_lossy().replace(['/', ' '], "_");
    out.join(&case.suite.name).join(format!("{slug}.v{}", case.variant))
}

struct Context {
    root: PathBuf,
    swift_oracle: PathBuf,
    rust_oracle: PathBuf,
    swift_stamp: u64,
    home: PathBuf,
    cache: Option<PathBuf>,
    out: PathBuf,
}

fn run_case(context: &Context, case: &Case) -> (Outcome, String) {
    let suite = case.suite;
    let flags = &suite.variants[case.variant];
    let extension = &suite.output;
    let directory = case_directory(&context.out, case, &context.root);
    let scratch = std::env::temp_dir().join(format!(
        "upleft-conform-{}-{}",
        std::process::id(),
        directory.file_name().unwrap().to_string_lossy()
    ));
    fs::create_dir_all(&scratch).unwrap();
    let layout_flags = |path: &Path| -> Vec<String> {
        let mut flags = flags.clone();
        if suite.command == "render" {
            flags.push("--layout".into());
            flags.push(path.to_string_lossy().into_owned());
        }
        flags
    };

    // Swift side, from cache when possible.
    let input_bytes = fs::read(&case.input).unwrap_or_default();
    let mut hasher = DefaultHasher::new();
    (context.swift_stamp, &input_bytes, &suite.command, flags).hash(&mut hasher);
    let key = format!("{:016x}", hasher.finish());
    let (swift_output, swift_layout) = match &context.cache {
        Some(cache) => (cache.join(format!("{key}.{extension}")), cache.join(format!("{key}.layout.json"))),
        None => (scratch.join(format!("swift.{extension}")), scratch.join("swift.layout.json")),
    };
    if !swift_output.exists() {
        let (code, stderr) = run_oracle(
            &context.swift_oracle,
            &context.home,
            &suite.command,
            &case.input,
            &swift_output,
            &layout_flags(&swift_layout),
        );
        if code != Some(0) {
            let _ = fs::remove_file(&swift_output);
            return (Outcome::Error, format!("downright-oracle exited {code:?}: {}", stderr.trim()));
        }
    }

    let rust_output = scratch.join(format!("rust.{extension}"));
    let rust_layout = scratch.join("rust.layout.json");
    let (code, stderr) = run_oracle(
        &context.rust_oracle,
        &context.home,
        &suite.command,
        &case.input,
        &rust_output,
        &layout_flags(&rust_layout),
    );
    match code {
        Some(0) => {}
        Some(NOT_PORTED) => {
            let _ = fs::remove_dir_all(&scratch);
            return (Outcome::NotPorted, String::new());
        }
        other => {
            let _ = fs::remove_dir_all(&scratch);
            return (Outcome::Error, format!("upleft-oracle exited {other:?}: {}", stderr.trim()));
        }
    }

    let mut report = Vec::new();
    let mut pass = true;
    if extension == "png" {
        let result = read_png(&swift_output)
            .and_then(|swift| read_png(&rust_output).map(|rust| (swift, rust)))
            .and_then(|(swift, rust)| {
                let diff = scratch.join("diff.png");
                compare_images(&swift, &rust, Some(&diff))
            });
        match result {
            Ok(comparison) if comparison.is_identical() => {}
            Ok(comparison) => {
                pass = false;
                report.push(format!("pixels: {comparison}"));
            }
            Err(error) => return (Outcome::Error, error),
        }
        if swift_layout.exists() && rust_layout.exists() {
            let differences = compare_files(&swift_layout, &rust_layout, 40);
            if !differences.is_empty() {
                pass = false;
                report.push("layout:".into());
                report.extend(differences);
            }
        }
    } else {
        let differences = compare_files(&swift_output, &rust_output, 40);
        if !differences.is_empty() {
            pass = false;
            report.extend(differences);
        }
    }

    if pass {
        let _ = fs::remove_dir_all(&scratch);
        return (Outcome::Pass, String::new());
    }
    // Keep the evidence.
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).unwrap();
    let _ = fs::copy(&swift_output, directory.join(format!("swift.{extension}")));
    if swift_layout.exists() {
        let _ = fs::copy(&swift_layout, directory.join("swift.layout.json"));
    }
    for entry in fs::read_dir(&scratch).into_iter().flatten().flatten() {
        let _ = fs::rename(entry.path(), directory.join(entry.file_name()));
    }
    let _ = fs::remove_dir_all(&scratch);
    let text = format!(
        "input: {}\nflags: {}\n{}\n",
        case.input.display(),
        flags.join(" "),
        report.join("\n")
    );
    let _ = fs::write(directory.join("differences.txt"), &text);
    (Outcome::Fail, report.into_iter().take(6).collect::<Vec<_>>().join("\n      "))
}

fn compare_files(swift: &Path, rust: &Path, limit: usize) -> Vec<String> {
    let load = |path: &Path| -> Result<Value, String> {
        let text = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
        serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))
    };
    match (load(swift), load(rust)) {
        (Ok(swift), Ok(rust)) => compare_json(&swift, &rust, limit).iter().map(ToString::to_string).collect(),
        (Err(error), _) | (_, Err(error)) => vec![format!("unreadable: {error}")],
    }
}

fn main() -> ExitCode {
    let options = parse_options();
    let root = root();
    let (corpus, suites) = load_suites(&root);
    let swift_oracle = root.join("target/oracle/release/downright-oracle");
    let rust_oracle = root.join("target/release/upleft-oracle");
    for (binary, hint) in [(&swift_oracle, "just oracle"), (&rust_oracle, "cargo build --release -p upleft-conformance")] {
        if !binary.exists() {
            eprintln!("conform: {} is missing; run `{hint}`", binary.display());
            return ExitCode::from(2);
        }
    }
    let home = root.join("target/conform-home");
    fs::create_dir_all(&home).unwrap();
    let cache = options.cache.then(|| root.join("target/conform-cache"));
    if let Some(cache) = &cache {
        fs::create_dir_all(cache).unwrap();
    }
    let context = Context {
        swift_stamp: binary_stamp(&swift_oracle),
        root: root.clone(),
        swift_oracle,
        rust_oracle,
        home,
        cache,
        out: root.join("conformance-out"),
    };

    let files = corpus_files(&corpus);
    let selected: Vec<&Suite> = suites
        .iter()
        .filter(|suite| options.suites.is_empty() || options.suites.contains(&suite.name))
        .collect();
    if selected.is_empty() {
        usage("no suite matches");
    }

    let mut any_failure = false;
    for suite in selected {
        let _ = fs::remove_dir_all(context.out.join(&suite.name));
        let mut cases: Vec<Case> = files
            .iter()
            .filter(|file| {
                let text = file.to_string_lossy();
                (suite.only.is_empty() || suite.only.iter().any(|only| text.contains(only.as_str())))
                    && options.filter.as_ref().is_none_or(|filter| text.contains(filter.as_str()))
            })
            .flat_map(|file| (0..suite.variants.len()).map(move |variant| Case { suite, input: file.clone(), variant }))
            .collect();
        if let Some(limit) = options.limit {
            cases.truncate(limit);
        }
        let jobs = if suite.serial { 1 } else { options.jobs.max(1) };
        let next = AtomicUsize::new(0);
        let stop = std::sync::atomic::AtomicBool::new(false);
        let results = Mutex::new(Vec::with_capacity(cases.len()));
        std::thread::scope(|scope| {
            for _ in 0..jobs {
                scope.spawn(|| {
                    loop {
                        if stop.load(Ordering::Relaxed) {
                            break;
                        }
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(case) = cases.get(index) else { break };
                        let (outcome, detail) = run_case(&context, case);
                        if options.fail_fast && matches!(outcome, Outcome::Fail | Outcome::Error) {
                            stop.store(true, Ordering::Relaxed);
                        }
                        results.lock().unwrap().push((index, outcome, detail));
                    }
                });
            }
        });
        let mut results = results.into_inner().unwrap();
        results.sort_by_key(|(index, ..)| *index);
        let count = |wanted: Outcome| results.iter().filter(|(_, outcome, _)| *outcome == wanted).count();
        let (pass, fail, not_ported, error) =
            (count(Outcome::Pass), count(Outcome::Fail), count(Outcome::NotPorted), count(Outcome::Error));
        println!(
            "{:<10} {:>5} cases  {:>5} pass  {:>5} fail  {:>5} error  {:>5} not ported",
            suite.name,
            results.len(),
            pass,
            fail,
            error,
            not_ported
        );
        for (index, outcome, detail) in results.iter().filter(|(_, outcome, _)| matches!(outcome, Outcome::Fail | Outcome::Error)).take(12) {
            let case = &cases[*index];
            let relative = case.input.strip_prefix(&root).unwrap_or(&case.input);
            println!(
                "  {:?} {} [{}]\n      {}",
                outcome,
                relative.display(),
                suite.variants[case.variant].join(" "),
                detail
            );
        }
        any_failure |= fail + error > 0;
    }
    if any_failure {
        println!("evidence: {}", context.out.display());
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
