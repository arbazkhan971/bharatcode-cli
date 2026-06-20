# BharatCode — Project Overview

BharatCode is an Indian, **local-first**, **Apache-2.0** Rust terminal AI coding
agent. It is a derivative work ("fork") of **Goose** (`block/goose`, Apache-2.0)
with selected components **ported from OpenAI Codex** (`openai/codex`,
Apache-2.0). It runs as a single CLI binary named **`bharatcode`**.

This document is an accurate, code-verified description of what is actually built
and wired today. It distinguishes shipped/default behaviour from **opt-in**
features that default to **OFF**. The authoritative, append-only build history is
in [`iterations.md`](../iterations.md); the roadmap is in
[`versions.md`](../versions.md).

> Accuracy note: opt-in features are explicitly marked **(opt-in, default OFF)**.
> They do nothing unless their `BHARATCODE_*` switch is set, so the default
> behaviour of the binary is unchanged from upstream Goose except for branding,
> telemetry-off, and local-first provider defaults.

---

## 1. What BharatCode is

- A terminal AI coding agent: interactive sessions, one-shot `run`, recipes,
  MCP extensions, multiple model providers, an agent loop with tools.
- **Local-first by default:** when nothing is configured, the active provider
  defaults to **Ollama** and the model to Ollama's default coder model
  (`crates/goose/src/config/providers.rs` — `DEFAULT_LOCAL_PROVIDER`,
  `get_active_provider`/`get_active_model`). Cloud/hosted providers remain fully
  supported but are opt-in (env/config take precedence over the local default).
- **Telemetry off:** the bundled PostHog product-analytics path is hard-disabled.
  `is_telemetry_enabled()` returns `false` and the embedded `POSTHOG_API_KEY` is
  empty (`crates/goose/src/posthog.rs`). BharatCode does not phone home.
- **Apache-2.0 compliant from the start:** upstream licenses retained, both
  upstreams attributed, modifications documented, trademarks stripped from
  user-facing surfaces only.
- Current internal version: **1.38.0** (workspace `Cargo.toml`; inherited from
  the Goose base).

### Relationship to Goose (the base)

BharatCode forks Goose and keeps its architecture, agent engine, provider
catalog, MCP system, session store, and CLI. The fork is a **surgical rebrand**
of the *user-facing surface* plus additive Indian/local-first features and two
Codex ports. Crucially, **internal Rust crate and symbol names are still
`goose-*` / `Goose*`** (e.g. crate `goose`, `goose-cli`, types like `GooseMode`).
A full internal crate rename is a documented, deferred roadmap item (v8), not
done yet. This is intentional: renaming identifiers used in `use goose::` path
deps would break the build, so v1 rebranded only:

- binary names (`goose` → `bharatcode`, `goosed` → `bharatcoded`),
- config app-name / config directory (`~/.config/goose` → `~/.config/bharatcode`;
  `crates/goose/src/config/paths.rs` `app_name = "bharatcode"`),
- environment-variable **string literals** (`GOOSE_*` → `BHARATCODE_*`),
- brand / identity / help / ASCII-logo display strings,
- telemetry default (now off).

There is **zero user-facing Goose/Block leakage** in CLI output (verified across
help, subcommands, configure, doctor, etc. in the iteration log). Internal
identifiers and a small set of deliberately-preserved semantic wire keys (e.g.
recipe-schema `goose_provider`/`goose_model`) are not user-facing.

### Relationship to Codex (the donor)

Two **pure** components were ported from OpenAI Codex as isolated crates behind
adapters, each carrying top-of-file Codex attribution and accompanied by
`LICENSES/LICENSE-codex` + a `NOTICE` section + `MODIFICATIONS.md` entries:

1. **`crates/bharatcode-apply-patch`** — Codex's streaming `*** Begin Patch`
   apply-patch file editor. Ported: `seek_sequence.rs` (verbatim),
   `streaming_parser.rs` (verbatim), `parser.rs` (adapted), and the
   compute/apply replacement core re-implemented on synchronous `std::fs`
   (`apply.rs`). Dependencies were reduced to **`thiserror` only** (dropped
   `codex-exec-server`, path-uri/absolute-path utils, tree-sitter, tokio). It is
   wired as the developer **`apply_patch`** tool
   (`crates/goose/src/agents/platform_extensions/developer/`).

