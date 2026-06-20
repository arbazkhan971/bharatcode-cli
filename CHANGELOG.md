# Changelog

All notable changes to BharatCode are documented here. BharatCode is licensed under
[Apache-2.0](LICENSE).

## BharatCode 0.7.0 (2026-06-20)

The first feature release on top of 0.1.0. `bharatcode --version` now reports **0.7.0**.
Everything below is additive; defaults are unchanged unless an opt-in `BHARATCODE_*`
switch is set. The full test suite stays green (**1898 passing, 0 failing**).

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

## BharatCode 0.1.0 (2026-06-20)

First public pre-1.0 release. BharatCode is an Indian, local-first, Apache-2.0 Rust
terminal AI coding agent.

### Branding & identity

- User-facing surface is **BharatCode**: binary `bharatcode` (server `bharatcoded`),
  config directory `~/.config/bharatcode`, environment-variable prefix `BHARATCODE_*`,
  and all brand/identity/help/about strings, ASCII art, and the agent's self-identity.
- Hints file `.bharatcodehints`; skills/discovery directory `.bharatcode`.

### Local-first

- **Telemetry is off by default** — product analytics are hard-disabled; no phone-home.
- With nothing configured, BharatCode defaults to a **local provider (Ollama)** and a
  local model — no forced cloud sign-in. Environment, config, and cloud providers still
  take precedence when set.
- First-run onboarding offers "Local model — Ollama (Recommended)".

### File editing & sandbox

- **apply-patch file editor** — a streaming patch parser/applier in the
  `bharatcode-apply-patch` crate, wired as the developer `apply_patch` tool.
- **In-process Linux exec sandbox** — landlock + seccomp (read-only / workspace-write
  filesystem ruleset and a network-deny seccomp filter). Linux-only; a no-op on other
  platforms. **Opt-in, default OFF** via `BHARATCODE_SANDBOX`.

### India & INR features

- **INR cost ledger** — `bharatcode cost` reports per-session/day/month spend in ₹
  (USD→INR rate via `BHARATCODE_USD_INR`, IST day/month buckets) plus a compact ₹ footer.
- **India model presets** — declarative **Sarvam AI** and **Ola Krutrim** providers, a
  "Recommended (India / open-weight)" setup choice, and a `bharatcode presets` listing.
- **India recipe library** — `bharatcode recipes-library [--show <id>]` ships curated
  templates (UPI review, Aadhaar/PII + DPDP audit, GST helper, Indic localization).
- **Budget gate** — `BHARATCODE_BUDGET_INR` with warn/deny on a ₹ spend cap. Opt-in.
- **Data-residency guard** and **offline mode** — `BHARATCODE_RESIDENCY` blocks
  non-allowlisted egress; `BHARATCODE_OFFLINE` composes local-only + residency +
  telemetry-off. Opt-in, default OFF.
- **`bharatcode git`** — a read-only repository summary.

### Internationalization (i18n)

- New CLI i18n layer with a locale resolver (`BHARATCODE_LANG` → config → `LANG` → `en`),
  a `tr!()` macro, and embedded `en.json` / `hi.json`. A starter set of high-traffic
  strings is wired through Hindi; everything else falls back to English.

### Additional opt-in capabilities (default OFF)

- **Exec policy** (`BHARATCODE_EXEC_POLICY`) — command allow/deny gate.
- **Approval modes** (`BHARATCODE_APPROVAL`) — chat/ask/auto/full with a safe default.
- **Verify-before-done** (`BHARATCODE_VERIFY`) — runs the project's test/build.
- **Cost-aware routing** (`BHARATCODE_COST_ROUTING`) — prefers cheaper/local models.
- **Prompt/response cache** (`BHARATCODE_CACHE`) — on-disk SHA-256-keyed cache.
- **Codebase context** (`BHARATCODE_CODEBASE_CONTEXT`) — a bounded, gitignore-respecting
  repo scanner.
- **Tiranga theming** (`BHARATCODE_THEME`, honors `NO_COLOR`) and **configurable
  keybindings** (`BHARATCODE_KEYS`).
- Retry/backoff tuning via `BHARATCODE_RETRY_*`.

### Quality & hardening

- Central wiring of opt-in gates (residency/offline egress guard, prompt cache, retry,
  cost routing) at shared choke points so they apply to every provider.
- Safety fixes: corrected an inverted approval-mode default, hardened the exec-policy
  command splitter, IST cost buckets.
- Sessions-DB schema migration (v14 → v15) so existing session databases upgrade cleanly.
- **Tests green** — 1834 tests pass; the `portable-default` build stays green.
