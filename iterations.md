# BharatCode (Rust) — fork of Goose, donor Codex

**Goal:** Build **BharatCode**, an Indian, local-first, **Apache-2.0-compliant** Rust
terminal AI coding agent, by **forking `block/goose`** (Apache-2.0) as the base and
**porting the best of `openai/codex`** (Apache-2.0) as a donor. Do it **compliantly
from commit 1** (the opposite of the earlier crush/FSL mistake): keep upstream
licenses, attribute both projects, state our changes, strip only trademarks.

## Why this base (decided, with evidence)
- **Local-first** is built into Goose: `crates/goose/src/providers/{ollama.rs,
  openai_compatible.rs}` + ~30 declarative providers + an in-process engine
  (`local_inference/`: llama.cpp/candle/mlx). Codex is hard-bound to the OpenAI
  Responses API (Chat Completions removed) → local-first would be weeks of surgery.
- **Rebrand surface** is far smaller on Goose; it even ships an official white-label
  guide (`CUSTOM_DISTROS.md`). Codex = 92 crates `codex_*` + ~1,012 `CODEX_*` refs.
- **Port FROM Codex** (its best, as isolated crates behind adapters): `apply-patch`
  streaming file editor; multi-OS exec sandbox (landlock/seccomp + seatbelt) +
  Starlark `execpolicy`; selective Ratatui TUI / MCP polish. (Avoid Codex `bwrap` →
  pulls LGPL bubblewrap.)

## License / compliance (Apache-2.0, both upstreams)
- Keep `LICENSE` (Apache-2.0); add `LICENSES/LICENSE-goose` (© Block, Inc.) and
  `LICENSES/LICENSE-codex` (© OpenAI) when Codex code is ported.
- `NOTICE` (mandatory — Codex ships one): BharatCode line + **add** Block attribution
  (Goose ships no NOTICE) + propagate Codex's NOTICE (OpenAI + Ratatui) + a §4(b)
  "we modified these files" statement. Also `MODIFICATIONS.md`.
- **Keep upstream git provenance** (do NOT wipe `.git` — that was the crush mistake).
- **Strip trademarks only** (Goose/Block/Square/CashApp/Tidal, Codex/OpenAI/ChatGPT)
  from user-facing surfaces; enforce with a CI grep gate over source AND binary.
- **Disable PostHog telemetry** by default (`crates/goose/src/posthog.rs` phones home
  to Block).

## Environment / build
- Goose workspace: edition 2021, toolchain pin **1.92** (rust-toolchain.toml). Host
  has rustup + Rust 1.96; cargo honors the pin (auto-fetches 1.92).
- CLI crate: `crates/goose-cli` → binary **`goose`** (also `goosed` in goose-server).
- **Light build** (v0, no native engines): `cargo build -p goose-cli
  --no-default-features --features portable-default` (drops local-inference / code-mode
  / mlx / nostr / system-keyring). Wrapper: `/home/arbaz/cargobc.sh`.
- Branding surface: `GOOSE_` env in ~90 files; `goose` ~4,224 lines; config `app_name
  = "goose"` (`crates/goose/src/config/paths.rs`); identity "My name is Goose…Block".

## Rebrand strategy — SURGICAL (build-safe)
Rust crate names (`goose`, `goose-cli`, …) are identifiers used in path-deps and
`use goose::` — a blanket rename would break the build. So v0 rebrands the
**user-facing surface only**, leaving internal crate/symbol names (full crate rename
is a later polish phase):
- binary names (`goose`→`bharatcode`, `goosed`→`bharatcoded`),
- config `app_name`/dir (`goose`→`bharatcode`),
- env var **string literals** (`GOOSE_`→`BHARATCODE_`),
- brand/identity/help/about display strings, ASCII/logo, "Block"/"Goose" marks,
- telemetry default off.

## Definition of DONE (this session's milestone, v1)
1. ✅ Builds: `cargo build -p goose-cli --features portable-default` (binary `bharatcode`).
2. ✅ Runs: `bharatcode --help` / `--version` show BharatCode; **zero** Goose/Block in
   user-facing output.
3. ✅ Rebranded: binary, env, config dir/app_name, brand + identity strings, marks/logos.
4. ✅ Telemetry OFF by default (no PostHog call to Block).
5. ✅ Compliant: `LICENSE` + `LICENSES/LICENSE-goose` + `NOTICE` (Block attribution +
   §4(b)) + `MODIFICATIONS.md`; upstream git history preserved.
6. ✅ Trademark gate passes (grep over source + built binary, user-facing surfaces).

**Follow-on (post-v1, documented not blocking):** port Codex `apply-patch` + sandbox;
full local-first in-process engine defaults; full crate rename; TUI uplift; Indic
i18n + India model presets; release engineering + `THIRD_PARTY_LICENSES`.

## Worker streams
- **R — Recon** (dynamic workflow, parallel read-only): exact rebrand+compliance spec.
- **S — Scaffold+Rebrand** (single-writer): compliance files + surgical rebrand + telemetry-off.
- **B — Build & verify** (loop): light build green; runs; trademark gate; compliance present.

## Iteration log
(append-only; newest at bottom)

### 2026-06-20 — Iteration 0: setup
- Chose base = Goose (option A) after a verified design workflow (local-first +
  rebrand surface both favor Goose; Codex = donor).
- Cloned `block/goose` → `/home/arbaz/bharatcode-rs` (KEEP its `.git` = provenance),
  `openai/codex` → `/home/arbaz/codex` (donor). Rust 1.96 via rustup; cargo honors
  Goose's 1.92 pin.
- Confirmed `portable-default` feature set avoids the heavy native build.
- Kicked off the baseline light build (warms dep cache; confirms buildability).
- Next: dynamic recon workflow → surgical rebrand + compliance scaffold → build/verify loop.

### 2026-06-20 — Iteration 1: rebrand applied; build-break diagnosed + fixed
- Detached runner applied: compliance scaffold; env GOOSE_->BHARATCODE_ (86 vars);
  bin names; config app_name x2 (paths.rs + goose-mcp/lib.rs); telemetry hard-off +
  PostHog key neutralized; .goosehints->.bharatcodehints; display-string rebrand
  (27 rust files, string-literals only); prompt/identity markdown.
- BUILD FAILED (20 errors): the `config_value!` macro (base.rs:207) derives getter
  names from the env-key token via pastey::paste!, so renaming GOOSE_*->BHARATCODE_*
  made it generate `get_bharatcode_*` while ~26 call sites still called `get_goose_*`.
  Type names (GooseMode, GooseError, ...) are hand-written + consistent (left as-is).
- FIX: uniform rename of generated/hand-written getter prefixes
  get_goose_/set_goose_/with_goose_ -> *_bharatcode_* (added to runner Phase 2g).
- Re-running detached runner (idempotent) → rebuild + test.

### 2026-06-20 — Iteration 2: display-rebrand regex bug → reverted → Rust-aware lexer
- 2nd build FAILED (46 errors): the regex display-rebrand was NOT Rust-aware — a `'"'`
  char literal made it pair quotes across CODE, corrupting identifiers
  (`use goose::`→`use bharatcode::`, `GooseMode`→`BharatCodeMode`, call/def mismatch).
- Recovery: `git checkout -- crates/` → pristine goose restored (structural rebrand
  steps in Phase 2 are build-safe and kept in the runner).
- Built `/home/arbaz/rebrand_strings.py`: a Rust-aware string lexer that rebrands
  ONLY inside real string literals (skips char literals incl `'"'`, line/block
  comments, lifetimes, and is raw-string-safe). Self-tested 6/6 incl. the bug case.
- Runner Phase 3 rewritten to use the lexer (goose-cli display) + prompt md + a
  phrase-targeted identity de-brand (AAIF/Block creator attributions; "Block" by
  phrase only to keep ContentBlock / "Blocked malicious package").
- Re-running detached runner from pristine → build + test.

### 2026-06-20 — Iteration 3: lexer broke inline format captures → made it brace-aware
- 3rd build FAILED (1+ errors): the lexer rebranded inside `{..}` format placeholders,
  so `format!("{goose_mode}")` -> `{bharatcode_mode}` referenced a non-existent var;
  term.rs had ~10 `{goose_bin}` shell-alias captures broken too.
- FIX: lexer now brands only OUTSIDE `{..}` placeholders (keeps inline captures; `{{`/`}}`
  escapes handled). Self-tested 5/5 incl. format-capture + char-literal + alias cases.
- Also identified CORE semantic strings to PROTECT (do NOT blind-rebrand): provider_id
  "goose", KEYRING_SERVICE, ACP metadata key "goose", x-client header, hardcoded
  `.config/goose` path joins. Core display leaks to fix surgically: 4 (checks "goose
  review", acp "gooseThinkingEffort", kimicode temp file, nostr "Goose Nostr").
- Revert to pristine + re-run runner with brace-aware lexer.

### 2026-06-20 — Iteration 4: BUILD GREEN ✅ (brace-aware lexer worked)
- `cargo build -p goose-cli --features portable-default` → BUILD OK; binary
  `target/debug/bharatcode` (`--version` 1.38.0). `--help` trademark gate: CLEAN.
- CLI lib tests: 230 passed, 4 failed — all rebrand-related TEST FIXTURES:
  test_what_is_your_name (recording says "My name is goose, created by Block"),
  term nushell hooks script test, 2 recipe-extract tests. To fix as test data.
- Launching dynamic AUDIT workflow (parallel): CLI surfaces ∥ binary strings ∥
  core display leaks ∥ compliance → consolidated leak/fix list.

### 2026-06-20 — Iteration 5: dynamic AUDIT workflow → dynamic FIX workflow
- Audit workflow (4 parallel auditors) found ZERO binary company-mark leaks and
  PASSING compliance, but ~47+ user-facing Goose/goose leaks: clap `///` doc
  comments (lexer skips comments) in cli.rs (review/term help), and CORE display
  strings in crates/goose/src (the lexer had only run on goose-cli). Pattern:
  "goose configure" / "goose run" / "Use goose with" / "Goose (Default)" /
  "You are Goose" across providers/*_acp.rs, acp/*, otel, doctor, platform_extensions.
  Plus data files: goose_doc_guide skill + goose-self-test.yaml recipe; latent
  block.xyz author in Cargo.toml.
- FIX workflow launched (parallel, disjoint): (1) lexer over crates/goose{,-mcp,
  -providers}/src core strings; (2) doc-comment rebrand in goose-cli/src; (3) data
  files + Cargo author. Then sequential build+test (fix rebrand-broken fixtures) →
  verify surfaces clean.
- Note: lexer on core will also rename a few SEMANTIC strings (provider_id "goose",
  KEYRING_SERVICE, ACP gooseThinkingEffort, x-client) consistently to bharatcode —
  acceptable for a new local-first product; flagged for review.

### 2026-06-20 — Iteration 6: ✅ DONE (v1 milestone met)
FIX workflow result: BUILD OK; **234 passed / 0 failed** tests. It fixed the
rebrand-broken fixtures correctly (term `/tmp/goose`→`/tmp/bharatcode`; 5 VCR
`what_is_your_name` recordings rebranded; and smartly REVERTED `goose_provider`/
`goose_model` serde keys that must stay internal). Verify phase: clean=true across
ALL surfaces. Independent spot-check confirmed:

| DONE criterion | Result |
|---|---|
| Builds + tests | ✅ `cargo build` OK; **234/234** cli tests pass |
| Runs; zero user-facing Goose/Block leak | ✅ 0 across --help, all 20 subcmds + nested, review/term/configure/doctor, skills/recipe, info -v, completion |
| Rebranded (binary/env/config/identity/marks) | ✅ binary `bharatcode`; `~/.config/bharatcode`; `BHARATCODE_*`; identity = BharatCode |
| Telemetry OFF | ✅ `is_telemetry_enabled`→false; `POSTHOG_API_KEY=""` |
| Apache-2.0 compliant | ✅ LICENSE + LICENSES/LICENSE-goose + NOTICE + MODIFICATIONS; upstream git provenance kept |
| Trademark gate | ✅ clean (the only `block` matches are the USER's external ~/.agents HyperFrames skills, "registry block" — not BharatCode) |

**v1 DONE.** BharatCode is a building, running, fully-rebranded, telemetry-free,
Apache-2.0-compliant Rust terminal AI coding agent forked from Goose, with both
upstreams attributed.

**Follow-on (post-v1, not blocking):** port Codex `apply-patch` + exec sandbox;
local-first default provider (Ollama/in-process) + onboarding; full internal crate
rename (goose-* → bharatcode-*); Indic/Hindi i18n + India model presets; release
engineering (THIRD_PARTY_LICENSES, installers, Dockerfile); LICENSES/LICENSE-codex
+ Codex NOTICE when Codex code is ported.

### 2026-06-20 — v2 ✅ Local-first defaults (dynamic workflow)
- get_active_provider() now defaults to Ollama ("ollama") and get_active_model() to
  qwen3 when nothing is configured (providers.rs:13/111/140); env/config/cloud still
  take precedence (opt-in). First-run onboarding (configure.rs) offers "Local model —
  Ollama (Recommended)"; provider picker preselects Ollama. No forced cloud sign-in.
- Build OK; 234/234 cli tests pass (incl. updated provider-default tests). Binary rebuilt.
- Chaining v3 (Codex apply-patch port).

### 2026-06-20 — v3 ✅ Codex apply-patch port (dynamic workflow)
- Discovered Codex's apply-patch is no longer self-contained (needs codex-exec-server),
  so vendored the PURE layer: new crate crates/bharatcode-apply-patch (edition 2024,
  dep: thiserror only) = seek_sequence + streaming_parser (verbatim) + parser (adapted,
  dropped PathUri) + new std::fs apply.rs. Wired as `apply_patch` tool in goose's
  DeveloperClient platform extension (developer/apply_patch.rs + mod.rs).
- COMPLIANCE: LICENSES/LICENSE-codex (Apache-2.0, © OpenAI); NOTICE Codex section;
  MODIFICATIONS.md line. Both upstreams (Goose + Codex) now attributed.
- Build OK; bharatcode-apply-patch 27/0 tests; goose-cli 234/0. cargo fmt clean.
- Chaining v4 (Codex exec sandbox).

### 2026-06-20 — v4 ✅ Codex exec sandbox (landlock + seccomp, behind an exec trait)
- Vendored the PURE in-process Linux sandbox primitives from Codex
  linux-sandbox/src/landlock.rs into a new crate crates/bharatcode-linux-sandbox
  (edition 2021; deps: thiserror all-targets + landlock 0.4.x/seccompiler 0.5/libc
  Linux-only): set_no_new_privs (PR_SET_NO_NEW_PRIVS), Landlock ABI::V5 FS ruleset
  (read-all of /, write to /dev/null + writable_roots), and the Restricted-mode
  network seccomp filter (denies ptrace/process_vm_*/io_uring + connect/bind/listen/
  send*/sockopt and socket()/socketpair() for every family except AF_UNIX). Public
  surface = plain SandboxPolicy { writable_roots, allow_network } + thiserror
  SandboxError; codex_protocol/AbsolutePathBuf dropped. ProxyRouted mode, bubblewrap,
  launcher, seatbelt/windows dropped; unsupported-arch panic → SandboxError::UnsupportedArch;
  no-op stub on non-Linux.
