//! Compiles `vendor/swift-cmark` the way its own `Package.swift` does: every
//! C file in `src/` and `extensions/` (the re2c `.re` sources are already
//! generated into `scanners.c` / `ext_scanners.c`), with the checked-in
//! headers under `src/include` and `extensions/include`.
use std::path::{Path, PathBuf};

fn c_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("reading {}: {error}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "c"))
        .collect();
    files.sort();
    files
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/swift-cmark");
    let src = root.join("src");
    let extensions = root.join("extensions");
    assert!(
        src.join("blocks.c").exists(),
        "vendor/swift-cmark is missing; run `git submodule update --init`"
    );
    println!("cargo:rerun-if-changed={}", src.display());
    println!("cargo:rerun-if-changed={}", extensions.display());

    let mut build = cc::Build::new();
    build
        .include(src.join("include"))
        .include(extensions.join("include"))
        .include(&src)
        .include(&extensions)
        .define("CMARK_GFM_STATIC_DEFINE", None)
        .define("CMARK_GFM_EXTENSIONS_STATIC_DEFINE", None)
        .flag_if_supported("-Wno-unused-parameter")
        .flag_if_supported("-Wno-sign-compare")
        .warnings(false)
        .opt_level(3);
    for file in c_files(&src).into_iter().chain(c_files(&extensions)) {
        build.file(file);
    }
    build.compile("cmark-gfm");
}
