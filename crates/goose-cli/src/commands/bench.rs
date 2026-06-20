//! `bharatcode bench` — a local, offline micro-benchmark harness.
//!
//! This command times a small, fixed set of **purely local** in-crate routines
//! and prints a clean timing table plus a one-line summary. It never touches the
//! network, never spins up a provider, and never makes a model call — every
//! benchmark is a tight loop over an existing pure function with a fixed,
//! embedded input.
//!
//! The suite covers three representative hot paths:
//!   * `token-approx`  — a cheap token-count approximation over a fixed corpus,
//!                        the kind of estimate used before a real tokenizer is
//!                        available (throughput-bound string scan).
//!   * `context-trim`  — `goose::utils::safe_truncate`, the UTF-8-safe trimming
//!                        used all over the CLI to fit text into a budget.
//!   * `argv-split`     — `goose::utils::split_command_args`, the quote-aware
//!                        command splitter used by the shell/extension wiring.
//!
//! The design splits *data and timing math* from *I/O*: the benchmark
//! definitions, the per-benchmark statistics ([`BenchStat`]), and the table
//! renderer are all pure and deterministic, so the whole formatting surface is
//! unit-testable without running a single benchmark. Only [`handle_bench`]
//! prints.
//!
//! Iteration counts default to a small, fast value and can be scaled up via the
//! `BHARATCODE_BENCH_ITERATIONS` environment variable (clamped to a sane upper
//! bound) — the default run is unchanged whether or not that variable is set.
//!
//! User-facing labels route through the i18n layer via [`label`], which falls
//! back to the English default when the active locale has no entry for the key
//! (mirroring the helper in `mcp_registry.rs`).
//!
//! Original BharatCode work; not ported from any third party.

use std::hint::black_box;
use std::time::Instant;

use anyhow::Result;

use goose::utils::{safe_truncate, split_command_args};

/// Default number of iterations per benchmark when
/// `BHARATCODE_BENCH_ITERATIONS` is unset or unparseable. Small enough that the
/// full suite finishes near-instantly on a cold machine.
const DEFAULT_ITERATIONS: u64 = 2_000;

/// Upper bound for the per-benchmark iteration count. A local micro-benchmark
/// should never be allowed to spin for an unbounded amount of time regardless of
/// what the environment requests.
const MAX_ITERATIONS: u64 = 5_000_000;

/// A fixed corpus reused by the throughput-style benchmarks. Embedded so every
/// run measures the same work and the numbers are comparable across machines.
const CORPUS: &str = "The quick brown fox jumps over the lazy dog. \
Pack my box with five dozen liquor jugs. \
Sphinx of black quartz, judge my vow. \
How vexingly quick daft zebras jump!";

/// A fixed command line for the quote-aware argv-split benchmark.
const ARGV_INPUT: &str = "run --model gpt-4o --text \"hello there\" --flag 'single quoted' tail";

/// A cheap, allocation-free token-count approximation: counts whitespace-
/// delimited words and scales by a fixed factor. This is intentionally the same
/// shape as the pre-tokenizer estimates used before a real BPE tokenizer is
/// loaded; it is pure and offline, which is exactly what this harness measures.
fn approx_token_count(text: &str) -> usize {
    let words = text.split_whitespace().count();
    // ~4/3 tokens per word is a common rough English heuristic.
    words + words / 3
}

/// Options accepted by [`handle_bench`].
#[derive(Debug, Clone, Default)]
pub struct BenchOptions {
    /// When `true`, emit the machine-readable JSON report; otherwise render the
    /// human-readable table.
    pub json: bool,
}

/// One benchmark's timing result. Pure data — produced by [`run_benchmark`] and
/// consumed by the renderers, with no I/O of its own.
#[derive(Debug, Clone, PartialEq)]
pub struct BenchStat {
    /// Stable id of the benchmark (also the table row label).
    pub id: String,
    /// Number of iterations timed.
    pub iterations: u64,
    /// Total wall-time across all iterations, in nanoseconds.
    pub total_ns: u128,
    /// An opaque accumulator kept so the optimiser cannot elide the work; not
    /// rendered, but carried for completeness and test assertions.
    pub checksum: u64,
}

