//! Chunked large-file reader (`read_lines`).
//!
//! Reads a bounded line-range window from a single file so the agent can
//! navigate huge files without pulling the whole thing into context. The
//! window is described by `offset` (number of leading lines to skip) and
//! `limit` (maximum number of lines to return, default 200, clamped to a
//! ceiling of ~2000). Regardless of `limit`, the returned slice is also
//! bounded by a hard byte cap so a single pathologically long line cannot
//! blow up memory: the cap defaults to 256 KiB and can be tuned via the
//! `BHARATCODE_READ_LINES_MAX_BYTES` environment variable.
//!
//! The tool only ever reads, is byte-bounded, and refuses non-UTF8 / binary
//! input gracefully, so it is always available (no opt-in gate). The env var
//! is a tuning knob only; it never toggles the tool on or off.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use rmcp::model::{CallToolResult, Content};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::mcp_utils::ToolResult;

/// Environment variable that tunes the hard byte cap (tuning only; the tool is
/// always present regardless of this value).
pub const READ_LINES_MAX_BYTES_ENV: &str = "BHARATCODE_READ_LINES_MAX_BYTES";

/// Default hard byte cap for a single `read_lines` window: 256 KiB.
pub const DEFAULT_MAX_BYTES: usize = 256 * 1024;

/// Default number of lines returned when `limit` is omitted.
pub const DEFAULT_LIMIT: usize = 200;

/// Upper bound on the number of lines a single call may return.
pub const MAX_LIMIT: usize = 2000;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadLinesParams {
    /// Path to the file to read. Resolved relative to the working directory
    /// when not absolute.
    pub path: String,
    /// Number of leading lines to skip before the returned window (0-based).
    /// Defaults to 0.
    #[serde(default)]
    pub offset: Option<usize>,
    /// Maximum number of lines to return. Defaults to 200, clamped to 2000.
    #[serde(default)]
    pub limit: Option<usize>,
}

pub struct ReadLinesTool;

impl ReadLinesTool {
    pub fn new() -> Self {
        Self
    }

    /// Resolve `params.path` under `cwd` (when relative) and read the requested
    /// line window.
    pub fn read_lines_with_cwd(
        &self,
        params: ReadLinesParams,
        cwd: Option<&Path>,
    ) -> ToolResult<CallToolResult> {
        let path = PathBuf::from(&params.path);
        let resolved = if path.is_absolute() {
            path
        } else {
            cwd.map(Path::to_path_buf)
                .or_else(|| std::env::current_dir().ok())
                .unwrap_or_else(|| PathBuf::from("."))
                .join(path)
        };

        Ok(self.read_window(&resolved, params.offset.unwrap_or(0), params.limit))
    }

    fn read_window(&self, path: &Path, offset: usize, limit: Option<usize>) -> CallToolResult {
        if !path.exists() {
            return error_result(&format!("Path does not exist: {}", path.display()));
        }
        if !path.is_file() {
            return error_result(&format!("Path is not a file: {}", path.display()));
        }

        let limit = limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT);
        let max_bytes = max_bytes_from_env();

        let file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(err) => {
                return error_result(&format!("Failed to open {}: {err}", path.display()));
            }
        };
        let mut reader = BufReader::new(file);

        let mut line_index: usize = 0;
        let mut collected = String::new();
        let mut returned_lines: usize = 0;
        let mut byte_capped = false;
        let mut window_full = false;
        let mut buf: Vec<u8> = Vec::new();

        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) => break,
                Ok(_) => {}
                Err(err) => {
                    return error_result(&format!("Failed to read {}: {err}", path.display()));
                }
            }

            let in_window = line_index >= offset && !window_full;

            if in_window {
                let chunk = match std::str::from_utf8(&buf) {
                    Ok(text) => text,
                    Err(_) => {
                        return error_result(&format!(
                            "Refusing to read {}: file is not valid UTF-8 (binary content)",
                            path.display()
                        ));
                    }
                };

                if collected.len() + chunk.len() > max_bytes {
                    let remaining = max_bytes.saturating_sub(collected.len());
                    if remaining > 0 {
                        let mut take = remaining;
                        while take > 0 && !chunk.is_char_boundary(take) {
                            take -= 1;
                        }
                        collected.push_str(
                            chunk
                                .get(..take)
                                .expect("take is adjusted to a UTF-8 character boundary"),
                        );
                    }
                    byte_capped = true;
                    window_full = true;
                } else {
                    collected.push_str(chunk);
                    returned_lines += 1;
                    if returned_lines >= limit {
                        window_full = true;
                    }
                }
            } else if std::str::from_utf8(&buf).is_err() {
                return error_result(&format!(
                    "Refusing to read {}: file is not valid UTF-8 (binary content)",
                    path.display()
                ));
            }

            line_index += 1;
        }

        let total_lines = line_index;
        // The window did not reach EOF if the byte cap stopped it, or if more
        // lines exist beyond the returned window.
        let truncated = byte_capped || total_lines > offset.saturating_add(returned_lines);

        let summary = if returned_lines == 0 {
            format!(
                "No lines returned (offset {offset} of {total_lines} total line{}).",
                plural(total_lines)
            )
        } else {
            let first = offset + 1;
            let last = offset + returned_lines;
            format!(
                "Lines {first}-{last} of {total_lines}{}:\n{collected}",
                if truncated { " (truncated)" } else { "" }
            )
        };

        let mut result = CallToolResult::success(vec![Content::text(summary).with_priority(0.0)]);
        result.structured_content = Some(json!({
            "path": path.display().to_string(),
            "offset": offset,
            "limit": limit,
            "returned_lines": returned_lines,
            "total_lines": total_lines,
            "truncated": truncated,
            "byte_capped": byte_capped,
            "max_bytes": max_bytes,
        }));
        result
    }
}

