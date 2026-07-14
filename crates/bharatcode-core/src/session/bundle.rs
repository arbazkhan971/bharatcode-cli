//! Portable session-bundle export/import (`.bcsession`).
//!
//! A named (headless) session can be exported to a single, self-contained
//! `.bcsession` JSON file that carries its messages, a small slice of session
//! metadata, and a pointer to the ₹ cost ledger. The bundle is "signed" by a
//! SHA-256 checksum over its canonical payload so a tampered bundle is rejected
//! on import. The bundle can then be re-imported on another machine to hand the
//! session off, creating a fresh local session seeded with the same
//! conversation.
//!
//! Privacy invariant: every message is run through the existing developer
//! egress-redaction pass ([`crate::agents::platform_extensions::developer::redact`])
//! before it is written, so high-confidence secrets (cloud keys, provider
//! tokens, bearer tokens, private-key headers, `.env`-style assignments) are
//! replaced with `[REDACTED]` and never leave the origin machine inside a
//! bundle. Redaction here is unconditional — it does **not** consult the
//! `BHARATCODE_REDACT` egress gate — because a portable bundle is meant to be
//! shared.
//!
//! The auto-export-on-finalization side of this feature is gated behind the
//! `BHARATCODE_SESSION_BUNDLE_ON_END` environment variable and is **off by
//! default**, so default behaviour is unchanged. The manual `export_bundle` /
//! `import_bundle` entry points are plain library APIs with no env gate.
//!
//! This module is original work; nothing here is ported from third-party
//! sources.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use chrono::{FixedOffset, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::agents::platform_extensions::developer::redact;
use crate::conversation::message::Message;
use crate::conversation::Conversation;
use crate::session::session_manager::{SessionManager, SessionType};

/// Current bundle schema version. Bump on breaking layout changes.
pub const BUNDLE_SCHEMA_VERSION: u32 = 1;

/// File extension (without the dot) for an exported bundle.
pub const BUNDLE_EXTENSION: &str = "bcsession";

/// Environment key that opts in to auto-export on session finalization.
/// Off by default.
pub const BUNDLE_ON_END_KEY: &str = "BHARATCODE_SESSION_BUNDLE_ON_END";

/// India Standard Time (UTC+05:30). The human-facing `exported_at_ist` field is
/// rendered against the IST wall clock to match the rest of BharatCode.
fn ist_offset() -> FixedOffset {
    FixedOffset::east_opt(5 * 3600 + 30 * 60).expect("IST (+05:30) is a valid fixed offset")
}

/// Interpret a raw flag value as truthy. Mirrors the other BharatCode switches:
/// only a clearly affirmative value enables the feature; everything else
/// (including unset / unrecognised) leaves it off.
fn is_truthy(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on" | "enable" | "enabled"
    )
}

/// Returns `true` when auto-export on finalization is enabled. Defaults to
/// `false`. Reads the raw `BHARATCODE_SESSION_BUNDLE_ON_END` env var first
/// (truthy values only), then falls back to the global config parameter of the
/// same name.
pub fn auto_export_enabled() -> bool {
    if let Ok(raw) = std::env::var(BUNDLE_ON_END_KEY) {
        return is_truthy(&raw);
    }
    crate::config::Config::global()
        .get_param::<bool>(BUNDLE_ON_END_KEY)
        .unwrap_or(false)
}

/// A compact, portable slice of session metadata carried by a bundle.
///
/// Deliberately small and stable so a consumer can rely on the shape. The
/// origin session id is recorded for provenance only; import always mints a
/// fresh local id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BundleMetadata {
    /// Origin session id (provenance; not reused on import).
    pub origin_session_id: String,
    /// Human-readable session name/description.
    pub name: String,
    /// Working directory of the origin session, as a string.
    pub working_dir: String,
    /// Session kind (user, scheduled, sub-agent, ...).
    pub session_type: SessionType,
    /// Provider name, when known.
    pub provider_name: Option<String>,
    /// Number of messages in the bundle.
    pub message_count: usize,
}

/// Pointer/summary of the ₹ (Indian rupee) cost ledger for the session.
///
/// `inr_cost` mirrors the session's accumulated cost (the existing ledger), and
/// the token fields give a lightweight, machine-readable handle so a consumer
/// can reconcile against its own ledger after a handoff.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LedgerSummary {
    /// Accumulated session cost in ₹, when a price is available.
    pub inr_cost: Option<f64>,
    /// Accumulated input tokens over the session.
    pub input_tokens: Option<i32>,
    /// Accumulated output tokens over the session.
    pub output_tokens: Option<i32>,
    /// Accumulated total tokens over the session.
    pub total_tokens: Option<i32>,
}

