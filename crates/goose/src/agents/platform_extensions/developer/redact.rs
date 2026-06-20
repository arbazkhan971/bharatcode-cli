//! Opt-in egress secret redaction for developer shell tool output.
//!
//! When the `BHARATCODE_REDACT` environment variable is set to a truthy value,
//! command stdout/stderr is scanned for a small set of *high-confidence* secret
//! shapes (cloud keys, provider tokens, private-key headers, bearer tokens and
//! `.env`-style assignments) and each match is replaced with `[REDACTED]`
//! before the text is handed back to the model.
//!
//! The regexes are deliberately conservative: they target well-known, uniquely
//! shaped credentials so that ordinary command output (logs, code, prose) is
//! left untouched. When the gate is off (the default) the input is returned
//! verbatim and no scanning is performed.

use std::sync::LazyLock;

use regex::Regex;

/// Replacement sentinel substituted in place of a detected secret.
pub const REDACTED: &str = "[REDACTED]";

/// Name of the environment variable that opts in to redaction.
pub const REDACT_ENV: &str = "BHARATCODE_REDACT";

/// Returns true when egress redaction is enabled via `BHARATCODE_REDACT`.
///
/// Accepted truthy values (case-insensitive): `1`, `true`, `yes`, `on`.
/// Anything else (including unset) leaves redaction disabled.
pub fn is_enabled() -> bool {
    matches!(
        std::env::var(REDACT_ENV)
            .ok()
            .as_deref()
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("1") | Some("true") | Some("yes") | Some("on")
    )
}

/// Compiled, high-confidence secret patterns applied in order.
///
/// Each entry produces a regex whose *entire* match is replaced by
/// [`REDACTED`]. Where only part of a match is the secret (e.g. an assignment
/// `API_KEY=...`), a capture group named `keep` marks the prefix that should be
/// preserved so the surrounding context stays readable.
static PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    let raw: &[&str] = &[
        // AWS access key IDs (AKIA/ASIA/AGPA/AIDA/AROA/AIPA/ANPA/ANVA/ABIA/ACCA).
        r"\b(?:AKIA|ASIA|AGPA|AIDA|AROA|AIPA|ANPA|ANVA|ABIA|ACCA)[0-9A-Z]{16}\b",
        // GitHub personal access / OAuth / app / refresh / server tokens.
        r"\bgh[pousr]_[A-Za-z0-9]{36,255}\b",
        // GitHub fine-grained PATs.
        r"\bgithub_pat_[A-Za-z0-9_]{22,255}\b",
        // Slack tokens (bot/user/app/refresh/legacy) and webhooks.
        r"\bxox[baprs]-[A-Za-z0-9-]{10,}\b",
        // Google API keys.
        r"\bAIza[0-9A-Za-z_\-]{35}\b",
        // Stripe live/test secret & restricted keys.
        r"\b(?:sk|rk)_(?:live|test)_[0-9A-Za-z]{16,}\b",
        // PEM private-key block headers (RSA/EC/DSA/OPENSSH/PGP/generic).
        r"-----BEGIN (?:RSA |EC |DSA |OPENSSH |PGP |ENCRYPTED )?PRIVATE KEY-----",
        // HTTP bearer/authorization tokens (keep the scheme word).
        r"(?i)(?P<keep>\b(?:bearer|authorization:\s*bearer)\s+)[A-Za-z0-9._\-]{16,}",
        // Generic `api_key = "value"` / `apikey: value` / `secret=value` /
        // `token=value` style assignments with a sufficiently long value.
        r#"(?i)(?P<keep>\b(?:api[_-]?key|secret[_-]?key|access[_-]?token|auth[_-]?token|client[_-]?secret|secret|token|passwd|password)\b\s*[:=]\s*)["']?[A-Za-z0-9._\-/+]{12,}["']?"#,
        // `.env`-style upper-snake assignments ending in a credential noun,
        // e.g. `MY_API_KEY=longvalue` or `DB_PASSWORD=hunter2hunter2`.
        r#"(?P<keep>\b[A-Z][A-Z0-9_]*(?:KEY|TOKEN|SECRET|PASSWORD|PASSWD)\s*=\s*)["']?[A-Za-z0-9._\-/+]{8,}["']?"#,
    ];
    raw.iter()
        .map(|p| Regex::new(p).expect("redaction regex must compile"))
        .collect()
});

