//! Post-edit test-generation nudge.
//!
//! After the agent finishes a turn that touched source files, this module can
//! optionally scan the set of changed files and emit a single advisory line
//! naming source files that do not appear to have a co-located test, gently
//! suggesting that tests be generated for them.
//!
//! The whole feature is gated behind configuration and is **off by default** so
//! it never adds noise unless the user opts in. Enable it with the
//! `BHARATCODE_TESTGEN` boolean (env var or config parameter).
//!
//! The "lacks a test" decision is a deliberately cheap, filesystem-only
//! heuristic — it never executes anything and never reads file bodies beyond a
//! small marker scan. Status labels are localized via a small self-contained
//! locale resolver that mirrors the project's existing scaffold
//! (`BHARATCODE_LANG` → `bharatcode_lang` config → `LANG` → English). This
//! module is original work; nothing here is ported from third-party sources.

use std::path::{Path, PathBuf};

const ENABLE_KEY: &str = "BHARATCODE_TESTGEN";

/// Maximum number of file names listed in a single advisory line.
const MAX_LISTED: usize = 3;

/// Whether the nudge is enabled. Off by default.
///
/// Reads the raw `BHARATCODE_TESTGEN` environment variable first (truthy
/// values only), then falls back to the global config parameter of the same
/// name, then defaults to `false`.
pub fn is_enabled() -> bool {
    if let Ok(raw) = std::env::var(ENABLE_KEY) {
        return is_truthy(&raw);
    }
    crate::config::Config::global()
        .get_param::<bool>(ENABLE_KEY)
        .unwrap_or(false)
}

fn is_truthy(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Heuristic: does this changed source file look like it lacks a co-located test?
///
/// Returns `false` (i.e. "do not nag") for anything that is not a recognized
/// source file, that already lives under a `tests/` directory, or that is
/// itself a test file. For recognized source files we look for evidence of an
/// accompanying test:
///   * Rust: an inline `#[cfg(test)]` marker, a sibling `*_test.rs`, a sibling
///     `tests/` directory peer, or a workspace `tests/<stem>.rs` integration
///     test.
///   * Python: a sibling `test_<name>.py` / `<name>_test.py`.
///   * JS/TS: a sibling `<name>.test.<ext>` / `<name>.spec.<ext>`.
///
/// When no such evidence is found, returns `true`.
pub fn lacks_test(path: &Path) -> bool {
    let Some(kind) = source_kind(path) else {
        return false;
    };

    if is_under_tests_dir(path) || is_test_file(path, kind) {
        return false;
    }

    match kind {
        SourceKind::Rust => !rust_has_test(path),
        SourceKind::Python => !sibling_exists(path, &python_test_candidates(path)),
        SourceKind::Js => !sibling_exists(path, &js_test_candidates(path)),
    }
}

/// Produce a one-line advisory naming up to [`MAX_LISTED`] changed source files
/// that appear to lack a co-located test.
///
/// Returns `None` when the feature is disabled (the default) or when every
/// changed file already has a test, so the caller can wire it in with a single
/// `if let` and never emit anything unless opted in.
pub fn suggest_testgen(changed: &[PathBuf]) -> Option<String> {
    if !is_enabled() {
        return None;
    }

    let mut untested: Vec<String> = Vec::new();
    for path in changed {
        if lacks_test(path) {
            untested.push(display_name(path));
        }
        if untested.len() >= MAX_LISTED {
            break;
        }
    }

    if untested.is_empty() {
        return None;
    }

    Some(format!("{} {}", label(Label::Prefix), untested.join(", ")))
}

// ----------------------------------------------------------------------------
// Source classification and the cheap "has a test" heuristic.
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceKind {
    Rust,
    Python,
    Js,
}

fn source_kind(path: &Path) -> Option<SourceKind> {
    match extension(path).as_deref() {
        Some("rs") => Some(SourceKind::Rust),
        Some("py") => Some(SourceKind::Python),
        Some("js") | Some("jsx") | Some("ts") | Some("tsx") | Some("mjs") | Some("cjs") => {
            Some(SourceKind::Js)
        }
        _ => None,
    }
}

fn extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
}

fn file_stem(path: &Path) -> Option<String> {
    path.file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
}

fn file_name(path: &Path) -> Option<String> {
    path.file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
}