2. **`crates/bharatcode-linux-sandbox`** — Codex's in-process **Linux** exec
   sandbox primitives (landlock + seccomp): `set_no_new_privs`
   (`PR_SET_NO_NEW_PRIVS`), a Landlock ABI::V5 filesystem ruleset, and a
   Restricted-mode network seccomp filter. Public surface is a plain
   `SandboxPolicy { writable_roots, allow_network }` + `thiserror SandboxError`.
   Deps: `thiserror` (all targets) + `landlock 0.4` / `seccompiler 0.5` / `libc`
   (Linux only); a no-op stub on non-Linux. **Codex's bubblewrap launcher was
   deliberately NOT ported** (avoids pulling LGPL bubblewrap); proxy-routed
   network mode and the seatbelt/Windows backends were also dropped.

Both ports preserve all upstream copyright notices; only trademark usage changed.

---

## 2. Architecture

A Cargo workspace (`members = ["crates/*"]`, plus others). The shipped CLI is the
**`bharatcode`** binary produced by the `goose-cli` crate
(`crates/goose-cli/Cargo.toml`: `[[bin]] name = "bharatcode"`).

| Crate | Role |
|---|---|
| `goose` | Core agent engine, providers, config, sessions, tools, platform extensions. Most BharatCode feature modules live here (`offline.rs`, `residency.rs`, `exec_policy.rs`, `verify.rs`, `prompt_cache.rs`, `cost_routing.rs`, `codebase_context.rs`, `posthog.rs`). |
| `goose-cli` | CLI entry; produces binary **`bharatcode`**. Hosts i18n (`src/i18n/`), theming (`theme.rs`), keybindings (`keybindings.rs`), and the cost/budget/git/presets/recipes-library commands. |
| `goose-providers` | Provider HTTP client + retry + canonical model registry. Central egress guard and `BHARATCODE_RETRY_*` overrides live here (`api_client.rs`, `retry.rs`). |
| `goose-server` | Backend daemon (binary `bharatcoded`). |
| `goose-mcp` | MCP extensions. |
| `goose-acp-macros`, `goose-sdk`, `goose-sdk-types`, `goose-test`, `goose-test-support` | ACP proc-macros, SDK, and test utilities. |
| **`bharatcode-apply-patch`** | Ported Codex apply-patch editor (edition 2024; dep: `thiserror`). |
| **`bharatcode-linux-sandbox`** | Ported Codex Linux landlock+seccomp sandbox (edition 2021; Linux-only native deps). |

**Light/portable build (the green invariant):**

```bash
cargo build -p goose-cli --no-default-features --features portable-default
```

This drops the heavy native engines (local-inference / code-mode / mlx / nostr /
system-keyring) and is the build the test suite and CI gate target. Tests:
`goose-cli --lib` and `goose --lib --features rustls-tls` (the goose crate needs
a TLS feature for its sqlx runtime). The latest logged status is build-green with
the full suite passing.

Entry points: CLI `crates/goose-cli/src/main.rs`; agent loop
`crates/goose/src/agents/agent.rs`; server `crates/goose-server/src/main.rs`.

### Added CLI subcommands (BharatCode)

Wired in `crates/goose-cli/src/cli.rs`: `bharatcode cost` (₹ spend summary),
`bharatcode git` (read-only repo summary), `bharatcode presets` (India/open-weight
model presets), `bharatcode recipes-library [--show <id>]` (curated Indian-dev
recipe templates). `doctor` is extended with a read-only BharatCode settings
summary.

---

## 3. `BHARATCODE_*` configuration / environment variables

Most settings are read through `goose::config::Config`, so each key can be set
either as an **environment variable** or as the matching key in the on-disk
config (`~/.config/bharatcode/config.yaml`); the environment generally takes
precedence. The large set below is grouped by purpose. The headline BharatCode
feature switches are documented individually; the long tail are the inherited
Goose operational knobs, renamed `GOOSE_* → BHARATCODE_*`.

### 3.1 Provider & model selection

| Variable | Effect |
|---|---|
| `BHARATCODE_PROVIDER` | Active provider id. Resolution: env → `active_provider` config → legacy flat key → **local-first default `ollama`**. |
| `BHARATCODE_MODEL` | Active model. Resolution: env → active provider's recorded model → legacy flat key → Ollama default coder model when local-first. |
| `BHARATCODE_PROVIDER__TYPE` / `__HOST` / `__API_KEY` | Structured ad-hoc provider override (type/host/api-key) without editing config. |
| `BHARATCODE_FAST_MODEL`, `BHARATCODE_PLANNER_PROVIDER`/`_MODEL`/`_CONTEXT_LIMIT`, `BHARATCODE_SUBAGENT_PROVIDER`/`_MODEL`/`_MAX_TURNS`, `BHARATCODE_TOOLSHIM*`, `BHARATCODE_PREDEFINED_MODELS`, `BHARATCODE_LOCAL_DRAFT_MODEL`, `BHARATCODE_LOCAL_ENABLE_THINKING` | Role/auxiliary model selection and local-inference knobs (inherited Goose behaviour). |

