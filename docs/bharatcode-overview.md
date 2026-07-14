# BharatCode architecture overview

BharatCode is a Rust workspace that ships a terminal CLI (`bharatcode`) and backend server
(`bharatcoded`). It is local-first: configuration, sessions, approvals, and built-in tools are
implemented in the workspace rather than delegated to a desktop application.

## Main crates

| Crate | Responsibility |
| --- | --- |
| `bharatcode-cli` | CLI parsing, configuration flows, terminal sessions, update/install commands |
| `bharatcode-core` | Agent loop, providers, permissions, sessions, extensions, ACP, security controls |
| `bharatcode-providers` | Provider contracts, model types, retries, HTTP policy |
| `bharatcode-server` | Authenticated HTTP backend and generated OpenAPI schema |
| `bharatcode-mcp` | Built-in MCP tools and subprocess helpers |
| `bharatcode-sdk` | Rust SDK and optional UniFFI bindings |
| `bharatcode-linux-sandbox` | Linux Landlock/seccomp execution sandbox |

Key entry points are `crates/bharatcode-cli/src/main.rs`,
`crates/bharatcode-server/src/main.rs`, and
`crates/bharatcode-core/src/agents/agent.rs`.

## Safety model

Fresh sessions default to SmartApprove rather than unrestricted execution. Shell and extension
processes pass through permission and policy checks; non-loopback server binds require a nonempty
secret; OAuth caches use private atomic files; request payload logging is opt-in. Egress,
residency, offline, and sandbox controls are configured through the `BHARATCODE_*` environment
namespace and the normal configuration store.

## Build and verify

```bash
cargo build
cargo test --workspace --all-targets
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

The portable CLI build omits in-process inference engines and the system keyring:

```bash
cargo build -p bharatcode-cli --no-default-features --features portable-default
```

Server route or type changes require regenerating
`crates/bharatcode-server/ui/desktop/openapi.json` with `just generate-openapi`.

## Other maintained surfaces

- `ui/text/`: local terminal launcher artifact used by `bharatcode tui`
- `services/ask-ai-bot/`: Discord assistant, built and tested with Bun in a container
- `.github/workflows/`: Rust CI, CLI releases, canary releases, container publication, and
  supply-chain checks
- `.github/recipes/`: repository-maintained recipe examples
