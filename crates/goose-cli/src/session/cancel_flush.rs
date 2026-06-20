//! Graceful-cancel partial-output flush for BharatCode CLI sessions.
//!
//! When a turn is interrupted mid-stream — the user hits Ctrl-C, or the agent's
//! [`CancellationToken`](tokio_util::sync::CancellationToken) is cancelled for
//! any other reason — the assistant has usually already streamed *some* text to
//! the terminal. The normal completion path renders that text and finishes
//! cleanly; an interrupt, by contrast, breaks out of the event loop with the
//! buffered tail still pending and no closing marker, so the session log shows a
//! truncated, dangling turn and the user is left unsure how much of the reply
//! actually survived.
//!
//! This module supplies the small piece that closes that gap. At the single
//! cancellation break in `process_agent_response`, [`on_interrupt`] is called
//! with the partial assistant text accumulated so far. It:
//!   * flushes any pending streamed text the renderer is still holding (so the
//!     last buffered chunk reaches the terminal before the marker), and
//!   * prints a localized one-line `interrupted — partial response saved`
//!     marker exactly once, so the turn ends on a clean, resumable boundary
//!     instead of a silent truncation.
//!
//! Design contract:
//!   * **Inert unless interrupted.** Nothing here runs on the normal completion
//!     path; the only call site is the cancellation arm of the stream loop. The
//!     happy path stays byte-identical. No env gate is needed — a real
//!     interrupt is the gate.
//!   * **No-op when empty.** If no partial text was accumulated (the interrupt
//!     landed before the model emitted any text), nothing is printed.
//!   * **Idempotent.** A bool guard ensures the marker is emitted at most once,
//!     so a double interrupt (or any re-entry) never double-prints.
//!
//! Original BharatCode work; not ported from any third party. The rendering and
//! flush plumbing it drives lives in the sibling `output` / `streaming_buffer`
//! modules.

use std::sync::atomic::{AtomicBool, Ordering};

use super::output;
use super::streaming_buffer::MarkdownBuffer;

/// i18n key for the one-line marker printed after a partial response is saved.
/// Falls back (via `tr!`) to the English string below if no locale entry maps.
const INTERRUPT_MARKER_KEY: &str = "session.interrupted_partial_saved";

/// English text for the interrupt marker. Kept here as the canonical source so
/// the unit tests can assert on it without loading the i18n tables, and so the
/// `tr!` fallback (key -> English) has a known target.
pub const INTERRUPT_MARKER_EN: &str = "interrupted — partial response saved";

/// Build the marker line that should be shown on interrupt, given the length of
/// the partial assistant text accumulated so far.
///
/// Returns `Some(marker)` only when `partial_len > 0` — i.e. the model had
/// actually streamed some text before the interrupt landed. When nothing was
/// accumulated there is no truncated turn to annotate, so this returns `None`
/// and the caller prints nothing.
///
/// Pure and string-level: it routes through `tr!` for localization but performs
/// no IO, so it is fully unit-testable without a live session.
///
/// The marker key is not in the shipped i18n tables, so `tr!` falls back to the
/// key itself; this maps that bare-key fallback onto the canonical English line
/// so the terminal never shows a raw `session.*` identifier. A future locale
/// entry for [`INTERRUPT_MARKER_KEY`] is picked up automatically.
pub fn render_interrupt_marker(partial_len: usize) -> Option<String> {
    if partial_len == 0 {
        return None;
    }
    let translated = crate::tr!(INTERRUPT_MARKER_KEY);
    if translated == INTERRUPT_MARKER_KEY {
        return Some(INTERRUPT_MARKER_EN.to_string());
    }
    Some(translated)
}

/// One-shot guard that runs a side-effecting interrupt action at most once.
///
/// The first call to [`InterruptFlusher::fire`] with a non-empty payload runs
/// `action`; every subsequent call is a no-op. This keeps the on-screen marker
/// from being printed twice when an interrupt path is reached more than once
/// (for example a second Ctrl-C while the first is still unwinding).
pub struct InterruptFlusher {
    fired: AtomicBool,
}

impl InterruptFlusher {
    pub fn new() -> Self {
        Self {
            fired: AtomicBool::new(false),
        }
    }

    /// Run `action` exactly once across the lifetime of this flusher, but only
    /// if `partial_len > 0`. Returns `true` if `action` was invoked on this
    /// call, `false` if it was skipped (already fired, or nothing to flush).
    ///
    /// A no-op (`partial_len == 0`) does **not** consume the one-shot budget:
    /// an empty interrupt followed by a non-empty one still fires once.
    pub fn fire<F: FnOnce()>(&self, partial_len: usize, action: F) -> bool {
        if partial_len == 0 {
            return false;
        }
        if self
            .fired
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return false;
        }
        action();
        true
    }
}

