use anyhow::Result;

use crate::session::build_session;
use crate::session::SessionBuilderConfig;

// RAG / index readiness deep check (BharatCode v49). Declared inline so the
// module lives alongside the other doctor checks without a separate `pub mod`
// in commands/mod.rs.
#[path = "index_check.rs"]
mod index_check;

// Large-repo readiness deep check (BharatCode v67). Declared inline alongside
// the other doctor checks, same posture as `index_check` above.
#[path = "repo_profile.rs"]
mod repo_profile;

// Session-DB integrity & fragmentation deep check (BharatCode v65). Declared
// inline alongside the other doctor checks, same posture as `index_check` above.
#[path = "db_integrity.rs"]
mod db_integrity;

// Ecosystem health deep check (BharatCode v73). Reports installed plugins,
// configured MCP extensions, and plugin tool-use hooks. Declared inline
// alongside the other doctor checks, same posture as `index_check` above.
#[path = "ecosystem_check.rs"]
mod ecosystem_check;

// Extension catalog readiness deep check (BharatCode v74). Surfaces how many
// curated catalog entries are present and how many are currently active. The
// catalog module is shared with the `catalog` subcommand (wired in cli.rs);
// declared inline here, same posture as `index_check` above, so the doctor row
// reuses the same embedded catalog without a separate `pub mod`.
#[path = "catalog.rs"]
mod catalog;

// CI-integration readiness deep check (BharatCode v77). A read-only probe that
// detects a CI provider in the repo (GitHub Actions / GitLab CI / Jenkins) and
// reports whether a non-interactive `bharatcode` step is present. Declared
// inline alongside the other doctor checks, same posture as `index_check` above.
#[path = "ci_check.rs"]
mod ci_check;

// Locale / accessibility readiness deep check (BharatCode v90). A read-only probe
// that reports the resolved active locale (en/hi/ta), the three-way en/hi/ta
// translation parity, and the opt-in UX toggles (BHARATCODE_A11Y / _NOTIFY /
// _COST_DASHBOARD). Declared inline alongside the other doctor checks, same
// posture as `index_check` above.
#[path = "i18n_check.rs"]
mod i18n_check;

/// Default Ollama endpoint used when `OLLAMA_HOST` is not configured.
const OLLAMA_DEFAULT_HOST: &str = "localhost";
const OLLAMA_DEFAULT_PORT: u16 = 11434;

pub async fn handle_doctor() -> Result<()> {
    print_settings_summary().await;
    print_deep_checks().await;

    let mut session = build_session(SessionBuilderConfig {
        no_session: true,
        interactive: true,
        ..Default::default()
    })
    .await;

    session.interactive(Some("/doctor".to_string())).await
}

/// Look up a user-facing label through the i18n layer, falling back to the
/// English `default` when the active locale table has no entry for `key`.
///
/// `t()` echoes the key back when it is missing, so an unchanged key is treated
/// as "untranslated" and the English default is used. This keeps the rendered
/// summary stable in English while leaving room for translations to land later
/// without touching this file.
fn label(key: &str, default: &str) -> String {
    let translated = crate::tr!(key);
    if translated == key {
        default.to_string()
    } else {
        translated
    }
}

/// Print a display-only summary of the active BharatCode configuration before
/// the interactive doctor session investigates further. Read-only: it never
/// mutates config, it only reports what is currently in effect.
async fn print_settings_summary() {
    let config = goose::config::Config::global();

    let not_configured = label("doctor.not_configured", "not configured");
    let provider = config
        .get_bharatcode_provider()
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| not_configured.clone());
    let model = config
        .get_bharatcode_model()
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| not_configured.clone());

    let host_port = ollama_host_port(config);
    let reachable = {
        let target = host_port.clone();
        tokio::task::spawn_blocking(move || tcp_reachable(&target))
            .await
            .unwrap_or(false)
    };
    let local_engine = if reachable {
        format!("{} ({})", label("doctor.reachable", "reachable"), host_port)
    } else {
        format!(
            "{} ({})",
            label("doctor.not_reachable", "not reachable"),
            host_port
        )
    };

    let config_dir = goose::config::paths::Paths::config_dir()
        .display()
        .to_string();

    let telemetry = if telemetry_enabled(config) {
        label("doctor.on", "on")
    } else {
        label("doctor.off", "off")
    };

    let not_set = label("doctor.not_set", "not set");
    // Probe the real config / environment keys each feature actually reads, so
    // the summary reports true settings instead of keys no module consults.
    let residency = read_setting(config, &[goose::residency::RESIDENCY_MODE_KEY])
        .unwrap_or_else(|| not_set.clone());
    let budget = read_setting(config, &[crate::commands::budget::BUDGET_INR_KEY])
        .unwrap_or_else(|| not_set.clone());
    // The USD->INR rate always has an effective value (configured or default),
    // so report what the cost ledger will actually use.
    let inr_rate = format!("{:.2}", crate::commands::cost_ledger::usd_inr_rate());
    let offline = if goose::offline::is_offline() {
        label("doctor.on", "on")
    } else {
        label("doctor.off", "off")
    };

    println!();
    println!(
        "{}",
        crate::theme::heading(label("doctor.title", "BharatCode Doctor"))
    );
    print_row(
        &label("doctor.provider_model", "Provider / Model"),
        &format!("{} / {}", provider, model),
    );
    print_row(
        &label("doctor.local_engine", "Local engine (Ollama)"),
        &local_engine,
    );
    print_row(&label("doctor.config_dir", "Config directory"), &config_dir);
    print_row(&label("doctor.telemetry", "Telemetry"), &telemetry);
    print_row(&label("doctor.residency", "Data residency"), &residency);
    print_row(&label("doctor.offline", "Offline mode"), &offline);
    print_row(&label("doctor.budget", "Budget (INR)"), &budget);
    print_row(
        &label("doctor.inr_rate", "INR rate (USD to INR)"),
        &inr_rate,
    );
    println!();
}