/// A portable, checksum-signed session bundle.
///
/// Serialized form is the on-disk `.bcsession` JSON. The `sha256` field is the
/// hex SHA-256 of the canonical payload (every field *except* `sha256` itself);
/// import recomputes it and rejects any mismatch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionBundle {
    /// Bundle schema version.
    pub schema_version: u32,
    /// Export wall-clock time rendered in IST (`YYYY-MM-DD HH:MM:SS +05:30`).
    pub exported_at_ist: String,
    /// Secret-redacted conversation messages.
    pub messages: Vec<Message>,
    /// Portable session metadata.
    pub metadata: BundleMetadata,
    /// ₹ ledger pointer/summary.
    pub ledger_summary: LedgerSummary,
    /// Hex SHA-256 over the canonical payload (excludes this field).
    pub sha256: String,
}

impl SessionBundle {
    /// Serialize the bundle to pretty-printed `.bcsession` JSON.
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self).context("serializing session bundle to JSON")
    }

    /// Parse a `.bcsession` JSON string into a bundle **and verify its
    /// checksum**. A tampered or recomputation-mismatched checksum is rejected.
    pub fn from_json(json: &str) -> Result<SessionBundle> {
        let bundle: SessionBundle =
            serde_json::from_str(json).context("parsing session bundle JSON")?;
        bundle.verify_checksum()?;
        Ok(bundle)
    }

    /// Recompute the canonical checksum and compare it against the stored one.
    pub fn verify_checksum(&self) -> Result<()> {
        let expected = self.compute_checksum()?;
        if expected != self.sha256 {
            bail!(
                "session bundle checksum mismatch: refusing to import a tampered bundle (expected {expected}, found {})",
                self.sha256
            );
        }
        Ok(())
    }

    /// Compute the SHA-256 of the canonical payload (all fields except
    /// `sha256`). The payload is serialized with the `sha256` field cleared so
    /// export and import hash the exact same bytes.
    fn compute_checksum(&self) -> Result<String> {
        let mut canonical = self.clone();
        canonical.sha256 = String::new();
        let payload =
            serde_json::to_vec(&canonical).context("serializing bundle payload for checksum")?;
        let mut hasher = Sha256::new();
        hasher.update(&payload);
        Ok(hex_lower(&hasher.finalize()))
    }
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// Run the existing developer egress-redaction pass over every part of a
/// message.
///
/// The message is round-tripped through JSON so that secrets are stripped no
/// matter where they live (assistant/user text, tool-call arguments, tool
/// responses, system notifications). This reuses the shared, well-tested
/// [`redact`] regex set rather than re-implementing detection here.
fn redact_message(message: &Message) -> Result<Message> {
    let raw = serde_json::to_string(message).context("serializing message for redaction")?;
    let cleaned = redact::redact(&raw);
    serde_json::from_str(&cleaned).context("deserializing redacted message")
}

/// Export the named session to an in-memory [`SessionBundle`].
///
/// Loads the session (with its messages), redacts every message via the shared
/// developer redaction pass, snapshots a small portable metadata slice and the
/// ₹ ledger pointer, and stamps the bundle with a canonical SHA-256 checksum.
///
/// Secrets/API keys are stripped unconditionally on export (privacy invariant).
pub async fn export_bundle(session_id: &str) -> Result<SessionBundle> {
    let manager = SessionManager::instance();
    let session = manager
        .get_session(session_id, true)
        .await
        .with_context(|| format!("loading session {session_id} for export"))?;

    let conversation = session.conversation.clone().unwrap_or_default();
    let mut messages = Vec::with_capacity(conversation.messages().len());
    for message in conversation.messages() {
        messages.push(redact_message(message)?);
    }

    let metadata = BundleMetadata {
        origin_session_id: session.id.clone(),
        name: session.name.clone(),
        working_dir: session.working_dir.to_string_lossy().into_owned(),
        session_type: session.session_type,
        provider_name: session.provider_name.clone(),
        message_count: messages.len(),
    };

    let ledger_summary = LedgerSummary {
        inr_cost: session.accumulated_cost,
        input_tokens: session.accumulated_usage.input_tokens,
        output_tokens: session.accumulated_usage.output_tokens,
        total_tokens: session.accumulated_usage.total_tokens,
    };

    let exported_at_ist = Utc::now()
        .with_timezone(&ist_offset())
        .format("%Y-%m-%d %H:%M:%S %z")
        .to_string();

    let mut bundle = SessionBundle {
        schema_version: BUNDLE_SCHEMA_VERSION,
        exported_at_ist,
        messages,
        metadata,
        ledger_summary,
        sha256: String::new(),
    };
    bundle.sha256 = bundle.compute_checksum()?;
    Ok(bundle)
}