/// Is any path component a `tests` (or `__tests__`) directory?
fn is_under_tests_dir(path: &Path) -> bool {
    path.components()
        .filter_map(|c| c.as_os_str().to_str())
        .any(|c| c == "tests" || c == "__tests__")
}

fn is_test_file(path: &Path, kind: SourceKind) -> bool {
    let Some(name) = file_name(path) else {
        return false;
    };
    let lower = name.to_ascii_lowercase();
    match kind {
        SourceKind::Rust => lower.ends_with("_test.rs"),
        SourceKind::Python => lower.starts_with("test_") || lower.ends_with("_test.py"),
        SourceKind::Js => {
            let stem = file_stem(path).unwrap_or_default().to_ascii_lowercase();
            stem.ends_with(".test") || stem.ends_with(".spec")
        }
    }
}

fn rust_has_test(path: &Path) -> bool {
    if has_cfg_test_marker(path) {
        return true;
    }

    let Some(stem) = file_stem(path) else {
        return false;
    };

    // Sibling `<stem>_test.rs` in the same directory.
    if let Some(dir) = path.parent() {
        if dir.join(format!("{stem}_test.rs")).is_file() {
            return true;
        }
        // A `tests/` directory peer beside the source file.
        let peer = dir.join("tests");
        if peer.join(format!("{stem}.rs")).is_file()
            || peer.join(format!("{stem}_test.rs")).is_file()
        {
            return true;
        }
    }

    false
}

/// Cheaply scan a Rust file for a `#[cfg(test)]` attribute. Reads the file as a
/// string but only checks for the literal marker; missing/unreadable files
/// count as "no marker" so we never panic on transient paths.
fn has_cfg_test_marker(path: &Path) -> bool {
    match std::fs::read_to_string(path) {
        Ok(body) => body.contains("#[cfg(test)]"),
        Err(_) => false,
    }
}

fn python_test_candidates(path: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let (Some(dir), Some(stem)) = (path.parent(), file_stem(path)) else {
        return out;
    };
    out.push(dir.join(format!("test_{stem}.py")));
    out.push(dir.join(format!("{stem}_test.py")));
    out.push(dir.join("tests").join(format!("test_{stem}.py")));
    out.push(dir.join("tests").join(format!("{stem}_test.py")));
    out
}

fn js_test_candidates(path: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let (Some(dir), Some(stem), Some(ext)) = (path.parent(), file_stem(path), extension(path))
    else {
        return out;
    };
    for suffix in ["test", "spec"] {
        out.push(dir.join(format!("{stem}.{suffix}.{ext}")));
        out.push(dir.join("__tests__").join(format!("{stem}.{suffix}.{ext}")));
    }
    out
}

fn sibling_exists(_source: &Path, candidates: &[PathBuf]) -> bool {
    candidates.iter().any(|c| c.is_file())
}

/// A short, human-friendly name for the advisory line: the file name when
/// available, otherwise the full path as given.
fn display_name(path: &Path) -> String {
    file_name(path).unwrap_or_else(|| path.to_string_lossy().to_string())
}

// ----------------------------------------------------------------------------
// Localization for the single user-facing advisory prefix.
// ----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
enum Locale {
    En,
    Hi,
}

#[derive(Debug, Clone, Copy)]
enum Label {
    Prefix,
}

fn label(which: Label) -> String {
    let s = match (active_locale(), which) {
        (Locale::En, Label::Prefix) => "Consider generating tests for:",
        (Locale::Hi, Label::Prefix) => "इनके लिए परीक्षण बनाने पर विचार करें:",
    };
    s.to_string()
}

fn normalize_locale(raw: &str) -> Locale {
    let lowered = raw.trim().to_ascii_lowercase();
    let primary = lowered.split(['_', '-', '.']).next().unwrap_or("");
    match primary {
        "hi" => Locale::Hi,
        _ => Locale::En,
    }
}

