//! Opt-in editor/IDE bridge — post-turn changed-files manifest sidecar.
//!
//! After the agent finalizes a turn, this module can drop a small,
//! machine-readable manifest at `<working_dir>/.bharatcode/last_changes.json`
//! listing the files the agent just touched in the working tree, each tagged
//! with a coarse change status (`added` / `modified` / `deleted` / `renamed` /
//! `untracked`). An editor, IDE extension, or filesystem watcher can read the
//! manifest and jump straight to what changed, rather than diffing the whole
//! tree itself.
//!
//! The whole feature is **opt-in and defaults to off**, gated on the raw
//! `BHARATCODE_EDITOR_BRIDGE` environment variable. When the switch is off,
//! [`is_enabled`] returns `false`, [`write_change_manifest`] is never called
//! from the finalization path, and no file is ever written, so default
//! behaviour is completely unchanged — and there is no extra I/O.
//!
//! Writes are **best-effort**: the directory is created if needed and any I/O
//! error along the way is swallowed rather than surfaced. A missing or stale
//! manifest is a convenience, never a correctness requirement, so it must never
//! disrupt the reply. This module is original work; nothing here is ported from
//! third-party sources.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::{FixedOffset, Utc};

/// Environment key for the editor-bridge manifest switch. Defaults to off.
pub const EDITOR_BRIDGE_KEY: &str = "BHARATCODE_EDITOR_BRIDGE";

/// Directory (under the working dir) that holds the manifest.
const MANIFEST_DIR: &str = ".bharatcode";

/// File name of the manifest within [`MANIFEST_DIR`].
const MANIFEST_FILE: &str = "last_changes.json";

/// India Standard Time (UTC+05:30). BharatCode targets India, so the
/// human-facing `generated_at_ist` field is rendered against the IST wall
/// clock.
fn ist_offset() -> FixedOffset {
    FixedOffset::east_opt(5 * 3600 + 30 * 60).expect("IST (+05:30) is a valid fixed offset")
}

/// Interpret a raw flag value as truthy. Mirrors the other BharatCode switches:
/// only a clearly affirmative value enables the feature; everything else
/// (including unset / unrecognised) leaves it off so default behaviour is never
/// flipped by accident.
fn is_truthy(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on" | "enable" | "enabled"
    )
}

/// Returns `true` when the editor-bridge manifest is enabled. Defaults to
/// `false`.
///
/// Reads the raw `BHARATCODE_EDITOR_BRIDGE` environment variable directly
/// (raw-env-first); any truthy value turns the feature on. Unset or
/// unrecognised resolves to "off".
pub fn is_enabled() -> bool {
    std::env::var(EDITOR_BRIDGE_KEY)
        .ok()
        .map(|raw| is_truthy(&raw))
        .unwrap_or(false)
}

/// Map a two-character `git status --porcelain` code to a coarse, stable status
/// label. Defaults to `"modified"` for anything unrecognised so a consumer
/// always gets a usable hint.
fn status_label(code: &str) -> &'static str {
    let bytes = code.as_bytes();
    let x = bytes.first().copied().unwrap_or(b' ');
    let y = bytes.get(1).copied().unwrap_or(b' ');
    if x == b'?' || y == b'?' {
        return "untracked";
    }
    // Prefer the staged (index) code, falling back to the worktree code.
    let primary = if x != b' ' { x } else { y };
    match primary {
        b'A' => "added",
        b'D' => "deleted",
        b'R' => "renamed",
        b'C' => "copied",
        _ => "modified",
    }
}

/// Best-effort lookup of per-path change status from `git status --porcelain`,
/// keyed by the same absolute paths [`crate::agents`] derives via its own
/// `git_changed_files` helper. Any failure (no git, not a repo, spawn error)
/// yields an empty map and every file falls back to `"modified"`.
async fn status_map(working_dir: &Path) -> HashMap<PathBuf, &'static str> {
    let mut map = HashMap::new();

    let output = tokio::process::Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=all"])
        .current_dir(working_dir)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .await;

    let Ok(output) = output else {
        return map;
    };
    if !output.status.success() {
        return map;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        if line.len() < 3 {
            continue;
        }
        let code = &line[..2];
        let rest = line.get(3..).unwrap_or("").trim();
        if rest.is_empty() {
            continue;
        }
        // Renames render as `orig -> new`; key on the destination path, which is
        // what `git_changed_files` records.
        let path = rest.rsplit(" -> ").next().unwrap_or(rest).trim();
        let path = path.trim_matches('"');
        if path.is_empty() {
            continue;
        }
        map.insert(working_dir.join(path), status_label(code));
    }
    map
}

/// Render the manifest payload as a pretty-printed JSON string.
///
/// Kept separate from the file write so it can be unit-tested without touching
/// the filesystem. Field keys are deliberately neutral and stable so an
/// external consumer can rely on them.
fn render_payload(
    working_dir: &Path,
    changed: &[PathBuf],
    statuses: &HashMap<PathBuf, &'static str>,
) -> String {
    let now_utc = Utc::now();
    let now_ist = now_utc.with_timezone(&ist_offset());

    let files: Vec<serde_json::Value> = changed
        .iter()
        .map(|p| {
            let status = statuses.get(p).copied().unwrap_or("modified");
            // Prefer a path relative to the working dir so a consumer can resolve
            // it against its own root; fall back to the full path when it lies
            // outside the working dir (which should not normally happen).
            let display = p
                .strip_prefix(working_dir)
                .unwrap_or(p)
                .to_string_lossy()
                .into_owned();
            serde_json::json!({
                "path": display,
                "status": status,
            })
        })
        .collect();

    let payload = serde_json::json!({
        "generated_at_utc": now_utc.to_rfc3339(),
        "generated_at_ist": now_ist.to_rfc3339(),
        "files": files,
    });

    // Pretty-print is infallible for a plain object; fall back to a compact
    // form on the impossible error path rather than panicking.
    serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string())
}