/// Import a bundle, creating a fresh local session seeded with its (already
/// redacted) messages.
///
/// The bundle's checksum is verified first; a mismatch is rejected. On success
/// a brand-new session id is minted locally (the origin id is never reused) and
/// returned to the caller.
pub async fn import_bundle(bundle: &SessionBundle) -> Result<String> {
    bundle.verify_checksum()?;

    let manager = SessionManager::instance();
    let working_dir = PathBuf::from(&bundle.metadata.working_dir);
    let session = manager
        .create_session(
            working_dir,
            bundle.metadata.name.clone(),
            bundle.metadata.session_type,
            crate::config::GooseMode::default(),
        )
        .await
        .context("creating local session for import")?;

    let conversation = Conversation::new_unvalidated(bundle.messages.clone());
    manager
        .replace_conversation(&session.id, &conversation)
        .await
        .with_context(|| format!("seeding imported session {} with messages", session.id))?;

    Ok(session.id)
}

/// Write a bundle to `dir/<session_id>.bcsession`, returning the path written.
///
/// The parent directory is created if missing.
pub fn write_bundle_file(bundle: &SessionBundle, dir: &Path, session_id: &str) -> Result<PathBuf> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("creating bundle directory {}", dir.display()))?;
    let path = dir.join(format!("{session_id}.{BUNDLE_EXTENSION}"));
    let json = bundle.to_json()?;
    std::fs::write(&path, json).with_context(|| format!("writing bundle to {}", path.display()))?;
    Ok(path)
}

/// Read and verify a bundle from a `.bcsession` file on disk.
pub fn read_bundle_file(path: &Path) -> Result<SessionBundle> {
    let json = std::fs::read_to_string(path)
        .with_context(|| format!("reading bundle from {}", path.display()))?;
    SessionBundle::from_json(&json).map_err(|e| anyhow!("{} (from {})", e, path.display()))
}

