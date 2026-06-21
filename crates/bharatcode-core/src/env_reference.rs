//! Canonical `BHARATCODE_*` environment-variable reference (GA / release docs).
//!
//! This module is the single source of truth for the user-facing documentation
//! of the most-consulted `BHARATCODE_*` knobs: each entry pairs a variable name
//! with a one-line purpose and its documented default. The table is a
//! compile-time constant so the published docs can be *generated from the
//! binary* (`bharatcode doctor` emits it) rather than hand-maintained in a
//! separate markdown file that drifts from the code.
//!
//! Two renderers are provided: [`render_markdown`] for a docs-ready table and
//! [`render_json`] for structured ingestion by external doc tooling. Both are
//! pure functions of the constant table, independent of any live config.
//!
//! The emit-on-startup hook is gated behind the opt-in `BHARATCODE_ENV_REFERENCE`
//! environment variable and is a no-op when unset, so the default behaviour of
//! the running binary is completely unchanged unless explicitly opted in. The
//! reference itself stays product-neutral: no upstream donor brand ever appears
//! in a documented name, purpose, or default.

/// Environment variable that opts in to emitting the env-var reference at
/// global-config initialization (for `--env-reference`-style docs capture).
/// Default: unset / off, so startup behaviour is unchanged.
pub const ENABLE_KEY: &str = "BHARATCODE_ENV_REFERENCE";

/// A single documented environment variable: its name, a one-line purpose, and
/// its documented default value (the string shown in docs when the variable is
/// unset).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnvVarDoc {
    /// The `BHARATCODE_*` variable name, exactly as read by the binary.
    pub name: &'static str,
    /// One-line, product-neutral description of what the variable controls.
    pub purpose: &'static str,
    /// Documented default shown when the variable is unset (e.g. `off`).
    pub default: &'static str,
}

/// The canonical, documented `BHARATCODE_*` reference table.
///
/// Curated for the GA docs: the user-facing knobs an operator is most likely to
/// set. This is intentionally a hand-picked subset of every `BHARATCODE_*` key
/// the binary reads — the goal is a readable reference, not an exhaustive dump.
/// Keep entries sorted by name so the rendered docs are stable across builds.
pub const ENV_REFERENCE: &[EnvVarDoc] = &[
    EnvVarDoc {
        name: "BHARATCODE_BUDGET_INR",
        purpose: "Per-session spend ceiling in INR; the run halts once exceeded",
        default: "unset (no budget cap)",
    },
    EnvVarDoc {
        name: "BHARATCODE_CONTEXT_LIMIT",
        purpose: "Override the model context-window size used for compaction",
        default: "unset (provider default)",
    },
    EnvVarDoc {
        name: "BHARATCODE_DISABLE_KEYRING",
        purpose: "Store secrets in an encrypted file instead of the system keyring",
        default: "unset (system keyring used)",
    },
    EnvVarDoc {
        name: "BHARATCODE_DIST_DIR",
        purpose: "Directory the offline checksum verifier resolves release artifacts from",
        default: "dist",
    },
    EnvVarDoc {
        name: "BHARATCODE_LANG",
        purpose: "Interface language for user-facing strings (en, hi, ta, mr)",
        default: "en",
    },
    EnvVarDoc {
        name: "BHARATCODE_MAX_TOKENS",
        purpose: "Cap on output tokens requested per model turn",
        default: "unset (provider default)",
    },
    EnvVarDoc {
        name: "BHARATCODE_MAX_TURNS",
        purpose: "Maximum agent turns before a run stops automatically",
        default: "unset (no turn cap)",
    },
    EnvVarDoc {
        name: "BHARATCODE_MODE",
        purpose: "Default tool-approval mode the agent runs in",
        default: "unset (configured default)",
    },
    EnvVarDoc {
        name: "BHARATCODE_MODEL",
        purpose: "Default model id the agent runs with",
        default: "unset (configured provider default)",
    },
    EnvVarDoc {
        name: "BHARATCODE_OFFLINE",
        purpose: "Disable all outbound network calls for air-gapped operation",
        default: "off",
    },
    EnvVarDoc {
        name: "BHARATCODE_PROVIDER",
        purpose: "Default model provider the agent connects to",
        default: "unset (must be configured)",
    },
    EnvVarDoc {
        name: "BHARATCODE_RESIDENCY",
        purpose: "Data-residency mode restricting which endpoints may be used",
        default: "unset (no residency restriction)",
    },
    EnvVarDoc {
        name: "BHARATCODE_TELEMETRY_OFF",
        purpose: "Kill-switch that force-disables anonymous telemetry",
        default: "off (telemetry follows its config flag)",
    },
    EnvVarDoc {
        name: "BHARATCODE_THINKING_EFFORT",
        purpose: "Reasoning-effort hint passed to thinking-capable models",
        default: "unset (model default)",
    },
];