impl BenchStat {
    /// Average nanoseconds per iteration. Returns `0` for a zero-iteration stat
    /// rather than dividing by zero.
    pub fn ns_per_op(&self) -> u64 {
        if self.iterations == 0 {
            return 0;
        }
        (self.total_ns / self.iterations as u128) as u64
    }

    /// Throughput in operations per second, computed from the average op time.
    /// Returns `0.0` when no time elapsed (e.g. a degenerate empty run).
    pub fn ops_per_sec(&self) -> f64 {
        let ns = self.ns_per_op();
        if ns == 0 {
            return 0.0;
        }
        1_000_000_000.0 / ns as f64
    }
}

/// Resolve the per-benchmark iteration count from `BHARATCODE_BENCH_ITERATIONS`,
/// falling back to the default and clamping to a sane upper bound. A value of
/// `0` or an unparseable value falls back to the default.
fn resolve_iterations() -> u64 {
    std::env::var("BHARATCODE_BENCH_ITERATIONS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|&v| v > 0)
        .unwrap_or(DEFAULT_ITERATIONS)
        .min(MAX_ITERATIONS)
}

/// Run one named benchmark for `iterations` rounds, timing the whole loop and
/// folding each round's output into a checksum so the optimiser cannot delete
/// the work. `body` returns a `u64` contribution per round.
fn run_benchmark<F>(id: &str, iterations: u64, mut body: F) -> BenchStat
where
    F: FnMut() -> u64,
{
    let start = Instant::now();
    let mut checksum: u64 = 0;
    for _ in 0..iterations {
        checksum = checksum.wrapping_add(black_box(body()));
    }
    let total_ns = start.elapsed().as_nanos();
    BenchStat {
        id: id.to_string(),
        iterations,
        total_ns,
        checksum,
    }
}

/// Execute the full embedded suite at the given iteration count and return one
/// [`BenchStat`] per benchmark, in a stable order.
pub fn run_suite(iterations: u64) -> Vec<BenchStat> {
    vec![
        run_benchmark("token-approx", iterations, || {
            approx_token_count(black_box(CORPUS)) as u64
        }),
        run_benchmark("context-trim", iterations, || {
            let trimmed = safe_truncate(black_box(CORPUS), 40);
            trimmed.len() as u64
        }),
        run_benchmark("argv-split", iterations, || {
            split_command_args(black_box(ARGV_INPUT))
                .map(|v| v.len() as u64)
                .unwrap_or(0)
        }),
    ]
}

/// Look up a user-facing label through the i18n layer, falling back to the
/// English `default` when the active locale has no entry for `key`.
fn label(key: &str, default: &str) -> String {
    let translated = crate::tr!(key);
    if translated == key {
        default.to_string()
    } else {
        translated
    }
}

/// Format a large ops/sec figure with thousands separators for readability,
/// e.g. `1234567.0` -> `1,234,567`.
fn format_ops(ops: f64) -> String {
    let rounded = ops.round() as u64;
    let digits = rounded.to_string();
    let bytes = digits.as_bytes();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    let len = bytes.len();
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (len - i) % 3 == 0 {
            out.push(',');
        }
        out.push(*b as char);
    }
    out
}