### 3.2 Cost & budget (INR)

| Variable | Effect |
|---|---|
| `BHARATCODE_USD_INR` | USD→INR conversion rate used for all ₹ display. Default **88.0** (`cost_ledger.rs DEFAULT_USD_INR`); non-positive/invalid falls back to default. |
| `BHARATCODE_BUDGET_INR` | **(opt-in, default OFF)** ₹ spend cap. Absent or non-positive ⇒ gate disabled. |
| `BHARATCODE_BUDGET_MODE` | `warn` (default; never blocks, nudges from 80% of cap) or `deny` (blocks the next model turn once the cap is exceeded). |
| `BHARATCODE_BUDGET_SCOPE` | `session` (default; this session's accumulated cost) or `day` (today's spend across sessions via the cost ledger). Ledger day/month buckets are computed in **IST (UTC+05:30)**. |
| `BHARATCODE_COST_ROUTING` | **(opt-in, default OFF)** Cost-aware routing: prefer the cheapest *capable* (and local/open-weight) model from candidates, using bundled canonical cost metadata. Off ⇒ caller's choice returned unchanged. |
| `BHARATCODE_COST_ROUTING_CANDIDATES` | Comma-separated extra candidate model names (same provider) routing may pick from. |

### 3.3 Data residency, offline & privacy

| Variable | Effect |
|---|---|
| `BHARATCODE_RESIDENCY` | **(opt-in, default `off`)** Egress guard mode: `off` / `warn` (log) / `strict` (block). Screens the endpoint host every provider is about to call. Enforced centrally in `goose-providers/api_client.rs` so it applies to *all* providers, including declarative ones. |
| `BHARATCODE_RESIDENCY_ALLOWLIST` | Comma/whitespace-separated permitted hostnames (subdomain + `*.` wildcard match). Loopback hosts are always allowed. |
| `BHARATCODE_OFFLINE` | **(opt-in, default OFF)** Single composed no-egress switch: forces local-only endpoints (loopback hosts only), treats residency as `strict`, and reports telemetry as off. Read-only over the underlying settings (composes, never mutates). |
| `BHARATCODE_TELEMETRY_ENABLED` / `BHARATCODE_TELEMETRY_OFF` | Telemetry preference keys. Note: telemetry is **hard-disabled** regardless — `is_telemetry_enabled()` returns `false` and the PostHog key is empty. |

### 3.4 Command execution safety

| Variable | Effect |
|---|---|
| `BHARATCODE_EXEC_POLICY` | **(opt-in, default OFF)** Path to a JSON `{allow:[], deny:[]}` command-prefix policy file (or `off`/`0`/`false`/empty to disable). Screens shell-tool commands before spawn: deny takes precedence; a non-empty allow-list switches to allow-list mode. Quote/operator/subshell-aware splitting; clean-room reimplementation (no policy-as-code engine). |
| `BHARATCODE_SANDBOX` | **(opt-in, default `off`; Linux-only effect)** `off` / `read-only` / `workspace-write`. When on (and not flatpak), wraps the shell tool with the ported landlock+seccomp sandbox: `read-only` ⇒ no writable roots + network denied; `workspace-write` ⇒ working dir writable + network allowed. |
| `BHARATCODE_APPROVAL` | **(opt-in; unset = no-op)** Approval posture mapped onto the engine's `GooseMode`: per-decision `ask`/`allow`/`deny` and coarse `read-only`/`auto`/`full` (+`yolo` alias), with lenient aliases. When unset, the existing mode is returned untouched — it can never silently widen permissions. |

### 3.5 Verify-before-done

| Variable | Effect |
|---|---|
| `BHARATCODE_VERIFY` | **(opt-in, default `false`)** After a turn that may have changed code, detect the build system and run a verification command, classifying the result as `Verified` / `Failed` / `Skipped(reason)`. |
| `BHARATCODE_VERIFY_TASK` | `build` (default) / `test` / `check`. |
| `BHARATCODE_VERIFY_TIMEOUT_SECS` | Wall-clock budget for the verify command; default **300**. |

