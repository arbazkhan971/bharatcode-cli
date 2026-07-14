<div align="center">

# BharatCode

### Your code stays in India 🇮🇳

**A local-first, Apache-2.0, Rust terminal AI coding agent — built for India.**

<p align="center">
  <a href="https://opensource.org/licenses/Apache-2.0"><img src="https://img.shields.io/badge/License-Apache_2.0-blue.svg" alt="License: Apache-2.0"></a>
  <img src="https://img.shields.io/badge/local--first-default-success" alt="Local-first by default">
  <img src="https://img.shields.io/badge/telemetry-off-success" alt="Telemetry off by default">
  <img src="https://img.shields.io/badge/built%20with-Rust-orange" alt="Built with Rust">
</p>

</div>

BharatCode is a terminal-native AI coding agent that runs on your machine and, by
default, talks to a **local model** instead of a cloud API. It is engineered for
developers who want data residency, predictable cost in rupees, and a tool that
defaults to keeping work on-device — without giving up a modern agentic coding
experience.

---

## Why BharatCode

- **Local-first by default.** With nothing configured, BharatCode targets a local
  [Ollama](https://ollama.com) provider and a local model — no forced cloud sign-in.
  Cloud and hosted providers remain available, but they are **opt-in**.
- **Data residency.** An opt-in egress guard can block requests to endpoints outside
  an India allowlist, and an offline switch can lock the agent to local-only operation.
- **Cost in rupees.** A built-in INR cost ledger and an optional budget cap let you
  track and bound spend in ₹, not dollars.
- **India model presets.** First-class presets and declarative providers for
  India-hosted, open-weight friendly options (Sarvam, Krutrim) alongside local
  Qwen-Coder / DeepSeek via Ollama.
- **Telemetry off.** No product analytics phone-home; telemetry is disabled in code,
  not merely defaulted off.

---

## Install (build from source)

BharatCode is currently built from source. You need a recent Rust toolchain
(`rustup` + cargo; the repo pins a toolchain via `rust-toolchain.toml`).

```bash
# from the repo root
cargo build -p bharatcode-cli --no-default-features --features portable-default
```

This produces the **`bharatcode`** binary:

```bash
./target/debug/bharatcode --help
./target/debug/bharatcode --version
```

Notes:

- The `portable-default` feature set is the light build: it drops heavy native
  in-process inference engines (llama.cpp / candle / mlx), code-mode, and the
  system keyring so the CLI builds quickly and runs anywhere. A plain
  `cargo build` uses the default features and pulls those engines in.
- Because BharatCode is local-first, the recommended setup is to install
  [Ollama](https://ollama.com) and pull a coding model, then run `bharatcode`
  with no provider configured — it will target your local Ollama by default.

---

## Quick start

```bash
# 1) (recommended) run a local model
ollama pull qwen2.5-coder       # or any coding model you prefer

# 2) start BharatCode in your project
bharatcode

# first run walks you through setup; "Local model — Ollama (Recommended)"
# is offered first, so you can stay fully on-device.
```

Handy commands:

```bash
bharatcode                  # interactive session in the current directory
bharatcode tui              # terminal UI (launches the ui/text artifact)
bharatcode presets          # list the India / open-weight model presets
bharatcode cost             # INR spend ledger (per session / day / month)
bharatcode git              # read-only summary of the current git repo
bharatcode recipes-library  # curated recipe templates for Indian dev workflows
bharatcode doctor           # environment + BharatCode settings summary
bharatcode configure        # providers, models, and onboarding
```

---

## Features

Everything below is actually built and wired in this repository. Features that are
**opt-in / default OFF** are marked with the environment variable that turns them on.

### Always on

- **Local-first defaults** — with no provider configured, BharatCode targets a local
  Ollama provider and a local default model; env/config/cloud still take precedence
  when you set them.
- **`apply_patch` file-editing tool** — a streaming patch parser/applier in the
  `bharatcode-apply-patch` crate, wired into the developer toolset as the `apply_patch`
  tool.
- **INR cost ledger** — per-session / day / month spend tracked and shown in ₹ via
  `bharatcode cost`, with a compact ₹ figure in the session footer. USD→INR rate is
  configurable with `BHARATCODE_USD_INR`; day/month buckets use IST.
- **India model presets** — declarative `sarvam` and `krutrim` providers plus a
  presets module / `bharatcode presets` listing and a "Recommended (India /
  open-weight)" first-run choice (local Qwen-Coder / DeepSeek via Ollama; hosted
  Sarvam / Krutrim / Qwen / DeepSeek).
- **India recipe library** — `bharatcode recipes-library` ships curated templates
  (e.g. UPI review, Aadhaar/PII + DPDP audit, GST helper, Indic localization).