/// Render the suite results as a clean, aligned timing table plus a summary
/// line. Pure: takes the stats, returns the rendered string, prints nothing.
pub fn render_table(stats: &[BenchStat]) -> String {
    let header_bench = label("bench.col.bench", "benchmark");
    let header_iters = label("bench.col.iters", "iters");
    let header_nsop = label("bench.col.nsop", "ns/op");
    let header_ops = label("bench.col.opssec", "ops/sec");

    let name_width = stats
        .iter()
        .map(|s| s.id.len())
        .chain(std::iter::once(header_bench.len()))
        .max()
        .unwrap_or(header_bench.len());

    let mut out = String::new();
    out.push('\n');
    out.push_str(&format!(
        "  {}\n\n",
        crate::theme::heading(label("bench.title", "BharatCode local micro-benchmarks"))
    ));

    out.push_str(&format!(
        "  {:<name_width$}  {:>10}  {:>10}  {:>14}\n",
        header_bench,
        header_iters,
        header_nsop,
        header_ops,
        name_width = name_width,
    ));
    out.push_str(&format!(
        "  {}\n",
        "-".repeat(name_width + 2 + 10 + 2 + 10 + 2 + 14)
    ));

    for s in stats {
        out.push_str(&format!(
            "  {:<name_width$}  {:>10}  {:>10}  {:>14}\n",
            s.id,
            s.iterations,
            s.ns_per_op(),
            format_ops(s.ops_per_sec()),
            name_width = name_width,
        ));
    }

    out.push('\n');
    let total_ns: u128 = stats.iter().map(|s| s.total_ns).sum();
    let total_ms = total_ns as f64 / 1_000_000.0;
    out.push_str(&format!(
        "  {}: {} {} {} {:.3}ms\n",
        label("bench.summary", "summary"),
        stats.len(),
        label("bench.summary.benchmarks", "benchmarks"),
        label("bench.summary.in", "in"),
        total_ms,
    ));
    out
}

/// Render the suite results as a stable, machine-readable JSON object.
pub fn render_json(stats: &[BenchStat]) -> String {
    let rows: Vec<serde_json::Value> = stats
        .iter()
        .map(|s| {
            serde_json::json!({
                "id": s.id,
                "iterations": s.iterations,
                "total_ns": s.total_ns.to_string(),
                "ns_per_op": s.ns_per_op(),
                "ops_per_sec": s.ops_per_sec(),
            })
        })
        .collect();
    let total_ns: u128 = stats.iter().map(|s| s.total_ns).sum();
    let value = serde_json::json!({
        "benchmarks": rows,
        "count": stats.len(),
        "total_ns": total_ns.to_string(),
    });
    serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string())
}

