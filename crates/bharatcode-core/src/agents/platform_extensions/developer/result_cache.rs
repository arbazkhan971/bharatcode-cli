//! Per-run memo cache for side-effect-free developer tool results.
//!
//! Some developer tool calls are pure reads: `cat`, `ls`, `git status`, and
//! similar. When the model issues the *same* read twice within a single run
//! (a very common pattern while it re-orients itself), re-spawning the process
//! is pure latency with no new information. This module memoizes those results,
//! keyed by tool name plus the canonicalized arguments, so a repeated identical
//! read returns instantly instead of shelling out again.
//!
//! The cache is intentionally conservative:
//!
//! * **Opt-in.** When `BHARATCODE_TOOL_CACHE` is unset (the default) every entry
//!   point here is a no-op: `lookup` always returns `None` and `store_*` never
//!   records anything, so behaviour is byte-identical to a build without this
//!   module.
//! * **Read-only only.** A result is stored only when the command is classified
//!   as read-only by the conservative allow-list in [`is_read_only_command`].
//!   Anything that could mutate state (or that we cannot prove is a pure read)
//!   is never cached.
//! * **Never caches errors.** A `CallToolResult` flagged `is_error` is not
//!   stored, so a transient failure cannot be served back as if it were a real
//!   answer.
//! * **Invalidated on mutation.** The shell `call_tool` arm calls
//!   [`invalidate_all`] for any non-read-only command, and write/edit tools call
//!   it too, so a cached read can never survive a write that might have changed
//!   what it observed.
//! * **Bounded.** Entries live in a process-global LRU capped at
//!   [`CACHE_CAPACITY`], so the cache cannot grow without bound.
//!
//! Original BharatCode work; not ported from any third party.

use lru::LruCache;
use rmcp::model::{CallToolResult, JsonObject};
use serde_json::Value;
use std::num::NonZeroUsize;
use std::sync::{LazyLock, Mutex};

/// Name of the environment variable that opts in to the tool-result memo cache.
pub const TOOL_CACHE_ENV: &str = "BHARATCODE_TOOL_CACHE";

/// Maximum number of memoized tool results held at once.
pub const CACHE_CAPACITY: usize = 128;

/// Returns true when the tool-result memo cache is enabled via
/// `BHARATCODE_TOOL_CACHE`.
///
/// Accepted truthy values (case-insensitive): `1`, `true`, `yes`, `on`.
/// Anything else (including unset) leaves the cache disabled, in which case
/// every public entry point in this module is a no-op.
pub fn is_enabled() -> bool {
    is_truthy(std::env::var(TOOL_CACHE_ENV).ok().as_deref())
}

fn is_truthy(raw: Option<&str>) -> bool {
    matches!(
        raw.map(str::trim).map(str::to_ascii_lowercase).as_deref(),
        Some("1") | Some("true") | Some("yes") | Some("on")
    )
}

static CACHE: LazyLock<Mutex<LruCache<String, CallToolResult>>> = LazyLock::new(|| {
    let capacity = NonZeroUsize::new(CACHE_CAPACITY).expect("cache capacity must be non-zero");
    Mutex::new(LruCache::new(capacity))
});

/// Build a stable cache key from the tool name and its arguments.
///
/// The key is invariant under argument key ordering (object keys are sorted)
/// and under leading/trailing or collapsible interior whitespace in the
/// `command` string, so semantically identical calls collide as intended.
fn cache_key(name: &str, arguments: &Option<JsonObject>) -> String {
    let canonical = match arguments {
        Some(map) => canonicalize_value(&Value::Object(map.clone())),
        None => Value::Null,
    };
    format!("{name}\u{1f}{canonical}")
}

/// Recursively canonicalize a JSON value: object keys are emitted in sorted
/// order and the `command` string (the shell payload) is whitespace-normalized
/// so cosmetic spacing differences do not defeat the cache.
fn canonicalize_value(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(&String, &Value)> = map.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            let mut out = serde_json::Map::with_capacity(entries.len());
            for (key, val) in entries {
                let canonical = if key == "command" {
                    if let Value::String(s) = val {
                        Value::String(normalize_whitespace(s))
                    } else {
                        canonicalize_value(val)
                    }
                } else {
                    canonicalize_value(val)
                };
                out.insert(key.clone(), canonical);
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(canonicalize_value).collect()),
        other => other.clone(),
    }
}

/// Collapse all runs of ASCII whitespace to a single space and trim the ends.
fn normalize_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Look up a previously memoized result for this tool call.
///
/// Returns `None` (and touches nothing) when the cache is disabled or there is
/// no matching entry. A hit refreshes the entry's LRU recency.
pub fn lookup(name: &str, arguments: &Option<JsonObject>) -> Option<CallToolResult> {
    if !is_enabled() {
        return None;
    }
    let key = cache_key(name, arguments);
    let mut cache = CACHE.lock().expect("tool-result cache mutex poisoned");
    cache.get(&key).cloned()
}