- EXEC-TRAIT SEAM: subprocess.rs gains `SandboxExt::apply_sandbox(&SandboxPolicy)` for
  tokio::process::Command + std::process::Command, registering a cfg(linux) pre_exec
  hook (mirrors configure_parent_death_signal).
- WIRING: developer/shell.rs build_shell_command reads opt-in BHARATCODE_SANDBOX
  (off|read-only|workspace-write, DEFAULT OFF); when on and not flatpak, builds a
  SandboxPolicy (workspace-write → working_dir writable + network allowed; read-only →
  no writable roots + network denied) and calls apply_sandbox before spawn.
- COMPLIANCE: NOTICE Codex section + MODIFICATIONS.md extended for the linux-sandbox
  port; LICENSES/LICENSE-codex reused.
- Default-OFF keeps invariants green: goose-cli portable-default build OK; goose-cli
  234/234 lib tests; bharatcode-linux-sandbox 3/3 tests. cargo fmt clean.
- Remainder (documented, out of scope): bubblewrap FS isolation, proxy-routed network,
  macOS seatbelt / Windows backends; precomputing the BPF program before fork (the
  filter is currently built inside pre_exec, matching Codex).

### 2026-06-20 — v4 ✅ Codex exec sandbox (dynamic workflow)
- New crate crates/bharatcode-linux-sandbox (landlock 0.4 + seccompiler, edition 2021):
  ports Codex's pure landlock/seccomp primitives (apply_to_current_thread; SandboxPolicy
  {writable_roots, allow_network}); non-linux = no-op; avoids bwrap/LGPL. 3/3 crate tests.
- Exec-trait seam in goose/src/subprocess.rs (apply_sandbox via pre_exec, mirrors
  configure_parent_death_signal); shell.rs reads opt-in BHARATCODE_SANDBOX (default OFF,
  so portable-default build/behavior unchanged). NOTICE + MODIFICATIONS extended (Codex).
