# BharatCode version roadmap (v1 → v30)

Each version is one dynamic-workflow cycle (understand → implement → build/test/verify)
on top of the green prior version. Invariant: `cargo build -p goose-cli
--no-default-features --features portable-default` stays green and cli tests pass.
Base = fork of block/goose (Apache-2.0); donor = openai/codex (Apache-2.0). Compliance
(LICENSE + LICENSES/* + NOTICE + MODIFICATIONS, attribute both) maintained throughout.

| v | Title | Goal |
|---|---|---|
| v1 | Rebrand + compliance | ✅ DONE — building, rebranded, telemetry-off, Apache-2.0 compliant, 234/234 tests |
| v2 | Local-first defaults | Default to Ollama/local; cloud opt-in; India model presets |
| v3 | Codex apply-patch | Vendor Codex apply-patch (Apache-2.0) as a tool, attributed |
| v4 | Codex exec sandbox | Port landlock/seccomp exec sandbox behind an exec trait |
| v5 | Release engineering | ✅ DONE — THIRD_PARTY_LICENSES.md (619 crates / 41 SPDX groups, scoped to portable-default), rebranded Dockerfile/installers/Justfile/release+docker workflows/release docs; build green, 234/234 |
| v6 | Indic i18n scaffold | ✅ DONE — `crates/goose-cli/src/i18n/` locale resolver (BHARATCODE_LANG→config→LANG→en) + `t()`/`tr!()` + embedded en.json/hi.json; 5 starter CLI strings wired (Hindi), English byte-identical; build green, 239/239 lib tests |
| v7 | India model presets | ✅ DONE — Sarvam/Krutrim declarative providers + `presets.rs` (Qwen-Coder/DeepSeek local via Ollama; Sarvam/Krutrim/Qwen/DeepSeek hosted), "Recommended (India / open-weight)" first-time-setup choice + `bharatcode presets` listing + i18n keys; fixed vercel trademark leak; build green, 239 cli + 35 goose declarative tests pass |
| v8 | Internal crate rename | goose-* crates/dirs/deps → bharatcode-* (uniform, build-verified) |
| v9 | INR cost ledger | Per-session/day/month spend tracked + shown in ₹ |
| v10 | Budget gate | Configurable INR budget cap with warn/deny |
| v11 | Codex execpolicy | Port Starlark exec policy for hardened command allow/deny |
| v12 | Diff rendering uplift | Cherry-pick Codex Ratatui diff rendering polish |
| v13 | Slash-command polish | Improve interactive slash commands / help |
| v14 | India inference gateways | Provider entries for India-hosted OpenAI-compatible gateways |
| v15 | Offline engine default | Enable in-process engine (candle/llamacpp) as an offline option |
| v16 | Session fork/share | Local session fork + export/import hardening |
| v17 | Approval modes | Refine ask/allow/deny + read-only/auto/full + --yolo |
| v18 | Verify-before-done | Agent runs project test/build/lint, reports Verified/Failed/Skipped |
| v19 | /goal autonomous mode | Bounded iterate-to-goal autonomy (Codex-style) |
| v20 | India recipe library | Curated recipes/templates for Indian dev workflows |
| v21 | Data-residency guard | Optional block of non-India endpoints; egress allowlist |
| v22 | MCP polish | MCP client/server refinements, OAuth |
| v23 | DPDP audit log | Local audit log of model/tool calls for compliance |
| v24 | Cost-aware routing | Route to cheapest capable model under budget |
| v25 | Privacy hardening | Full no-phone-home audit + offline assertion test |
| v26 | Hindi TUI completion | Complete Hindi translations across user-facing output |
| v27 | Eval suite | Offline benchmark / codex-parity eval harness |
| v28 | Doctor enhancements | Richer `bharatcode doctor` (providers, LSP, local engine, residency) |
| v29 | India installers | apt/brew/curl install scripts + checksums |
| v30 | 1.0 polish + docs | README/docs site, version bump, final compliance + trademark gate |

Status log is in iterations.md (append-only). Driven by the generic version engine
`/home/arbaz/wf-version.js` (bespoke for v2/v3); each version logged on completion.

## v31 → v100 (themed continuation)

Driven by the same engine (`/home/arbaz/wf-version.js`), build-green + zero-leak +
Apache-2.0 compliance invariants enforced per version. Themes (each ~10 versions):

- **v31–v40 — Providers & models:** more India/open-weight providers; model fallback
  & routing; response caching; embeddings; multimodal input; model registry;
  quantization presets; streaming improvements; per-model cost metadata; offline pack.
- **v41–v50 — Agent capabilities:** subagents; planner; persistent memory; codebase
  RAG; web search tool; multi-file refactor; test generation; code-review agent;
  doc generation; framework-migration agent.
- **v51–v60 — Enterprise & compliance:** DPDP mode; full audit log; RBAC; SSO; secret
  vault; egress allowlist; on-prem/air-gap profile; license scanning; SBOM; policy engine.
- **v61–v70 — Performance & reliability:** prompt/context caching; parallel tool exec;
  incremental context; token-budget optimizer; retry/backoff; crash recovery; session
  DB tuning; large-repo handling; streaming perf; resource limits.
- **v71–v80 — Ecosystem & extensibility:** plugin SDK; MCP registry; recipe library;
  extension catalog; theming; keybindings; scripting API; CI integration; deeper git
  integration; IDE bridges.
- **v81–v90 — UX & i18n:** complete Hindi + regional languages; accessibility; TUI
  polish; onboarding wizard; cost dashboard; notifications; help system; tutorials;
  themes; quick-start.
- **v91–v100 — Scale & release:** server/multi-user mode; privacy-preserving analytics;
  benchmark suite; packaging matrix; docs site; security hardening; perf release;
  1.0 → 2.0 GA; community; final compliance + trademark gate.

Each version is logged in iterations.md on completion. The chain runs continuously;
progress is persisted in the repo and resumable.
