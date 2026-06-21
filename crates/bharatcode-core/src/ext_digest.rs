//! Active-extension digest for the system prompt — BharatCode v79.
//!
//! When enabled (opt-in via `BHARATCODE_EXT_DIGEST`), this renders a compact
//! `# Available extensions` block listing the MCP servers / extensions that are
//! *currently active* for the turn — each as a name plus a one-line role — so
//! the model picks the right tool group instead of guessing from tool names
//! alone.
//!
//! The descriptors are derived from the extensions already visible to the
//! prompt builder (the same set whose tools are exposed to the model); no new
//! capability facts are introduced here. The renderer is pure over a slice of
//! [`ExtDescriptor`]: it depends only on its inputs, never touches the process
//! environment, and is fully deterministic — which is what makes it cheap to
//! unit test.
//!
//! The feature is a strict no-op when the toggle is unset or when there are no
//! active extensions: [`ext_digest_block`] returns `None`, the gated call site
//! inserts nothing, and the built system prompt is byte-identical to default
//! behaviour.
//!
//! Original BharatCode work; not ported from any third party.

/// Opt-in toggle name, read raw from the process environment. Defaults to off.
const ENABLE_KEY: &str = "BHARATCODE_EXT_DIGEST";

/// Hard cap on the number of extensions listed, to keep the block compact.
const MAX_ENTRIES: usize = 24;

/// Hard cap on the length (in chars) of a single rendered summary line.
const MAX_SUMMARY_CHARS: usize = 140;

/// Hard cap on the rendered block size, in bytes. The unit test asserts the
/// block stays under a sane length, and the renderer enforces this by dropping
/// trailing entries (with an ellipsis marker) until it fits.
const MAX_BLOCK_BYTES: usize = 4096;

/// A single active extension / MCP server, reduced to the facts the digest
/// needs: a display `name`, a coarse `kind` (e.g. `mcp`, `builtin`), and a
/// one-line `summary` of its role. Pure data — the renderer is a function of a
/// slice of these.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtDescriptor {
    /// Display name of the extension / MCP server.
    pub name: String,
    /// Coarse classification (e.g. `mcp`, `builtin`, `frontend`). Rendered as a
    /// parenthetical tag when non-empty.
    pub kind: String,
    /// One-line description of the extension's role.
    pub summary: String,
}

impl ExtDescriptor {
    /// Construct a descriptor, trimming each field. Convenience for call sites
    /// mapping their existing extension records into the digest's input shape.
    pub fn new(
        name: impl Into<String>,
        kind: impl Into<String>,
        summary: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into().trim().to_string(),
            kind: kind.into().trim().to_string(),
            summary: summary.into().trim().to_string(),
        }
    }
}