/// Memoize `result` for this tool call, but only when the command is a proven
/// read-only operation and the result is not an error.
///
/// This is a no-op when the cache is disabled, when the command is not
/// classified read-only, or when `result.is_error` is `Some(true)`.
pub fn store_if_read_only(name: &str, arguments: &Option<JsonObject>, result: &CallToolResult) {
    if !is_enabled() {
        return;
    }
    if result.is_error == Some(true) {
        return;
    }
    if !arguments_are_read_only(arguments) {
        return;
    }
    let key = cache_key(name, arguments);
    let mut cache = CACHE.lock().expect("tool-result cache mutex poisoned");
    cache.put(key, result.clone());
}

/// Returns true when the `command` argument is classified read-only.
///
/// Arguments without a `command` string are treated as not cacheable.
pub fn arguments_are_read_only(arguments: &Option<JsonObject>) -> bool {
    arguments
        .as_ref()
        .and_then(|map| map.get("command"))
        .and_then(Value::as_str)
        .map(is_read_only_command)
        .unwrap_or(false)
}

/// Drop every memoized entry.
///
/// Called whenever a mutation could have invalidated prior reads (a
/// non-read-only shell command, or any edit/write tool). A no-op when the cache
/// is disabled.
pub fn invalidate_all() {
    if !is_enabled() {
        return;
    }
    let mut cache = CACHE.lock().expect("tool-result cache mutex poisoned");
    cache.clear();
}

/// Conservative read-only shell-command classifier.
///
/// Returns true only when *every* simple command in the pipeline/sequence is
/// drawn from a small allow-list of commands that do not mutate state, and the
/// command line contains no constructs (redirection, command substitution,
/// subshells, background jobs) that could smuggle a side effect past the
/// allow-list. When in doubt it returns false, so a command is cached only when
/// we can prove it is a pure read.
pub fn is_read_only_command(command: &str) -> bool {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return false;
    }

    // Reject anything that can write, substitute, or spawn beyond a plain
    // pipeline of read commands.
    const FORBIDDEN: &[char] = &['>', '<', '`', '$', '(', ')', '{', '}', '&', ';', '\n', '\\'];
    if trimmed.contains(FORBIDDEN) {
        return false;
    }

    // Split on the only operators we tolerate (`|` and `&&` is excluded by the
    // `&` ban above, so just the pipe), and require every stage to be a known
    // read-only command.
    let stages: Vec<&str> = trimmed.split('|').collect();
    if stages.iter().any(|s| s.trim().is_empty()) {
        return false;
    }
    stages.iter().all(|stage| stage_is_read_only(stage.trim()))
}

