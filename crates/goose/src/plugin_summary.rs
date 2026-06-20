//! End-of-session summary side-channel for plugins.
//!
//! When enabled, at agent finalization this module builds a compact JSON
//! summary of the session — turn count, changed files, tool names used, and an
//! optional ₹ cost — and hands it to plugins two ways:
//!
//!   1. it fires the existing [`crate::hooks`] `SessionEnd` hook with the
//!      summary attached to the hook context, so plugin hook scripts receive it
//!      on stdin alongside the usual event payload; and
//!   2. it writes a small sidecar JSON file under the data directory that any
//!      plugin can read out-of-band.
//!
//! The whole feature is **off by default** and gated behind the
//! `BHARATCODE_PLUGIN_SUMMARY` boolean. When disabled, [`is_enabled`] short
//! circuits before any work happens, so the default path does zero extra IO and
//! fires no hooks.
//!
//! This module is original work; nothing here is ported from third-party
//! sources.

use std::path::Path;

use serde::Serialize;
use serde_json::{json, Value};

/// Opt-in toggle name, shared by env var and config parameter.
const ENABLE_KEY: &str = "BHARATCODE_PLUGIN_SUMMARY";

/// Cap on how many entries we record per list so the summary stays compact and
/// the sidecar file/hook payload never balloons.
const MAX_LIST: usize = 100;

/// Whether the plugin session-summary side-channel is enabled. Off by default.
///
/// Reads the raw `BHARATCODE_PLUGIN_SUMMARY` environment variable first (truthy
/// values only), mirroring `memory_store::is_enabled` so a bare `1` survives
/// any boolean coercion, then falls back to the global config parameter of the
/// same name, then defaults to `false`.
pub fn is_enabled() -> bool {
    if let Ok(raw) = std::env::var(ENABLE_KEY) {
        return is_truthy(&raw);
    }
    crate::config::Config::global()
        .get_param::<String>(ENABLE_KEY)
        .map(|raw| is_truthy(&raw))
        .unwrap_or(false)
}

fn is_truthy(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// A compact, plugin-facing summary of a finished session.
///
/// Field set is deliberately small and stable so plugins can rely on the shape:
/// `turns`, `changed_files`, `tools`, and an optional `inr_cost`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SessionSummary {
    /// Number of model turns taken during the session.
    pub turns: u32,
    /// Repo-relative or absolute paths of files changed during the session.
    pub changed_files: Vec<String>,
    /// Distinct tool names invoked during the session, in first-seen order.
    pub tools: Vec<String>,
    /// Session cost in Indian rupees (₹), when a price is available.
    pub inr_cost: Option<f64>,
}

/// Build a [`SessionSummary`] from finalization inputs.
///
/// `working_dir` anchors any relative-path normalization for `changed_files`;
/// paths that live under `working_dir` are recorded relative to it so the
/// summary is portable, otherwise the path is kept as given. Tool names are
/// de-duplicated preserving first-seen order. All lists are capped at
/// [`MAX_LIST`] to keep the payload small.
pub fn build_summary(
    working_dir: &Path,
    turns: u32,
    changed_files: &[String],
    tools: &[String],
    inr_cost: Option<f64>,
) -> SessionSummary {
    let changed_files = changed_files
        .iter()
        .map(|p| normalize_path(working_dir, p))
        .take(MAX_LIST)
        .collect();

    let mut seen: Vec<String> = Vec::new();
    for tool in tools {
        let tool = tool.trim();
        if tool.is_empty() {
            continue;
        }
        if !seen.iter().any(|t| t == tool) {
            seen.push(tool.to_string());
        }
        if seen.len() >= MAX_LIST {
            break;
        }
    }

    SessionSummary {
        turns,
        changed_files,
        tools: seen,
        inr_cost,
    }
}

/// Record a path relative to `working_dir` when it lives underneath it; keep it
/// verbatim otherwise.
fn normalize_path(working_dir: &Path, path: &str) -> String {
    let candidate = Path::new(path);
    match candidate.strip_prefix(working_dir) {
        Ok(rel) => rel.to_string_lossy().into_owned(),
        Err(_) => path.to_string(),
    }
}