- Build OK; goose-cli 234/234; sandbox default off. (Note: version engine's `args`
  channel didn't bind — agents recovered via versions.md; baking spec inline for v5+.)
- Chaining v5 (Release engineering).

### 2026-06-20 — v5 ✅ Release engineering (THIRD_PARTY_LICENSES + packaging rebrand)
- DELIVERABLE A — THIRD_PARTY_LICENSES.md (NEW, repo root, zero build impact): cargo-about
  not installed/offline, so used the reliable fallback — scoped the crate set to the actual
  release build via `cargo tree -e no-dev -p goose-cli --no-default-features --features
  portable-default`, joined name+version against `cargo metadata --format-version 1`
  license/repository fields (Python generator), excluded the 13 first-party workspace crates,
  deduped, grouped by SPDX expression. Result: 619 third-party crates across 41 SPDX license
  expressions (MIT OR Apache-2.0 dominates at 262); ALL carry an SPDX license field (zero
  UNKNOWN). Header states BharatCode is Apache-2.0 + derivative of Goose, attribution in
  NOTICE/MODIFICATIONS. Code-mode/desktop/v8 deps correctly excluded (not in portable-default).
- DELIVERABLE B — packaging rebrand (sed/targeted, NOT the Rust lexer; converge on the names
  the running self-updater update.rs already expects):
  - Dockerfile: COPY/ENTRYPOINT/useradd/HOME/WORKDIR + LABELs → bharatcode; FIXED broken
    binary path (release/goose → release/bharatcode); source LABEL → aaif-bharatcode/bharatcode;
    vendor=BharatCode. Kept `--package goose-cli` (internal crate).
  - download_cli.sh + download_cli.ps1: REPO=aaif-bharatcode/bharatcode; OUT_FILE=bharatcode[.exe];
    GOOSE_*→BHARATCODE_* env vars; asset filenames bharatcode-<triple>.tar.bz2/.zip (EXACT match
    to update.rs asset_name()); bharatcode-package subdir (matches update.rs:408); /tmp temp dir,
    chmod/mv basenames, and all user-facing messages. `bash -n` clean.
  - .github/workflows/build-cli.yml (CRITICAL): `--bin goose`→`--bin bharatcode` (musl release),
    goose.exe checks, goose-package→bharatcode-package, tar/zip + ARTIFACT_* + upload-artifact
    name → bharatcode-${TARGET}. Kept `-p goose-cli`.
  - release.yml/canary.yml: CLI artifact globs goose-*→bharatcode-* (kept Goose*.zip desktop
    glob = remainder); publish-docker.yml: ghcr image + subject-name → .../bharatcode.
  - Justfile: copy-binary/copy-binary-intel/copy-binary-windows/release-windows binary basenames
    goosed→bharatcoded, goose→bharatcode (were BROKEN); run-server `--bin goosed`→`--bin bharatcoded`;
    FIXED `GOOSE_RECORD_MCP=1`→`BHARATCODE_RECORD_MCP=1` (test reads BHARATCODE_); product-name
    echoes. Kept all `-p goose-*` crate refs + Goose.app desktop path (remainder).
  - Docs: RELEASE.md/RELEASE_CHECKLIST.md (product/config-dir ~/.config/bharatcode/deeplink
    bharatcode://); CUSTOM_DISTROS.md (TARGETED line-addressed sed: product name, GOOSE_PROVIDER/
    MODEL/DISABLE_KEYRING/TELEMETRY→BHARATCODE_, config dir, repo clone, trademark wording rewrite
    to keep upstream Goose/Block marks with their owners). KEPT crate paths crates/goose…, the
    architecture-diagram boxes, the literal recipe-schema keys `goose_provider`/`goose_model`
    (mod.rs:101/104), and `GOOSE_BUNDLE_NAME` (read by un-rebranded desktop).
  - Build-safe metadata: workspace Cargo.toml repository + deny.toml issue-URL → aaif-bharatcode.
- GUARD (invariants proven, not assumed): cargo fmt = no-op (tree already clean, --check passes);
  `cargo build -p goose-cli --no-default-features --features portable-default` GREEN; goose-cli
  234/234 lib tests; all 4 touched workflows yaml-parse OK; grep gate over packaging files =
  zero residual aaif-goose / block/goose / ghcr goose / user-facing goose.
- REMAINDER (documented, tied to un-rebranded ui/desktop + internal CI; out of v5 core):
  desktop bundle naming (Goose*.zip / Goose.app / Goose-darwin-* / GOOSE_BUNDLE_NAME), publish-npm
  + bundle-desktop*.yml, the CI-bot workflows goose-{issue-solver,pr-reviewer,release-notes}.yml /
  code-review.yml / test-finder.yml (pull ghcr.io/aaif-goose/goose image), and goose-docs.ai in
  two .github issue/discussion templates. Crate rename goose-*→bharatcode-* stays v8.
- Chaining v6 (Indic i18n scaffold).

### 2026-06-20 — v5 ✅ Release engineering (dynamic workflow)
- THIRD_PARTY_LICENSES.md (NEW, 889 lines): aggregated from the actual portable-default
  dep tree (`cargo tree` + `cargo metadata` license/repo fields), grouped by SPDX.
- Packaging rebranded to bharatcode (binary path fixes too): Dockerfile, download_cli.sh
  (84 bharatcode refs, 0 goose), download_cli.ps1, Justfile copy-binary, and the main CI
  release workflows (build-cli/release/canary/publish-docker artifact names + --bin).
- Build OK (6m04s); 234/234; leak-free. Documented remainder (partial): desktop/CI-bot
  workflows tied to the un-rebranded ui/desktop (deferred). The one Dockerfile `goose`
  ref is the intentional `cargo -p goose-cli` crate name (renamed at v8).
- Chaining v6 (Indic i18n scaffold).

### 2026-06-20 — v6 ✅ Indic i18n scaffold (dynamic workflow)
- New crates/goose-cli/src/i18n/ (mod.rs + en.json + hi.json), std-only (LazyLock maps).
  Locale resolver: BHARATCODE_LANG -> config bharatcode_lang -> LANG -> "en". `tr!(key)`
  macro + t() with English fallback. Wired a starter set of high-traffic strings (ready
  banner, "No provider/model configured", configure welcome) through tr!; Hindi table for them.
- Original work (not a Codex port → no extra license files). English output byte-identical.
- Build OK; 239/239 lib tests (5 new i18n tests); leak-free.
- Chaining v7 (India model presets).

### 2026-06-20 — v7 ✅ India model presets (dynamic workflow)
- New declarative providers: sarvam.json (Sarvam AI), krutrim.json (Ola Krutrim) — India-
  hosted openai-compatible, auto-registered. New presets module + configure hook offering
  "Recommended (India / open-weight)": local Ollama (qwen2.5-coder, deepseek-coder) +
  hosted (Sarvam, Krutrim, Qwen/DeepSeek). i18n keys added. Build OK; tests green; leak-free.
- v8 (internal crate rename) DEFERRED to late (high-churn/internal); proceeding with feature batch.
- Switching to PARALLEL BATCH engine: firing v8-v14 (ledger, budget? no—execpolicy, slash,
  verify-before-done, residency, doctor) implemented in parallel + single integrate.

### 2026-06-20 — v8–v14 ✅ PARALLEL BATCH integrated (green build)
- Integration: `cargo fmt` clean; `cargo check`/`cargo build -p goose-cli --no-default-features --features portable-default` OK; 242/242 lib tests pass; no Goose/Block user-facing leak (--help, `cost`, doctor surfaces clean).
- v8 ✅ INR cost ledger — new commands/cost_ledger.rs + cost.rs, `bharatcode cost` subcommand wired in cli.rs/mod.rs, ₹ footer in session/output.rs. Compiled & integrated; done.
- v10 ✅ Codex-style exec policy (opt-in, default-off) — new goose/src/exec_policy.rs, gate in developer/shell.rs, pub mod in lib.rs. Compiled & integrated; done.
- v11 ✅ Slash-command & /help polish — grouped help + help_tr fallback in session/input.rs. Compiled & integrated; done.
- v12 ✅ Verify-before-done — new goose/src/verify.rs (#[path] mod in agent.rs), guarded finalization call. Compiled & integrated; done.
- v13 ✅ Data-residency guard (opt-in) — new goose/src/residency.rs, guard_endpoint call in providers/openai.rs, pub mod in lib.rs. Compiled & integrated; done.
- v14 ✅ Doctor enhancements — read-only BharatCode settings summary in commands/doctor.rs. Compiled & integrated; done.

### 2026-06-20 — BATCH v8,v10-v14 ✅ (PARALLEL workflow, ~16 min for 6 versions)
- v8 INR cost ledger: cost_ledger.rs + `bharatcode cost` (₹ summary, USD->INR via
  BHARATCODE_USD_INR), compact ₹ in session footer.
- v10 execpolicy: clean-room opt-in command allow/deny gate (BHARATCODE_EXEC_POLICY, default off).
- v11 slash/help polish: grouped interactive /help, i18n-ready.
- v12 verify-before-done: verify.rs runs project test/build, emits Verified/Failed/Skipped
  (BHARATCODE_VERIFY, default off).
- v13 data-residency guard: residency.rs egress allowlist (BHARATCODE_RESIDENCY, default off).
- v14 doctor enhancements: richer settings summary.
- Integrate: clean first compile; build OK; 242/0 lib tests; leak-free. (Numbering is
  execution-sequential; maps to roadmap content, not 1:1 with versions.md rows.)
- Firing batch 2 (v15-v20): budget gate, approval modes, recipe library, cost-aware routing,
  MCP polish, offline/privacy mode.

### 2026-06-20 — v15–v20 ✅ PARALLEL BATCH integrated (green build)
- Integration: `cargo fmt` clean; `cargo check`/`cargo build -p goose-cli --no-default-features --features portable-default` OK (first compile clean, no fixups needed); 250/250 lib tests pass (+8 new); no Goose/Block user-facing leak (--help, recipes-library surfaces clean; only internal GooseMode/GooseClient type names + BudgetAction::Block enum retained per invariant).
- v15 ✅ Budget gate (₹ INR spend cap, default OFF via BHARATCODE_BUDGET_INR) — new commands/budget.rs (reuses cost_ledger), session/mod.rs per-turn guard, mod.rs pub mod, en/hi i18n keys. Compiled & integrated; done.
- v16 ✅ Approval modes refinement — new goose/src/permission/approval_mode.rs (ApprovalMode<->GooseMode, BHARATCODE_APPROVAL opt-in, resolve_mode no-op when unset), permission/mod.rs re-exports. Compiled & integrated; done.
- v17 ✅ India recipe library — new commands/recipes_library.rs + 4 embedded YAML templates, `bharatcode recipes-library [--show <id>]` wired in cli.rs/mod.rs, en/hi i18n keys. Compiled & integrated; done.
- v18 ✅ Cost-aware model routing (opt-in, default OFF via BHARATCODE_COST_ROUTING) — new goose/src/cost_routing.rs, lib.rs pub mod, model_config.rs with_configured_fast_model hook. Compiled & integrated; done.
- v19 ✅ MCP UX polish — additive in goose/src/agents/mcp_client.rs (connection_status, describe_service_error, summarize_tools/resources/prompts + tests). Compiled & integrated; done.
- v20 ✅ Offline / no-egress mode (single BHARATCODE_OFFLINE switch, default OFF) — new goose/src/offline.rs (composes residency/telemetry/local-host guard), lib.rs pub mod. Compiled & integrated; done.

### 2026-06-20 — BATCH v15-v20 ✅ (PARALLEL, clean first compile, 250/0 tests)
- v15 budget gate (BHARATCODE_BUDGET_INR + warn|deny, ₹, default off, reuses ledger).
- v16 approval modes (BHARATCODE_APPROVAL: chat/ask/auto/full+yolo, safe default).
- v17 India recipe library (`bharatcode recipes-library`: UPI review, Aadhaar/PII+DPDP
  audit, GST helper, Indic localization — 4 templates + --show).
- v18 cost-aware model routing (BHARATCODE_COST_ROUTING, prefers cheap/local, default off).
- v19 MCP UX polish (clearer client errors/status).
- v20 offline/no-egress mode (BHARATCODE_OFFLINE composes local-only+residency+telemetry-off).
- Build OK; 250/0; leak-free. 20 versions delivered. Firing batch 3 (v21-v26).

### 2026-06-20 — v21–v26 ✅ PARALLEL BATCH integrated (green build)
- Integration: `cargo fmt` clean; `cargo check` + `cargo build -p goose-cli --no-default-features --features portable-default` OK (first compile clean, no fixups needed); 260/260 lib tests pass (+10 new: v21 theme 6, v25 keybindings 4); no Goose/Block user-facing leak (--help + `git --help`/`git` surfaces clean; the only "goose" strings in `git` output are real working-tree filenames/paths the read-only helper reports, not hardcoded UI). New-file grep for user-facing goose/Block = none.
- v21 ✅ CLI theming (Tiranga) — new goose-cli/src/theme.rs (DEFAULT/TIRANGA/NONE Style sets, NO_COLOR + BHARATCODE_THEME resolution, unset = byte-identical default), lib.rs pub mod. 6 tests. Compiled & integrated; done.
- v22 ✅ `bharatcode git` read-only repo summary — new commands/git_helper.rs (branch/commit/status/ahead-behind/recent commits via read-only git subcommands), cli.rs Command::Git + dispatch + get_command_name, commands/mod.rs pub mod. Compiled & integrated; done.
- v23 ✅ Opt-in on-disk prompt cache (default OFF via BHARATCODE_CACHE) — new goose/src/prompt_cache.rs (SHA-256 key, JSON store, cached_complete hook), lib.rs pub mod, githubcopilot.rs request-boundary wrap. 4 tests. Compiled & integrated; done.
- v24 ✅ Retry/backoff hardening — new goose/src/retry_policy.rs (exp backoff + jitter, BHARATCODE_RETRY_* env, clamped defaults, retry_async wrapper), lib.rs pub mod, nanogpt.rs fetch_supported_models wired. 8 tests. Compiled & integrated; done.
- v25 ✅ Customizable interactive keybindings — new goose-cli/src/keybindings.rs (Keybindings::default reproduces built-ins, from_config honors legacy BHARATCODE_CLI_NEWLINE_KEY + BHARATCODE_KEYS, tolerant parse_key), lib.rs pub mod, session/builder.rs config read. 4 tests. Compiled & integrated; done.
- v26 ✅ Codebase context (RAG-lite, default OFF via BHARATCODE_CODEBASE_CONTEXT) — new goose/src/codebase_context.rs (bounded gitignore-respecting scanner, compact layout/manifest/README blob), lib.rs pub mod. 7 tests. Compiled & integrated; done.
- Build OK; 260/0; leak-free. 26 versions delivered.

### 2026-06-20 — BATCH v21-v26 ✅ (PARALLEL, clean first compile, 260/0)
- v21 Tiranga CLI theming (BHARATCODE_THEME; NO_COLOR respected). v22 `bharatcode git`
  read-only repo summary. v23 prompt/response cache (BHARATCODE_CACHE, default off).
  v24 retry/backoff policy (BHARATCODE_RETRY_*). v25 keybindings config. v26 codebase
  RAG-lite scanner (BHARATCODE_CODEBASE_CONTEXT, default off).
- Build OK; 260/0; leak-free. 26 versions delivered.
- NEXT: adversarial REVIEW pass (parallel critics) over v8-v26 — flagged risk: some
  features (cache/retry/cost-routing) wired into a single provider, may be dead for others.

### 2026-06-20 — WIRING/BUG-FIX INTEGRATION ✅ (dead features now fire for ALL providers)
- Resolves the flagged risk above: cache/retry/cost-routing/residency were single-site
  (dead for most providers). Re-wired at central choke points + fixed safety bugs.
- Integration gate: `cargo fmt` clean; `cargo check` + `cargo build -p goose-cli
  --no-default-features --features portable-default` OK (clean first compile, NO fixups
  needed — every changed file compiled as-is). `bharatcode --help` leak-free; binary 1.38.0.
- Tests: goose-cli `--lib` 261/0 pass. goose `--lib` with `--features rustls-tls`
  (the meaningful run; portable-default pulls rustls-tls) = 1564 pass / 9 fail — ALL 9 are
  PRE-EXISTING rebrand artifacts, none caused by this integration:
  prompt_manager insta snapshots (system prompt now says "bharatcode", .snap not regenerated),
  toolshim test (input literal still "goose-fork" vs expected "bharatcode-fork"),
  session-backed tests (stale dev sessions.db still has `goose_mode` column vs renamed
  `bharatcode_mode` read — migration gap; fresh/CI DB unaffected), 2 acp + 1 skills env tests.
  NOTE: the bare `cargo test -p goose --lib` from the prompt is itself broken in this repo
  (goose `default = []` enables no sqlx/jsonwebtoken runtime feature) → 120 spurious
  "runtime-tokio feature must be enabled" panics; only meaningful under a TLS feature.
- (1) RESIDENCY+OFFLINE egress guard: now central in goose-providers/api_client.rs
  `send_request` → `screen_endpoint` (hook installed by goose::offline::install_egress_guard
  → enforce_egress_policy → residency::guard_endpoint_with_mode), registered at
  init_registry(). Fires for EVERY provider (incl. declarative sarvam/krutrim); redundant
  openai.rs single-site call removed.
- (2) PROMPT CACHE: wired into the streaming path (reply_parts.rs stream_response_from_provider)
  the agent loop actually uses — HIT short-circuits to zero-cost stream, MISS tees+stores on
  completion. cached_complete HIT cost bug fixed (zeroes usage). No-op unless BHARATCODE_CACHE.
- (3) COST ROUTING: route_lead_model now applied to the LEAD model in both central
  model_config constructors (was fast-model only). Default off = unchanged.
- (4) RETRY: BHARATCODE_RETRY_* now applied centrally in ProviderRetry::with_retry via
  RetryConfig::with_env_overrides() (all providers honor it); dead parallel retry_policy.rs
  stack deleted (pub mod removed, no dangling refs — build confirms).
- BUG FIXES verified firing: approval_mode "never"/"untrusted"/"read-only" now → Ask (was the
  inverted-safety Full/yolo); exec_policy splitter hardened ($(...)/backtick/subshell/group);
  computercontroller automation_script gated via self-contained exec_policy_gate; cost_ledger
  day/month buckets now IST (+05:30); doctor.rs reports REAL keys (RESIDENCY_MODE_KEY,
  BUDGET_INR_KEY, usd_inr_rate(), offline) — phantom keys gone; theme applied at doctor/cost/
  git_helper sites; i18n cost.* keys 29/29 en↔hi parity. Leak fixes landed (agent.rs
  ~/.config/bharatcode/adversary.md, mcp_client.rs doc "BharatCode").
- Build OK; goose-cli 261/0; goose 1564/9 (9 pre-existing rebrand, 0 integration regressions);
  leak-free (--help + integration-added code). Residual brand strings in touched files are
  pre-existing internal Rust identifiers (GooseClient/GoosePlatform/goose.external_dispatch/
  goose_mode col) + one stale computercontroller cache-path comment — out of scope, not user-facing.

### 2026-06-20 — 9-TEST FIX INTEGRATION ✅ (goose suite now 1573/0)
- Integrated the fixes for the 9 pre-existing rebrand-artifact failures from the prior batch.
  `cargo fmt` clean; **goose `--lib --features rustls-tls` = 1573 passed / 0 failed** (was
  1564/9); **goose-cli `--no-default-features --features portable-default --lib` = 261/0**
  (held green). `bharatcode --help` runs, BharatCode branding, zero user-facing Goose/Block.
- Mix of CODE-FIXES (real regressions) and FIXTURE/IDENTITY updates, all verified:
  - CODE: session_manager.rs — CURRENT_SCHEMA_VERSION 14→15 + migration arm 15 renames the
    on-disk `goose_mode`→`bharatcode_mode` column (pragma-guarded ALTER … RENAME COLUMN; no-op
    on fresh DBs, adds col if neither exists). Fixes prepare_tools_returns_sorted_tools_… which
    panicked at create_session on the renamed column. Live dev DB migrated to v15.
  - CODE: acp/server.rs — `ClientCapabilitiesMeta.goose` field got `#[serde(rename =
    "bharatcode", default)]` so the BharatCode client's customNotifications/mcpHostCapabilities
    are read from the `bharatcode` meta key (was silently dropped). Fixes
    test_goose_custom_notifications_capability_reads_client_meta.
  - FIXTURE: 4 prompt_manager insta snapshots regenerated to the rebranded "bharatcode" identity
    header (old goose/AAIF text + code_execution block removed — code_execution is
    `cfg(feature="code-mode")`-gated, not in rustls-tls/portable-default).
  - FIXTURE: skills/client.rs test writes SKILL.md to `.bharatcode/skills` (discovery dropped
    `.goose`); toolshim.rs windows-path test uses `BharatCode-fork` (input+expected consistent;
    `\B` is not a JSON escape so the sanitizer is preserved — avoids the pre-existing `\b`
    JSON-recovery corruption flagged as out-of-scope); onboarding.rs model-only import asserts
    no `active_provider` persisted (local-first default makes get_bharatcode_provider() no longer
    Err on empty config — intentional, not reverted).
- LEAK GATE: grep of all 9 changed files for goose/Block → only internal Rust identifiers
  (GooseMode/GooseAcpAgent/GooseClientCapabilities, `goose_*` field/fn/test names, `goose_providers`
  crate paths), code comments, the English word "block" / ACP `ContentBlock` protocol type, and
  wire keys that already emit `"bharatcode"`. Snapshots say "bharatcode"; onboarding label is
  "BharatCode configuration". Zero user-facing leaks. leak_free = true.

### 2026-06-20 — QUALITY PASS ✅ (review → wiring/bug fixes → fix9): full suite GREEN
- Adversarial review found ~half of v8-v26 was dead/single-site or buggy. Fixed:
  * Central egress guard (residency+offline) in goose-providers/api_client.rs → screens
    ALL providers (was OpenAI-only). prompt_cache wired into the real stream path
    (reply_parts.rs) + ₹-on-hit bug fixed. Retry folded into central ProviderRetry
    (BHARATCODE_RETRY_*); deleted parallel retry_policy.rs. Cost-routing now routes the
    LEAD model. approval "never" inversion fixed (→ safe Ask) + regression test.
    exec_policy substitution/subshell bypass hardened + computercontroller gate. IST
    day/month bucketing. 2 doc-comment leaks (agent.rs, mcp_client.rs) fixed.
- CI gap fixed: was only testing goose-cli; now also `cargo test -p goose --lib
  --features rustls-tls` (the goose crate needs that feature for sqlx runtime).
- CRITICAL fix (fix9): rebrand renamed sessions DB column goose_mode→bharatcode_mode
  with NO migration → existing users' DBs would break. Added schema v14→v15 migration
  (idempotent RENAME COLUMN). Verified live DB migrated. Also fixed ACP _meta namespace
  regression; updated rebrand snapshot/fixture tests.
- RESULT: goose 1573/0 + goose-cli 261/0 = **1834 tests pass**; build green; 0 leaks;
  6/6 compliance files. 26 versions of real, now-actually-wired features. → release track.

### 2026-06-20 — PRE-RELEASE GATE ✅ (release binary built, RELEASE-READY)
- FINAL release gate run against the green tree. All five gates pass; release_ready = true.
- RELEASE BUILD: `cargo build -p goose-cli --release --no-default-features --features
  portable-default` → `Finished release in 10m46s`, exit 0. Binary:
  `target/release/bharatcode` (ELF x86-64, 146 MB). After the one leak fix below, an
  incremental rebuild (`Finished in 2m39s`, exit 0) reproduced the clean binary.
- FULL TESTS: `cargo test -p goose --lib --features rustls-tls` = **1573 passed / 0
  failed**; `cargo test -p goose-cli --no-default-features --features portable-default
  --lib` = **261 passed / 0 failed**. Total 1834/0.
- ALL-COMMAND SMOKE: `--version` (1.38.0) + `doctor` (exit 0, BharatCode branding) +
  `<cmd> --help` for all 23 subcommands present in the build (configure, info, doctor,
  session, run, mcp, acp, serve, project, projects, recipe, recipes-library, skills,
  plugin, schedule, gateway, term, tui, completion, cost, git, presets, review) — all
  exit 0. NOTE: the top-level `update` (self-update) command is `#[cfg(feature="update")]`
  -gated and `update` is intentionally NOT in `portable-default` (it pulls
  `sigstore-verify`), so it is by-design absent from the portable release binary.
- LEAK GATE: found + fixed ONE real user-facing leak — the embedded `load_tutorial`
  content `crates/goose-mcp/src/tutorial/tutorials/build-mcp-extension.md` (embedded via
  `include_dir!`, served to users) contained `goose session` / `goose run` command
  examples (matched `strings | grep "goose run"`). Rebranded all 11 `goose`→`bharatcode`
  refs; rebuilt; re-ran gate → `strings target/release/bharatcode | grep -iE "created by
  Block|Block's|goose configure|goose run"` now CLEAN, broader `goose <cmd>` scan CLEAN,
  and all help outputs CLEAN. (Out-of-binary: `crates/goose/acp-schema.json` has one
  "goose session" in a schema description, but it is a generated dev artifact, NOT
  embedded in the binary — confirmed absent from `strings`.)
- COMPLIANCE: LICENSE (Apache-2.0), LICENSES/LICENSE-goose (© Block), LICENSES/
  LICENSE-codex (© OpenAI), NOTICE (Goose + Codex + §4(b)), MODIFICATIONS.md,
  THIRD_PARTY_LICENSES.md (scoped to portable-default), README.md, CHANGELOG.md
  (BharatCode 0.1.0) — all present and accurate.
- release_ready = release_build_ok && full_tests_ok && all_commands_ok && leak_free &&
  compliance_ok = **TRUE**.

### 2026-06-20 — v27–v31 ✅ PARALLEL BATCH integrated (green build)
- v27 ✅ Web search tool — new goose/src/agents/platform_extensions/developer/web_search.rs (real reqwest search against DuckDuckGo HTML endpoint, egress-policy guarded, dep-free HTML parse, structured_content), wired into developer/mod.rs (mod/use/field/get_tools/call_tool + developer_tools_are_flat list). 6 tests. INTEGRATE FIX: raw-string literal `r#"…href="#"…"#` terminated early on the inner `"#`; switched the test fixture to `r##"…"##`. Compiled & integrated; done.
- v28 ✅ Persistent memory — new goose/src/memory_store.rs (JSON store under config dir, opt-in BHARATCODE_MEMORY, recall_for_prompt block capped to 25 facts), lib.rs pub mod, prompt_manager.rs build() injects "# Memory" block. 8 tests. Compiled & integrated; done.
- v29 ✅ DPDP audit log — new commands/audit.rs (append-only JSONL of model/tool turns, IST+UTC ts, tokens, ₹ cost, default OFF via BHARATCODE_AUDIT, viewer), commands/mod.rs pub mod, session/mod.rs per-turn write + end-of-session pointer. 4 tests. INTEGRATE FIX: is_enabled() read BHARATCODE_AUDIT via get_param::<String> which fails to deserialize the numeric `1` (config coerces to JSON number) → flag always OFF and round_trip_through_a_temp_log read 0/2 records; fixed by reading the raw env string first (is_truthy helper), mirroring v28. Compiled & integrated; done.
- v30 ✅ Code-review command — new commands/review_cmd.rs (`bharatcode review-diff` single-pass sibling of existing `review`; collects working git diff, runs one agent turn via shared build_session+headless path), commands/mod.rs pub mod, cli.rs ReviewDiff variant + help-name + dispatch, en/hi i18n keys. 3 tests. Compiled & integrated; done.
- v31 ✅ Context/token optimizer — new goose/src/context_optimizer.rs (opt-in BHARATCODE_CONTEXT_OPTIMIZE, relevance+recency message selection within budget, first+recent-tail anchored), lib.rs pub mod, context_mgmt/mod.rs do_compact() pre-selection hook. 7 tests. Compiled & integrated; done.
- Build OK (goose-cli --no-default-features --features portable-default). goose `--lib --features rustls-tls` = 1595/0. goose-cli `--lib --no-default-features --features portable-default` = 268/0 (after audit fix; was 267/1). leak-free (--help, review-diff --help, new-file string grep all CLEAN of goose/Block).

### 2026-06-20 — v32–v36 ✅ PARALLEL BATCH integrated (green build)
- v32 ✅ Model fallback chain — new goose/src/providers/fallback.rs (opt-in BHARATCODE_FALLBACK_MODELS, parse_fallback_chain + is_fallback_worthy classifier over all 12 ProviderError variants), providers/mod.rs `pub mod fallback;`, reply_parts.rs wires `stream_with_fallback(...)` into the real streaming path (call site line 454; wrapper line 117) so default-OFF = one attempt, worthy errors walk the chain. 7 tests. Evidence: `cargo test -p goose providers::fallback` = 7/0 (parse empty/bare/qualified/mixed-whitespace/empty-skip + worthy/unworthy classifier). Compiled & integrated; done.
- v33 ✅ Static model registry — new goose/src/model_registry.rs (20-model REGISTRY w/ cost+capability metadata, case-insensitive longest-prefix lookup, per-1K USD/INR helpers), lib.rs `pub mod model_registry;`, cost.rs wires candidate_models()+render_known_models() into `bharatcode cost` (call site lines 202-203, renders ₹/1K table for registry-known active+session models, unknown skipped). 7 tests. Evidence: `cargo test -p goose model_registry` = 7/0 (coverage, case/prefix lookup, longest-prefix, unknown→None, per-1K USD math, INR rate-scaling). Compiled & integrated; done.
- v34 ✅ Secret redaction guard — new goose/src/agents/platform_extensions/developer/redact.rs (opt-in BHARATCODE_REDACT, LazyLock regex set for AWS/GitHub/Slack/Google/Stripe/PEM/Bearer/api_key/env-style secrets → `[REDACTED]`, redact_counted), developer/mod.rs adds redact_shell_result + wires it into the real `"shell"` call_tool arm (call site line 270) over both text blocks and structured_content stdout/stderr. 10 tests. Evidence: `cargo test -p goose developer::redact` = 10/0 (each secret class masked, Bearer scheme + env key preserved, ordinary/short-low-entropy text byte-identical, is_enabled reflects env). Compiled & integrated; done.
- v35 ✅ `bharatcode privacy` — new commands/privacy.rs (PrivacyPosture::resolve reads real residency/offline/redact/audit/telemetry/provider keys via the same accessors the features use, 7-pillar ✓/✗ report + lockdown hint), commands/mod.rs pub mod, cli.rs Privacy variant + dispatch (line 2186) + help-name (line 1385), en/hi i18n +16 keys each. 5 tests. Evidence: `./target/debug/bharatcode privacy` renders all 7 pillars w/ source keys (live run confirmed); i18n parity 49/49 en=hi, no missing/extra. Compiled & integrated; done.
- v36 ✅ Doctor deep checks — new commands/doctor_checks.rs (best-effort local-provider TCP/HTTP probe, config-dir writable, git available, offline/residency coherence via pure offline_implies_strict, session-DB storage; run_all aggregator, Status::glyph), doctor.rs wires print_deep_checks().await on the real run path (line 12; spawn_blocking for reqwest::blocking), commands/mod.rs pub mod. 5 tests. Evidence: `./target/debug/bharatcode doctor` prints "Deep checks" w/ 5 ✓ rows (git 2.43.0, config dir, sessions DB 76.0 KB — live run confirmed). Compiled & integrated; done.
- Build OK (goose-cli --no-default-features --features portable-default). goose `--lib --features rustls-tls` = 1619/0. goose-cli `--lib --no-default-features --features portable-default` = 279/0. No INTEGRATE fixes needed — batch compiled clean on first cargo check. leak-free (--help + privacy --help + new-file string grep all CLEAN of user-facing goose/Block).

### 2026-06-20 — Ultracode skill (dynamic-workflow procedure) ✅
- Implemented the "Ultracode for Codex" skill (dev.to/pablonax) as a portable SKILL.md:
  Direct/Workflow/Delegated modes; plan→split→run→check→integrate→verify; disjoint parallel
  packets; evidence-backed (source, not vote) integration; `.workflow/ultracode/<slug>/`
  artifacts; "Verification still needed" instead of guessing.
- Shipped 3 ways: (1) BharatCode BUILTIN skill crates/goose/src/skills/builtins/ultracode.md
  (embedded via include_dir!, surfaces in `bharatcode skills list` as builtin://skills/ultracode);
  (2) ~/.codex/skills/ultracode/SKILL.md; (3) ~/.claude/skills/ultracode -> ~/.agents/skills/ultracode.
- Verified: rebuilt goose-cli (portable-default) clean; `bharatcode skills list` shows ultracode.

## v37 — Text embeddings client (default OFF) — done
- Feature fires: `goose::providers::EmbeddingClient` (re-exported in providers/mod.rs) is built by
  `EmbeddingClient::from_env()` gated on `BHARATCODE_EMBED_MODEL`; unset => `None` => zero behaviour change.
  `embed_texts()` routes through the shared `ApiClient::api_post` path so the residency/offline egress
  guard fires automatically. Real call site: re-exported public type reachable by RAG/codebase-context consumers.
- Evidence: providers::embeddings unit tests pass (OpenAI-compatible vector parse, Ollama-suffix endpoint,
  from_env None when key absent, empty-input short-circuit). 4/4 green.

## v38 — Multimodal vision preflight guard on read_image (default OFF) — done
- Feature fires: developer extension `call_tool` "read_image" arm now returns `Self::guard_image_result(result)`;
  guard is a no-op unless `BHARATCODE_VISION_GUARD` is truthy, then it prepends a low-priority advisory when the
  active model's v33 registry capabilities show vision=false. Real call site: the sole read_image result producer.
- Evidence: vision_guard unit tests pass (text-only model => advisory, vision model => None, unknown => None,
  is_enabled env toggle). 4/4 green.

## v39 — Offline model pack manifest `bharatcode model-pack` — done (fixed)
- Fix: `ModelPackEntry` fields changed `&'static str` -> `Cow<'static, str>` so the JSON round-trip test can
  Deserialize into owned strings while `static PACK` keeps cheap `Cow::Borrowed` literals (E0597 'static borrow).
- Feature fires: wired in goose-cli/src/cli.rs (mod decl, `Command::ModelPack`, get_command_name arm, dispatch to
  `model_pack::handle_model_pack`). Verified: `bharatcode model-pack --help` and `bharatcode model-pack` render the
  manifest (qwen2.5-coder ... with registry ctx windows), no network. cli lib tests + model_pack tests green.

## v40 — Active-model capability advisory in system prompt (default OFF) — done
- Feature fires: `SystemPromptBuilder::build()` inserts `bharatcode_model_caps` into system_prompt_extras when
  `provider_caps::capability_block()` returns Some — only when `BHARATCODE_MODEL_CAPS` is truthy AND the active
  model is in the v33 registry; otherwise None => byte-identical default prompt. Real call site: exercised prompt
  assembly path. Updated the stale `all_platform_extensions` snapshot to include the tracked `ultracode` builtin
  skill (no Goose reintroduced; only a builtin skill line + assertion-line metadata shift).
- Evidence: provider_caps unit tests pass (disabled=>None, enabled+gpt-4o=>caps block <500 chars with vision/tools,
  enabled+unknown=>None, is_truthy table). 4/4 green.

## Wave v41-v50 — integrated, GREEN (2026-06-20)

- v41 done: Subagent delegation profiles (tester/reviewer/refactorer), default OFF behind BHARATCODE_SUBAGENTS. Real call site: `subagent_profiles::{resolve, enabled, profile_to_task_config}` resolve into the live `agents::subagent_task_config::TaskConfig` used by the subagent runner; `pub mod subagent_profiles;` in lib.rs. Evidence: 5/5 unit tests green (resolve known/unknown, enabled-unset-false, max_turns carry, is_truthy).
- v42 done: Plan-mode plan-file persistence, default OFF behind BHARATCODE_PLAN_FILE. Real call site: inside `Session::handle_plan_mode` (session/mod.rs), gated `plan_file::is_enabled()` block saves the reasoner plan and prints the pointer. Integration fix: serialized the env-mutating tests through `env_lock::lock_env` to kill a BHARATCODE_PATH_ROOT cross-test race. Evidence: 4/4 plan_file tests green, stable across 3 reruns.
- v43 done: Lightweight BM25 codebase index injected into the system prompt, default OFF behind BHARATCODE_CODEBASE_INDEX. Real call site: `SystemPromptBuilder::build` (prompt_manager.rs) inserts the `bharatcode_codebase_index` extra via `codebase_index::relevant_files_block`. Evidence: 6/6 tests green (tokenizer, ranking, gitignore, disabled=>None, enabled lists files + asserts no goose leak).
- v44 done: `rename_symbol` developer tool (gitignore-respecting, word-boundary, dry-run default TRUE). Real call site: registered in DeveloperClient `get_tools()` and dispatched in `call_tool` ("rename_symbol" arm). Evidence: 4 refactor tests + `developer_tools_are_flat` green; tool present in the flat tool list.
- v45 done: `bharatcode gen-tests <path>` command. Real call site: `Command::GenTests` dispatch in cli.rs calls `gen_tests::handle_gen_tests` via the shared build_session/headless path; visible in `--help`. Evidence: 6/6 helper tests green; `bharatcode gen-tests --help` works.
- v46 done/FIXED: `bharatcode gen-docs <path> [--write]` command. Integration gap fixed: v46 declared the module in commands/mod.rs but the CLI dispatch (owned by v45/cli.rs) was missing, so the subcommand did not exist. Added the `GenDocs` enum variant, get_command_name arm, and `crate::commands::gen_docs::handle_gen_docs` dispatch in cli.rs. Real call site now reachable: `bharatcode gen-docs --help` works and the subcommand runs the shared headless path. Evidence: 5/5 gen_docs tests green; subcommand present in top-level `--help`.
- v47 done: Framework-migration advisory, default OFF behind BHARATCODE_MIGRATE=<from>:<to>. Real call site: `build_session` (session/builder.rs) calls `agent_ptr.extend_system_prompt("bharatcode_migration", migrate::advisory_block(&spec))` after extension load. Evidence: 7/7 migrate tests green (parse valid/malformed, unset-None, known+unknown advisory).
- v48 done/FIXED: Diff/patch-aware compaction, default OFF behind BHARATCODE_DIFF_COMPACT. Compile fix: cloned `msg.role` (Role is not Copy) in `select_diff_aware_owned` and dropped a needless `mut` on the `entry` closure. Real call site: `do_compact` (context_mgmt/mod.rs) runs `diff_compact::select_diff_aware_owned` when enabled. Evidence: 6/6 diff_compact tests green (looks_like_diff, summarize counts/hunks, identity-when-disabled).
- v49 done: Planner/sub-task model preset advisory (pure metadata + resolver), default-inert behind BHARATCODE_PLANNER_MODEL. Real call site: `pub mod planner_presets;` in providers/mod.rs exposes `resolve_planner`/`list_presets` as reachable public API for plan-mode/subagent consumers. Evidence: 5/5 tests green (presets well-formed, unique ids, inert-when-unset, provider/model pair + bare-id resolve).
- v50 done/FIXED: Agent-capability toggles as typed config getters + summary helper, default OFF. Test fix: `read_key` now reads the raw env var first (so a bare `1` survives as a string instead of being coerced to a number by the config parser), mirroring `memory_store::is_enabled`. Real call site: `Config::agent_caps_summary` (config/base.rs) calls `agent_caps::summary_lines_for_config`, plus six `config_value!(BHARATCODE_*)` getters. Evidence: 6/6 agent_caps tests green.

## Wave v51-v60 — integrated, GREEN (2026-06-20)

Integration note: this wave was authored against the pre-v41-v50 base, so applying it produced merge conflicts in four contended files (cli.rs, developer/mod.rs, prompt_manager.rs, context_mgmt/mod.rs). All resolved by keeping BOTH waves side by side: developer tool list now carries `rename_symbol` (v44) AND `delegate` (v51) as separate entries with their own dispatch arms (test `developer_tools_are_flat` expectation updated to `[…, web_search, rename_symbol, delegate]`); context_mgmt `do_compact` runs the v48 diff_compact pre-selection then the v51 docgen prepend on one `mut` binding; prompt_manager keeps both the v43 codebase_index extra and the v52 plan_mode extra; cli.rs keeps GenTests/GenDocs (v45/v46) AND the new Refactor subcommand plus its `#[path]` module decl, get_command_name arm, and dispatch.

- v51 done: `delegate` developer tool — hand a bounded sub-task to a fresh subagent. Real call site: registered in `DeveloperClient::get_tools()` and dispatched in `call_tool` ("delegate" arm) -> `self.delegate_tool.delegate(params, &ctx.session_id)` (agents/platform_extensions/developer/delegate.rs). Tunable via BHARATCODE_SUBAGENT_PROVIDER / BHARATCODE_SUBAGENT_MAX_TURNS. Evidence: 3/3 delegate tests green; tool present in the flat tool list (developer_tools_are_flat updated and passing).
- v52 done: Explicit plan-mode planner directive injected into the system prompt, default OFF behind BHARATCODE_PLAN. Real call site: `SystemPromptBuilder::build` (prompt_manager.rs) inserts the `bharatcode_plan` extra via `plan_mode::plan_block()`; None when disabled => byte-identical prompt. Evidence: 4/4 plan_mode tests green (disabled=>None, enabled block contains "Plan First"/numbered/confirm).
- v53 done: Lexical RAG retrieval (RAG-lite) prepends a `# Relevant files` block to the per-turn system prompt, default OFF behind BHARATCODE_RAG. Real call site: `Agent::stream_response_from_provider` path in agents/reply_parts.rs derives a query from the latest user-visible message and calls `semantic_index::retrieval_block(&query, &cwd)`; no walk/injection when disabled. Evidence: 8/8 semantic_index tests green (incl. disabled=>None assertion, no goose leak).
- v54 done: Post-edit test-generation nudge, default OFF behind BHARATCODE_TESTGEN. Real call site: agent finalization in agents/agent.rs runs `git_changed_files()` then `testgen::suggest_testgen(&changed_files)` and emits an InlineMessage system notification when `testgen::is_enabled()`. Evidence: 15/15 testgen tests green.
- v55 done: Doc-gen context preservation prepends a distilled public-API digest so symbols-to-document survive compaction, default OFF behind BHARATCODE_DOCGEN. Real call site: `do_compact` (context_mgmt/mod.rs) inserts `docgen::api_digest_block(...)` at message head when `docgen::is_enabled()`; None => compaction unchanged. Evidence: 5/5 docgen tests green.
- v56 done: `bharatcode refactor --find/--replace [--glob] [--apply]` multi-file find/replace preview (gitignore-aware, dry-run by default). Real call site: `Command::Refactor` dispatch in cli.rs -> `refactor::handle_refactor(RefactorOptions{..})`; subcommand visible in top-level `--help` and `refactor --help` (both leak-clean). Evidence: 3/3 refactor tests green.
- v57 done: `bharatcode doctor` deep-check now reports RAG/index readiness (read-only bounded gitignore-aware scan count), default-visible. Real call site: `print_deep_checks` (commands/doctor.rs) calls `index_check::index_readiness(&cwd)` and prints a Status glyph + message. Toggle BHARATCODE_RAG affects the message. Evidence: 6/6 index_check tests green.
- v58 done: `bharatcode cost` now renders a "Recent patch activity" diffstat footer derived from the session patch sidecar. Real call site: `handle_cost` (commands/cost.rs) calls `patch_stats::recent_patch_envelope()` -> `parse_patch_stats` -> `render_diffstat`; absent sidecar => byte-identical cost output. Evidence: 6/6 patch_stats tests green.
- v59 done: Subagent runtime settings (max-concurrent, max-turns, model override) loaded into the live session, defaults preserve current behavior. Real call site: `build_session` (session/builder.rs) calls `SubagentSettings::from_config(config)` and logs it. Tunable via BHARATCODE_SUBAGENT_MAX_CONCURRENT / BHARATCODE_SUBAGENT_MAX_TURNS / BHARATCODE_SUBAGENT_MODEL (values clamped). Evidence: 6/6 subagent_settings tests green (unset matches current behavior, clamps bounds, reads overrides).
- v60 done: `framework-migration` builtin skill surfaced to the agent. Real call site: shipped as crates/goose/src/skills/builtins/framework-migration.md, auto-discovered by `builtin::get_all()` (include_dir!) and surfaced in the platform-extensions system prompt — snapshot `all_platform_extensions.snap` updated to list it between `bharatcode-doc-guide` and `ultracode` (no Goose reintroduced; product-neutral migration guidance, "blocked" used only as the English status word). Evidence: 3/3 framework_migration_skill integration tests green + snapshot test passes.
- v61 done: Parallel tool-execution governor (concurrency cap + per-tool timeout), default OFF behind BHARATCODE_TOOL_MAX_INFLIGHT / BHARATCODE_TOOL_TIMEOUT_SECS. Real call site: the `with_id` map feeding the sole `stream::select_all` in agents/agent.rs calls `TOOL_GOVERNOR.wrap_stream(request_id, stream)` (LazyLock from_env), covering both pre-approved and approval-required tool streams; unset env => byte-identical no-op pass-through. Timeout yields a single synthetic ErrorData::INTERNAL_ERROR result. Evidence: 7/7 tool_governor tests green (no-op when unset, clamp 0->1/999->64, permit peak never exceeds cap, 1ms timeout yields exactly one Err).
- v62 done: Incremental context token-count cache (transparent, no env gate). Real call site: `check_if_compaction_needed` (context_mgmt/mod.rs) None-total branch calls `token_cache::count_cached(&token_counter, msg)` per message, memoizing per content-hash so estimation is O(new messages) with identical totals. Evidence: 5/5 token_cache tests green (cached_sum_equals_direct_sum asserts totals unchanged, eviction caps map at 50k, clear empties).
- v63 done: Crash/session resume pointer (last-good-turn recovery), default OFF behind BHARATCODE_RESUME. Real call site: `process_agent_response` (session/mod.rs) builds a RecoveryPoint and calls `recovery::record(&point)` per completed turn; `interactive()` surfaces a resume hint via `recovery::load()` at start and `recovery::clear()` on clean exit. Atomic tmp+rename sidecar under config_dir/bharatcode/recovery.json; disabled => no IO. Evidence: 6/6 recovery tests green (record/load round-trip, disabled records nothing, clear removes sidecar).
- v64 done: `bharatcode db` session-DB vacuum/tune/stats subcommand. Real call site: `Command::Db { vacuum, stats }` dispatch in cli.rs -> `db_cmd::handle_db(DbOptions{..})`; subcommand visible in top-level `--help` and `db --help`. Verified end-to-end: `bharatcode db --stats` runs, exits 0, prints size/reclaimable/integrity for <data_dir>/sessions/sessions.db (leak-clean). Evidence: 5/5 db_cmd tests green + live run confirmed.
- v65 done: Streaming throughput meter + adaptive flush hint, default OFF behind BHARATCODE_STREAM_STATS. Real call site: `stream_response_from_provider` (agents/reply_parts.rs) tees each text delta through `meter.tick(...)` in both toolshim branches and logs `meter.finish().summary_line()` via tracing on clean end when `stream_meter::is_enabled()`; measurement only, yielded items unchanged. Module file relocated to crates/goose/src/stream_meter.rs to match the `#[path = "../stream_meter.rs"]` decl. Evidence: 4/4 stream_meter tests green (empty stream => ttft None + 0.0 tok/s no panic).
- v66 done: Resource & time-limit typed config / turn budget guard, all ceilings default None (unlimited). Real call site: `Config::resource_limits_summary()` (config/base.rs) calls `resource_limits::from_config(self).summary_lines()`; three config_value! getters (BHARATCODE_MAX_TOOL_CALLS_PER_TURN / MAX_TURN_SECS / MAX_SESSION_TOKENS) registered first-class with raw-env-first reads and sane clamps. Evidence: 9/9 resource_limits tests green (empty => all None + unlimited summary, bare `1` survives coercion, absurd clamps, zero/non-numeric => unlimited).
- v67 done: Large-repo readiness deep check, always-on (env only tunes the warn threshold BHARATCODE_LARGE_REPO_FILE_WARN). Real call site: `print_deep_checks` (commands/doctor.rs) calls `repo_profile::profile(&cwd)` then `repo_profile::readiness_line(&profile)` and prints a Status glyph + message row right after the index-readiness row; read-only bounded gitignore-aware walk (FILE_CAP 50k, DEPTH_CAP 64). Evidence: 6/6 repo_profile tests green (files/bytes/depth/largest, gitignored excluded, Warn above file/byte thresholds, file-cap bounded).
- v68 done: Provider request deadline + graceful cancellation wrapper, default OFF behind BHARATCODE_PROVIDER_DEADLINE_SECS. Real call site: `pub mod deadline;` registered in providers/mod.rs (mirroring v32 fallback / v49 planner_presets), exposing `with_deadline(fut, cancel)` as reachable provider-layer API; no deadline AND no token => future awaited unchanged (zero overhead). Deadline elapse => ProviderError::RequestFailed; pre-cancelled token wins over deadline. Evidence: 11/11 deadline tests green (parse rejects blank/0/junk, absurd/u64::MAX clamp, never-completing hits deadline, pre-cancelled short-circuits).
- v69 done: Chunked large-file reader tool `read_lines`, always available, read-only, byte-bounded (BHARATCODE_READ_LINES_MAX_BYTES tunes the cap; can't be disabled). Real call site: wired through the live DeveloperClient path — `Tool::new("read_lines", ...)` in `get_tools()` and a `"read_lines" =>` arm in `call_tool` (developer/mod.rs) calling `read_lines_with_cwd(params, working_dir)`; default limit 200 lines / 256 KiB, UTF-8-boundary-safe truncation, refuses binary. Evidence: 6/6 read_lines tests green (offset/limit window + total_lines, byte cap stops a 2 MiB line, binary refused, cwd-relative path).
- v70 done: Incremental-context repo digest extra (cached structural snapshot injected into the system prompt), default OFF behind BHARATCODE_REPO_DIGEST. Real call site: `SystemPromptBuilder::build` (agents/prompt_manager.rs) inserts the `bharatcode_repo_digest` extra when `repo_digest::is_enabled()` and `digest_block(&cwd)` is Some; off => byte-identical prompt. Per-cwd fingerprint memo re-renders only when the metadata fingerprint changes. Module file relocated to crates/goose/src/repo_digest.rs to match the `#[path = "../repo_digest.rs"]` decl. Evidence: 4/4 repo_digest tests green (disabled => None, lists top entries + fingerprint, cached block reused unchanged, recomputes on change, explicit no-"goose" assertion).

## Wave v61-v70 (second batch — persistence, retry, caching, DB health, streaming/coalesce)
- v61 done: Cross-turn persistent token-count disk cache, default OFF behind BHARATCODE_TOKEN_CACHE (gate-off => pure pass-through to the in-memory counter, byte-identical, no file). Real call site: `check_if_compaction_needed` (context_mgmt/mod.rs:229) calls `token_cache_disk::count_cached_persistent(&token_counter, msg)` for the per-message estimate — the only production estimate call site, so the SHA-256-keyed on-disk JSON LRU (<config_dir>/bharatcode/token_cache.json, MAX 5000, drop-oldest) is genuinely reachable in the running binary. Evidence: token_cache + token_cache_disk tests green (same text returns cached count without re-invoking the stub, serialize round-trip, over-capacity eviction, gate-off writes no file, corrupt/oversize loads empty); fixed shared-global TOKEN_CACHE test contamination by serializing all cache-touching tests on one poison-tolerant guard and asserting per-message presence instead of absolute map size.
- v62 done: Transient tool-failure auto-retry on dispatch, default OFF behind BHARATCODE_TOOL_RETRY (unset => RetryPolicy::single() => exactly one dispatch, byte-identical). Real call site: `handle_approved_and_denied_tools` (agents/agent.rs:847) wraps the per-request `dispatch_tool_call` in `tool_retry::with_tool_retry(cancel_token.clone(), ...)`; classifier keeps INVALID_PARAMS/request-shape/policy-deny terminal, retries INTERNAL_ERROR/unknown with exponential backoff (clamped 1ms..30s) and cancellation-aware sleep. Evidence: tool_retry unit tests green (classifier table, spec parse/clamp, backoff cap, N-1-then-success, budget exhaustion, attempts=1, terminal/policy-deny not retried, pre-cancelled stops, live token retries).
- v63 done: Read-only per-run tool-result memo cache for the developer extension, default OFF behind BHARATCODE_TOOL_CACHE. Real call site: the `"shell"` arm of `DeveloperClient::call_tool` (developer/mod.rs) does lookup -> run+redact -> store-if-read-only, with `invalidate_all()` on write/edit/apply_patch arms so a mutation never leaves stale reads cached. Conservative read-only classifier (allow-list pipelines, rejects shell metachars; read-only git subcommands only). Evidence: result_cache tests green (key stability under key-order + whitespace, read-only store+hit, write no-op, error not stored, invalidate empties, gate-off lookup None, classifier accept/reject matrix).
- v64 done: Read-only session-store storage-health footer for `bharatcode cost`, currency-free single line. Real call site: `handle_cost` (commands/cost.rs) prints `db_health::storage_footer().await` (theme::muted) after the patch-activity footer; resolves sessions.db via Paths::data_dir()/SESSIONS_FOLDER/DB_NAME, shows DB/WAL bytes + session/message counts + a 'vacuum recommended' marker when wal/db >= 0.25; absent/empty DB omits the footer entirely (default output unchanged). Evidence: db_health tests green (single line with all stats, no hint on small WAL, hint + singular nouns on high WAL, empty-DB never recommends vacuum, absent-DB None, human_bytes formatting).
- v65 done: Doctor session-DB integrity & fragmentation deep check (always-on; BHARATCODE_DB_FRAG_WARN_RATIO only tunes the warn threshold, default 0.25). Real call site: `print_deep_checks` (commands/doctor.rs) calls `db_integrity::check().await` and renders Status/glyph after the repo_profile row; opens a short-lived read-only sqlite pool, runs PRAGMA quick_check/freelist_count/page_count, classifies Fail (integrity) > Warn (fragmentation) > OK; missing DB => benign OK, any failure => non-fatal Warn. Evidence: db_integrity tests green (classify Fail/Warn/OK incl. boundary + zero-page, fragmentation math, missing-DB benign, real-path check never panics).
- v66 done: Graceful-cancel partial-output flush, default ON but INERT (only fires on a real interrupt; no env gate). Real call site: the `_ = cancel_token_clone.cancelled() =>` select arm in `process_agent_response` (session/mod.rs) calls `cancel_flush::on_interrupt(&accumulated_text, &mut markdown_buffer)` (text-render modes only); flushes the pending markdown buffer then emits a localized 'interrupted — partial response saved' marker once via an atomic one-shot guard. Normal completion path stays byte-identical. Evidence: cancel_flush tests green (marker-by-length, key-not-leaked text, fire-exactly-once, empty no-op, budget preserved across empty->nonempty).
- v67 done: Startup session-DB integrity quick-check + heal pointer, warn-only by default (user-visible heal line gated behind BHARATCODE_DB_PREFLIGHT; healthy DB silent). Real call site: `build_session` (session/builder.rs) calls `db_preflight::preflight().await` early (after telemetry, before Config::global()/Agent::new()); opens its OWN short-lived read-only pool, runs PRAGMA quick_check, logs warn on non-ok, all errors swallowed to Ok(None) — never blocks/panics. i18n key db.preflight.hint added (en+hi). Evidence: db_preflight tests green (heal_advice None-on-ok / Some-on-corruption naming `db --vacuum`, is_truthy spellings, missing-path Err, preflight_path over missing DB Ok(None) no panic).
- v68 done: Typed streaming/render perf-tuning getters as a single validated source of truth (defaults preserve current behavior; nothing changes unless a BHARATCODE_* var is set). Real call site: public `Config::streaming_perf_summary()` (config/base.rs) delegates to `streaming_perf::summary_lines_for_config(self)` — the reachable doctor/info surface, same pattern as the shipped resource_limits_summary/agent_caps_summary; three config_value! getters (BHARATCODE_STREAM_FLUSH_MS / MAX_CODE_BLOCK_LINES / STREAM_COALESCE_LINES) registered with env-first reads + clamps. Evidence: 9/9 streaming_perf tests green (in-range pass-through, over-range clamp, garbage/zero->default, bare-1 honored env-first, below-min clamp-up, all-unset defaults, summary override + default rows).
- v69 done: In-flight provider-request coalescer (single-flight), default OFF behind BHARATCODE_COALESCE (gate-off => awaits producer directly, no hashing/map insertion, byte-for-byte pass-through). Real call site: `pub mod coalesce;` + `pub use coalesce::RequestCoalescer;` in providers/mod.rs expose the reachable public API (`RequestCoalescer::coalesce(key, producer)` + `request_key(...)`) for the streaming/embeddings paths; concurrent same-key callers share one `Shared<BoxFuture<T>>` via a Weak-reclaimed process registry. Fixed the integration compile error: `T` now bounded `Clone + Send + Sync + 'static` so the `Shared` keep-alive can be type-erased into `Arc<dyn Any + Send + Sync>`. Evidence: coalesce tests green (two same-key calls run producer once + fan out identical value, different keys run twice, gate-off pass-through, is_truthy, request_key determinism).
- v70 done: Periodic turn-checkpoint writer, default OFF behind BHARATCODE_CHECKPOINT (unset => zero-I/O no-op, verified no file created). Real call site: per-turn block in `process_agent_response`/`reply` loop (goose-cli session/mod.rs) right after the recovery::record call computes turn_index (User-message count) + last_message_id and calls `goose::turn_checkpoint::record(...)`; throttled via should_write (default 5000ms, BHARATCODE_CHECKPOINT_INTERVAL_MS) and atomic temp-file rename into <config_dir>/bharatcode/checkpoint.json (ts_utc + IST +05:30), errors swallowed (never blocks the turn). Evidence: turn_checkpoint tests green (serde round-trip, should_write boundary, record no-op when disabled, record writes + latest reads back when enabled, summary_line single line, is_enabled honors env).
- v71 done: `bharatcode catalog` — a read-only, offline, embedded catalog (13 India-relevant MCP/plugin/recipe entries) with `--show <id>` / `--kind mcp|plugin|recipe`. Real call site: cli.rs dispatch arm `Some(Command::Catalog { show, kind }) => catalog_cmd::handle_catalog(show, kind)` (reachable in the binary; verified via `bharatcode catalog`/`catalog --help` rendering the listing with zero plugins installed). No env gate (always available, pure embedded data); strings routed through the i18n `label()` fallback. Evidence: catalog unit tests green (non-empty, unique ids, valid kinds, filter_by_kind only-mcp, find Some/None, no-leak assert); binary leak-gate clean.
- v72 done: opt-in "Extensions in use" attribution footer for `bharatcode cost`, default OFF behind BHARATCODE_COST_EXTENSIONS. Real call site: `handle_cost` (commands/cost.rs) prints `cost_extensions::extensions_footer()` immediately after the v58 patch_stats footer, before the empty-sessions early return (reachable whether or not spend exists). Names only (plugins + enabled MCP servers, dedup/sorted), no commands/args/paths. Default OFF => byte-identical cost output. Evidence: cost_extensions tests green (is_enabled false unset, footer None when disabled, footer None on empty temp plugins root via env_lock'd BHARATCODE_PATH_ROOT, render block contains each name + no brand, name-derivation). Test env races eliminated by routing all BHARATCODE_PATH_ROOT mutators onto the shared workspace env_lock.
- v73 done: always-visible Ecosystem section in `bharatcode doctor` deep-checks (plugin count, configured MCP count, plugin tool-use hooks presence; each with a Status glyph). Real call site: `print_deep_checks` (commands/doctor.rs) prints an "Ecosystem" heading then loops `ecosystem_check::ecosystem_rows()` (reachable via handle_doctor -> print_deep_checks). Rule: count 0 => Warn, >0 => Ok; hooks Ok if pre OR post registered else Warn (never Fail/blocks). Evidence: ecosystem_check test green (exactly 3 rows, plugin/MCP 0 => Warn, hooks Warn on empty env_lock'd root, glyph mapping + no brand leak).
- v74 done: curated embedded MCP server registry as pure data + resolver (8 entries; lookup case-insensitive; to_extension_config builds real ExtensionConfig::Stdio/Sse). Real call site: `pub mod mcp_registry;` in providers/mod.rs exposes the reachable public API `goose::providers::mcp_registry` (same wiring posture as the shipped planner_presets/fallback siblings) for recipe/session/catalog consumers. No env gate (inert library API). Evidence: mcp_registry tests green (non-empty, unique+lowercase ids, lookup GIT/Git -> git + nope None, to_extension_config git => Stdio uvx mcp-server-git, SSE => ExtensionConfig::Sse correct uri, every entry valid, no brand).
- v75 done: opt-in `run_script` developer automation tool, gated on BHARATCODE_SCRIPTS (default OFF). Real call site: `DeveloperClient::get_tools()` pushes `run_script::script_tool()` only when `run_script::is_enabled()`, and `call_tool` has the `"run_script" => run_script::run(...)` dispatch arm (developer/mod.rs; reachable in the running binary). Resolves `<cwd>/.bharatcode/scripts/<name>` with strict name validation (rejects separators/`..`), executable-bit check, runs directly (not via shell), applies the v4 sandbox under BHARATCODE_SANDBOX, output byte-capped. Evidence: run_script tests green (is_enabled unset/env-reflective, name traversal rejected, executable hello returns stdout, args forwarded, missing/non-exec => error, description no brand); developer_tools_are_flat asserts absent by default, run_script_tool_present_when_enabled asserts present. Fixed the flaky cross-module BHARATCODE_SCRIPTS env race by adding a shared `SCRIPTS_ENV_TEST_LOCK` used by both the run_script tests and the developer tool-list tests.
- v76 done: CI integration hook on agent finalization, default OFF behind BHARATCODE_CI (auto-detects cargo fmt --check / npm lint / ruff from marker files, or runs a verbatim override). Real call site: the finalization block in agents/agent.rs hoists `changed_files` when `testgen::is_enabled() || ci_integration::is_enabled()`, then `if ci_integration::is_enabled() { if let Some(msg) = ci_integration::run_ci(&changed_files, &working_dir).await { yield system InlineMessage } }` — reuses the already-computed changed_files, reachable in the running binary. Default OFF => finalization byte-identical. Evidence: ci_integration tests green (is_enabled false unset, ci_command auto-detect cargo/node/python + explicit override wins, run_ci None on empty changed_files / when disabled, message format CI: prefix + no brand). Fixed the `run_ci_none_when_disabled` race by serializing every BHARATCODE_CI test on a shared ENV_LOCK (EnvGuard now holds the lock and added an `unset` constructor).
- v77 done: installed-extensions advisory injected into the system prompt, default OFF behind BHARATCODE_EXT_ADVISORY (caps 24 names / 600 bytes). Real call site: `SystemPromptBuilder::build()` (agents/prompt_manager.rs) inserts `ext_advisory::advisory_block()` into `system_prompt_extras` under key `bharatcode_ext_advisory` right after the model-caps insert; that map is sanitized and joined into the final system prompt downstream, so the block genuinely reaches the model. Default OFF => prompt byte-identical (None when disabled). Evidence: ext_advisory tests green (disabled => None, enabled with empty env_lock'd plugins root => None, render block under 600B contains each name + "extensions" + no brand, empty-list None, truncation under cap, is_truthy).
- v78 done: recipe import/export round-trip hardening at session build, opt-in behind BHARATCODE_RECIPE_LOCK (default unset => zero behavior change). Real call site: `build_session` (session/builder.rs) — after extensions load and the migrate advisory — gated by `if recipe_lock::is_enabled()` resolves recipe_path(), calls lock_recipe, logs Fresh/Matched/Drifted via tracing (reachable in the running CLI binary). Canonicalizes via Recipe::from_file_path->to_yaml, SHA-256 hashes, writes/compares a `.bharatcode/recipe.lock` sidecar. Evidence: recipe_lock tests green (is_enabled false unset/blank, true when set; Fresh->Matched->Drifted lifecycle; hash stable + 64 hex; LockOutcome Debug no brand).
- v79 done: ecosystem config getters + summary covering the six wave-v71-v80 toggles (CATALOG/SCRIPTS/CI/EXT_ADVISORY/RECIPE_LOCK/COST_EXTENSIONS), all default OFF. Real call site: `Config::ecosystem_summary()` (config/base.rs) delegates to `ecosystem_config::summary_lines_for_config(self)` — a public Config method reachable for configure/doctor (same posture as the shipped agent_caps_summary); plus six `config_value!` typed getters layering env over config file. read_key reads raw env first so a bare `1`/`true` survives as a string. Evidence: ecosystem_config tests green (all-unset => every row off; env_lock'd BHARATCODE_CI=1 flips only that row; bare `1` honored via read_key; unrecognised => off; no row label brand leak).
- v80 done: editor/IDE bridge breadcrumb file, default OFF behind BHARATCODE_IDE_BRIDGE. Real call site: `stream_response_from_provider` (agents/reply_parts.rs), immediately after the WAITING_LLM_STREAM_START marker and before stream_with_fallback: `if ide_bridge::is_enabled() { if let Ok(cwd) = std::env::current_dir() { ide_bridge::write_breadcrumb(session_id, &model_config.model_name, "streaming", &cwd); } }` — the agent's streaming reply path, reachable in the running binary. Atomically writes `<cwd>/.bharatcode/ide-bridge.json` (temp-file + rename) with session_id/model/status/updated_at_utc/updated_at_ist(+05:30)/pid; brand-neutral keys. Default OFF => no file writes. Evidence: ide_bridge tests green (is_enabled false unset, write creates parseable JSON with all keys, second write updates timestamp + no .tmp partial, disabled writes nothing, payload keys no brand).
- v71 (ecosystem-ext wave-2) done: MCP server registry + `bharatcode mcp-registry [list|search|show]`. Real call site: cli.rs declares `#[path="commands/mcp_registry.rs"] pub mod mcp_registry_cmd;` and dispatches at cli.rs:2419-2425 -> `mcp_registry_cmd::handle_mcp_registry(action)`; commands/mod.rs adds `pub mod mcp_registry;` + re-export. Static REGISTRY of well-known + India-relevant MCP servers; `show` emits a `{"mcpServers":{command/args/env}}` snippet parseable by goose::plugins::mcp_servers::McpServersDocument. No env gate (offline, read-only). Evidence: `bharatcode mcp-registry list/show filesystem` render clean (no brand leak); 11 unit tests green (unique ids, snippet round-trips through validate_mcp_server_document, no brand).
- v72 (ecosystem-ext wave-2) done: Recipe share — `bharatcode recipe export <name> [-o file.bcr]` / `recipe import <file.bcr|url>`. Real call site: clap `RecipeCommand::Export/Import` (cli.rs:846-865) dispatch to `crate::recipes::share::{export_recipe,import_recipe}`; recipes/mod.rs adds `pub mod share;`. Bundles a validated recipe + sub-recipe attachments into a SHA-256-checksummed `.bcr`; import verifies format-version + checksum (refuses tampered/future bundles) before writing only under the recipe library dir. Evidence: share tests green (round-trip, sub-recipe pack/restore, tampered-checksum + future-version rejection, unsafe-path guard); export/import --help clean.
- v73 (ecosystem-ext wave-2) done: Plugin SDK — typed lifecycle-hook event-payload API. Real call site: `pub mod plugin_sdk;` in lib.rs (line 40) exposes `bharatcode::plugin_sdk::{HookContext,HookDecision,HookEvent, supported_events(), event_contract(), PluginSdkVersion::CURRENT}`, a thin additive faade over crate::hooks compiled into the crate and exercised by its own tests. No env gate (pure additive API). Evidence: plugin_sdk tests green 5/5 (supported_events cover dispatched variants no dups, event_contract total/panic-free non-empty object for every variant with round-tripping name + stdin schema matching serialized HookContext, semver-shaped version).
- v74 (ecosystem-ext wave-2) done: Extension catalog — curated embedded read-only index. Real call site: `pub mod catalog_index;` in providers/mod.rs (line 20) exposes `crate::providers::catalog_index::{all(),find(),india_hosted()}`; entries reuse real shipped ids (providers sarvam/krutrim/ollama, MCP filesystem/git/fetch/sarvam-doc/krutrim-doc, developer builtin) and its tests cross-check live sibling tables (planner_presets, mcp_registry). No env gate (read-only catalog). Evidence: catalog_index tests green 10/10 (ids unique/lowercase, case-insensitive find, residency tags sarvam+krutrim india_hosted / ollama not, no brand in descriptions/hints).
- v75 (ecosystem-ext wave-2) done: Automation/scripting context block, default OFF behind BHARATCODE_AUTOMATION. Real call site: `SystemPromptBuilder::build()` (agents/prompt_manager.rs:263-264) inserts `automation_mode::automation_block()` under key `bharatcode_automation` alongside memory/plan extras (module wired via `#[path="../automation_mode.rs"]`, lib.rs untouched). Default OFF => insert skipped, prompt byte-identical. Directive instructs deterministic non-interactive behavior ending in a `STATUS: <SUCCESS|FAILURE>` line. Evidence: automation_mode tests green 4/4 (disabled=>None & is_enabled false, enabled=>block contains "automation"+"STATUS:" + no brand, falsey "0" stays off).
- v76 (ecosystem-ext wave-2) done: read-only `git_advanced` developer tool (worktree_list/blame/pr_context). Real call site: registered in `DeveloperClient::get_tools()` (developer/mod.rs:353, read-only annotations) and dispatched in `call_tool` (mod.rs:523-524) -> `git_advanced::run_git_advanced(params, working_dir)`; reachable via the developer extension in the running binary. Shells only read-only git subcommands, no network/mutation, returns clean errors not panics. Evidence: git_advanced tests green 6/6 (porcelain worktree/blame parsers, unknown-op clean error is_error=true, blame-without-file error, range normalisation accept/reject, no brand); developer_tools_are_flat lists git_advanced.
- v77 (ecosystem-ext wave-2) done: read-only CI-readiness doctor check. Real call site: `print_deep_checks()` (commands/doctor.rs:210) calls `ci_check::ci_readiness(&cwd)` after the index_check row and, when not Ok, prints `ci_check::sample_workflow()` as a muted hint (so sample_workflow is genuinely reachable, no dead_code) — module wired via `#[path="ci_check.rs"]`. Detects GitHub Actions/GitLab/Jenkins config, greps read-only for a `bharatcode` step, reports BHARATCODE_AUTOMATION state. No env gate (informational row only). Evidence: ci_check tests green (gh-with-step=>Ok, no-step=>Warn, no-CI=>neutral Ok, gitlab/jenkins, gh-priority, sample_workflow has BHARATCODE_AUTOMATION + `bharatcode run --recipe`).
- v79 (ecosystem-ext wave-2) done: i18n — localized ecosystem command strings. Real call site: i18n table (`pub mod i18n;` in lib.rs) consumed live via `crate::tr!` (mcp_registry.rs, ecosystem_check.rs, recipes_library.rs); added 7 `ecosystem.*` keys to BOTH en.json + hi.json (real Devanagari) with identical key sets, plus NEW i18n/ecosystem_keys.rs (`ECOSYSTEM_KEYS` + parity helper) declared via `pub mod ecosystem_keys;` in i18n/mod.rs. BHARATCODE_LANG=hi exercises Hindi via the existing resolver; en default byte-identical. Evidence: ecosystem_keys tests green (keys present+non-empty in both, unique, identical full key sets en/hi, pre-existing en values unchanged ASCII).
- v78 (ecosystem-ext wave-2) done: extension/plugin config getters + summary (bharatcode_ext_*). Real call site: `Config::extension_settings_summary()` (config/base.rs:605-606) delegates to `ext_settings::summary_lines_for_config(self)` — a public Config method reachable by doctor/info (mirrors agent_caps_summary); module wired via `#[path="../ext_settings.rs"] pub(crate) mod ext_settings;` plus typed `config_value!` getters (PLUGIN_AUTO_UPDATE/MCP_REGISTRY_BANNER/RECIPE_OUT_DIR Option forms). All defaults preserve current behavior; read_key reads raw env first. Evidence: ext_settings tests green 5/5 (all-unset lists every key source=default with current values, BHARATCODE_AUTOMATION=1 flips that line to env, recipe-dir override, unrecognised-toggle, no brand).
- v80 (ecosystem-ext wave-2) done: editor/IDE bridge post-turn changed-files manifest sidecar, default OFF behind BHARATCODE_EDITOR_BRIDGE. Real call site: agent finalization (agents/agent.rs:2699-2702) — guard widened to include `editor_bridge::is_enabled()`, and inside the block where `changed_files` is already computed it calls `editor_bridge::write_change_manifest(&working_dir, &changed_files).await` (module wired via `#[path="../editor_bridge.rs"]`, reuses the already-computed changed_files, no extra git call). Writes `<cwd>/.bharatcode/last_changes.json` (generated_at_utc/ist + files[{path,status}] from `git status --porcelain`), best-effort never-panic. Default OFF => zero extra I/O. Evidence: editor_bridge tests green 6/6 (disabled=>no file, enabled=>parseable JSON with files+ist+status, smaller re-run overwrites cleanly, status_label, no brand on keys).
- v71 done (wave 2): MCP server registry + `bharatcode mcp-registry` (list/search/show). Real call site: cli.rs `#[path="commands/mcp_registry.rs"] pub mod mcp_registry_cmd;` + `Command::McpRegistry { action }` variant (alias `mcp-reg`) dispatched to `mcp_registry_cmd::handle_mcp_registry(action)` — verified reachable via `bharatcode mcp-registry list` rendering 15 curated entries and `mcp-registry --help` listing list/search/show. Read-only, no env gate. Evidence: 11 unit tests green (registry non-empty, unique ids, search matches git/filters by category+name, empty search => all, get github Some/unknown None, every entry snippet passes real `goose::plugins::mcp_servers::validate_mcp_server_document`, snippet keys round-trip, no entry leaks "goose"); leak-gate on the live subcommand clean.
- v72 done (wave 2): Plugin SDK lifecycle SessionSummary hook, default OFF behind BHARATCODE_PLUGIN_SUMMARY. Real call site: agent reply finalization block in agents/agent.rs (inside `if !is_token_cancelled(...)`, next to verify/testgen/ci_integration) gated by `if plugin_summary::is_enabled()` — gathers git_changed_files + tool names from the in-scope conversation, build_summary(...), then `plugin_summary::dispatch(s, &self.hook_manager, &session_config.id).await`. Default OFF => is_enabled() short-circuits before any IO. Evidence: tests green (disabled_when_unset, enabled_on_bare_one, build_summary field values, to_json is_object with the 4 documented keys + no "goose" branding).
- v73 done (wave 2): portable recipe sharing (export -> versioned+SHA-256 JSON bundle -> import with integrity verify), opt-in behind BHARATCODE_RECIPE_SHARE. Real call site: `pub mod recipe_share;` + re-exports (run_recipe_share/recipe_share_export/recipe_share_import/RecipeBundle) in commands/mod.rs expose the reachable crate API consumed by cli.rs's RecipeShare arm; `run(args)` env-gated dispatcher for `export`/`import`. Byte-identical round-trip (verbatim content) + tamper detection (digest recompute). Evidence: 8 tests green (stable SHA-256 hex, byte-identical round-trip, exported_by==bharatcode + schema, tampered-content fails integrity, future-schema rejected, dispatch round-trip, unknown-subcommand rejected, env-gate inert). Verified green across 3 consecutive full-suite runs (initial cold-cache flake did not reproduce).
- v74 done (wave 2): extension catalog API + doctor "Extensions catalog" readiness row. Real call site: catalog.rs is a LIVE module (cli.rs `bharatcode catalog` subcommand) AND is `#[path]`-included a second time in doctor.rs; `print_deep_checks()` prints the always-visible row via `catalog::catalog_readiness()` (reachable when `bharatcode doctor` runs). Reads live enabled set best-effort (catch_unwind) for "{label} ({total} entries, {active} active)"; Warn when active==0 else Ok, never Fail. Evidence: tests green (all()==CATALOG.len(), get Some/None, readiness msg non-empty + contains count + non-fatal + no brand).
- v75 done (wave 2): `bharatcode-script` JSONL command runner as a pure default-inert public lib. Real call site: `pub mod automation;` in goose/src/lib.rs exposes the reachable public API (Script/Step/parse_jsonl/execute_offline) for the CLI/headless scripting consumer. serde-tagged Step (run-prompt/set-env/assert-output-contains/comment); unknown type errors at parse time; execute_offline sets env / asserts on last_output / skips run-prompt. Default-inert (caller-driven). Evidence: 6 tests green (4-line parse, blank-line skip, bogus-type Err, execute_offline ok/ok/fail with env actually set, run-prompt skipped pass, no "goose"/"block" in any detail).
- v76 done (wave 2): CI-friendly `bharatcode cost` machine-readable footer + budget exit signal, gated on CI/GITHUB_ACTIONS/BHARATCODE_CI. Real call site: end of `handle_cost` (commands/cost.rs, before the empty-sessions early return) — `if ci_report::is_ci()` reads the ₹ budget cap (budget::BUDGET_INR_KEY), computes day/month from the existing ledger, prints `ci_footer_json(...)` then `github_annotation(...)`. handle_cost is the live `bharatcode cost` entry point; default (no CI signal) => human output byte-identical. Evidence: 8 tests green (is_ci false unset / true per signal, footer single-line JSON with 4 keys, over_budget true when day>cap / false+null with no cap, annotation None unless GHA, ::error::/::notice:: levels, no brand leak).
- v77 done (wave 2): deeper git context (worktree/blame/PR) injected into the session prompt, default OFF behind BHARATCODE_GIT_CONTEXT. Real call site: `build_session` (session/builder.rs) after the v47 migrate block — `if git_context::is_enabled() { if let Some(block) = git_context::git_context_block(&git_context::collect(&cwd)) { agent_ptr.extend_system_prompt("bharatcode_git_context", block).await } }` — reachable in the running binary. collect() is the only git-spawning fn (read-only queries only) and is reached only behind the gate. Default OFF => byte-identical prompt + zero git subprocesses. Evidence: tests green (is_enabled gate table, block renders worktree+branch+author lines, no-upstream form, worktree/author capping, None for default/bare contexts, no "goose"/"block.xyz" in rendered block).
- v78 done (wave 2): read-only `editor_locator` developer tool (editor-jump targets from path + optional line/col). Real call site: developer/mod.rs registers `Tool::new("editor_locator", ...)` in `get_tools()` and dispatches `"editor_locator" =>` in `call_tool` (no env gate, always available like read_lines) — reachable in the running binary. Builds vscode_uri/vscode_cli/jetbrains_cli/generic; missing line omits suffixes, column-without-line ignored, zero clamped to 1, cwd-relative resolved against working_dir. Evidence: tests green (line+col targets, missing-line omits, column-without-line ignored, zero clamped, relative resolution, missing-path error, no "goose" in any target); developer_tools_are_flat updated to include editor_locator.
- v79 done (wave 2): MCP/extension awareness in the system prompt (active-extension digest), default OFF behind BHARATCODE_EXT_DIGEST. Real call site: `SystemPromptBuilder::build()` (agents/prompt_manager.rs) alongside the repo_digest extra — when ext_digest::is_enabled(), derives Vec<ExtDescriptor> from the live sanitized `context.extensions` and inserts ext_digest_block(...) into system_prompt_extras under `bharatcode_ext_digest`, which flows into the final system prompt. Off-path byte-identical (is_enabled short-circuits). ext_digest_block is pure over its slice (caps 24 entries / 4096 bytes). Evidence: tests green (is_enabled gate table, empty/all-blank => None, 2-descriptor render asserts both names + summaries + (mcp) tag, no "goose" + under cap, omission/one-line-clip/truncation-marker).
- v80 done (wave 2): ecosystem config surface — typed extensibility getters + `Config::ecosystem_caps_summary()`. Real call site: config/base.rs `#[path="../ecosystem_caps.rs"] pub(crate) mod ecosystem_caps;` + 5 `config_value!` getters (PLUGIN_SUMMARY/GIT_CONTEXT/EXT_DIGEST/MCP_REGISTRY_PIN/AUTOMATION_SCRIPT) + `pub fn ecosystem_caps_summary(&self)` delegating to `ecosystem_caps::summary_lines_for_config(self)` on the global Config singleton (same posture as agent_caps_summary). read_key reads raw env first so a bare `1`/path survives. Named ecosystem_caps_summary (not ecosystem_summary) to avoid colliding with the existing sibling method. Evidence: 6 tests green (empty config => all-off line, bare 1 => "git context: on", automation/mcp-pin verbatim, unrecognised toggle off, multi-setting rows, no "goose"/"Block" in any line).
- v81 (i18n wave-3) done: Tamil (ta) i18n pack + tri-locale resolver. Real call site: `commands/configure.rs` declares `#[path="../i18n/ta_locale.rs"] mod ta_locale;` and prints `ta_locale::active_lang_name()` in the first-run banner (configure.rs:143); resolver maps BHARATCODE_LANG=ta / ta_IN.UTF-8 -> Locale::Ta (i18n/mod.rs normalize_locale). New ta.json mirrors all 77 en.json keys with real Tamil; en/ta default output byte-identical (Tamil only on opt-in). Evidence: i18n tests green — tri_locale_tables_cover_all_english_keys, tamil_translation_differs_from_english, normalize_locale_maps_tamil_variants all ok; en==ta key sets (77).
- v82 (i18n wave-3) done: Hindi coverage deepening across /help + doctor deep-checks (15 keys). Real call site: existing help_tr("help.title",..)/help_tr("help.group.*",..) in session/input.rs:452-458 and label("doctor.*",..) in commands/doctor.rs + doctor_checks.rs already echo the key; adding the Hindi values makes them render Hindi under BHARATCODE_LANG=hi with NO call-site edits. New i18n/hi_coverage.rs (HINDI_DEEPENED_KEYS) wired via `pub mod hi_coverage;` (i18n/mod.rs:14). Integrator mirrored the 15 keys into en.json + ta.json so all three tables hold 77 identical keys. Evidence: hi_coverage tests green — deepened_keys_have_distinct_hindi_values, en_and_hi_have_identical_key_sets ok; default en output byte-identical.
- v83 (UX wave-3) done: opt-in screen-reader plain-text session ready banner, default OFF behind BHARATCODE_A11Y / BHARATCODE_SCREEN_READER. Real call site: ready-banner render in session/output.rs:1374-1378 branches on `a11y_banner::is_enabled()` and emits `a11y_banner::plainify(...)` (decoration stripped, no console::style) — module included as `#[path="a11y.rs"] mod a11y_banner;` to avoid colliding with v88s a11y module. Default (unset) banner byte-identical. Evidence: a11y tests green (is_enabled env table, plainify Cow::Borrowed when already plain, strips box-drawing/emoji/spinner).
- v84 (UX wave-3) done: first-run onboarding wizard `bharatcode onboard` (language -> provider preset -> privacy posture -> summary). Real call site: cli.rs `#[path="commands/onboarding.rs"] mod onboarding;` (cli.rs:83), `Command::Onboard { noninteractive }` variant + get_command_name arm (cli.rs:1608) + dispatch `onboarding::handle_onboard(...)` (cli.rs:2413). Reachable via `bharatcode onboard --help`. Persists only bharatcode_lang on confirm; non-TTY/--noninteractive prints the plan. Conflicting pre-seeded onboard.rs removed (single coherent impl). Evidence: onboarding unit tests green (wizard_plan order, summary_lines non-empty + brand-neutral, is_noninteractive); leak-gate on `onboard --help` clean.
- v85 (cost wave-3) done: `bharatcode cost` dashboard panel, env-gated BHARATCODE_COST_DASHBOARD (default OFF) + FIXED a pre-existing E0063 build break (removed stray `dashboard` field from CostOptions so cli.rs literal compiles). Real call site: handle_cost (commands/cost.rs:196) gates on `cost_dashboard::is_enabled()` then prints `cost_dashboard::render_dashboard(&ledger, rate, &candidates, active)` and returns early. Aligned ASCII panel, ₹ via format_inr, NO_COLOR honoured. Default (unset) cost output byte-identical. Evidence: cost_dashboard tests green (rupee+scope labels+total, model-row alignment equal offsets, empty-ledger no-spend panel, NO_COLOR no 0x1b, no brand).
- v86 (UX wave-3) done: opt-in desktop notification on long-turn completion, default OFF behind BHARATCODE_NOTIFY (threshold BHARATCODE_NOTIFY_AFTER_SECS, default 20). Real call site: agent reply finalization in agents/agent.rs — `turn_started_at` captured at :1919, and at :2764 inside the `!is_token_cancelled` block `if desktop_notify::is_enabled() { if elapsed >= threshold_secs() { notify(title, body) } }`. New goose/src/desktop_notify.rs (notify-send/osascript detached probe, BEL fallback, brand-neutral localized message). is_enabled() short-circuits before reading the clock => unset behavior byte-identical. Evidence: goose lib tests green incl desktop_notify (threshold clamp 0->1/cap, completion_message brand-neutral, no panic without backend).
- v87 (UX wave-3) done: in-app help/command index reusable library API in goose-cli. Real call site: `pub mod help_index;` (lib.rs:9) exposes ENTRIES/search()/render() as public crate surface; module doctest drives render(None)/render(Some("cost")) so render() is a live (non-dead) call. Covers 11 real subcommands + 8 BHARATCODE_* toggles, all vendor-neutral, localized via crate::tr! with English fallback. Read-only, additive, no behavior change. Evidence: help_index tests green (ENTRIES non-empty + unique names, search("cost") hits cost cmd + cost toggles, render contains Commands heading + BHARATCODE_ row, no goose/block leak).
- v88 (TUI wave-3) done: locale- and NO_COLOR-aware status-line footer wired into the interactive session loop. Real call site: session/mod.rs `#[path="status_line.rs"] mod status_line;` (:34) and `render_status_footer(...)` (mod.rs:1902) called from display_context_usage() in both Ok/Err branches (:1877,:1887), which calls `output::render_text(&status_line::format_status(ctx), None, true)` (:1930) once per turn boundary. Renders `model … provider … context NN% … ₹spend`, captions via tr_or fallback, NO_COLOR => zero ANSI, truncates to terminal width with ellipsis. Default appearance unchanged. Evidence: status_line tests green (content has model+provider+NN%+₹, NO_COLOR no 0x1B, over-budget ellipsis within budget, percent clamp, no brand).
- v89 (UX wave-3) done: interactive tutorials (5 brand-neutral walkthroughs). Real call site: session/builder.rs already `#[path="../commands/tutorials.rs"] mod tutorials;` (:50) and calls `tutorials::first_run_nudge()` on the session-build path (:755) plus `tutorials::list()`/`tutorials::show(id)` behind BHARATCODE_TUTORIAL (:541-542) — genuinely reachable in the running binary. Also surfaced as `commands::tutorials` crate API via `pub mod tutorials;` + re-exports in commands/mod.rs. Locale-aware via crate::tr! fallback (no i18n table edits). Backward-compat signatures (list/show/first_run_nudge) preserved so builder.rs keeps compiling. Evidence: tutorials tests green (TUTORIALS unique lower-kebab ids, bodies non-empty + brand-neutral, render_one Some/None, nudge env-suppressed).
- v90 (i18n wave-3) done: locale/a11y doctor i18n-readiness deep-check. Real call site: commands/doctor.rs keeps `#[path="i18n_check.rs"] mod i18n_check;` (:49) and print_deep_checks() prints the single Status-glyph row via `i18n_check::i18n_readiness()` (doctor.rs:297) — default-visible on every `bharatcode doctor` run. Reports three-way en/hi/ta key-count parity (robust: derives Tamil from current en.json key set) + BHARATCODE_A11Y/NOTIFY/COST_DASHBOARD on/off; Ok when parity holds + toggles off, Warn otherwise. Read-only, non-fatal, no env gate. Evidence: i18n_check tests green (tri-locale parity, Ok with toggles off, Warn + reported-on when A11Y/NOTIFY set, three-locale normalize_locale, no brand leak).
