//! Helpers for format-crate tests that round-trip every matching file from the game.
//!
//! Tests read the unpacked tree from `$SCRIBBLE_EXTRACTED` (default: `<workspace>/extracted`,
//! produced by `scribble unpack game extracted`). If it is missing, tests are skipped.

use crate::{format::assert_roundtrip, Context, Format, ResPath};
use std::path::{Path, PathBuf};

pub fn extracted_root() -> Option<PathBuf> {
    let root = std::env::var_os("SCRIBBLE_EXTRACTED")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../extracted"));
    root.join("manifest.json").exists().then_some(root)
}

/// Builds the full [`Context`] (resource names plus the id namespaces derived from other files)
/// for an unpacked directory. Core cannot depend on the format crates that know how to do that,
/// so the loader is installed from outside: `scribble_formats::install_test_context()` (format
/// crates' tests call it; it installs `scribble_formats::load_context`).
pub type ContextLoader = fn(&Path) -> crate::Result<Context>;

static LOADER: std::sync::OnceLock<ContextLoader> = std::sync::OnceLock::new();
static CONTEXT: std::sync::Mutex<Option<std::sync::Arc<Context>>> = std::sync::Mutex::new(None);

/// Install the loader [`context`] uses (first call wins).
pub fn set_context_loader(loader: ContextLoader) {
    let _ = LOADER.set(loader);
}

/// The context for the unpacked tree: built by the installed loader (see [`set_context_loader`])
/// and cached, or just the resource names from `extracted/manifest.json` when none is installed.
pub fn context() -> Context {
    let Some(root) = extracted_root() else { return Context::empty() };
    if let Some(loader) = LOADER.get() {
        let mut cache = CONTEXT.lock().unwrap();
        if cache.is_none() {
            *cache = Some(std::sync::Arc::new(loader(&root).expect("loading the test context")));
        }
        return Context::clone(cache.as_ref().unwrap());
    }
    names_context(&root)
}

/// Just the resource names from `<root>/manifest.json`.
pub fn names_context(root: &Path) -> Context {
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(root.join("manifest.json")).unwrap()).unwrap();
    Context::from_names(v["files"].as_array().unwrap().iter().map(|f| {
        (f["index"].as_u64().unwrap() as u32, f["path"].as_str().unwrap().to_string())
    }))
}

/// Every unpacked resource `(logical path, disk path)` accepted by `pred`.
pub fn resources(pred: impl Fn(&ResPath) -> bool) -> Vec<(String, PathBuf)> {
    let Some(root) = extracted_root() else { return Vec::new() };
    let mut out = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else if p.file_name().unwrap() != "manifest.json" {
                let rel = p.strip_prefix(&root).unwrap();
                let logical = rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("\\");
                if pred(&ResPath::new(&logical)) {
                    out.push((logical, p));
                }
            }
        }
    }
    out.sort();
    out
}

/// Round-trip every resource matching `pred` through `T`, panicking with a summary of failures.
/// Returns the number of files checked.
pub fn check_all<T: Format>(pred: impl Fn(&ResPath) -> bool) -> usize {
    let files = resources(pred);
    if files.is_empty() {
        eprintln!("no extracted resources found for {}; skipping", T::NAME);
        return 0;
    }
    let ctx = context();
    let mut failures = Vec::new();
    for (logical, path) in &files {
        let data = std::fs::read(path).unwrap();
        if let Err(e) = assert_roundtrip::<T>(&data, &ctx) {
            failures.push(format!("{logical}: {e:#}"));
        }
    }
    if !failures.is_empty() {
        for f in failures.iter().take(20) {
            eprintln!("FAIL {f}");
        }
        panic!("{}: {}/{} files failed to round-trip", T::NAME, failures.len(), files.len());
    }
    eprintln!("{}: {} files round-trip", T::NAME, files.len());
    files.len()
}
