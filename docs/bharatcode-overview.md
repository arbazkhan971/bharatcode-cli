# BharatCode — Project Overview

BharatCode is an Indian, **local-first**, **Apache-2.0** Rust terminal AI coding
agent. It runs as a single CLI binary named **`bharatcode`**.

This document is an accurate, code-verified description of what is actually built
and wired today. It distinguishes shipped/default behaviour from **opt-in**
features that default to **OFF**.

> Accuracy note: opt-in features are explicitly marked **(opt-in, default OFF)**.
> They do nothing unless their `BHARATCODE_*` switch is set, so the default
> behaviour of the binary is branding, telemetry-off, and local-first provider
> defaults only.

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
- **apply-patch editor & Linux sandbox:** a streaming apply-patch file editor
  (`crates/bharatcode-apply-patch`) wired as the developer `apply_patch` tool, and
  an opt-in in-process Linux landlock+seccomp exec sandbox
  (`crates/bharatcode-linux-sandbox`).

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
| `bharatcode-apply-patch` | Streaming `*** Begin Patch` apply-patch file editor (edition 2024; dep: `thiserror`). |
| `bharatcode-linux-sandbox` | In-process Linux landlock+seccomp exec sandbox (edition 2021; Linux-only native deps). |

> Note: internal Rust crate and symbol names (`goose`, `goose-cli`, `GooseMode`, …)
> are kept as-is; only the user-facing surface, binary names, and config are
> BharatCode. A full internal crate rename is a deferred roadmap item.

**Light/portable build (the green invariant):**

```bash
cargo build -p goose-cli --no-default-features --features portable-default
```

This drops the heavy native engines (local-inference / code-mode / mlx / nostr /
system-keyring) and is the build the test suite and CI gate target. Tests:
`goose-cli --lib` and `goose --lib --features rustls-tls` (the goose crate needs
a TLS feature for its sqlx runtime).

Entry points: CLI `crates/goose-cli/src/main.rs`; agent loop
`crates/goose/src/agents/agent.rs`; server `crates/goose-server/src/main.rs`.

### Added CLI subcommands

Wired in `crates/goose-cli/src/cli.rs`: `bharatcode cost` (₹ spend summary),
`bharatcode git` (read-only repo summary), `bharatcode presets` (India/open-weight
model presets), `bharatcode recipes-library [--show <id>]` (curated Indian-dev
recipe templates), `bharatcode privacy` (data-governance posture). `doctor` is
extended with deep checks and a read-only settings summary.

---

## 3. `BHARATCODE_*` configuration / environment variables

Most settings are read through `goose::config::Config`, so each key can be set
either as an **environment variable** or as the matching key in the on-disk
config (`~/.config/bharatcode/config.yaml`); the environment generally takes
precedence.

### 3.1 Provider & model selection

| Variable | Effect |
|---|---|
| `BHARATCODE_PROVIDER` | Active provider id. Resolution: env → `active_provider` config → legacy flat key → **local-first default `ollama`**. |
| `BHARATCODE_MODEL` | Active model. Resolution: env → active provider's recorded model → legacy flat key → Ollama default coder model when local-first. |
| `BHARATCODE_PROVIDER__TYPE` / `__HOST` / `__API_KEY` | Structured ad-hoc provider override (type/host/api-key) without editing config. |
| `BHARATCODE_FALLBACK_MODELS` | **(opt-in, default OFF)** Comma-separated fallback chain; on a rate-limit/overload error the next model is tried transparently. |
| `BHARATCODE_FAST_MODEL`, `BHARATCODE_PLANNER_*`, `BHARATCODE_SUBAGENT_*`, `BHARATCODE_TOOLSHIM*`, `BHARATCODE_PREDEFINED_MODELS`, `BHARATCODE_LOCAL_DRAFT_MODEL`, `BHARATCODE_LOCAL_ENABLE_THINKING` | Role/auxiliary model selection and local-inference knobs. |

### 3.2 Cost & budget (INR)

