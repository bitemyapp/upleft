//! Builds the Swift shim (`swift-shim/`, see PORTING.md) as a static library
//! and links it, with the Swift runtime, into every binary that depends on
//! `upleft-app`.
//!
//! - `swift build -c release` with its scratch directory under `OUT_DIR`, so
//!   Cargo's own caching and `cargo clean` cover it; SwiftPM rebuilds
//!   incrementally when a shim source changes.
//! - The triple follows Cargo's target architecture, at Downright's
//!   deployment target (macOS 14.0), matching `MACOSX_DEPLOYMENT_TARGET` in
//!   `.cargo/config.toml`.
//! - The Swift object carries `LC_LINKER_OPTION` autolink entries
//!   (`-lswiftCore`, `-framework FoundationModels`, …). The linker honours
//!   them; this script only adds the search paths `swiftc` itself would pass
//!   (`swift -print-target-info`'s runtime library paths and the SDK's
//!   `usr/lib/swift`). `rustc-link-search` and `rustc-link-lib` propagate to
//!   dependent crates' binaries and tests, unlike `rustc-link-arg`.
//! - No stamping: a static library has no `LC_BUILD_VERSION` of its own that
//!   AppKit reads. The object inside records `minos 14.0`; the executable's
//!   build version still comes from the Rust link (docs/BUILD-VERSION.md).

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let shim = manifest.join("swift-shim");
    println!("cargo:rerun-if-changed={}", shim.join("Package.swift").display());
    println!("cargo:rerun-if-changed={}", shim.join("Sources").display());
    println!("cargo:rerun-if-env-changed=DEVELOPER_DIR");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let arch = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("aarch64") => "arm64",
        Ok("x86_64") => "x86_64",
        Ok(other) => panic!("upleft-app: no Swift triple for target architecture {other}"),
        Err(_) => panic!("upleft-app: CARGO_CFG_TARGET_ARCH is not set"),
    };
    let triple = format!("{arch}-apple-macosx14.0");
    let scratch = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("swift-shim");

    let swift_build = |extra: &[&str]| -> std::process::Output {
        let mut command = Command::new("swift");
        command
            .arg("build")
            .args(["-c", "release"])
            .arg("--package-path")
            .arg(&shim)
            .arg("--scratch-path")
            .arg(&scratch)
            .args(["--triple", &triple])
            .args(extra)
            // Cargo's own compiler settings are not the shim's business.
            .env_remove("RUSTFLAGS")
            .env_remove("CARGO_ENCODED_RUSTFLAGS");
        command.output().unwrap_or_else(|error| panic!("upleft-app: could not run `swift build`: {error}"))
    };

    let built = swift_build(&[]);
    if !built.status.success() {
        panic!(
            "upleft-app: `swift build` of {} failed\n--- stdout\n{}\n--- stderr\n{}",
            shim.display(),
            String::from_utf8_lossy(&built.stdout),
            String::from_utf8_lossy(&built.stderr)
        );
    }
    let bin = swift_build(&["--show-bin-path"]);
    assert!(bin.status.success(), "upleft-app: `swift build --show-bin-path` failed");
    let bin_path = PathBuf::from(String::from_utf8_lossy(&bin.stdout).trim());
    assert!(
        bin_path.join("libUpleftSwiftShim.a").is_file(),
        "upleft-app: {} has no libUpleftSwiftShim.a",
        bin_path.display()
    );

    println!("cargo:rustc-link-search=native={}", bin_path.display());
    println!("cargo:rustc-link-lib=static=UpleftSwiftShim");

    for path in swift_runtime_library_paths(&triple) {
        println!("cargo:rustc-link-search=native={}", path.display());
    }
    if let Some(sdk) = command_line("xcrun", &["--sdk", "macosx", "--show-sdk-path"]) {
        let sdk_swift = Path::new(&sdk).join("usr/lib/swift");
        if sdk_swift.is_dir() {
            println!("cargo:rustc-link-search=native={}", sdk_swift.display());
        }
    }
}

/// `runtimeLibraryPaths` from `swift -print-target-info -target <triple>`:
/// the toolchain's `usr/lib/swift/macosx` and `/usr/lib/swift`.
fn swift_runtime_library_paths(triple: &str) -> Vec<PathBuf> {
    let Some(info) = command_line("swift", &["-print-target-info", "-target", triple]) else {
        panic!("upleft-app: `swift -print-target-info` failed");
    };
    // A small scan instead of a JSON dependency: the array holds plain paths.
    let Some(start) = info.find("\"runtimeLibraryPaths\"") else {
        return Vec::new();
    };
    let rest = &info[start..];
    let (Some(open), Some(close)) = (rest.find('['), rest.find(']')) else {
        return Vec::new();
    };
    rest[open + 1..close]
        .split(',')
        .map(|entry| entry.trim().trim_matches('"').replace("\\/", "/"))
        .filter(|entry| !entry.is_empty())
        .map(PathBuf::from)
        .collect()
}

fn command_line(program: &str, arguments: &[&str]) -> Option<String> {
    let output = Command::new(program).args(arguments).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
