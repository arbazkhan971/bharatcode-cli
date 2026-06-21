//! Plugin SDK: a stable, documented public surface for plugin and hook
//! authors.
//!
//! Third-party hook scripts receive a JSON payload on stdin when a lifecycle
//! event fires (see [`crate::hooks`]). Historically that payload was an
//! ad-hoc, undocumented shape. This module turns it into a *versioned
//! contract*: plugin authors can enumerate the lifecycle events the host
//! dispatches via [`supported_events`], inspect the stdin JSON schema each
//! event delivers via [`event_contract`], and pin against a semver-shaped
//! [`PluginSdkVersion`].
//!
//! It is a thin, panic-free façade over [`crate::hooks`]: the strongly-typed
//! [`HookEvent`] and [`HookContext`] are re-exported unchanged, so emitting
//! and observing events stays type-checked, while the contract helpers
//! describe the on-the-wire JSON to external (possibly non-Rust) scripts.
//!
//! This is purely additive public API — default host behavior is unchanged.

pub use crate::hooks::{HookContext, HookDecision, HookEvent};

/// Versioned marker for the plugin SDK contract.
///
/// The SDK version is independent of the host binary version: it bumps only
/// when the public SDK surface or the event contract changes in a way plugin
/// authors must react to. Pin against [`PluginSdkVersion::CURRENT`] /
/// [`PluginSdkVersion::as_str`] to detect drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PluginSdkVersion;

impl PluginSdkVersion {
    /// The current SDK contract version, semver-shaped (`MAJOR.MINOR.PATCH`).
    pub const CURRENT: &'static str = "1.0.0";

    /// Returns the current SDK contract version string.
    pub fn as_str(&self) -> &'static str {
        Self::CURRENT
    }
}

impl std::fmt::Display for PluginSdkVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(Self::CURRENT)
    }
}

/// The lifecycle events the host actively dispatches to hook scripts.
///
/// This is intentionally narrower than every [`HookEvent`] variant: it lists
/// the events that are wired to a real call site and therefore carry a stable
/// stdin contract. Plugin authors should treat anything not in this slice as
/// reserved.
pub fn supported_events() -> &'static [HookEvent] {
    &[
        HookEvent::SessionStart,
        HookEvent::PreToolUse,
        HookEvent::PostToolUse,
        HookEvent::PostToolUseFailure,
        HookEvent::BeforeShellExecution,
        HookEvent::BeforeReadFile,
    ]
}

