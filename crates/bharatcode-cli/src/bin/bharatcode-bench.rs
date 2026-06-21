//! `bharatcode-bench` — the thin binary that drives the offline benchmark /
//! eval harness.
//!
//! This is a deliberately tiny entry point: it parses three flags and calls
//! [`bharatcode_cli::commands::bench::handle_bench`], which owns all the logic. The
//! harness is fully offline and read-only by default — it scores an embedded
//! suite of shipped pure helpers (exec-policy command splitting and ₹ cost
//! formatting) and prints a pass/score report, so regressions in those helpers
//! are catchable without ever touching a model or the network.
//!
//! Usage:
//!   bharatcode-bench            # run the offline suite, styled report
//!   bharatcode-bench --json     # same, machine-readable JSON
//!   bharatcode-bench --list     # print case ids and exit, run nothing
//!   bharatcode-bench --live     # future model-backed scoring (stub: skips all)

use clap::Parser;

use bharatcode_cli::commands::bench::{handle_bench, BenchOptions};

/// Command-line surface for the offline benchmark / eval harness.
#[derive(Debug, Parser)]
#[command(
    name = "bharatcode-bench",
    about = "Run the offline benchmark / eval suite for core helpers",
    long_about = None,
)]
struct Cli {
    /// List the embedded case ids and exit without running anything.
    #[arg(long)]
    list: bool,

    /// Run future model-backed scoring (currently a stub that skips every case
    /// and contacts no provider).
    #[arg(long)]
    live: bool,

    /// Emit the report as machine-readable JSON instead of a styled table.
    #[arg(long)]
    json: bool,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    handle_bench(BenchOptions {
        list: cli.list,
        live: cli.live,
        json: cli.json,
    })
}