impl Default for ReadLinesTool {
    fn default() -> Self {
        Self::new()
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

fn error_result(message: &str) -> CallToolResult {
    CallToolResult::error(vec![
        Content::text(format!("Error: {message}")).with_priority(0.0)
    ])
}

/// Resolve the hard byte cap from `BHARATCODE_READ_LINES_MAX_BYTES`, falling
/// back to [`DEFAULT_MAX_BYTES`]. A missing, empty, unparseable, or zero value
/// yields the default so the cap can never be disabled outright.
fn max_bytes_from_env() -> usize {
    std::env::var(READ_LINES_MAX_BYTES_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(DEFAULT_MAX_BYTES)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::RawContent;
    use std::fs;
    use tempfile::TempDir;

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

    fn ten_line_file() -> (TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.txt");
        let body: String = (1..=10).map(|n| format!("line {n}\n")).collect();
        fs::write(&path, body).unwrap();
        (dir, path)
    }

    #[test]
    fn reads_first_three_lines_and_reports_total() {
        let (_dir, path) = ten_line_file();
        let tool = ReadLinesTool::new();

        let result = tool
            .read_lines_with_cwd(
                ReadLinesParams {
                    path: path.display().to_string(),
                    offset: Some(0),
                    limit: Some(3),
                },
                None,
            )
            .unwrap();

        let sc = structured(&result);
        assert_eq!(sc["returned_lines"], 3);
        assert_eq!(sc["total_lines"], 10);
        assert_eq!(sc["truncated"], true);

        let body = text(&result);
        assert!(body.contains("line 1"));
        assert!(body.contains("line 2"));
        assert!(body.contains("line 3"));
        assert!(!body.contains("line 4"));
    }

    #[test]
    fn offset_past_eof_returns_empty_and_not_truncated() {
        let (_dir, path) = ten_line_file();
        let tool = ReadLinesTool::new();

        let result = tool
            .read_lines_with_cwd(
                ReadLinesParams {
                    path: path.display().to_string(),
                    offset: Some(100),
                    limit: Some(5),
                },
                None,
            )
            .unwrap();

        let sc = structured(&result);
        assert_eq!(sc["returned_lines"], 0);
        assert_eq!(sc["total_lines"], 10);
        assert_eq!(sc["truncated"], false);
    }

    #[test]
    fn reads_full_window_to_eof_not_truncated() {
        let (_dir, path) = ten_line_file();
        let tool = ReadLinesTool::new();

        let result = tool
            .read_lines_with_cwd(
                ReadLinesParams {
                    path: path.display().to_string(),
                    offset: Some(8),
                    limit: Some(50),
                },
                None,
            )
            .unwrap();

        let sc = structured(&result);
        assert_eq!(sc["returned_lines"], 2);
        assert_eq!(sc["total_lines"], 10);
        assert_eq!(sc["truncated"], false);
    }

    #[test]
    fn byte_cap_stops_a_huge_line_and_flags_truncation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("huge.txt");
        // A single line far larger than any sane cap, plus a trailing newline.
        let huge: String = "x".repeat(2 * 1024 * 1024);
        fs::write(&path, format!("{huge}\n")).unwrap();

        std::env::set_var(READ_LINES_MAX_BYTES_ENV, "4096");
        let tool = ReadLinesTool::new();
        let result = tool
            .read_lines_with_cwd(
                ReadLinesParams {
                    path: path.display().to_string(),
                    offset: Some(0),
                    limit: Some(10),
                },
                None,
            )
            .unwrap();
        std::env::remove_var(READ_LINES_MAX_BYTES_ENV);

        let sc = structured(&result);
        assert_eq!(sc["truncated"], true);
        assert_eq!(sc["byte_capped"], true);
        let body = text(&result);
        assert!(body.len() < huge.len(), "output should be byte-bounded");
    }

    #[test]
    fn path_resolves_relative_to_cwd() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("rel.txt"), "alpha\nbeta\ngamma\n").unwrap();
        let tool = ReadLinesTool::new();

        let result = tool
            .read_lines_with_cwd(
                ReadLinesParams {
                    path: "rel.txt".to_string(),
                    offset: Some(0),
                    limit: Some(2),
                },
                Some(dir.path()),
            )
            .unwrap();

        let sc = structured(&result);
        assert_eq!(sc["returned_lines"], 2);
        assert_eq!(sc["total_lines"], 3);
        let body = text(&result);
        assert!(body.contains("alpha"));
        assert!(body.contains("beta"));
        assert!(!body.contains("gamma"));
    }

    #[test]
    fn refuses_binary_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("blob.bin");
        fs::write(&path, [0xFF, 0xFE, 0x00, 0x01, 0x80, 0x90]).unwrap();
        let tool = ReadLinesTool::new();

        let result = tool
            .read_lines_with_cwd(
                ReadLinesParams {
                    path: path.display().to_string(),
                    offset: Some(0),
                    limit: Some(5),
                },
                None,
            )
            .unwrap();

        assert_eq!(result.is_error, Some(true));
        assert!(text(&result).contains("not valid UTF-8"));
    }
}
