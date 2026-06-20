use anyhow::Result;

use crate::session::build_session;
use crate::session::SessionBuilderConfig;

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
