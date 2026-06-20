//! Editor/IDE bridge: `editor_locator` developer tool.
//!
//! Given a file path and an optional line/column, this read-only tool builds a
//! set of editor-jump targets so an external IDE/editor bridge can open exactly
//! where the agent points:
//!
//! * `vscode_uri`  — a `vscode://file/<abs>:<line>:<col>` deep-link URI,
//! * `vscode_cli`  — the equivalent `code -g <abs>:<line>:<col>` CLI form,
//! * `jetbrains_cli` — a `--line`/`--column` invocation for JetBrains IDEs,
//! * `generic`     — a plain `<abs>:<line>` form understood by most tooling,
//! * `abs_path`    — the resolved absolute path.
//!
//! It never spawns an editor and never reads the file's contents; it only
//! resolves the path (relative paths are joined onto the working directory) and
//! formats link strings. Invalid line/column values are clamped to the smallest
//! valid 1-based index. The tool is therefore always available (no opt-in gate),
//! mirroring `read_lines`.

use std::path::{Path, PathBuf};

use rmcp::model::{CallToolResult, Content};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::mcp_utils::ToolResult;

/// Input schema for the `editor_locator` tool. The handler reads the raw JSON
/// value directly; this struct exists so the tool advertises a typed schema.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct EditorLocatorParams {
    /// Path to the file to locate. Resolved relative to the working directory
    /// when not absolute. No file contents are read.
    pub path: String,
    /// Optional 1-based line to jump to. Values below 1 are clamped to 1.
    #[serde(default)]
    pub line: Option<u64>,
    /// Optional 1-based column to jump to (only used when `line` is present).
    /// Values below 1 are clamped to 1.
    #[serde(default)]
    pub col: Option<u64>,
}

/// Editor-jump targets for a resolved path and optional position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorTargets {
    /// The resolved absolute path (or the input path verbatim if it could not
    /// be made absolute).
    pub abs_path: String,
    /// `vscode://file/<abs>[:line[:col]]` deep-link URI.
    pub vscode_uri: String,
    /// `code -g <abs>[:line[:col]]` CLI form.
    pub vscode_cli: String,
    /// JetBrains CLI form using `--line`/`--column` flags.
    pub jetbrains_cli: String,
    /// Generic `<abs>[:line]` form understood by most tooling.
    pub generic: String,
}

/// Clamp a user-supplied 1-based position to a minimum of 1.
///
/// A supplied `0` (or absent value handled by the caller) is invalid as a
/// 1-based editor coordinate, so it is raised to `1`.
fn clamp_pos(value: u64) -> u64 {
    value.max(1)
}

/// Build the editor-jump [`EditorTargets`] for `path` at an optional
/// `line`/`col`.
///
/// `path` should already be absolute; callers that accept relative input should
/// resolve it against a working directory first (see [`editor_locator`]). Line
/// and column are 1-based; out-of-range values (e.g. `0`) are clamped to `1`. A
/// missing line omits the position suffix entirely (a column without a line is
/// ignored, since `file:col` is ambiguous).
pub fn build_targets(path: &Path, line: Option<u64>, col: Option<u64>) -> EditorTargets {
    let abs_path = path.display().to_string();

    let line = line.map(clamp_pos);
    let col = col.map(clamp_pos);

    // Position suffix shared by the URI / CLI / generic forms. Without a line we
    // emit no suffix at all.
    let (line_col_suffix, line_only_suffix) = match line {
        Some(l) => match col {
            Some(c) => (format!(":{l}:{c}"), format!(":{l}")),
            None => (format!(":{l}"), format!(":{l}")),
        },
        None => (String::new(), String::new()),
    };

    let vscode_uri = format!("vscode://file/{abs_path}{line_col_suffix}");
    let vscode_cli = format!("code -g {abs_path}{line_col_suffix}");
    let generic = format!("{abs_path}{line_only_suffix}");

    let jetbrains_cli = match line {
        Some(l) => match col {
            Some(c) => format!("idea --line {l} --column {c} {abs_path}"),
            None => format!("idea --line {l} {abs_path}"),
        },
        None => format!("idea {abs_path}"),
    };

    EditorTargets {
        abs_path,
        vscode_uri,
        vscode_cli,
        jetbrains_cli,
        generic,
    }
}

/// Resolve `params.path` (relative paths are joined onto `working_dir`) and
/// return the editor-jump targets as a text block plus structured content.
///
/// Mirrors the return shape of `read_lines`: a single text content block with a
/// human-readable summary and a `structured_content` object carrying every
/// individual target. Read-only — no file contents are read and no process is
/// spawned.
pub fn editor_locator(
    params: serde_json::Value,
    working_dir: Option<&Path>,
) -> ToolResult<CallToolResult> {
    let path_str = match params.get("path").and_then(|v| v.as_str()) {
        Some(p) if !p.is_empty() => p.to_string(),
        _ => return Ok(error_result("Missing required `path` argument")),
    };

    let line = params.get("line").and_then(|v| v.as_u64());
    let col = params.get("col").and_then(|v| v.as_u64());

    let path = PathBuf::from(&path_str);
    let resolved = if path.is_absolute() {
        path
    } else {
        working_dir
            .map(Path::to_path_buf)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."))
            .join(path)
    };

    let targets = build_targets(&resolved, line, col);

    let summary = format!(
        "Editor jump targets for {abs}:\n  VS Code URI: {uri}\n  VS Code CLI: {cli}\n  JetBrains CLI: {jb}\n  Generic: {generic}",
        abs = targets.abs_path,
        uri = targets.vscode_uri,
        cli = targets.vscode_cli,
        jb = targets.jetbrains_cli,
        generic = targets.generic,
    );

    let mut result = CallToolResult::success(vec![Content::text(summary).with_priority(0.0)]);
    result.structured_content = Some(json!({
        "abs_path": targets.abs_path,
        "vscode_uri": targets.vscode_uri,
        "vscode_cli": targets.vscode_cli,
        "jetbrains_cli": targets.jetbrains_cli,
        "generic": targets.generic,
        "line": line.map(clamp_pos),
        "col": col.map(clamp_pos),
    }));
    Ok(result)
}

