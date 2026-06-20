//! Lightweight, opt-in command execution policy for the shell tool.
//!
//! When `BHARATCODE_EXEC_POLICY` is set to the path of a JSON policy file, shell
//! commands are screened against command-prefix allow/deny lists before they are
//! spawned. When the variable is unset (or set to `off`/`0`/`false`/empty) the
//! policy is disabled and every command runs exactly as before — this is the
//! default, so existing behaviour is unchanged.
//!
//! This is an intentionally small, dependency-free gate (whitespace tokenisation
//! + quote-aware operator splitting), not a full shell parser. It is an
//! independent reimplementation of the general "allow/deny command prefixes"
//! idea; it does not use a policy-as-code engine and shares no code with any
//! external project.
//!
//! ## Policy file format
//!
//! ```json
//! {
//!   "allow": ["git", "cargo build", "ls"],
//!   "deny":  ["rm -rf", "sudo", "curl"]
//! }
//! ```
//!
//! Each entry is a whitespace-separated command *prefix*. A command segment
//! matches an entry when the segment's leading tokens equal the entry's tokens
//! (so `git push` matches `git push origin main` but not `git status`).
//!
//! ## Decision rules
//!
//! The command line is split into segments on the shell operators `;`, `&&`,
//! `||`, `|`, `&` and newlines, plus command-substitution / subshell boundaries
//! (`$(...)`, backticks, `(...)` subshells and `{ ...; }` groups), with quotes
//! and `${...}` parameter expansion respected. Every segment is screened
//! independently:
//!
//! * If a segment matches any `deny` prefix, the command is **denied** (deny
//!   takes precedence over allow).
//! * If `allow` is non-empty and a segment matches no `allow` prefix, the
//!   command is **denied** (allow-list mode).
//! * Otherwise the command is **allowed**.
//!
//! If the policy is enabled but the file cannot be read or parsed, the command
//! is denied with a clear error rather than silently bypassing the restriction.

use std::path::PathBuf;

use serde::Deserialize;

const ENV_VAR: &str = "BHARATCODE_EXEC_POLICY";

/// Outcome of screening a command against the policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// The command may run.
    Allow,
    /// The command is blocked; `reason` is a user-facing explanation.
    Deny { reason: String },
}

/// Allow/deny command-prefix lists loaded from the policy file.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ExecPolicy {
    /// When non-empty, a command segment must match one of these prefixes.
    #[serde(default)]
    pub allow: Vec<String>,
    /// A command segment matching any of these prefixes is always denied.
    #[serde(default)]
    pub deny: Vec<String>,
}

impl ExecPolicy {
    /// Parse a policy from JSON text.
    pub fn from_json(text: &str) -> Result<Self, String> {
        serde_json::from_str(text).map_err(|error| format!("invalid exec policy JSON: {error}"))
    }

    /// Screen a full command line, returning [`Decision::Deny`] on the first
    /// segment that violates the policy.
    pub fn check(&self, command_line: &str) -> Decision {
        for segment in split_segments(command_line) {
            let tokens = tokenize(&segment);
            if tokens.is_empty() {
                continue;
            }

            if let Some(prefix) = matched_prefix(&self.deny, &tokens) {
                return Decision::Deny {
                    reason: format!(
                        "Command blocked by exec policy ({ENV_VAR}): `{}` matches denied prefix `{}`.",
                        segment.trim(),
                        prefix
                    ),
                };
            }

            if !self.allow.is_empty() && matched_prefix(&self.allow, &tokens).is_none() {
                return Decision::Deny {
                    reason: format!(
                        "Command blocked by exec policy ({ENV_VAR}): `{}` is not in the allowed-command list.",
                        segment.trim()
                    ),
                };
            }
        }
        Decision::Allow
    }
}

/// Resolve the policy file path from the environment, or `None` when the policy
/// is disabled (the default).
fn policy_path() -> Option<PathBuf> {
    let value = std::env::var(ENV_VAR).ok()?;
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.eq_ignore_ascii_case("off")
        || trimmed.eq_ignore_ascii_case("false")
        || trimmed == "0"
    {
        return None;
    }
    Some(PathBuf::from(trimmed))
}

fn load_policy(path: &std::path::Path) -> Result<ExecPolicy, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    ExecPolicy::from_json(&text)
}

/// Screen `command_line` against the active policy.
///
/// Returns [`Decision::Allow`] when the policy is disabled (default). When the
/// policy is enabled but its file cannot be loaded, the command is denied with a
/// clear error so an opted-in restriction never fails open.
pub fn check_command(command_line: &str) -> Decision {
    let Some(path) = policy_path() else {
        return Decision::Allow;
    };
    match load_policy(&path) {
        Ok(policy) => policy.check(command_line),
        Err(error) => Decision::Deny {
            reason: format!(
                "Command blocked: exec policy is enabled ({ENV_VAR}) but could not be loaded: {error}"
            ),
        },
    }
}