/// Serialize the summary to the stable JSON object plugins consume.
///
/// The object always carries the keys `turns`, `changed_files`, `tools`, and
/// `inr_cost` (the last is `null` when no price is available).
pub fn to_json(summary: &SessionSummary) -> Value {
    json!({
        "turns": summary.turns,
        "changed_files": summary.changed_files,
        "tools": summary.tools,
        "inr_cost": summary.inr_cost,
    })
}

/// Path of the sidecar summary file plugins may read out-of-band.
fn sidecar_path() -> std::path::PathBuf {
    crate::config::paths::Paths::in_data_dir("plugin_session_summary.json")
}

/// Best-effort: write the summary JSON to the sidecar file. Never errors so a
/// write failure cannot disturb finalization.
fn write_sidecar(value: &Value) {
    let path = sidecar_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(serialized) = serde_json::to_string_pretty(value) {
        let _ = std::fs::write(&path, serialized);
    }
}

/// Dispatch a finished-session summary to plugins.
///
/// Writes the sidecar file and fires the existing `SessionEnd` hook with the
/// summary JSON carried on the hook context, reusing the same `emit` path the
/// agent already uses for the `Stop` hook. A misbehaving hook can never crash
/// the host because [`crate::hooks::HookManager::emit`] swallows per-hook
/// errors.
pub async fn dispatch(
    summary: SessionSummary,
    hook_manager: &crate::hooks::HookManager,
    session_id: &str,
) {
    let value = to_json(&summary);
    write_sidecar(&value);

    if !hook_manager.has_hooks(crate::hooks::HookEvent::SessionEnd) {
        return;
    }

    let payload = value.to_string();
    let ctx = crate::hooks::HookContext::new(crate::hooks::HookEvent::SessionEnd, session_id)
        .with_message(payload);
    hook_manager
        .emit(crate::hooks::HookEvent::SessionEnd, ctx)
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialize env access so the toggle tests don't race other env mutators.
    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn disabled_when_unset() {
        let _g = env_guard();
        std::env::remove_var(ENABLE_KEY);
        assert!(!is_enabled());
    }

    #[test]
    fn enabled_on_bare_one() {
        let _g = env_guard();
        std::env::set_var(ENABLE_KEY, "1");
        assert!(is_enabled());
        std::env::remove_var(ENABLE_KEY);
    }

    #[test]
    fn build_summary_yields_expected_fields() {
        let working_dir = Path::new("/repo");
        let changed = vec![
            "/repo/src/main.rs".to_string(),
            "/elsewhere/other.rs".to_string(),
        ];
        let tools = vec![
            "developer__shell".to_string(),
            "developer__shell".to_string(),
            "developer__text_editor".to_string(),
            "  ".to_string(),
        ];

        let summary = build_summary(working_dir, 7, &changed, &tools, Some(42.5));

        assert_eq!(summary.turns, 7);
        assert_eq!(
            summary.changed_files,
            vec!["src/main.rs".to_string(), "/elsewhere/other.rs".to_string()]
        );
        assert_eq!(
            summary.tools,
            vec![
                "developer__shell".to_string(),
                "developer__text_editor".to_string()
            ]
        );
        assert_eq!(summary.inr_cost, Some(42.5));
    }

    #[test]
    fn to_json_is_object_with_documented_keys() {
        let summary = build_summary(
            Path::new("/repo"),
            3,
            &["/repo/a.rs".to_string()],
            &["developer__shell".to_string()],
            None,
        );
        let value = to_json(&summary);

        assert!(value.is_object());
        let obj = value.as_object().expect("object");
        assert!(obj.contains_key("turns"));
        assert!(obj.contains_key("changed_files"));
        assert!(obj.contains_key("tools"));
        assert!(obj.contains_key("inr_cost"));
        assert_eq!(obj["turns"], json!(3));
        assert!(obj["inr_cost"].is_null());

        let rendered = value.to_string().to_ascii_lowercase();
        assert!(!rendered.contains("goose"), "no donor branding in output");
    }
}