fn error_result(message: &str) -> CallToolResult {
    CallToolResult::error(vec![
        Content::text(format!("Error: {message}")).with_priority(0.0)
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::RawContent;
    use std::path::Path;

    fn assert_no_brand(targets: &EditorTargets) {
        for s in [
            &targets.abs_path,
            &targets.vscode_uri,
            &targets.vscode_cli,
            &targets.jetbrains_cli,
            &targets.generic,
        ] {
            assert!(
                !s.to_lowercase().contains("goose"),
                "target string must not leak the upstream brand: {s}"
            );
        }
    }

    #[test]
    fn build_targets_with_line_and_col() {
        let targets = build_targets(Path::new("/abs/foo.rs"), Some(42), Some(3));

        assert!(
            targets.vscode_uri.ends_with("foo.rs:42:3"),
            "uri was {}",
            targets.vscode_uri
        );
        assert_eq!(targets.vscode_cli, "code -g /abs/foo.rs:42:3");
        assert_eq!(targets.generic, "/abs/foo.rs:42");
        assert_eq!(
            targets.jetbrains_cli,
            "idea --line 42 --column 3 /abs/foo.rs"
        );
        assert_eq!(targets.abs_path, "/abs/foo.rs");
        assert_no_brand(&targets);
    }

    #[test]
    fn missing_line_omits_position_suffix() {
        let targets = build_targets(Path::new("/abs/foo.rs"), None, None);

        assert_eq!(targets.vscode_uri, "vscode://file//abs/foo.rs");
        assert_eq!(targets.vscode_cli, "code -g /abs/foo.rs");
        assert_eq!(targets.generic, "/abs/foo.rs");
        assert_eq!(targets.jetbrains_cli, "idea /abs/foo.rs");
        assert_no_brand(&targets);
    }

    #[test]
    fn column_without_line_is_ignored() {
        // A column without a line is ambiguous as `file:col`, so the position
        // suffix is omitted entirely.
        let targets = build_targets(Path::new("/abs/foo.rs"), None, Some(7));

        assert_eq!(targets.generic, "/abs/foo.rs");
        assert_eq!(targets.vscode_cli, "code -g /abs/foo.rs");
        assert_eq!(targets.jetbrains_cli, "idea /abs/foo.rs");
        assert_no_brand(&targets);
    }

    #[test]
    fn line_with_column_includes_both() {
        let targets = build_targets(Path::new("/abs/foo.rs"), Some(10), None);

        assert_eq!(targets.vscode_cli, "code -g /abs/foo.rs:10");
        assert_eq!(targets.generic, "/abs/foo.rs:10");
        assert!(targets.vscode_uri.ends_with("foo.rs:10"));
        assert_eq!(targets.jetbrains_cli, "idea --line 10 /abs/foo.rs");
        assert_no_brand(&targets);
    }

    #[test]
    fn invalid_zero_position_is_clamped() {
        let targets = build_targets(Path::new("/abs/foo.rs"), Some(0), Some(0));

        // 0 is invalid as a 1-based editor coordinate and is raised to 1.
        assert_eq!(targets.vscode_cli, "code -g /abs/foo.rs:1:1");
        assert_eq!(targets.generic, "/abs/foo.rs:1");
        assert!(targets.vscode_uri.ends_with("foo.rs:1:1"));
        assert_no_brand(&targets);
    }

    fn structured(result: &CallToolResult) -> &serde_json::Value {
        result
            .structured_content
            .as_ref()
            .expect("structured_content should be present")
    }

    fn text(result: &CallToolResult) -> &str {
        match &result.content[0].raw {
            RawContent::Text(t) => &t.text,
            _ => panic!("expected text content"),
        }
    }

    #[test]
    fn editor_locator_absolute_path_returns_targets() {
        let result =
            editor_locator(json!({ "path": "/abs/foo.rs", "line": 42, "col": 3 }), None).unwrap();

        assert_eq!(result.is_error, Some(false));
        let sc = structured(&result);
        assert_eq!(sc["abs_path"], "/abs/foo.rs");
        assert_eq!(sc["vscode_cli"], "code -g /abs/foo.rs:42:3");
        assert_eq!(sc["generic"], "/abs/foo.rs:42");
        assert!(sc["vscode_uri"].as_str().unwrap().ends_with("foo.rs:42:3"));

        let body = text(&result);
        assert!(body.contains("/abs/foo.rs:42:3"));
        assert!(!body.to_lowercase().contains("goose"));
    }

    #[test]
    fn editor_locator_resolves_relative_to_working_dir() {
        let dir = tempfile::tempdir().unwrap();
        let result = editor_locator(
            json!({ "path": "src/main.rs", "line": 5 }),
            Some(dir.path()),
        )
        .unwrap();

        let sc = structured(&result);
        let expected = dir.path().join("src/main.rs");
        assert_eq!(sc["abs_path"], expected.display().to_string());
        assert_eq!(sc["generic"], format!("{}:5", expected.display()));
    }

    #[test]
    fn editor_locator_missing_path_errors() {
        let result = editor_locator(json!({ "line": 5 }), None).unwrap();
        assert_eq!(result.is_error, Some(true));
        assert!(text(&result).contains("path"));
    }
}