| Variable | Effect |
|---|---|
| `BHARATCODE_USD_INR` | USD→INR conversion rate used for all ₹ display. Default **88.0** (`cost_ledger.rs DEFAULT_USD_INR`); non-positive/invalid falls back to default. |
| `BHARATCODE_BUDGET_INR` | **(opt-in, default OFF)** ₹ spend cap. Absent or non-positive ⇒ gate disabled. |
| `BHARATCODE_BUDGET_MODE` | `warn` (default; never blocks, nudges from 80% of cap) or `deny` (blocks the next model turn once the cap is exceeded). |
| `BHARATCODE_BUDGET_SCOPE` | `session` (default) or `day` (today's spend across sessions). Ledger day/month buckets are computed in **IST (UTC+05:30)**. |
| `BHARATCODE_COST_ROUTING` | **(opt-in, default OFF)** Prefer the cheapest *capable* (and local/open-weight) model from candidates. Off ⇒ caller's choice returned unchanged. |
| `BHARATCODE_COST_ROUTING_CANDIDATES` | Comma-separated extra candidate model names (same provider) routing may pick from. |

### 3.3 Data residency, offline & privacy

| Variable | Effect |
|---|---|
| `BHARATCODE_RESIDENCY` | **(opt-in, default `off`)** Egress guard mode: `off` / `warn` (log) / `strict` (block). Screens the endpoint host every provider is about to call. Enforced centrally in `goose-providers/api_client.rs` so it applies to *all* providers. |
| `BHARATCODE_RESIDENCY_ALLOWLIST` | Comma/whitespace-separated permitted hostnames (subdomain + `*.` wildcard match). Loopback hosts are always allowed. |
| `BHARATCODE_OFFLINE` | **(opt-in, default OFF)** Single composed no-egress switch: forces local-only endpoints, treats residency as `strict`, and reports telemetry as off. |
| `BHARATCODE_REDACT` | **(opt-in, default OFF)** Masks high-confidence secrets in developer shell output before they reach the model. |
| `BHARATCODE_AUDIT` | **(opt-in, default OFF)** Append-only JSONL audit log of model/tool turns (provider, model, IST timestamp, tokens, ₹). |
| `BHARATCODE_TELEMETRY_ENABLED` / `BHARATCODE_TELEMETRY_OFF` | Telemetry preference keys. Note: telemetry is **hard-disabled** regardless. |

### 3.4 Command execution safety

| Variable | Effect |
|---|---|
| `BHARATCODE_EXEC_POLICY` | **(opt-in, default OFF)** Path to a JSON `{allow:[], deny:[]}` command-prefix policy file. Screens shell-tool commands before spawn: deny takes precedence; a non-empty allow-list switches to allow-list mode. Quote/operator/subshell-aware splitting. |
| `BHARATCODE_SANDBOX` | **(opt-in, default `off`; Linux-only effect)** `off` / `read-only` / `workspace-write`. When on, wraps the shell tool with the landlock+seccomp sandbox: `read-only` ⇒ no writable roots + network denied; `workspace-write` ⇒ working dir writable + network allowed. |
| `BHARATCODE_APPROVAL` | **(opt-in; unset = no-op)** Approval posture: per-decision `ask`/`allow`/`deny` and coarse `read-only`/`auto`/`full`. When unset, the existing mode is returned untouched — it can never silently widen permissions. |

### 3.5 Verify-before-done

| Variable | Effect |
|---|---|
| `BHARATCODE_VERIFY` | **(opt-in, default `false`)** After a turn that may have changed code, detect the build system and run a verification command (`Verified` / `Failed` / `Skipped`). |
| `BHARATCODE_VERIFY_TASK` | `build` (default) / `test` / `check`. |
| `BHARATCODE_VERIFY_TIMEOUT_SECS` | Wall-clock budget for the verify command; default **300**. |

### 3.6 Performance: cache, context & retry

| Variable | Effect |
|---|---|
| `BHARATCODE_CACHE` | **(opt-in, default OFF)** On-disk prompt/response cache keyed by `(provider, model, request hash)`. HIT short-circuits to a zero-cost stream, MISS tees and stores on completion. |
| `BHARATCODE_CONTEXT_OPTIMIZE` | **(opt-in, default OFF)** Relevance+recency message selection within the token budget, wired into the compaction path. |
| `BHARATCODE_MEMORY` | **(opt-in, default OFF)** Cross-session memory store recalled into the system prompt. |
| `BHARATCODE_RETRY_MAX` / `_BASE_MS` / `_MAX_MS` | Central retry/backoff overrides applied to **every** provider. |

### 3.7 UX: theme, language, keybindings

| Variable | Effect |
|---|---|
| `BHARATCODE_THEME` | CLI color theme: plain aliases ⇒ plain; `tiranga`/`bharat`/`india` ⇒ Tiranga (saffron/white/green) palette; `default`/`stock` ⇒ stable default. **`NO_COLOR` is honoured first.** Unset ⇒ byte-identical default look. |
| `BHARATCODE_LANG` | UI locale. Resolution: `BHARATCODE_LANG` → `bharatcode_lang` config → `LANG` → `en`. Drives the i18n scaffold (`en.json`/`hi.json`). |
| `BHARATCODE_KEYS` | Interactive keybinding overrides as `action=key` pairs, e.g. `cancel=ctrl-g;history_prev=ctrl-p`. Actions: `submit`, `cancel`, `history_prev`, `history_next`, `newline`. |

### 3.8 Codebase context

| Variable | Effect |
|---|---|
| `BHARATCODE_CODEBASE_CONTEXT` | **(opt-in, default OFF)** RAG-lite: a single bounded, `.gitignore`-respecting repo scan producing a compact blob (top-level layout + manifests + README excerpt) to seed the agent. |

### 3.9 Operational variables

Standard operational knobs (set the env-string literal or the config key).
Non-exhaustive: `BHARATCODE_MODE`, `BHARATCODE_MAX_TURNS`, `BHARATCODE_MAX_TOKENS`,
`BHARATCODE_TEMPERATURE`, `BHARATCODE_CONTEXT_LIMIT`, `BHARATCODE_INPUT_LIMIT`,
`BHARATCODE_AUTO_COMPACT_THRESHOLD`, `BHARATCODE_THINKING_EFFORT`,
`BHARATCODE_DISABLE_KEYRING`, `BHARATCODE_DISABLE_SESSION_NAMING`,
`BHARATCODE_HINTS_FILENAME`, `BHARATCODE_RECIPE_PATH`,
`BHARATCODE_RECIPE_GITHUB_REPO`, `BHARATCODE_TUNNEL*`, `BHARATCODE_NOSTR_RELAYS`,
`BHARATCODE_OAUTH_CALLBACK_PORT`, `BHARATCODE_TLS_CERT_PATH`/`_KEY_PATH`,
`BHARATCODE_WORKING_DIR`, `BHARATCODE_SHELL`, `BHARATCODE_TERMINAL`,
`BHARATCODE_DEBUG`, `BHARATCODE_STREAM_TIMEOUT`, `BHARATCODE_USER_AGENT`, among
others. (Full set: grep `BHARATCODE_` over `crates/`.)

---

## 4. License

BharatCode is licensed under **Apache-2.0** — see [`LICENSE`](../LICENSE).

---

## 5. Source pointers

- Feature modules: `crates/goose/src/{offline,residency,exec_policy,verify,prompt_cache,cost_routing,codebase_context,model_registry,memory_store,context_optimizer,posthog}.rs`,
  `crates/goose-cli/src/{theme,keybindings}.rs`,
  `crates/goose-cli/src/commands/{budget,cost_ledger,cost,git_helper,presets,recipes_library,audit,privacy,review_cmd}.rs`,
  `crates/goose-providers/src/{api_client,retry}.rs`
- apply-patch / sandbox crates: `crates/bharatcode-apply-patch/`, `crates/bharatcode-linux-sandbox/`
- Built-in skills: `crates/goose/src/skills/builtins/` (incl. `ultracode.md`)