/// Interpret a raw flag value as truthy. Mirrors the sibling BharatCode
/// switches (`repo_digest`, `plan_mode`, ...) so the gates behave consistently.
fn is_truthy(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Whether the active-extension digest is enabled. Opt-in via the
/// `BHARATCODE_EXT_DIGEST` environment variable; any truthy-ish value (`1`,
/// `true`, `yes`, `on`) enables it. Reads the raw process environment so the
/// gate is unambiguous and defaults to `false` when unset.
pub fn is_enabled() -> bool {
    std::env::var(ENABLE_KEY)
        .map(|raw| is_truthy(&raw))
        .unwrap_or(false)
}

/// Collapse internal whitespace and clip a summary to one tidy line of at most
/// [`MAX_SUMMARY_CHARS`] characters (char-boundary safe).
fn one_line(summary: &str) -> String {
    let collapsed = summary.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= MAX_SUMMARY_CHARS {
        return collapsed;
    }
    let mut clipped: String = collapsed.chars().take(MAX_SUMMARY_CHARS).collect();
    clipped.push('…');
    clipped
}

/// Render one bullet line for a descriptor. Lines have the shape
/// `- {name} ({kind}): {summary}`, with the parenthetical and the summary each
/// omitted when their source field is empty.
fn format_entry(d: &ExtDescriptor) -> String {
    let mut line = String::from("- ");
    line.push_str(&d.name);
    if !d.kind.is_empty() {
        line.push_str(" (");
        line.push_str(&d.kind);
        line.push(')');
    }
    let summary = one_line(&d.summary);
    if !summary.is_empty() {
        line.push_str(": ");
        line.push_str(&summary);
    }
    line
}

/// Assemble the full block over a fixed list of already-bounded entries.
fn format_block(entries: &[&ExtDescriptor], truncated: bool) -> String {
    let mut out = String::new();
    out.push_str("# Available extensions\n");
    out.push_str(
        "\nThese MCP servers / extensions are active this turn. Prefer the one whose role \
         matches the task instead of guessing from tool names.\n\n",
    );
    for entry in entries {
        out.push_str(&format_entry(entry));
        out.push('\n');
    }
    if truncated {
        out.push_str("- …\n");
    }
    out
}

/// The active-extension digest block to inject into the system prompt, or
/// `None` when there are no active extensions.
///
/// Pure over `exts`: it does **not** consult the environment (the caller gates
/// on [`is_enabled`]). Entries with a blank name are skipped. The output is
/// capped to [`MAX_ENTRIES`] entries and kept under [`MAX_BLOCK_BYTES`]; if it
/// would still overflow, trailing entries are dropped and a `…` marker is
/// appended. Returns `None` for an empty (or all-blank) slice so the gated
/// call site inserts nothing and the prompt stays byte-identical.
pub fn ext_digest_block(exts: &[ExtDescriptor]) -> Option<String> {
    let named: Vec<&ExtDescriptor> = exts.iter().filter(|d| !d.name.is_empty()).collect();
    if named.is_empty() {
        return None;
    }

    let total = named.len();
    let mut shown: Vec<&ExtDescriptor> = named.into_iter().take(MAX_ENTRIES).collect();
    let mut truncated = total > shown.len();

    loop {
        let block = format_block(&shown, truncated);
        if block.len() <= MAX_BLOCK_BYTES || shown.len() <= 1 {
            return Some(block);
        }
        shown.pop();
        truncated = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialise tests that mutate the shared process env so the
    /// `BHARATCODE_EXT_DIGEST` toggle does not race across threads.
    fn env_guard(value: Option<&str>) -> env_lock::EnvGuard<'_> {
        env_lock::lock_env([(ENABLE_KEY, value)])
    }

    #[test]
    fn is_enabled_gate_table() {
        let cases = [
            (None, false),
            (Some("1"), true),
            (Some("true"), true),
            (Some("YES"), true),
            (Some("on"), true),
            (Some("0"), false),
            (Some("off"), false),
            (Some(""), false),
            (Some("nonsense"), false),
        ];
        for (value, expected) in cases {
            let _guard = env_guard(value);
            assert_eq!(is_enabled(), expected, "value={value:?}");
        }
    }

    #[test]
    fn empty_slice_yields_none() {
        assert!(ext_digest_block(&[]).is_none());
    }

    #[test]
    fn all_blank_names_yield_none() {
        let exts = vec![
            ExtDescriptor::new("   ", "mcp", "ignored"),
            ExtDescriptor::new("", "builtin", "also ignored"),
        ];
        assert!(ext_digest_block(&exts).is_none());
    }

    #[test]
    fn renders_both_names_and_summaries() {
        let exts = vec![
            ExtDescriptor::new(
                "filesystem",
                "mcp",
                "Read and write files in the working directory",
            ),
            ExtDescriptor::new(
                "github",
                "mcp",
                "Open issues and pull requests against the remote repo",
            ),
        ];
        let block = ext_digest_block(&exts).expect("two descriptors should render a block");

        assert!(block.contains("# Available extensions"), "got: {block}");
        // Both names present.
        assert!(block.contains("filesystem"), "got: {block}");
        assert!(block.contains("github"), "got: {block}");
        // Both summaries present.
        assert!(
            block.contains("Read and write files in the working directory"),
            "got: {block}"
        );
        assert!(
            block.contains("Open issues and pull requests against the remote repo"),
            "got: {block}"
        );
        // Kind tag rendered.
        assert!(block.contains("(mcp)"), "got: {block}");

        // Zero user-facing donor/internal-brand leakage.
        assert!(
            !block.to_ascii_lowercase().contains("goose"),
            "leak: {block}"
        );

        // Stays under the sane length cap.
        assert!(
            block.len() <= MAX_BLOCK_BYTES,
            "block too long: {}",
            block.len()
        );
    }

    #[test]
    fn missing_kind_or_summary_is_omitted_cleanly() {
        let exts = vec![ExtDescriptor::new("solo", "", "")];
        let block = ext_digest_block(&exts).expect("one named descriptor renders");
        assert!(block.contains("- solo\n"), "got: {block}");
        // No empty parenthesis or trailing colon for the blank fields.
        assert!(!block.contains("- solo ()"), "got: {block}");
        assert!(!block.contains("- solo:"), "got: {block}");
    }

    #[test]
    fn long_summary_is_clipped_to_one_line() {
        let long = "word ".repeat(80);
        let exts = vec![ExtDescriptor::new("verbose", "mcp", long)];
        let block = ext_digest_block(&exts).expect("renders");
        // Multi-line input collapses to a single bullet line.
        let bullet_lines = block.lines().filter(|l| l.starts_with("- ")).count();
        assert_eq!(bullet_lines, 1, "got: {block}");
        assert!(block.contains('…'), "expected ellipsis clip: {block}");
    }

    #[test]
    fn caps_entry_count_and_marks_truncation() {
        let exts: Vec<ExtDescriptor> = (0..(MAX_ENTRIES + 5))
            .map(|i| ExtDescriptor::new(format!("ext{i}"), "mcp", "role"))
            .collect();
        let block = ext_digest_block(&exts).expect("renders");
        let bullet_lines = block.lines().filter(|l| l.starts_with("- ")).count();
        // MAX_ENTRIES real entries plus the trailing ellipsis marker line.
        assert_eq!(bullet_lines, MAX_ENTRIES + 1, "got: {block}");
        assert!(block.contains("- …"), "expected truncation marker: {block}");
        assert!(
            block.len() <= MAX_BLOCK_BYTES,
            "block too long: {}",
            block.len()
        );
    }
}