/// Return the matched prefix (joined for display) if any entry in `prefixes` is
/// a leading-token prefix of `tokens`.
fn matched_prefix(prefixes: &[String], tokens: &[String]) -> Option<String> {
    for prefix in prefixes {
        let prefix_tokens = tokenize(prefix);
        if prefix_tokens.is_empty() || prefix_tokens.len() > tokens.len() {
            continue;
        }
        if tokens[..prefix_tokens.len()] == prefix_tokens[..] {
            return Some(prefix_tokens.join(" "));
        }
    }
    None
}

fn tokenize(segment: &str) -> Vec<String> {
    segment.split_whitespace().map(str::to_string).collect()
}

/// Split a command line into individual command segments on the shell operators
/// `;`, `&&`, `||`, `|`, `&` and newlines, plus command-substitution / subshell
/// boundaries (`$(...)`, backticks, `(...)` and `{ ...; }`), leaving operators
/// inside single or double quotes untouched and keeping `${...}` parameter
/// expansion intact.
fn split_segments(command_line: &str) -> Vec<String> {
    let chars: Vec<char> = command_line.chars().collect();
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        if in_single {
            if c == '\'' {
                in_single = false;
            }
            current.push(c);
            i += 1;
            continue;
        }
        if in_double {
            if c == '"' {
                in_double = false;
            }
            current.push(c);
            i += 1;
            continue;
        }

        match c {
            '\'' => {
                in_single = true;
                current.push(c);
                i += 1;
            }
            '"' => {
                in_double = true;
                current.push(c);
                i += 1;
            }
            ';' | '\n' => {
                segments.push(std::mem::take(&mut current));
                i += 1;
            }
            '&' => {
                segments.push(std::mem::take(&mut current));
                i += if i + 1 < chars.len() && chars[i + 1] == '&' {
                    2
                } else {
                    1
                };
            }
            '|' => {
                segments.push(std::mem::take(&mut current));
                i += if i + 1 < chars.len() && chars[i + 1] == '|' {
                    2
                } else {
                    1
                };
            }
            // Command substitution and subshell/group boundaries. Without these a
            // denied command tucked inside `$(...)`, backticks, a `(...)` subshell
            // or a `{ ...; }` group would never start a screened segment, letting
            // e.g. `echo $(rm -rf x)` slip past a `rm -rf` deny rule. Splitting on
            // these openers puts the inner command at the head of its own segment
            // so prefix matching still catches it. `${...}` parameter expansion is
            // deliberately excluded (the `$`-guarded `{`), so `echo ${HOME}` is
            // not fragmented.
            '`' | '(' | ')' => {
                segments.push(std::mem::take(&mut current));
                i += 1;
            }
            '{' if !current.ends_with('$') => {
                segments.push(std::mem::take(&mut current));
                i += 1;
            }
            _ => {
                current.push(c);
                i += 1;
            }
        }
    }

    segments.push(current);
    segments
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(allow: &[&str], deny: &[&str]) -> ExecPolicy {
        ExecPolicy {
            allow: allow.iter().map(|s| s.to_string()).collect(),
            deny: deny.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn is_denied(decision: Decision) -> bool {
        matches!(decision, Decision::Deny { .. })
    }

    #[test]
    fn empty_policy_allows_everything() {
        let p = ExecPolicy::default();
        assert_eq!(p.check("rm -rf /"), Decision::Allow);
        assert_eq!(p.check("anything goes"), Decision::Allow);
    }

    #[test]
    fn deny_blocks_matching_prefix() {
        let p = policy(&[], &["rm -rf", "sudo"]);
        assert!(is_denied(p.check("rm -rf /tmp/x")));
        assert!(is_denied(p.check("sudo apt update")));
        assert_eq!(p.check("rm file.txt"), Decision::Allow);
        assert_eq!(p.check("ls -la"), Decision::Allow);
    }

    #[test]
    fn deny_reason_is_user_facing() {
        let p = policy(&[], &["curl"]);
        match p.check("curl https://example.com") {
            Decision::Deny { reason } => {
                assert!(reason.contains("exec policy"));
                assert!(reason.contains("curl"));
            }
            Decision::Allow => panic!("expected deny"),
        }
    }

    #[test]
    fn allow_list_restricts_to_listed_prefixes() {
        let p = policy(&["git", "cargo build", "ls"], &[]);
        assert_eq!(p.check("git status"), Decision::Allow);
        assert_eq!(p.check("cargo build --release"), Decision::Allow);
        assert_eq!(p.check("ls -la"), Decision::Allow);
        assert!(is_denied(p.check("cargo test")));
        assert!(is_denied(p.check("python script.py")));
    }

    #[test]
    fn deny_takes_precedence_over_allow() {
        let p = policy(&["git"], &["git push"]);
        assert_eq!(p.check("git status"), Decision::Allow);
        assert!(is_denied(p.check("git push origin main")));
    }

    #[test]
    fn prefix_must_match_on_token_boundaries() {
        let p = policy(&[], &["rm"]);
        // "rmdir" must not be treated as starting with the "rm" token.
        assert_eq!(p.check("rmdir olddir"), Decision::Allow);
        assert!(is_denied(p.check("rm file")));
    }

    #[test]
    fn chained_commands_are_each_screened() {
        let p = policy(&[], &["rm -rf"]);
        assert!(is_denied(p.check("git status && rm -rf /")));
        assert!(is_denied(p.check("echo hi; rm -rf .")));
        assert!(is_denied(p.check("cat x | rm -rf y")));
        assert_eq!(p.check("echo a && echo b"), Decision::Allow);
    }

    #[test]
    fn denied_command_inside_substitution_is_caught() {
        let p = policy(&[], &["rm -rf"]);
        // Command substitution: `$(...)` and backticks must not hide a denied
        // command from screening.
        assert!(is_denied(p.check("echo $(rm -rf x)")));
        assert!(is_denied(p.check("echo `rm -rf x`")));
        assert!(is_denied(p.check("foo=$(rm -rf /tmp/x) bar")));
        // Nested substitution.
        assert!(is_denied(p.check("echo $(echo $(rm -rf x))")));
    }

    #[test]
    fn denied_command_inside_subshell_or_group_is_caught() {
        let p = policy(&[], &["rm -rf"]);
        assert!(is_denied(p.check("(rm -rf x)")));
        assert!(is_denied(p.check("(cd /tmp && rm -rf x)")));
        assert!(is_denied(p.check("{ rm -rf x; }")));
    }

    #[test]
    fn parameter_expansion_is_not_fragmented() {
        // `${...}` is parameter expansion, not command execution, so it must not
        // be split into bogus segments that a strict allow-list would then reject.
        let p = policy(&["echo"], &[]);
        assert_eq!(p.check("echo ${HOME}"), Decision::Allow);
        assert_eq!(p.check("echo ${HOME}/bin"), Decision::Allow);
        assert_eq!(p.check("echo ${x:-default}"), Decision::Allow);
    }

    #[test]
    fn substitution_does_not_smuggle_past_allow_list() {
        // In allow-list mode, a command hidden in a substitution must still be
        // screened against the allow list (and rejected when unlisted).
        let p = policy(&["echo"], &[]);
        assert!(is_denied(p.check("echo $(curl evil.test)")));
        assert!(is_denied(p.check("echo `curl evil.test`")));
    }

    #[test]
    fn operators_inside_quotes_are_not_split() {
        let p = policy(&[], &["rm"]);
        // The "rm" here is inside a quoted echo argument, not a command.
        assert_eq!(p.check("echo 'a && rm b'"), Decision::Allow);
        assert_eq!(p.check("echo \"x | rm y\""), Decision::Allow);
    }

    #[test]
    fn allow_list_rejects_unlisted_segment_in_chain() {
        let p = policy(&["echo", "git"], &[]);
        assert_eq!(p.check("echo hi && git status"), Decision::Allow);
        assert!(is_denied(p.check("echo hi && curl evil.test")));
    }

    #[test]
    fn from_json_parses_allow_and_deny() {
        let p = ExecPolicy::from_json(r#"{"allow":["git"],"deny":["rm -rf"]}"#).unwrap();
        assert_eq!(p.allow, vec!["git".to_string()]);
        assert_eq!(p.deny, vec!["rm -rf".to_string()]);
        assert!(is_denied(p.check("rm -rf /")));
        assert_eq!(p.check("git log"), Decision::Allow);
    }

    #[test]
    fn from_json_allows_missing_fields() {
        let p = ExecPolicy::from_json("{}").unwrap();
        assert!(p.allow.is_empty());
        assert!(p.deny.is_empty());
        assert_eq!(p.check("anything"), Decision::Allow);
    }

    #[test]
    fn from_json_rejects_malformed_input() {
        assert!(ExecPolicy::from_json("not json").is_err());
    }
}