/// Run and print the deep diagnostic checks (provider reachability, config dir
/// writability, git, offline/residency coherence, session DB storage). The
/// probes touch the network/disk and block, so they run on a blocking task to
/// keep the async runtime responsive; each result prints with a ✓/⚠/✗ glyph and
/// an optional hint, matching the summary's spacing.
async fn print_deep_checks() {
    use crate::commands::doctor_checks::{run_all, Status};

    let results = tokio::task::spawn_blocking(run_all)
        .await
        .unwrap_or_default();

    println!(
        "{}",
        crate::theme::heading(label("doctor.checks_title", "Deep checks"))
    );
    for result in results {
        let glyph = result.status.glyph();
        let painted = match result.status {
            Status::Ok => crate::theme::success(glyph),
            Status::Warn => crate::theme::warning(glyph),
            Status::Fail => crate::theme::error(glyph),
        };
        println!("  {} {}", painted, result.label);
        if !result.hint.is_empty() {
            println!("      {}", crate::theme::muted(&result.hint));
        }
    }

    // RAG / index readiness: a read-only pre-flight reporting how many files a
    // bounded, gitignore-aware scan would index. Always shown like the other
    // deep checks; falls back to "." when the cwd cannot be resolved.
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let (st, msg) = index_check::index_readiness(&cwd);
    let glyph = match st {
        Status::Ok => crate::theme::success(st.glyph()),
        Status::Warn => crate::theme::warning(st.glyph()),
        Status::Fail => crate::theme::error(st.glyph()),
    };
    println!("  {} {}", glyph, msg);

    // CI-integration readiness: a read-only probe that detects a known CI
    // provider in the tree (GitHub Actions / GitLab CI / Jenkins) and reports
    // whether a non-interactive `bharatcode` step is already wired plus the
    // `BHARATCODE_AUTOMATION` headless state. Always shown like the other deep
    // checks; warns only when a provider is present without a bharatcode step.
    // Reuses the cwd resolved above.
    let (st, msg) = ci_check::ci_readiness(&cwd);
    let glyph = match st {
        Status::Ok => crate::theme::success(st.glyph()),
        Status::Warn => crate::theme::warning(st.glyph()),
        Status::Fail => crate::theme::error(st.glyph()),
    };
    println!("  {} {}", glyph, msg);
    // When CI is not yet wired (provider present without a bharatcode step, or
    // no provider at all), surface a copy-paste GitHub Actions snippet as a
    // muted hint so the operator has a known-good starting point.
    if st != Status::Ok {
        for line in ci_check::sample_workflow().lines() {
            println!("      {}", crate::theme::muted(line));
        }
    }

    // Large-repo readiness: a read-only, bounded, gitignore-aware profile of the
    // current tree (file count, total bytes, deepest depth, largest file).
    // Always shown like the other deep checks; warns when the repo crosses the
    // tunable thresholds. Reuses the cwd resolved above.
    let profile = repo_profile::profile(&cwd);
    let (st, msg) = repo_profile::readiness_line(&profile);
    let glyph = match st {
        Status::Ok => crate::theme::success(st.glyph()),
        Status::Warn => crate::theme::warning(st.glyph()),
        Status::Fail => crate::theme::error(st.glyph()),
    };
    println!("  {} {}", glyph, msg);

    // Session-DB integrity & fragmentation: a read-only, best-effort probe that
    // runs SQLite `PRAGMA quick_check` plus a freelist/page-count read on
    // sessions.db, reporting OK / integrity issue / vacuum recommended. Opens its
    // own short-lived read-only connection (never the live session pool); a
    // missing DB reports a benign "no DB yet".
    let (st, msg) = db_integrity::check().await;
    let glyph = match st {
        Status::Ok => crate::theme::success(st.glyph()),
        Status::Warn => crate::theme::warning(st.glyph()),
        Status::Fail => crate::theme::error(st.glyph()),
    };
    println!("  {} {}", glyph, msg);

    // Ecosystem health: a read-only, always-visible report of the user's
    // extensibility surface — installed plugins, configured MCP extensions, and
    // plugin tool-use hooks — so they can confirm it is actually wired. Each row
    // carries its own status; nothing here ever gates the doctor run.
    println!(
        "{}",
        crate::theme::heading(label("ecosystem.title", "Ecosystem"))
    );
    for row in ecosystem_check::ecosystem_rows() {
        let glyph = match row.status {
            Status::Ok => crate::theme::success(row.status.glyph()),
            Status::Warn => crate::theme::warning(row.status.glyph()),
            Status::Fail => crate::theme::error(row.status.glyph()),
        };
        println!("  {} {}: {}", glyph, row.label, row.detail);
    }

    // Extension catalog readiness: a read-only, always-visible row reporting how
    // many curated catalog entries are present and how many are currently
    // active. Best-effort — if the enabled-extension list cannot be read it
    // reports the catalog total only. Rendered in the same shape as the
    // index/repo readiness rows above.
    let (st, msg) = catalog::catalog_readiness();
    let glyph = match st {
        Status::Ok => crate::theme::success(st.glyph()),
        Status::Warn => crate::theme::warning(st.glyph()),
        Status::Fail => crate::theme::error(st.glyph()),
    };
    println!("  {} {}", glyph, msg);

    // Locale / accessibility readiness: a read-only, always-visible row reporting
    // the resolved active locale (en/hi/ta), the three-way en/hi/ta translation
    // parity, and the opt-in UX toggles (BHARATCODE_A11Y / BHARATCODE_NOTIFY /
    // BHARATCODE_COST_DASHBOARD) so the operator can verify their UX configuration
    // at a glance. Rendered in the same shape as the index/repo readiness rows
    // above; warns on a parity gap or when an accessibility/notification toggle is
    // active.
    let (st, msg) = i18n_check::i18n_readiness();
    let glyph = match st {
        Status::Ok => crate::theme::success(st.glyph()),
        Status::Warn => crate::theme::warning(st.glyph()),
        Status::Fail => crate::theme::error(st.glyph()),
    };
    println!("  {} {}", glyph, msg);

    println!();
}