fn active_locale() -> Locale {
    if let Some(loc) = std::env::var("BHARATCODE_LANG")
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        return normalize_locale(&loc);
    }
    if let Some(loc) = crate::config::Config::global()
        .get_param::<String>("bharatcode_lang")
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        return normalize_locale(&loc);
    }
    if let Some(loc) = std::env::var("LANG").ok().filter(|s| !s.trim().is_empty()) {
        return normalize_locale(&loc);
    }
    Locale::En
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "bc_testgen_{}_{}_{:?}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn truthy_only_for_known_affirmatives() {
        assert!(is_truthy("1"));
        assert!(is_truthy("true"));
        assert!(is_truthy(" YES "));
        assert!(is_truthy("On"));
        assert!(!is_truthy("0"));
        assert!(!is_truthy("false"));
        assert!(!is_truthy(""));
        assert!(!is_truthy("maybe"));
    }

    #[test]
    fn lacks_test_true_for_bare_lib_rs() {
        let dir = unique_dir("bare");
        let lib = dir.join("lib.rs");
        std::fs::write(&lib, "pub fn add(a: i32, b: i32) -> i32 { a + b }\n").unwrap();
        assert!(lacks_test(&lib));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn lacks_test_false_for_path_under_tests_dir() {
        let dir = unique_dir("undertests");
        let tests = dir.join("tests");
        std::fs::create_dir_all(&tests).unwrap();
        let integ = tests.join("integration.rs");
        std::fs::write(&integ, "#[test] fn it_works() {}\n").unwrap();
        assert!(!lacks_test(&integ));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn lacks_test_false_when_inline_cfg_test_present() {
        let dir = unique_dir("inline");
        let src = dir.join("widget.rs");
        std::fs::write(
            &src,
            "pub fn f() {}\n#[cfg(test)]\nmod tests { #[test] fn t() {} }\n",
        )
        .unwrap();
        assert!(!lacks_test(&src));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn lacks_test_false_with_sibling_test_file() {
        let dir = unique_dir("sibling");
        let src = dir.join("parser.rs");
        std::fs::write(&src, "pub fn parse() {}\n").unwrap();
        std::fs::write(dir.join("parser_test.rs"), "#[test] fn t() {}\n").unwrap();
        assert!(!lacks_test(&src));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn lacks_test_ignores_non_source_files() {
        assert!(!lacks_test(Path::new("README.md")));
        assert!(!lacks_test(Path::new("Cargo.toml")));
        assert!(!lacks_test(Path::new("data.json")));
    }

    #[test]
    fn suggest_none_when_disabled() {
        let _guard = env_lock::lock_env([(ENABLE_KEY, Some("false"))]);
        let changed = vec![PathBuf::from("src/lib.rs")];
        assert_eq!(suggest_testgen(&changed), None);
    }

    #[test]
    fn suggest_some_naming_untested_file_when_enabled() {
        let dir = unique_dir("enabled");
        let lib = dir.join("lib.rs");
        std::fs::write(&lib, "pub fn add() {}\n").unwrap();

        let _guard = env_lock::lock_env([(ENABLE_KEY, Some("1"))]);
        let changed = vec![lib.clone()];
        let out = suggest_testgen(&changed).expect("advisory expected when enabled");
        assert!(
            out.contains("lib.rs"),
            "advisory should name the file: {out}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn suggest_none_when_all_have_tests_even_if_enabled() {
        let dir = unique_dir("allcovered");
        let src = dir.join("engine.rs");
        std::fs::write(
            &src,
            "pub fn run() {}\n#[cfg(test)]\nmod t { #[test] fn x() {} }\n",
        )
        .unwrap();

        let _guard = env_lock::lock_env([(ENABLE_KEY, Some("true"))]);
        let changed = vec![src.clone()];
        assert_eq!(suggest_testgen(&changed), None);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn suggest_caps_list_at_three() {
        let dir = unique_dir("cap");
        let mut changed = Vec::new();
        for i in 0..5 {
            let f = dir.join(format!("mod{i}.rs"));
            std::fs::write(&f, "pub fn f() {}\n").unwrap();
            changed.push(f);
        }

        let _guard = env_lock::lock_env([(ENABLE_KEY, Some("yes"))]);
        let out = suggest_testgen(&changed).expect("advisory expected");
        let listed = out.matches(".rs").count();
        assert_eq!(listed, MAX_LISTED, "should cap the listed files: {out}");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn normalize_locale_maps_hindi_variants() {
        assert!(matches!(normalize_locale("hi"), Locale::Hi));
        assert!(matches!(normalize_locale("hi_IN.UTF-8"), Locale::Hi));
        assert!(matches!(normalize_locale("en_US"), Locale::En));
        assert!(matches!(normalize_locale("fr"), Locale::En));
    }
}