- **`bharatcode git`** — a concise, read-only repo summary (branch, status,
  ahead/behind, recent commits).
- **`bharatcode doctor`** — environment checks plus a read-only summary of the active
  BharatCode settings (residency mode, budget, USD↔INR rate, offline, etc.).
- **Tiranga theme** — a saffron/white/green CLI palette, selectable with
  `BHARATCODE_THEME` (`tiranga` / `bharat` / `india`); honors `NO_COLOR`, and when
  unset the output is byte-identical to the default theme.
- **Telemetry off** — product analytics are disabled in code (no PostHog key, the
  enablement check returns false); nothing phones home.
- **i18n / Hindi scaffold** — a std-only locale resolver (`BHARATCODE_LANG` → config →
  `LANG` → `en`) with embedded `en.json` / `hi.json` and a `tr!()` helper. A starter
  set of high-traffic strings is translated to Hindi today; English output is
  unchanged. This is a scaffold, not a complete Hindi UI.

### Opt-in (default OFF)

- **`BHARATCODE_SANDBOX`** — in-process Linux exec sandbox (landlock + seccomp) applied
  to shell commands. Modes: `read-only` / `workspace-write` (default off). Linux-only;
  a no-op stub elsewhere.
- **`BHARATCODE_EXEC_POLICY`** — path to a JSON allow/deny policy that gates shell
  commands (a hardened, clean-room exec policy).
- **`BHARATCODE_RESIDENCY`** (+ `BHARATCODE_RESIDENCY_ALLOWLIST`) — data-residency
  egress guard that can block requests to non-allowlisted (e.g. non-India) endpoints.
  Wired centrally so it screens every provider.
- **`BHARATCODE_OFFLINE`** — a single switch that composes local-only egress +
  residency + telemetry-off for an offline/no-egress posture.
- **`BHARATCODE_BUDGET_INR`** (+ `BHARATCODE_BUDGET_MODE` = `warn` default / `deny`) —
  an INR spend cap that warns near the cap and, in `deny` mode, refuses to start the
  next model turn once exceeded.
- **`BHARATCODE_COST_ROUTING`** — cost-aware routing that prefers cheaper / local
  models for the lead model when enabled.
- **`BHARATCODE_VERIFY`** — verify-before-done: runs your project's test/build and
  reports `Verified` / `Failed` / `Skipped` before finalizing.
- **`BHARATCODE_CACHE`** — on-disk prompt/response cache (SHA-256 keyed) wired into the
  streaming path; a hit short-circuits to a zero-cost stream.
- **`BHARATCODE_RETRY_*`** — retry/backoff tuning (`BHARATCODE_RETRY_MAX`,
  `BHARATCODE_RETRY_BASE_MS`, `BHARATCODE_RETRY_MAX_MS`) applied centrally so all
  providers honor it.
- **`BHARATCODE_CODEBASE_CONTEXT`** — a bounded, gitignore-respecting codebase
  scanner (RAG-lite) that builds a compact repo layout/manifest blob.
- **`BHARATCODE_KEYS`** / **`BHARATCODE_CLI_NEWLINE_KEY`** — customizable interactive
  keybindings (built-in defaults reproduced when unset).
- **`BHARATCODE_APPROVAL`** — approval-mode selection (chat / ask / auto / full) with a
  safe default when unset.

> Heads-up on honesty: the i18n layer is a **scaffold** (starter Hindi strings only).
> The sandbox is **Linux-only** and **off by default**. Cost routing, prompt cache,
> retry, residency, offline, budget, exec-policy, verify, and codebase context are all
> **off unless you set their environment variable**. None of these change default
> behavior until you opt in.

---

## Configuration

BharatCode reads its configuration from `~/.config/bharatcode/` and from
`BHARATCODE_*` environment variables (the env namespace was rebranded throughout).
Common ones:

| Variable | Effect |
|---|---|
| `BHARATCODE_PROVIDER` / `BHARATCODE_MODEL` | Override the active provider / model |
| `BHARATCODE_LANG` | UI language (`en`, `hi`) |
| `BHARATCODE_THEME` | `tiranga` / `bharat` / `india` for the Tiranga palette |
| `BHARATCODE_USD_INR` | USD→INR rate for the cost ledger |
| `BHARATCODE_OFFLINE` | Local-only / no-egress posture |
| `BHARATCODE_RESIDENCY` | Egress residency guard |
| `BHARATCODE_SANDBOX` | `read-only` / `workspace-write` exec sandbox (Linux) |

See the per-feature notes above for the full opt-in set.

---

## License

Apache License 2.0 — see [`LICENSE`](LICENSE).