fn print_row(name: &str, value: &str) {
    println!("  {:<24} {}", format!("{}:", name), value);
}

/// Telemetry is opt-in. It counts as enabled only when the config flag is `true`
/// and the `BHARATCODE_TELEMETRY_OFF` environment kill-switch is not set.
fn telemetry_enabled(config: &goose::config::Config) -> bool {
    let off_via_env = std::env::var("BHARATCODE_TELEMETRY_OFF")
        .map(|v| {
            let v = v.trim().to_ascii_lowercase();
            !v.is_empty() && v != "0" && v != "false" && v != "no"
        })
        .unwrap_or(false);
    if off_via_env {
        return false;
    }
    config
        .get_param::<bool>("BHARATCODE_TELEMETRY_ENABLED")
        .unwrap_or(false)
}

/// Return the first non-empty value among `keys`, rendered as a display string.
/// Handles both string and numeric config values (e.g. a budget or INR rate).
fn read_setting(config: &goose::config::Config, keys: &[&str]) -> Option<String> {
    for &key in keys {
        if let Ok(value) = config.get_param::<serde_json::Value>(key) {
            let rendered = match value {
                serde_json::Value::String(s) => s,
                serde_json::Value::Null => continue,
                other => other.to_string(),
            };
            let rendered = rendered.trim().to_string();
            if !rendered.is_empty() {
                return Some(rendered);
            }
        }
    }
    None
}

/// Resolve the Ollama endpoint to a `host:port` string for the reachability
/// probe, mirroring how the provider reads `OLLAMA_HOST`.
fn ollama_host_port(config: &goose::config::Config) -> String {
    let raw = config
        .get_param::<String>("OLLAMA_HOST")
        .unwrap_or_else(|_| OLLAMA_DEFAULT_HOST.to_string());
    let trimmed = raw.trim();
    let stripped = trimmed
        .strip_prefix("http://")
        .or_else(|| trimmed.strip_prefix("https://"))
        .unwrap_or(trimmed)
        .trim_end_matches('/');
    if stripped.is_empty() {
        format!("{}:{}", OLLAMA_DEFAULT_HOST, OLLAMA_DEFAULT_PORT)
    } else if stripped.contains(':') {
        stripped.to_string()
    } else {
        format!("{}:{}", stripped, OLLAMA_DEFAULT_PORT)
    }
}

/// Best-effort TCP reachability probe with a short timeout. Returns `false` on
/// any resolution or connection failure.
fn tcp_reachable(host_port: &str) -> bool {
    use std::net::{TcpStream, ToSocketAddrs};
    use std::time::Duration;

    let addrs = match host_port.to_socket_addrs() {
        Ok(addrs) => addrs,
        Err(_) => return false,
    };
    for addr in addrs {
        if TcpStream::connect_timeout(&addr, Duration::from_millis(400)).is_ok() {
            return true;
        }
    }
    false
}