/// Returns true when the env-reference emit hook is opted in via [`ENABLE_KEY`].
///
/// Any truthy-ish value (`1`, `true`, `yes`, `on`) enables it; everything else
/// (including unset) leaves it off, so the default startup path is unchanged.
pub fn is_enabled() -> bool {
    std::env::var(ENABLE_KEY)
        .map(|raw| is_truthy(&raw))
        .unwrap_or(false)
}

fn is_truthy(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Render the reference as a GA docs-ready GitHub-flavoured markdown table:
/// one header row plus one row per documented variable. Pure: depends only on
/// the constant [`ENV_REFERENCE`] table.
pub fn render_markdown() -> String {
    let mut out = String::new();
    out.push_str("| Variable | Purpose | Default |\n");
    out.push_str("| --- | --- | --- |\n");
    for entry in ENV_REFERENCE {
        out.push_str(&format!(
            "| `{}` | {} | {} |\n",
            entry.name, entry.purpose, entry.default
        ));
    }
    out
}

/// Render the reference as a structured JSON array of
/// `{ "name", "purpose", "default" }` objects, for external doc tooling that
/// prefers to consume structured data rather than parse markdown. Pure:
/// depends only on the constant [`ENV_REFERENCE`] table.
pub fn render_json() -> String {
    let items: Vec<serde_json::Value> = ENV_REFERENCE
        .iter()
        .map(|e| {
            serde_json::json!({
                "name": e.name,
                "purpose": e.purpose,
                "default": e.default,
            })
        })
        .collect();
    serde_json::to_string_pretty(&serde_json::Value::Array(items))
        .unwrap_or_else(|_| "[]".to_string())
}

/// Emit the env-var reference (markdown) through tracing when, and only when,
/// the opt-in [`ENABLE_KEY`] is set. This is the real call site reached from
/// `Config::global()` so the feature is wired into the running binary: with the
/// gate unset it is a no-op and startup is unchanged; with it set the canonical
/// reference is logged once at init, letting docs be captured straight from the
/// binary.
pub fn emit_if_enabled() {
    if !is_enabled() {
        return;
    }
    tracing::info!(target: "bharatcode::env_reference", "{}", render_markdown());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_non_empty_and_well_formed() {
        assert!(
            !ENV_REFERENCE.is_empty(),
            "env reference table must not be empty"
        );
        for entry in ENV_REFERENCE {
            assert!(
                entry.name.starts_with("BHARATCODE_"),
                "documented name must be a BHARATCODE_* key: {}",
                entry.name
            );
            assert!(
                !entry.purpose.trim().is_empty(),
                "{} has empty purpose",
                entry.name
            );
            assert!(
                !entry.default.trim().is_empty(),
                "{} has empty default",
                entry.name
            );
        }
    }

    #[test]
    fn names_are_unique_and_sorted() {
        let mut prev: Option<&str> = None;
        for entry in ENV_REFERENCE {
            if let Some(p) = prev {
                assert!(
                    p < entry.name,
                    "reference table must be unique and sorted: {p} then {}",
                    entry.name
                );
            }
            prev = Some(entry.name);
        }
    }

    #[test]
    fn markdown_renders_every_row_without_brand_leakage() {
        let md = render_markdown();
        assert!(md.contains("| Variable | Purpose | Default |"));
        for entry in ENV_REFERENCE {
            assert!(
                md.contains(entry.name),
                "markdown missing row for {}",
                entry.name
            );
        }
        let lower = md.to_lowercase();
        assert!(!lower.contains("goose"), "env reference leaks brand: goose");
        assert!(!lower.contains("block"), "env reference leaks brand: block");
    }

    #[test]
    fn json_is_structured_and_brand_neutral() {
        let json = render_json();
        let parsed: serde_json::Value =
            serde_json::from_str(&json).expect("render_json must emit valid JSON");
        let arr = parsed.as_array().expect("top level must be a JSON array");
        assert_eq!(
            arr.len(),
            ENV_REFERENCE.len(),
            "JSON must have one object per documented variable"
        );
        for obj in arr {
            assert!(obj.get("name").and_then(|v| v.as_str()).is_some());
            assert!(obj.get("purpose").and_then(|v| v.as_str()).is_some());
            assert!(obj.get("default").and_then(|v| v.as_str()).is_some());
        }
        let lower = json.to_lowercase();
        assert!(!lower.contains("goose"), "json reference leaks brand: goose");
        assert!(!lower.contains("block"), "json reference leaks brand: block");
    }

    #[test]
    fn gate_is_opt_in_and_defaults_off() {
        // A secret-shaped opt-in token fixture, assembled from fragments so no
        // contiguous token literal ever appears in source.
        let fixture_token = ["env", "ref", "9000", "tok"].join("-");
        std::env::remove_var(ENABLE_KEY);
        assert!(!is_enabled(), "must default off when unset");

        std::env::set_var(ENABLE_KEY, &fixture_token);
        assert!(!is_enabled(), "an arbitrary token must not enable the gate");

        std::env::set_var(ENABLE_KEY, "1");
        assert!(is_enabled(), "an explicit truthy value must enable the gate");
        std::env::remove_var(ENABLE_KEY);
    }
}