/// Describes the stdin JSON contract a hook script receives for `event`.
///
/// The returned value is always a JSON object with at least:
/// - `"event"`: the event's canonical name (matches `hooks.json`),
/// - `"description"`: a human-readable summary of when the event fires,
/// - `"stdin"`: an object whose keys are the fields delivered on stdin, each
///   mapped to a short description of its meaning. Optional fields are noted.
///
/// The `stdin` shape mirrors the serialized [`HookContext`]: `event`,
/// `session_id`, and `matcher_context` are always present; the remaining
/// keys are populated per event. This function is total and panic-free for
/// every [`HookEvent`] variant.
pub fn event_contract(event: HookEvent) -> serde_json::Value {
    use serde_json::json;

    let common = |description: &str, extra: serde_json::Value| {
        let mut stdin = json!({
            "event": "always present; the event name, matching hooks.json",
            "session_id": "always present; the active session identifier",
            "matcher_context": "string or null; the value tested against a rule's `matcher` regex",
        });
        if let (Some(obj), Some(extra_obj)) = (stdin.as_object_mut(), extra.as_object()) {
            for (k, v) in extra_obj {
                obj.insert(k.clone(), v.clone());
            }
        }
        json!({
            "event": event.to_string(),
            "description": description,
            "stdin": stdin,
        })
    };

    match event {
        HookEvent::SessionStart => common(
            "Fires once when a session begins, before any tool runs.",
            json!({}),
        ),
        HookEvent::PreToolUse => common(
            "Fires before a tool call is executed; may block the call.",
            json!({
                "tool_name": "the tool about to run (also the matcher_context)",
                "tool_input": "optional; the tool's input arguments as JSON",
            }),
        ),
        HookEvent::PostToolUse => common(
            "Fires after a tool call completes successfully.",
            json!({
                "tool_name": "the tool that ran (also the matcher_context)",
                "tool_input": "optional; the tool's input arguments as JSON",
                "tool_output": "optional; the tool's output as JSON",
            }),
        ),
        HookEvent::PostToolUseFailure => common(
            "Fires after a tool call fails.",
            json!({
                "tool_name": "the tool that ran (also the matcher_context)",
                "tool_input": "optional; the tool's input arguments as JSON",
                "tool_output": "optional; failure detail as JSON, when available",
                "message": "optional; a human-readable failure message",
            }),
        ),
        HookEvent::BeforeShellExecution => common(
            "Fires before a shell command runs; may block the command.",
            json!({
                "message": "the command line about to run (also the matcher_context)",
                "working_dir": "optional; the directory the command runs in",
            }),
        ),
        HookEvent::BeforeReadFile => common(
            "Fires before a file is read; may block the read.",
            json!({
                "matcher_context": "string or null; the file path tested against the matcher",
                "message": "optional; the file path about to be read",
                "working_dir": "optional; the active working directory",
            }),
        ),
        HookEvent::SessionEnd => common("Fires once when a session ends.", json!({})),
        HookEvent::UserPromptSubmit => common(
            "Fires when the user submits a prompt.",
            json!({ "message": "optional; the submitted prompt text" }),
        ),
        HookEvent::AfterFileEdit => common(
            "Fires after a file edit is applied.",
            json!({
                "matcher_context": "string or null; the edited file path",
                "message": "optional; the edited file path",
            }),
        ),
        HookEvent::AfterShellExecution => common(
            "Fires after a shell command completes.",
            json!({
                "message": "the command that ran (also the matcher_context)",
                "working_dir": "optional; the directory the command ran in",
            }),
        ),
        HookEvent::Stop => common(
            "Fires when the agent attempts to stop; may block the stop.",
            json!({}),
        ),
        HookEvent::SubagentStart => common(
            "Fires when a subagent starts.",
            json!({ "message": "optional; subagent identification" }),
        ),
        HookEvent::SubagentStop => common(
            "Fires when a subagent stops; may block the stop.",
            json!({ "message": "optional; subagent identification" }),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every event the host actively dispatches must appear in
    /// `supported_events`. If `hooks/mod.rs` starts dispatching a new event,
    /// this guards that the SDK contract is extended to match.
    #[test]
    fn supported_events_cover_dispatched_variants() {
        let dispatched = [
            HookEvent::PreToolUse,
            HookEvent::PostToolUse,
            HookEvent::PostToolUseFailure,
            HookEvent::SessionStart,
            HookEvent::BeforeShellExecution,
            HookEvent::BeforeReadFile,
        ];
        let supported = supported_events();
        for event in dispatched {
            assert!(
                supported.contains(&event),
                "supported_events() is missing dispatched event {event}"
            );
        }
    }

    #[test]
    fn supported_events_has_no_duplicates() {
        let supported = supported_events();
        for (i, a) in supported.iter().enumerate() {
            for b in &supported[i + 1..] {
                assert_ne!(a, b, "duplicate event {a} in supported_events()");
            }
        }
    }

    #[test]
    fn event_contract_is_a_documented_object_for_every_variant() {
        let all = [
            HookEvent::PreToolUse,
            HookEvent::PostToolUse,
            HookEvent::PostToolUseFailure,
            HookEvent::SessionStart,
            HookEvent::SessionEnd,
            HookEvent::UserPromptSubmit,
            HookEvent::BeforeReadFile,
            HookEvent::AfterFileEdit,
            HookEvent::BeforeShellExecution,
            HookEvent::AfterShellExecution,
            HookEvent::Stop,
            HookEvent::SubagentStart,
            HookEvent::SubagentStop,
        ];
        for event in all {
            let contract = event_contract(event);
            let obj = contract
                .as_object()
                .unwrap_or_else(|| panic!("{event} contract is not a JSON object"));
            assert!(!obj.is_empty(), "{event} contract object is empty");

            let description = obj
                .get("description")
                .unwrap_or_else(|| panic!("{event} contract missing `description`"));
            assert!(
                description.as_str().is_some_and(|s| !s.is_empty()),
                "{event} `description` must be a non-empty string"
            );

            let stdin = obj
                .get("stdin")
                .unwrap_or_else(|| panic!("{event} contract missing `stdin`"));
            let stdin_obj = stdin
                .as_object()
                .unwrap_or_else(|| panic!("{event} `stdin` is not a JSON object"));
            assert!(
                stdin_obj.contains_key("event") && stdin_obj.contains_key("session_id"),
                "{event} `stdin` must document the always-present fields"
            );

            assert_eq!(
                obj.get("event").and_then(|v| v.as_str()),
                Some(event.to_string().as_str()),
                "{event} contract `event` name must round-trip"
            );
        }
    }

    #[test]
    fn supported_event_contracts_match_their_runtime_payload_keys() {
        // Build a representative HookContext for an event and confirm every
        // serialized key is documented in the contract's `stdin` object.
        let ctx = HookContext::new(HookEvent::PostToolUse, "session-xyz")
            .with_tool("developer__shell", Some(serde_json::json!({"cmd": "ls"})))
            .with_tool_output(serde_json::json!({"ok": true}));
        let serialized = serde_json::to_value(&ctx).expect("context serializes");
        let serialized_obj = serialized.as_object().expect("context is an object");

        let contract = event_contract(HookEvent::PostToolUse);
        let stdin = contract["stdin"].as_object().expect("stdin object");
        for key in serialized_obj.keys() {
            assert!(
                stdin.contains_key(key),
                "PostToolUse contract `stdin` is missing runtime key `{key}`"
            );
        }
    }

    #[test]
    fn sdk_version_is_semver_shaped() {
        let version = PluginSdkVersion;
        let s = version.as_str();
        assert_eq!(s, PluginSdkVersion::CURRENT);
        assert_eq!(s, version.to_string());

        let parts: Vec<&str> = s.split('.').collect();
        assert_eq!(parts.len(), 3, "version `{s}` must be MAJOR.MINOR.PATCH");
        for part in parts {
            assert!(
                !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()),
                "version component `{part}` must be numeric"
            );
        }
    }
}