/// Entry point for the `bharatcode bench` surface.
///
/// Runs the embedded local micro-benchmark suite and prints the timing table
/// (or JSON per `opts.json`). Fully offline: no network, no provider, no model
/// call.
pub fn handle_bench(opts: BenchOptions) -> Result<()> {
    let iterations = resolve_iterations();
    let stats = run_suite(iterations);

    if opts.json {
        println!("{}", render_json(&stats));
    } else {
        print!("{}", render_table(&stats));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stat(id: &str, iterations: u64, total_ns: u128) -> BenchStat {
        BenchStat {
            id: id.to_string(),
            iterations,
            total_ns,
            checksum: 0,
        }
    }

    // ---- Per-benchmark statistics math. ----

    #[test]
    fn ns_per_op_divides_total_by_iterations() {
        let s = stat("x", 100, 5_000);
        assert_eq!(s.ns_per_op(), 50);
    }

    #[test]
    fn ns_per_op_zero_iterations_is_zero() {
        let s = stat("x", 0, 5_000);
        assert_eq!(s.ns_per_op(), 0);
    }

    #[test]
    fn ops_per_sec_inverts_ns_per_op() {
        // 50 ns/op -> 20,000,000 ops/sec.
        let s = stat("x", 100, 5_000);
        assert!((s.ops_per_sec() - 20_000_000.0).abs() < 1.0);
    }

    #[test]
    fn ops_per_sec_zero_time_is_zero() {
        let s = stat("x", 100, 0);
        assert_eq!(s.ops_per_sec(), 0.0);
    }

    // ---- format_ops thousands grouping. ----

    #[test]
    fn format_ops_groups_thousands() {
        assert_eq!(format_ops(0.0), "0");
        assert_eq!(format_ops(7.0), "7");
        assert_eq!(format_ops(1234.0), "1,234");
        assert_eq!(format_ops(1234567.0), "1,234,567");
        // Rounds before grouping.
        assert_eq!(format_ops(999.6), "1,000");
    }

    // ---- iteration resolution constants/clamp. ----

    #[test]
    fn iteration_bounds_are_sane() {
        assert!(DEFAULT_ITERATIONS > 0);
        assert!(MAX_ITERATIONS >= DEFAULT_ITERATIONS);
    }

    // ---- The embedded benchmark bodies actually do real work. ----

    #[test]
    fn approx_token_count_is_positive_for_corpus() {
        assert!(approx_token_count(CORPUS) > 0);
        assert_eq!(approx_token_count(""), 0);
    }

    #[test]
    fn run_suite_returns_one_stat_per_benchmark() {
        // A tiny iteration count keeps the test fast while exercising the real
        // benchmark bodies (no network, no provider).
        let stats = run_suite(8);
        assert_eq!(stats.len(), 3);
        let ids: Vec<&str> = stats.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["token-approx", "context-trim", "argv-split"]);
        for s in &stats {
            assert_eq!(s.iterations, 8);
        }
    }

    #[test]
    fn run_benchmark_counts_iterations_and_folds_checksum() {
        let s = run_benchmark("ones", 5, || 1);
        assert_eq!(s.iterations, 5);
        // Each round contributed 1, folded with wrapping_add.
        assert_eq!(s.checksum, 5);
    }

    // ---- Table renderer is well-formed and reflects the data. ----

    #[test]
    fn render_table_contains_ids_and_summary() {
        let stats = vec![
            stat("token-approx", 100, 5_000),
            stat("argv-split", 100, 9_000),
        ];
        let table = render_table(&stats);
        assert!(table.contains("token-approx"));
        assert!(table.contains("argv-split"));
        // Header labels (English fallback) are present.
        assert!(table.contains("ns/op"));
        assert!(table.contains("ops/sec"));
        // Summary counts the benchmarks.
        assert!(table.contains("summary"));
        assert!(table.contains("2 benchmarks"));
    }

    #[test]
    fn render_table_aligns_to_longest_id() {
        // A very long id must not break alignment (no panic, id present).
        let stats = vec![stat("a-very-long-benchmark-identifier", 1, 1)];
        let table = render_table(&stats);
        assert!(table.contains("a-very-long-benchmark-identifier"));
    }

    #[test]
    fn render_table_empty_suite_is_just_header_and_summary() {
        let table = render_table(&[]);
        assert!(table.contains("ns/op"));
        assert!(table.contains("0 benchmarks"));
    }

    // ---- JSON renderer is valid and complete. ----

    #[test]
    fn render_json_is_valid_and_complete() {
        let stats = vec![
            stat("token-approx", 100, 5_000),
            stat("argv-split", 100, 9_000),
        ];
        let json = render_json(&stats);
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(parsed["count"], 2);
        let arr = parsed["benchmarks"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["id"], "token-approx");
        assert_eq!(arr[0]["iterations"], 100);
        // 5_000ns / 100 = 50 ns/op.
        assert_eq!(arr[0]["ns_per_op"], 50);
        // total_ns is serialized as a string to stay lossless for u128.
        assert_eq!(parsed["total_ns"], "14000");
    }

    // ---- No user-facing upstream brand leakage in static benchmark data. ----

    #[test]
    fn embedded_data_is_brand_free() {
        let banned = ["goose", "block "];
        let fields = [CORPUS, ARGV_INPUT];
        for field in fields {
            let lower = field.to_lowercase();
            for b in banned {
                assert!(!lower.contains(b), "embedded data leaks brand '{}'", b);
            }
        }
        for s in run_suite(1) {
            let lower = s.id.to_lowercase();
            for b in banned {
                assert!(!lower.contains(b), "benchmark id leaks brand '{}'", b);
            }
        }
    }
}
