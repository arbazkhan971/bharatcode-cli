# Modifications

BharatCode is a derivative work ("fork") of **Goose** (https://github.com/block/goose),
Copyright 2024 Block, Inc., licensed under the Apache License, Version 2.0. This file
documents the significant changes made by the BharatCode project, as required by
Apache-2.0 Section 4(b). The upstream license is retained in `LICENSE`, and the
upstream's licensed copy is kept at `LICENSES/LICENSE-goose`.

## Changes
- **Rebrand (trademarks removed, not licensed):** product name, CLI binary
  (`goose` → `bharatcode`), configuration directory / app name, and environment
  variable prefix (`GOOSE_` → `BHARATCODE_`) were changed to BharatCode. Upstream
  names and logos (Goose, Block, Square, Cash App, Tidal) were removed from
  user-facing surfaces. Trademarks remain the property of their respective owners
  and are not covered by the Apache-2.0 grant.
- **Telemetry off by default:** the bundled third-party product analytics
  (PostHog) is disabled by default; BharatCode does not phone home.
- **Local-first defaults:** configuration is oriented toward local / India-hosted
  model providers.
- **Indic i18n scaffold (original work):** added a lightweight CLI localization
  module (`crates/goose-cli/src/i18n/`) — a locale resolver
  (`BHARATCODE_LANG` → `bharatcode_lang` config → `LANG` → `en`), a `t(key)` /
  `tr!(key)` helper, and embedded flat `en.json` / `hi.json` string tables. A
  starter set of high-traffic CLI strings (ready banner, "no provider/model
  configured" errors, first-time-setup welcome) routes through it with a Hindi
  translation table; English output is unchanged. Original code, not ported.
- **India / open-weight model presets (original work):** added curated model
  presets so users can pick a recommended model fast. New OpenAI-compatible
  declarative providers `sarvam` (Sarvam AI) and `krutrim` (Ola Krutrim),
  India-hosted and gated by `SARVAM_API_KEY` / `KRUTRIM_API_KEY` (no secrets
  embedded). A presets data module (`crates/goose-cli/src/commands/presets.rs`)
  lists local Ollama presets (Qwen2.5-Coder, DeepSeek-Coder) and hosted India/Asia
  presets (Sarvam, Krutrim, Qwen via DashScope, DeepSeek). The first-time-setup
  flow offers a "Recommended (India / open-weight)" choice, and a `bharatcode
  presets` command lists them. Pure data + a thin CLI surface; not ported from any
  third party. Also corrected residual trademark leakage in
  `vercel_ai_gateway.json` attribution headers (`goose-docs.ai`/`goose` →
  `bharatcode-docs.ai`/`bharatcode`).

## Ported from OpenAI Codex (Apache-2.0)
The `crates/bharatcode-apply-patch` crate was vendored/ported from OpenAI Codex's
`codex-apply-patch` crate (https://github.com/openai/codex), Copyright 2025
OpenAI, licensed under the Apache License, Version 2.0. The donor's licensed copy
is kept at `LICENSES/LICENSE-codex`, and OpenAI is attributed in `NOTICE`.

Ported source files (each carries a top-of-file Codex attribution comment):
  - `seek_sequence.rs` — the fuzzy context line matcher (vendored verbatim).
  - `streaming_parser.rs` — the incremental `*** Begin Patch` parser (verbatim).
  - `parser.rs` — the patch parser producing `Hunk`s.
  - The `compute_replacements` / `apply_replacements` transform core (ported from
    Codex's `lib.rs` into `apply.rs`).

Modifications made during the port (per Apache-2.0 Section 4(b)):
  - Removed the dependence on `codex-exec-server`, `codex-utils-path-uri`,
    `codex-utils-absolute-path`, `tree-sitter`, and `tokio`. The vendored crate
    depends only on `thiserror`.
  - Dropped `Hunk::resolve_path` (which used `codex-utils-path-uri`) and the
    shell-heredoc detection in `invocation.rs` (tree-sitter based); the direct
    tool receives patch text as an argument, so heredoc parsing is unnecessary.
  - Re-implemented the filesystem apply step (`apply.rs`) on synchronous
    `std::fs` (`read_to_string` / `write` / `create_dir_all` / `remove_file` /
    `rename`-via-write+remove) instead of Codex's async `ExecutorFileSystem`
    sandbox trait.
  - Rewrote the parser unit tests that relied on Codex's `PathUri` /
    `AbsolutePath` test helpers to use `std::path::PathBuf` + `tempfile`.
  - The crate sets `edition = "2024"` in its own `Cargo.toml` (supported by the
    workspace toolchain) so the upstream let-chain syntax compiles unchanged.

The ported editor is wired into BharatCode as the `apply_patch` developer tool in
`crates/goose/src/agents/platform_extensions/developer` (declared in `get_tools`
and dispatched in `call_tool`).

## Ported from OpenAI Codex (Apache-2.0) — Linux exec sandbox
The `crates/bharatcode-linux-sandbox` crate was vendored/ported from OpenAI
Codex's `codex-linux-sandbox` crate (`linux-sandbox/src/landlock.rs`,
https://github.com/openai/codex), Copyright 2025 OpenAI, licensed under the
Apache License, Version 2.0. The donor's licensed copy is kept at
`LICENSES/LICENSE-codex`, and OpenAI is attributed in `NOTICE`.

Ported source (carries a top-of-file Codex attribution comment):
  - `src/lib.rs` — the pure in-process primitives: `set_no_new_privs`
    (`PR_SET_NO_NEW_PRIVS` via `libc::prctl`), the Landlock ABI::V5 filesystem
    ruleset (read-all of `/`, write to `/dev/null` + writable roots), and the
    Restricted-mode network seccomp filter (denies ptrace / process_vm_* /
    io_uring and connect/bind/listen/send*/sockopt, plus socket()/socketpair()
    for every address family except AF_UNIX).

Modifications made during the port (per Apache-2.0 Section 4(b)):
  - Dropped the dependence on `codex-protocol` (`PermissionProfile`,
    `NetworkSandboxPolicy`, `CodexErr`, `SandboxErr`) and on
    `codex-utils-absolute-path::AbsolutePathBuf`; replaced them with a plain
    local `SandboxPolicy { writable_roots: Vec<PathBuf>, allow_network: bool }`
    and a `thiserror` `SandboxError` enum.
  - Dropped the bubblewrap launcher (`bwrap.rs`, `launcher.rs`,
    `bundled_bwrap.rs`) and the heavier `codex-sandboxing` crate to avoid LGPL
    (bubblewrap) and unrelated dependencies; kept only the in-process
    landlock + seccomp path.
  - Dropped the proxy-routed (`NetworkSeccompMode::ProxyRouted`) network mode
    and the managed-network / `to_runtime_permissions` orchestration; kept only
    the Restricted-mode network seccomp filter.
  - Replaced the unsupported-architecture `unimplemented!()` panic with a
    returned `SandboxError::UnsupportedArch` so a sandbox request on a
    non-x86_64/aarch64 CPU degrades gracefully instead of aborting the child.
  - Added a no-op `apply_to_current_thread` stub on non-Linux targets so callers
    can link the crate unconditionally; the heavy landlock/seccompiler/libc deps
    are gated behind `cfg(target_os = "linux")`.

The ported sandbox is exposed behind a `SandboxExt::apply_sandbox` exec trait in
`crates/goose/src/subprocess.rs` (a `pre_exec` hook mirroring the existing
`configure_parent_death_signal`) and wired into the developer `shell` tool
(`developer/shell.rs`) behind the opt-in `BHARATCODE_SANDBOX` environment toggle
(`off` | `read-only` | `workspace-write`, default `off`). It stays default-off so
the portable-default build and the `goose-cli` lib tests remain unaffected.

No upstream copyright notices have been removed from source files; only
user-facing trademark usage was changed.

## Provenance
Forked from block/goose (Apache-2.0) at upstream commit 6c2ec554de1632636d484e4124fbb3c011105342. Donor: openai/codex (Apache-2.0).
