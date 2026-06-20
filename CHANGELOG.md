# Changelog

All notable changes to BharatCode are documented here. This project adheres to
[Apache-2.0](LICENSE) and is a derivative work ("fork") of
[Goose](https://github.com/block/goose) with components ported from
[OpenAI Codex](https://github.com/openai/codex). Both upstreams are attributed in
[NOTICE](NOTICE) and [MODIFICATIONS.md](MODIFICATIONS.md).

## BharatCode 0.7.0 (2026-06-20)

The first feature release on top of the 0.1.0 fork. `bharatcode --version` now reports
**0.7.0**. Everything below is additive; defaults are unchanged unless an opt-in
`BHARATCODE_*` switch is set. The full test suite stays green (**goose 1619 + goose-cli
279 = 1898 passing, 0 failing** as of the v36 milestone) and there is zero user-facing
Goose/Block leakage.

### Ultracode — structured dynamic workflows

- New built-in **`ultracode`** skill (`bharatcode skills list` → `builtin://skills/ultracode`):
  a portable procedure that scales the agent from one-pass answers to
  **plan → split → run independent work → check → integrate → verify**, with three modes
  (Direct / Workflow / Delegated), disjoint parallel work packets, evidence-backed
  (source-over-vote) integration, and `.workflow/ultracode/<run-slug>/` artifacts.

### Agent tools & capabilities

- **Web search tool** — a real `web_search` agent tool (egress-guarded HTTP, structured
  results) wired into the developer toolset.
- **Persistent memory** (`BHARATCODE_MEMORY`) — cross-session facts stored under the
  config dir and recalled into the system prompt.
- **Context/token optimizer** (`BHARATCODE_CONTEXT_OPTIMIZE`) — relevance+recency message
  selection wired into the compaction path.
- **`bharatcode review-diff`** — a single-pass code review over the working git diff.

### Providers, models & cost

- **Model fallback chain** (`BHARATCODE_FALLBACK_MODELS`) — on a rate-limit/overload error,
  transparently retries the next model in the chain, wired into the central streaming path.
- **Model registry + ₹ cost metadata** — a static registry (India-built, open-weight, and
  common cloud models) with context-window and per-1K ₹ input/output costs surfaced in
  `bharatcode cost`.
- **Embeddings client** and **offline model-pack** support, plus **vision/multimodal
  preflight** and **per-model capability advisories** in the prompt.

### Privacy, compliance & diagnostics

- **DPDP audit log** (`BHARATCODE_AUDIT`) — append-only JSONL of model/tool turns
  (provider, model, IST timestamp, tokens, ₹) with a built-in viewer.
- **Secret redaction** (`BHARATCODE_REDACT`) — high-confidence secrets in developer shell
  output are masked before reaching the model.
- **`bharatcode privacy`** — a one-screen posture report (residency, offline, redaction,
  audit, telemetry, local-first provider) read from the real config/env keys.
- **`bharatcode doctor` deep checks** — local-provider reachability probe, config-dir
  writability, git availability, residency/offline coherence, and session-DB storage.



First public pre-1.0 release. BharatCode is an Indian, local-first, Apache-2.0 Rust
terminal AI coding agent. This release covers the fork itself: a clean rebrand, full
relicensing-to-fork compliance, local-first defaults, two ported Codex components, a
set of India/INR features, Hindi i18n scaffolding, and a quality/hardening pass that
left the test suite fully green.

### Rebrand

- Rebranded the user-facing surface to **BharatCode**: binary `bharatcode` (server
  `bharatcoded`), config directory `~/.config/bharatcode`, environment-variable prefix
  `BHARATCODE_*`, and all brand/identity/help/about strings, ASCII art, and the agent's
  self-identity.
- Hints file renamed `.goosehints` → `.bharatcodehints`; skills/discovery directory
  `.goose` → `.bharatcode`.
- Zero user-facing Goose/Block trademark leakage across `--help`, all subcommands,
  configure/doctor, and the built binary, enforced by a grep gate over source and binary.
- Scope note: this is a **surgical, user-facing** rebrand. Internal Rust crate and symbol
  names (`goose`, `goose-cli`, `GooseMode`, …) are intentionally left unchanged to keep
  the build stable; a full internal crate rename is deferred to a later release.

### License & compliance (Apache-2.0)

- Preserved the upstream Apache-2.0 `LICENSE` and the original Goose git history
  (provenance kept, not wiped).
- Added `LICENSES/LICENSE-goose` (© Block, Inc.) and `LICENSES/LICENSE-codex` (© OpenAI).
- `NOTICE` attributes **both** upstreams (Goose and Codex) and carries the Apache-2.0
  §4(b) "we modified these files" statement; `MODIFICATIONS.md` details every change.
- Added `THIRD_PARTY_LICENSES.md` aggregating the dependency tree of the actual release
  build (scoped to the `portable-default` feature set), grouped by SPDX expression.
- Trademarks are not licensed and remain the property of their owners; only trademarks
  were stripped from user-facing surfaces.

### Local-first

- **Telemetry is off by default** — the upstream PostHog product-analytics call is
  hard-disabled and the API key is neutralized. No phone-home.
- When nothing is configured, BharatCode defaults to a **local provider (Ollama)** and a
  local model — no forced cloud sign-in. Environment, config, and cloud providers still
  take precedence when set.
- First-run onboarding offers "Local model — Ollama (Recommended)" and preselects the
  local provider.

### Ported from OpenAI Codex (Apache-2.0, attributed)

- **apply-patch file editor** — vendored the pure parser/applier subset into the new
  `crates/bharatcode-apply-patch` crate (depends only on `thiserror`) and wired it as the
  developer `apply_patch` tool. Enabled as part of the developer toolset.
- **In-process Linux exec sandbox** — ported Codex's landlock + seccomp primitives into
  the new `crates/bharatcode-linux-sandbox` crate (read-only/workspace-write filesystem
  ruleset and a network-deny seccomp filter). Linux-only; a no-op on other platforms.
  **Opt-in, default OFF** via `BHARATCODE_SANDBOX` (`off|read-only|workspace-write`).
  Codex's bubblewrap launcher (which would pull LGPL) was intentionally excluded.

### India & INR features

- **INR cost ledger** — `bharatcode cost` reports per-session/day/month spend in ₹
  (USD→INR rate via `BHARATCODE_USD_INR`, IST day/month buckets) plus a compact ₹ footer
  in the session output.
- **India model presets** — new declarative providers for **Sarvam AI** and **Ola
  Krutrim** (India-hosted, OpenAI-compatible), plus a "Recommended (India / open-weight)"
  setup choice and a `bharatcode presets` listing (local Qwen-Coder/DeepSeek via Ollama;
  hosted Sarvam/Krutrim/Qwen/DeepSeek).
- **India recipe library** — `bharatcode recipes-library [--show <id>]` ships curated
  templates (UPI review, Aadhaar/PII + DPDP audit, GST helper, Indic localization).
- **Budget gate** — `BHARATCODE_BUDGET_INR` with warn/deny on a ₹ spend cap.
  **Opt-in, default OFF.**
- **Data-residency guard** and **offline mode** — `BHARATCODE_RESIDENCY` blocks
  non-allowlisted egress endpoints; `BHARATCODE_OFFLINE` composes local-only + residency
  + telemetry-off into a single switch. **Opt-in, default OFF.**
- **`bharatcode git`** — a read-only repository summary (branch, status, ahead/behind,
  recent commits) via read-only git subcommands.

### Internationalization (i18n)

- New CLI i18n layer (`crates/goose-cli/src/i18n/`) with a locale resolver
  (`BHARATCODE_LANG` → config → `LANG` → `en`), a `tr!()` macro, and embedded
  `en.json` / `hi.json` tables.
- A starter set of high-traffic strings is wired through Hindi; everything else falls
  back to English. With no locale set, English output is byte-identical to before.
  (Complete Hindi coverage is a later release.)

### Additional opt-in capabilities (default OFF)

These are wired but disabled unless their `BHARATCODE_*` switch is set:

- **Exec policy** (`BHARATCODE_EXEC_POLICY`) — command allow/deny gate, with hardened
  handling of substitution/subshell bypasses.
- **Approval modes** (`BHARATCODE_APPROVAL`) — chat/ask/auto/full mapping with a safe
  default (unsafe modes resolve to "ask").
- **Verify-before-done** (`BHARATCODE_VERIFY`) — runs the project's test/build and
  reports Verified/Failed/Skipped.
- **Cost-aware routing** (`BHARATCODE_COST_ROUTING`) — prefers cheaper/local models.
- **Prompt/response cache** (`BHARATCODE_CACHE`) — on-disk SHA-256-keyed cache.
- **Codebase context** (`BHARATCODE_CODEBASE_CONTEXT`) — a bounded, gitignore-respecting
  repo scanner.
- **Tiranga theming** (`BHARATCODE_THEME`, honors `NO_COLOR`) and **configurable
  keybindings** (`BHARATCODE_KEYS`). Unset = default appearance/behavior unchanged.
- Retry/backoff tuning via `BHARATCODE_RETRY_*`.

### Quality & hardening pass

- **Central wiring fix** — an adversarial review found several features had been wired at
  a single provider site and were effectively dead for other providers. They were
  re-wired at central choke points so they apply to **every** provider:
  - egress guard (residency + offline) moved into the shared provider API client;
  - prompt cache moved into the real streaming reply path (with a ₹-on-cache-hit cost bug
    fixed);
  - retry/backoff folded into the central provider-retry path (the parallel dead retry
    module was deleted);
  - cost routing applied to the lead model, not just the fast model.
- **Safety bug fixes** — corrected an inverted approval-mode default (unsafe modes now
  resolve to "ask"), hardened the exec-policy command splitter, switched cost buckets to
  IST, and removed two doc-comment brand leaks.
- **Critical sessions-DB migration fix** — the rebrand had renamed the on-disk session
  column `goose_mode` → `bharatcode_mode` with **no migration**, which would have broken
  existing users' databases. Added schema migration **v14 → v15** (idempotent,
  pragma-guarded `RENAME COLUMN`; no-op on fresh DBs) so existing session databases
  upgrade cleanly.
- **Tests green** — **1834 tests pass** (goose 1573 + goose-cli 261); the
  `portable-default` build stays green; CI now exercises the `goose` crate under its
  required TLS feature in addition to the CLI.