fn stage_is_read_only(stage: &str) -> bool {
    let mut words = stage.split_whitespace();
    let Some(cmd) = words.next() else {
        return false;
    };

    // `find` can mutate via `-exec`/`-delete`; reject those forms outright while
    // still allowing plain directory listings.
    if cmd == "find" {
        return !stage.split_whitespace().skip(1).any(|w| {
            matches!(
                w,
                "-exec" | "-execdir" | "-delete" | "-ok" | "-okdir" | "-fprint"
            )
        });
    }

    match cmd {
        // Plain readers. Excluded on purpose: `sed`/`awk` (in-place `-i` /
        // `system()` can mutate), `env` (`env CMD ...` runs an arbitrary CMD).
        "cat" | "head" | "tail" | "ls" | "pwd" | "wc" | "echo" | "grep" | "egrep" | "fgrep"
        | "rg" | "file" | "stat" | "du" | "df" | "tree" | "which" | "whoami" | "hostname"
        | "uname" | "date" | "printenv" | "sort" | "uniq" | "cut" | "nl" | "basename"
        | "dirname" | "realpath" | "readlink" | "true" | "diff" | "cmp" | "column" | "tr"
        | "fold" | "od" | "xxd" | "less" | "more" => true,

        // git: only a handful of read-only subcommands.
        "git" => {
            let mut rest = stage.split_whitespace().skip(1).peekable();
            // Skip leading `-C <dir>` / `-c key=val` global options.
            while let Some(opt) = rest.peek() {
                match *opt {
                    "-C" | "-c" => {
                        rest.next();
                        rest.next();
                    }
                    o if o.starts_with('-') => {
                        rest.next();
                    }
                    _ => break,
                }
            }
            // Only subcommands that are read-only in *every* invocation form.
            // `config`, `remote`, `tag`, and `branch` are deliberately excluded
            // because they each have a state-mutating variant.
            matches!(
                rest.next(),
                Some("status")
                    | Some("diff")
                    | Some("log")
                    | Some("show")
                    | Some("rev-parse")
                    | Some("ls-files")
                    | Some("ls-tree")
                    | Some("blame")
                    | Some("describe")
                    | Some("shortlog")
                    | Some("cat-file")
                    | Some("show-ref")
            )
        }

        // cargo / npm read-only subcommands only.
        "cargo" => matches!(words.next(), Some("tree") | Some("metadata")),
        "npm" => matches!(words.next(), Some("ls") | Some("list") | Some("view")),

        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::Content;
    use rmcp::object;

    fn args(command: &str) -> Option<JsonObject> {
        Some(object!({ "command": command }))
    }

    fn ok_result(text: &str) -> CallToolResult {
        CallToolResult::success(vec![Content::text(text)])
    }

    fn err_result(text: &str) -> CallToolResult {
        CallToolResult::error(vec![Content::text(text)])
    }

    fn reset_cache() {
        CACHE
            .lock()
            .expect("tool-result cache mutex poisoned")
            .clear();
    }

    struct EnvGuard;
    impl EnvGuard {
        fn on() -> Self {
            std::env::set_var(TOOL_CACHE_ENV, "1");
            EnvGuard
        }
    }
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            std::env::remove_var(TOOL_CACHE_ENV);
        }
    }

    #[test]
    fn cache_key_is_stable_under_key_order() {
        let a = Some(object!({ "command": "ls", "timeout_secs": 5 }));
        let b = Some(object!({ "timeout_secs": 5, "command": "ls" }));
        assert_eq!(cache_key("shell", &a), cache_key("shell", &b));
    }

    #[test]
    fn cache_key_is_stable_under_command_whitespace() {
        let a = args("git   status");
        let b = args("  git status  ");
        assert_eq!(cache_key("shell", &a), cache_key("shell", &b));
    }

    #[test]
    #[serial_test::serial]
    fn read_only_command_stores_and_hits() {
        let _guard = EnvGuard::on();
        reset_cache();

        let a = args("git status");
        assert!(lookup("shell", &a).is_none());

        store_if_read_only("shell", &a, &ok_result("clean"));

        let hit = lookup("shell", &a).expect("read-only result should be cached");
        assert_eq!(hit, ok_result("clean"));

        // Whitespace-normalized variant collides with the stored entry.
        let spaced = args("  git   status ");
        assert!(lookup("shell", &spaced).is_some());

        reset_cache();
    }

    #[test]
    #[serial_test::serial]
    fn write_command_is_not_stored() {
        let _guard = EnvGuard::on();
        reset_cache();

        let write = args("rm -rf build");
        store_if_read_only("shell", &write, &ok_result("done"));
        assert!(lookup("shell", &write).is_none());

        reset_cache();
    }

    #[test]
    #[serial_test::serial]
    fn error_result_is_not_stored() {
        let _guard = EnvGuard::on();
        reset_cache();

        let a = args("cat missing.txt");
        store_if_read_only("shell", &a, &err_result("No such file"));
        assert!(lookup("shell", &a).is_none());

        reset_cache();
    }

    #[test]
    #[serial_test::serial]
    fn invalidate_all_empties_the_cache() {
        let _guard = EnvGuard::on();
        reset_cache();

        let a = args("ls");
        store_if_read_only("shell", &a, &ok_result("a\nb"));
        assert!(lookup("shell", &a).is_some());

        invalidate_all();
        assert!(lookup("shell", &a).is_none());

        reset_cache();
    }

    #[test]
    #[serial_test::serial]
    fn gate_off_lookup_is_always_none() {
        // Ensure the gate is off (no EnvGuard).
        std::env::remove_var(TOOL_CACHE_ENV);
        reset_cache();

        let a = args("ls");
        // Even if we force an entry in directly, a gated-off lookup returns None.
        CACHE
            .lock()
            .unwrap()
            .put(cache_key("shell", &a), ok_result("x"));
        assert!(lookup("shell", &a).is_none());

        // store/invalidate are also no-ops while gated off.
        store_if_read_only("shell", &a, &ok_result("y"));
        reset_cache();
    }

    #[test]
    fn classifier_accepts_reads_and_rejects_writes() {
        // Read-only forms.
        assert!(is_read_only_command("cat foo.txt"));
        assert!(is_read_only_command("ls -la"));
        assert!(is_read_only_command("git status"));
        assert!(is_read_only_command("git -C sub diff"));
        assert!(is_read_only_command("git log | head"));
        assert!(is_read_only_command("grep -rn needle src | wc -l"));
        assert!(is_read_only_command("find . -name '*.rs'"));

        // Mutating or unprovable forms.
        assert!(!is_read_only_command("rm -rf build"));
        assert!(!is_read_only_command("git commit -m x"));
        assert!(!is_read_only_command("git config user.name bob"));
        assert!(!is_read_only_command("git tag v1"));
        assert!(!is_read_only_command("cat foo > bar"));
        assert!(!is_read_only_command("echo hi; rm x"));
        assert!(!is_read_only_command("cat $(which sh)"));
        assert!(!is_read_only_command("ls && rm x"));
        assert!(!is_read_only_command("npm install"));
        assert!(!is_read_only_command("sed -i s/a/b/ f"));
        assert!(!is_read_only_command("find . -delete"));
        assert!(!is_read_only_command("find . -exec rm {} +"));
        assert!(!is_read_only_command(""));
    }
}