impl Default for InterruptFlusher {
    fn default() -> Self {
        Self::new()
    }
}

/// Process-wide one-shot guard for the interrupt marker.
///
/// `process_agent_response` is `&mut self` and runs one turn at a time, so a
/// single shared guard is sufficient and keeps the call site a one-liner. It is
/// armed by [`on_interrupt`] and never reset within a process: once a turn has
/// been interrupted, re-entry on the same break point will not double-print. A
/// fresh turn that completes normally never touches this guard at all.
static MARKER_GUARD: InterruptFlusher = InterruptFlusher {
    fired: AtomicBool::new(false),
};

/// Flush a renderer's pending streamed text, then emit the interrupt marker.
///
/// Drives the full interrupt-time behavior against the live renderer:
///   1. flush the markdown buffer so the last pending chunk reaches the terminal
///      before the marker line, and
///   2. print the localized marker exactly once (idempotent) — and only when
///      `partial_text` is non-empty.
///
/// This is the variant the call site in `process_agent_response` uses when it
/// still holds the streaming buffer. The buffer flush mirrors the normal
/// completion path, so the partial text renders identically; only the trailing
/// marker is added.
pub fn on_interrupt(partial_text: &str, buffer: &mut MarkdownBuffer) {
    output::flush_markdown_buffer_current_theme(buffer);
    emit_marker(partial_text);
}

/// Emit only the interrupt marker (no buffer flush), idempotently and only when
/// `partial_text` is non-empty. Useful where the renderer flush already ran.
pub fn emit_marker(partial_text: &str) {
    let len = partial_text.trim().len();
    MARKER_GUARD.fire(len, || {
        if let Some(marker) = render_interrupt_marker(len) {
            output::render_text(&marker, Some(console::Color::Yellow), true);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn marker_present_only_for_nonempty_partial() {
        assert!(render_interrupt_marker(0).is_none());

        let marker = render_interrupt_marker(42).expect("non-empty partial yields a marker");
        assert!(!marker.is_empty());
    }

    #[test]
    fn marker_text_resolves_to_interrupt_string() {
        // The key is not in the shipped tables, so the bare-key fallback is
        // remapped onto the canonical English line — the terminal must never
        // show a raw `session.*` identifier.
        let marker = render_interrupt_marker(1).expect("non-empty partial yields a marker");
        assert_ne!(
            marker, INTERRUPT_MARKER_KEY,
            "marker must not surface the raw i18n key"
        );
        assert_eq!(
            marker, INTERRUPT_MARKER_EN,
            "default locale resolves to the canonical interrupt line"
        );
    }

    #[test]
    fn flusher_fires_action_exactly_once() {
        let flusher = InterruptFlusher::new();
        let count = AtomicUsize::new(0);

        let first = flusher.fire(10, || {
            count.fetch_add(1, Ordering::SeqCst);
        });
        let second = flusher.fire(10, || {
            count.fetch_add(1, Ordering::SeqCst);
        });

        assert!(first, "first interrupt should fire the action");
        assert!(!second, "second interrupt must be a no-op (idempotent)");
        assert_eq!(count.load(Ordering::SeqCst), 1, "action ran exactly once");
    }

    #[test]
    fn flusher_is_noop_for_empty_partial() {
        let flusher = InterruptFlusher::new();
        let count = AtomicUsize::new(0);

        let fired = flusher.fire(0, || {
            count.fetch_add(1, Ordering::SeqCst);
        });

        assert!(!fired, "empty partial must not fire the action");
        assert_eq!(
            count.load(Ordering::SeqCst),
            0,
            "no action ran for empty partial"
        );
    }

    #[test]
    fn empty_interrupt_does_not_consume_one_shot_budget() {
        let flusher = InterruptFlusher::new();
        let count = AtomicUsize::new(0);

        // An interrupt with no accumulated text is a no-op and leaves the
        // one-shot budget intact for a later, real partial flush.
        assert!(!flusher.fire(0, || {
            count.fetch_add(1, Ordering::SeqCst);
        }));
        assert!(flusher.fire(7, || {
            count.fetch_add(1, Ordering::SeqCst);
        }));

        assert_eq!(
            count.load(Ordering::SeqCst),
            1,
            "the real flush still fired once"
        );
    }
}