/// Scan `input` and replace every high-confidence secret with [`REDACTED`].
///
/// This does not consult the env gate; callers decide when to apply it (the
/// developer shell arm only invokes this when [`is_enabled`] is true). The
/// number of redactions performed is returned alongside the transformed text.
pub fn redact_counted(input: &str) -> (String, usize) {
    let mut count = 0usize;
    let mut text = input.to_string();
    for re in PATTERNS.iter() {
        text = re
            .replace_all(&text, |caps: &regex::Captures<'_>| {
                count += 1;
                match caps.name("keep") {
                    Some(keep) => format!("{}{}", keep.as_str(), REDACTED),
                    None => REDACTED.to_string(),
                }
            })
            .into_owned();
    }
    (text, count)
}

/// Convenience wrapper returning only the redacted text.
pub fn redact(input: &str) -> String {
    redact_counted(input).0
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test fixtures below build fake credentials from fragments at runtime so no
    // contiguous, real-looking secret literal exists in source (which would trip
    // provider-side secret scanners / push protection on a synthetic example).

    #[test]
    fn masks_aws_access_key() {
        let secret = format!("AKIA{}", "IOSFODNN7EXAMPLE");
        let (out, n) = redact_counted(&format!("export AWS_KEY={secret} done"));
        assert!(out.contains(REDACTED), "expected redaction, got: {out}");
        assert!(!out.contains(&secret));
        assert!(n >= 1);
    }

    #[test]
    fn masks_github_token() {
        let secret = format!("ghp_{}", "1234567890abcdefghijklmnopqrstuvwxyz");
        let out = redact(&format!("token is {secret}"));
        assert!(!out.contains(&secret));
        assert!(out.contains(REDACTED));
    }

    #[test]
    fn masks_slack_token() {
        let secret = format!(
            "xoxb-{}",
            "2401234567-1234567890123-AbCdEfGhIjKlMnOpQrStUvWx"
        );
        let out = redact(&secret);
        assert!(out.contains(REDACTED));
        assert!(!out.contains(&secret));
    }

    #[test]
    fn masks_private_key_header() {
        let out = redact("-----BEGIN RSA PRIVATE KEY-----\nMIIEo...");
        assert!(out.contains(REDACTED));
        assert!(!out.contains("BEGIN RSA PRIVATE KEY"));
    }

    #[test]
    fn masks_bearer_token_but_keeps_scheme() {
        let out = redact("Authorization: Bearer abcdefghijklmnopqrstuvwxyz0123456789");
        assert!(out.contains(REDACTED));
        assert!(out.to_lowercase().contains("bearer"));
        assert!(!out.contains("abcdefghijklmnopqrstuvwxyz0123456789"));
    }

    #[test]
    fn masks_env_style_assignment_keeps_key_name() {
        let out = redact("DATABASE_PASSWORD=sup3rs3cretvalue123");
        assert!(out.contains("DATABASE_PASSWORD="));
        assert!(out.contains(REDACTED));
        assert!(!out.contains("sup3rs3cretvalue123"));
    }

    #[test]
    fn masks_generic_api_key_assignment() {
        let out = redact(r#"api_key = "abcd1234efgh5678ijkl""#);
        assert!(out.contains(REDACTED));
        assert!(!out.contains("abcd1234efgh5678ijkl"));
    }

    #[test]
    fn leaves_ordinary_text_untouched() {
        let input = "Compiling project... 42 files changed, 7 insertions(+). All tests passed.";
        let (out, n) = redact_counted(input);
        assert_eq!(out, input);
        assert_eq!(n, 0);
    }

    #[test]
    fn does_not_redact_short_low_entropy_assignments() {
        // Short values like `x=1` or `count=3` must not be treated as secrets.
        let input = "x=1\ncount=3\nname=foo";
        let (out, n) = redact_counted(input);
        assert_eq!(out, input);
        assert_eq!(n, 0);
    }

    #[test]
    fn is_enabled_reflects_env() {
        // Note: relies on env not being globally set in the test process.
        std::env::remove_var(REDACT_ENV);
        assert!(!is_enabled());
        std::env::set_var(REDACT_ENV, "1");
        assert!(is_enabled());
        std::env::set_var(REDACT_ENV, "false");
        assert!(!is_enabled());
        std::env::remove_var(REDACT_ENV);
    }
}