/// Write the changed-files manifest to
/// `<working_dir>/.bharatcode/last_changes.json`.
///
/// The directory is created if needed and the manifest is overwritten in place
/// on each call so a consumer always sees the latest turn's changes. This is
/// **best-effort**: any I/O failure (unwritable directory, etc.) is swallowed
/// and the function never panics. A manifest is a convenience, never a
/// correctness requirement, so it must never interfere with the reply.
pub async fn write_change_manifest(working_dir: &Path, changed: &[PathBuf]) {
    let statuses = status_map(working_dir).await;
    let body = render_payload(working_dir, changed, &statuses);
    let _ = write_manifest_inner(working_dir, body).await;
}

/// Inner worker returning `io::Result` so the happy path stays terse; the public
/// entry point discards the result to keep the feature best-effort.
async fn write_manifest_inner(working_dir: &Path, body: String) -> std::io::Result<()> {
    let dir = working_dir.join(MANIFEST_DIR);
    tokio::fs::create_dir_all(&dir).await?;
    let dest = dir.join(MANIFEST_FILE);
    tokio::fs::write(&dest, body.as_bytes()).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn is_enabled_is_false_when_unset() {
        // Snapshot and clear the var so the test is deterministic regardless of
        // the ambient environment, then restore it afterwards.
        let prior = std::env::var(EDITOR_BRIDGE_KEY).ok();
        std::env::remove_var(EDITOR_BRIDGE_KEY);
        assert!(
            !is_enabled(),
            "feature must default OFF when env var is unset"
        );
        if let Some(v) = prior {
            std::env::set_var(EDITOR_BRIDGE_KEY, v);
        }
    }

    #[tokio::test]
    async fn disabled_path_writes_nothing() {
        // The finalization call site is guarded by `is_enabled()`; emulate the
        // disabled environment and confirm no manifest is produced when the
        // guard would be false.
        let prior = std::env::var(EDITOR_BRIDGE_KEY).ok();
        std::env::remove_var(EDITOR_BRIDGE_KEY);

        let tmp = tempfile::tempdir().expect("temp dir");
        if is_enabled() {
            write_change_manifest(tmp.path(), &[]).await;
        }
        let path = tmp.path().join(MANIFEST_DIR).join(MANIFEST_FILE);
        assert!(!path.exists(), "disabled feature must not write any file");

        if let Some(v) = prior {
            std::env::set_var(EDITOR_BRIDGE_KEY, v);
        }
    }

    #[tokio::test]
    async fn write_change_manifest_creates_parseable_json_with_files_and_ist() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let changed = vec![tmp.path().join("src/main.rs"), tmp.path().join("README.md")];

        write_change_manifest(tmp.path(), &changed).await;

        let path = tmp.path().join(MANIFEST_DIR).join(MANIFEST_FILE);
        assert!(path.exists(), "manifest file must be created");

        let raw = std::fs::read_to_string(&path).expect("read manifest");
        let v: Value = serde_json::from_str(&raw).expect("manifest must be valid JSON");

        // IST timestamp field is present and stringy.
        assert!(
            v["generated_at_ist"].is_string(),
            "manifest must carry an IST timestamp"
        );

        let files = v["files"].as_array().expect("files array");
        assert_eq!(files.len(), 2, "both changed files must be listed");

        let listed: Vec<String> = files
            .iter()
            .map(|f| f["path"].as_str().unwrap_or_default().replace('\\', "/"))
            .collect();
        assert!(
            listed.contains(&"src/main.rs".to_string()),
            "got: {listed:?}"
        );
        assert!(listed.contains(&"README.md".to_string()), "got: {listed:?}");

        // Each entry carries a non-empty status hint.
        for f in files {
            assert!(
                f["status"].as_str().map(|s| !s.is_empty()).unwrap_or(false),
                "each file must carry a status"
            );
        }
    }

    #[tokio::test]
    async fn rerunning_overwrites_cleanly() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let path = tmp.path().join(MANIFEST_DIR).join(MANIFEST_FILE);

        let first = vec![tmp.path().join("a.rs"), tmp.path().join("b.rs")];
        write_change_manifest(tmp.path(), &first).await;
        let v1: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v1["files"].as_array().unwrap().len(), 2);

        // A second run with a different, smaller set must fully replace the
        // previous manifest — no stale entries left behind.
        let second = vec![tmp.path().join("c.rs")];
        write_change_manifest(tmp.path(), &second).await;
        let v2: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let files = v2["files"].as_array().unwrap();
        assert_eq!(files.len(), 1, "manifest must be overwritten, not appended");
        let only = files[0]["path"].as_str().unwrap().replace('\\', "/");
        assert_eq!(only, "c.rs");
    }

    #[test]
    fn status_label_maps_porcelain_codes() {
        assert_eq!(status_label("A "), "added");
        assert_eq!(status_label(" M"), "modified");
        assert_eq!(status_label("M "), "modified");
        assert_eq!(status_label("D "), "deleted");
        assert_eq!(status_label("R "), "renamed");
        assert_eq!(status_label("??"), "untracked");
        assert_eq!(status_label("  "), "modified");
    }

    #[test]
    fn payload_keys_carry_no_upstream_branding() {
        let body = render_payload(Path::new("/tmp/work"), &[], &HashMap::new());
        let v: Value = serde_json::from_str(&body).unwrap();
        let obj = v.as_object().unwrap();
        for key in obj.keys() {
            let lower = key.to_ascii_lowercase();
            assert!(
                !lower.contains("goose") && !lower.contains("block"),
                "payload key must not leak upstream branding: {key}"
            );
        }
    }
}