### 3.6 Performance: cache & retry

| Variable | Effect |
|---|---|
| `BHARATCODE_CACHE` | **(opt-in, default OFF)** On-disk prompt/response cache keyed by `(provider, model, request hash)` (system prompt + messages + tools, SHA-256). Wired into the streaming path: HIT short-circuits to a zero-cost stream, MISS tees and stores on completion. Best-effort; failures degrade to a miss. |
| `BHARATCODE_RETRY_MAX` | Total attempt count (including the first call) applied centrally via `RetryConfig::with_env_overrides`, so **every** provider honours it. |
| `BHARATCODE_RETRY_BASE_MS` | Base exponential-backoff delay. |
| `BHARATCODE_RETRY_MAX_MS` | Backoff ceiling. |

### 3.7 UX: theme, language, keybindings

| Variable | Effect |
|---|---|
| `BHARATCODE_THEME` | CLI color theme: `none`/`off`/`mono`/`plain`/`nocolor` ⇒ plain; `tiranga`/`bharat`/`india` ⇒ Tiranga (saffron/white/green) palette; `default`/`stock` ⇒ stable default. **`NO_COLOR` is honoured first.** Unset ⇒ byte-identical default look. |
| `BHARATCODE_LANG` | UI locale. Resolution: `BHARATCODE_LANG` → `bharatcode_lang` config → `LANG` → `en`. Drives the i18n scaffold (`en.json`/`hi.json`); English output is byte-identical, a starter set of strings has Hindi. |
| `BHARATCODE_KEYS` | Interactive keybinding overrides as `action=key` pairs (`;`/`,` separated), e.g. `cancel=ctrl-g;history_prev=ctrl-p`. Actions: `submit`, `cancel`, `history_prev`, `history_next`, `newline`. Defaults reproduce the built-ins exactly. |
| `BHARATCODE_CLI_NEWLINE_KEY` | Legacy newline-key override (still honoured by the keybindings layer). |
| `BHARATCODE_CLI_THEME`, `BHARATCODE_CLI_DARK_THEME`, `BHARATCODE_CLI_LIGHT_THEME`, `BHARATCODE_CLI_SHOW_COST`, `BHARATCODE_CLI_SHOW_THINKING`, `BHARATCODE_CLI_MIN_PRIORITY` | Inherited Goose CLI display knobs. |

### 3.8 Codebase context

| Variable | Effect |
|---|---|
| `BHARATCODE_CODEBASE_CONTEXT` | **(opt-in, default OFF)** RAG-lite: a single bounded, `.gitignore`-respecting repo scan producing a compact blob (top-level layout + manifests + README excerpt) to seed the agent. Read-only and bounded by hard limits. Off ⇒ nothing is scanned. |

### 3.9 Inherited operational variables (renamed `GOOSE_* → BHARATCODE_*`)

These are pre-existing Goose knobs whose env-string literals were rebranded;
behaviour is unchanged from upstream. Non-exhaustive: `BHARATCODE_MODE`,
`BHARATCODE_MAX_TURNS`, `BHARATCODE_MAX_TOKENS`, `BHARATCODE_TEMPERATURE`,
`BHARATCODE_CONTEXT_LIMIT`, `BHARATCODE_INPUT_LIMIT`,
`BHARATCODE_AUTO_COMPACT_THRESHOLD`, `BHARATCODE_THINKING_EFFORT`,
`BHARATCODE_DISABLE_KEYRING`, `BHARATCODE_DISABLE_SESSION_NAMING`,
`BHARATCODE_HINTS_FILENAME`, `BHARATCODE_RECIPE_PATH`,
`BHARATCODE_RECIPE_GITHUB_REPO`, `BHARATCODE_TUNNEL*`, `BHARATCODE_NOSTR_RELAYS`,
`BHARATCODE_OAUTH_CALLBACK_PORT`, `BHARATCODE_TLS_CERT_PATH`/`_KEY_PATH`,
`BHARATCODE_CA_CERT_PATH`, `BHARATCODE_CLIENT_CERT_PATH`/`_KEY_PATH`,
`BHARATCODE_SERVER__SECRET_KEY`, `BHARATCODE_WORKING_DIR`, `BHARATCODE_SHELL`,
`BHARATCODE_TERMINAL`, `BHARATCODE_DEBUG`, `BHARATCODE_RECORD_MCP`,
`BHARATCODE_DEFAULT_EXTENSION_TIMEOUT`, `BHARATCODE_STREAM_TIMEOUT`,
`BHARATCODE_USER_AGENT`, and the `BHARATCODE_APP_TYPE`/`_DESKTOP`/`_TERMINAL`
client-identity flags, among others. (Full set: grep `BHARATCODE_` over
`crates/`.)