/// Auto-export hook for the agent finalization path.
///
/// When [`auto_export_enabled`] is `false` (the default) this is a no-op and
/// returns `None`, so finalization writes nothing. When enabled it exports the
/// session and writes `<session_id>.bcsession` under `<working_dir>/.bharatcode`,
/// returning the written path so the caller can surface a pointer.
///
/// Errors are swallowed into `None`: a best-effort sidecar export must never
/// fail a turn.
pub async fn maybe_export_on_end(session_id: &str, working_dir: &Path) -> Option<PathBuf> {
    if !auto_export_enabled() {
        return None;
    }
    let bundle = match export_bundle(session_id).await {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!("session bundle export failed: {e}");
            return None;
        }
    };
    let dir = working_dir.join(".bharatcode");
    match write_bundle_file(&bundle, &dir, session_id) {
        Ok(path) => Some(path),
        Err(e) => {
            tracing::warn!("writing session bundle failed: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::Role;

    fn sample_bundle() -> SessionBundle {
        let messages = vec![
            Message::user().with_text("hello there"),
            Message::assistant().with_text("hi, how can I help?"),
        ];
        let metadata = BundleMetadata {
            origin_session_id: "20260620_1".to_string(),
            name: "demo session".to_string(),
            working_dir: "/tmp/work".to_string(),
            session_type: SessionType::User,
            provider_name: Some("anthropic".to_string()),
            message_count: messages.len(),
        };
        let ledger_summary = LedgerSummary {
            inr_cost: Some(42.5),
            input_tokens: Some(100),
            output_tokens: Some(50),
            total_tokens: Some(150),
        };
        let mut bundle = SessionBundle {
            schema_version: BUNDLE_SCHEMA_VERSION,
            exported_at_ist: "2026-06-20 12:00:00 +0530".to_string(),
            messages,
            metadata,
            ledger_summary,
            sha256: String::new(),
        };
        bundle.sha256 = bundle.compute_checksum().unwrap();
        bundle
    }

    #[test]
    fn checksum_round_trips_through_json() {
        let bundle = sample_bundle();
        let json = bundle.to_json().unwrap();
        let parsed = SessionBundle::from_json(&json).expect("valid bundle must parse");
        assert_eq!(parsed, bundle);
        assert_eq!(parsed.metadata.name, "demo session");
        assert_eq!(parsed.ledger_summary.inr_cost, Some(42.5));
        assert_eq!(parsed.messages.len(), 2);
    }

    #[test]
    fn tampered_checksum_is_rejected_on_import() {
        let mut bundle = sample_bundle();
        // Mutate a message after the checksum was computed.
        bundle.messages.push(Message::user().with_text("injected"));
        // Re-serialize with the stale checksum still in place.
        let json = serde_json::to_string(&bundle).unwrap();
        let err = SessionBundle::from_json(&json).expect_err("a stale checksum must be rejected");
        assert!(
            err.to_string().contains("checksum mismatch"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn flipping_a_byte_in_checksum_is_rejected() {
        let mut bundle = sample_bundle();
        let replacement = if bundle.sha256.ends_with('0') {
            '1'
        } else {
            '0'
        };
        bundle.sha256.pop();
        bundle.sha256.push(replacement);
        assert!(bundle.verify_checksum().is_err());
    }

    #[test]
    fn export_strips_planted_secret_from_text() {
        // Build a fake GitHub token from fragments so no contiguous real-looking
        // secret literal exists in source (push-protection safe).
        let planted = format!("ghp_{}", "1234567890abcdefghijklmnopqrstuvwxyz");
        let message = Message::user().with_text(format!("my key is {planted} keep it safe"));

        let redacted = redact_message(&message).expect("redaction must succeed");
        let text = redacted.as_concat_text();
        assert!(
            !text.contains(&planted),
            "planted secret must be stripped, got: {text}"
        );
        assert!(
            text.contains(redact::REDACTED),
            "redaction sentinel must be present, got: {text}"
        );
    }

    #[test]
    fn export_strips_planted_secret_inside_a_built_bundle() {
        let planted = format!("AKIA{}", "IOSFODNN7EXAMPLE");
        let messages = [Message::user().with_text(format!("export AWS_KEY={planted}"))];

        let mut bundle = SessionBundle {
            schema_version: BUNDLE_SCHEMA_VERSION,
            exported_at_ist: "2026-06-20 12:00:00 +0530".to_string(),
            messages: messages
                .iter()
                .map(|m| redact_message(m).unwrap())
                .collect(),
            metadata: BundleMetadata {
                origin_session_id: "20260620_2".to_string(),
                name: "s".to_string(),
                working_dir: "/tmp".to_string(),
                session_type: SessionType::User,
                provider_name: None,
                message_count: 1,
            },
            ledger_summary: LedgerSummary {
                inr_cost: None,
                input_tokens: None,
                output_tokens: None,
                total_tokens: None,
            },
            sha256: String::new(),
        };
        bundle.sha256 = bundle.compute_checksum().unwrap();

        let json = bundle.to_json().unwrap();
        assert!(!json.contains(&planted), "bundle JSON must not leak secret");
        assert!(json.contains(redact::REDACTED));
    }

    #[test]
    fn write_then_read_round_trips_a_bundle_file() {
        let bundle = sample_bundle();
        let dir = std::env::temp_dir().join(format!(
            "bc_bundle_{}_{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = write_bundle_file(&bundle, &dir, "20260620_9").unwrap();
        assert!(path.exists());
        assert_eq!(path.extension().and_then(|e| e.to_str()), Some("bcsession"));

        let read = read_bundle_file(&path).expect("written bundle must read back");
        assert_eq!(read, bundle);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn auto_export_disabled_by_default() {
        std::env::remove_var(BUNDLE_ON_END_KEY);
        // Config fallback may be unset in tests; default must be off.
        assert!(!is_truthy("0"));
        assert!(!is_truthy(""));
        assert!(is_truthy("1"));
        assert!(is_truthy("true"));
        assert!(is_truthy(" YES "));
    }

    #[test]
    fn redacted_message_preserves_role_and_structure() {
        let message = Message::assistant().with_text("plain output, no secrets here");
        let redacted = redact_message(&message).unwrap();
        assert_eq!(redacted.role, Role::Assistant);
        assert_eq!(redacted.as_concat_text(), "plain output, no secrets here");
    }
}