---

## 4. Compliance & licensing posture

BharatCode is **Apache-2.0** and is built to be compliant *from commit 1*, with
both upstreams attributed.

- **`LICENSE`** — Apache License 2.0 (retained from upstream).
- **`LICENSES/LICENSE-goose`** — upstream Goose license copy (© Block, Inc.).
- **`LICENSES/LICENSE-codex`** — upstream Codex license copy (© OpenAI), present
  because Codex code is ported.
- **`NOTICE`** — BharatCode line + Goose (Block) attribution + Codex (OpenAI)
  attribution with per-port detail + an Apache-2.0 §4(b) "we modified these
  files" statement. (Note: Goose ships no NOTICE upstream; this one was added.)
- **`MODIFICATIONS.md`** — detailed §4(b) change log: rebrand, telemetry-off,
  local-first defaults, the i18n scaffold and India model presets (original
  work), and the two Codex ports with exactly what was changed during porting.
- **`THIRD_PARTY_LICENSES.md`** — aggregated from the actual `portable-default`
  dependency tree (`cargo tree` + `cargo metadata`), grouped by SPDX expression;
  every crate carries an SPDX license field.
- **Trademarks stripped, not licensed:** Goose/Block/Square/Cash App/Tidal and
  Codex/OpenAI/ChatGPT marks were removed from **user-facing** surfaces only and
  remain the property of their owners. A grep gate over source and the built
  binary enforces zero user-facing leakage.
- **Upstream git provenance preserved** (the `.git` history was deliberately kept,
  not wiped).
- **Telemetry off by default** — the PostHog phone-home path is hard-disabled.

### What is NOT claimed (honesty notes)

- **Internal crate/symbol rename is not done.** Internal crate names are still
  `goose`, `goose-cli`, etc., and types like `GooseMode` remain. Only the
  user-facing surface is rebranded. (`crates/goose-cli` still produces the
  `bharatcode` binary.) Full rename is roadmap v8, deferred.
- **The Linux sandbox is Linux-only and opt-in.** On non-Linux it is a no-op
  stub; even on Linux it does nothing unless `BHARATCODE_SANDBOX` is set. macOS
  seatbelt, Windows, bubblewrap FS isolation, and proxy-routed network were not
  ported.
- **Opt-in features default to OFF.** `BHARATCODE_OFFLINE`, `_RESIDENCY`,
  `_EXEC_POLICY`, `_SANDBOX`, `_VERIFY`, `_CACHE`, `_COST_ROUTING`,
  `_BUDGET_INR`, `_CODEBASE_CONTEXT`, and `_APPROVAL` (when unset) do nothing
  until explicitly enabled; default binary behaviour is unchanged.
- **i18n / Hindi coverage is a scaffold**, not a complete translation: the
  resolver + `en.json`/`hi.json` exist and a starter set of high-traffic strings
  is wired; full Hindi UI is roadmap work.
- Some inherited internal wire keys (e.g. recipe-schema `goose_provider` /
  `goose_model`, certain serde field names) are intentionally preserved for
  compatibility and are not user-facing.

---

## 5. Source-of-truth pointers

- Build history (append-only): [`iterations.md`](../iterations.md)
- Roadmap (v1→v100): [`versions.md`](../versions.md)
- Modifications / §4(b): [`MODIFICATIONS.md`](../MODIFICATIONS.md),
  [`NOTICE`](../NOTICE)
- Licenses: [`LICENSE`](../LICENSE), [`LICENSES/`](../LICENSES/),
  [`THIRD_PARTY_LICENSES.md`](../THIRD_PARTY_LICENSES.md)
- Feature modules: `crates/goose/src/{offline,residency,exec_policy,verify,prompt_cache,cost_routing,codebase_context,posthog}.rs`,
  `crates/goose-cli/src/{theme,keybindings}.rs`,
  `crates/goose-cli/src/commands/{budget,cost_ledger,cost,git_helper,presets,recipes_library}.rs`,
  `crates/goose-providers/src/{api_client,retry}.rs`
- Codex ports: `crates/bharatcode-apply-patch/`, `crates/bharatcode-linux-sandbox/`
