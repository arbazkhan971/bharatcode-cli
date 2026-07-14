# AGENTS Instructions

bharatcode is a local-first AI agent framework in Rust. It ships a terminal CLI
(`bharatcode`) and a backend server (`bharatcoded`). There is **no Electron desktop app
in this repository** — the only UI is the terminal UI artifact under `ui/text/`, which
`bharatcode tui` launches.

## Setup
```bash
source bin/activate-hermit
cargo build
```

## Commands

### Build
```bash
cargo build                                                    # debug, default features
cargo build --release                                          # release
cargo build -p bharatcode-cli --no-default-features \
  --features portable-default                                  # light CLI build (no llama.cpp/candle/mlx, no keyring)
just release-binary                                            # cargo build --release
```

### Test
```bash
cargo test                            # all tests
cargo test -p bharatcode-core         # specific crate
cargo test --package bharatcode-core --test mcp_integration_test
just record-mcp-tests                 # re-record MCP replay fixtures
```

### Lint/Format
```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
```

### Terminal UI (ui/text)
```bash
just build-tui   # node build.js — copies src/tui.js to dist/tui.js
just test-tui    # node test.js
```
`bharatcode tui` launches the current BharatCode binary without network resolution. Set
`BHARATCODE_TUI_SCRIPT` to an existing local JavaScript launcher to override it.

## Structure
```
crates/
├── bharatcode-core          # core logic, agents, ACP, permission/security
├── bharatcode-cli           # CLI entry (binary: bharatcode)
├── bharatcode-server        # backend (binary: bharatcoded)
├── bharatcode-providers     # Provider trait + canonical model registry
├── bharatcode-mcp           # MCP extensions
├── bharatcode-apply-patch   # streaming patch parser/applier
├── bharatcode-linux-sandbox # Linux exec sandbox (landlock + seccomp)
├── bharatcode-sdk           # SDK (+ bharatcode-sdk-types)
├── bharatcode-acp-macros    # ACP proc macros
├── bharatcode-test          # test utilities
└── bharatcode-test-support  # test helpers

ui/text/            # terminal UI artifact + launcher for `bharatcode tui`
services/           # ask-ai-bot (TypeScript)
scripts/            # helper + release scripts
```

## Development Loop
```bash
# 1. source bin/activate-hermit
# 2. Make changes
# 3. cargo fmt
```

### Run these only if the user has asked you to build/test your changes:
```
# 1. cargo build
# 2. cargo test -p <crate>
# 3. cargo clippy --all-targets -- -D warnings
```

## Rules

- Test: Prefer tests/ folder, e.g. crates/bharatcode-core/tests/
- Test: When adding features, update bharatcode-self-test.yaml, rebuild, then run `bharatcode run --recipe bharatcode-self-test.yaml` to validate
- Error: Use anyhow::Result
- Provider: Implement the `Provider` trait, see crates/bharatcode-providers/src/base.rs
- MCP: Extensions in crates/bharatcode-mcp/
- Env: The live env namespace is `BHARATCODE_*` (e.g. `BHARATCODE_PROVIDER`,
  `BHARATCODE_PATH_ROOT`, `BHARATCODE_SERVER__SECRET_KEY`). No `GOOSE_*` variable is read
  by the current code — do not reintroduce them.
- Server: Route/type changes affect the OpenAPI schema. `just generate-openapi` regenerates
  `crates/bharatcode-server/ui/desktop/openapi.json`; `just check-openapi-schema` verifies it.

## Known gaps

- **ACP types**: `just generate-acp-schema` regenerates
  `crates/bharatcode-core/acp-schema.json` and `acp-meta.json`. The old TypeScript type
  generation targeted `ui/sdk/`, which does not exist here; that step is gone.

## Code Quality

- Comments: Write self-documenting code - prefer clear names over comments
- Comments: Never add comments that restate what code does
- Comments: Only comment for complex algorithms, non-obvious business logic, or "why" not "what"
- Simplicity: Don't make things optional that don't need to be - the compiler will enforce
- Simplicity: Booleans should default to false, not be optional
- Errors: Don't add error context that doesn't add useful information (e.g., `.context("Failed to X")` when error already says it failed)
- Simplicity: Avoid overly defensive code - trust Rust's type system
- Logging: Clean up existing logs, don't add more unless for errors or security events

## Ink / Terminal UI (ui/text)

- Ink renders React to a fixed character grid — not a browser. Content that exceeds a Box's dimensions is NOT clipped; it visually overflows into neighboring cells and breaks the layout.

- Ink-Text: Never use `wrap="wrap"` inside a fixed-height Box — wrapped text can exceed the Box height and bleed into adjacent components. Use `wrap="truncate"` and pre-truncate the string to fit the available character budget (lines × width).
  
- Ink-Layout: When changing card/cell dimensions, always recalculate how much content fits. Account for borders (2 chars), padding, margins, and sibling elements when computing the
remaining space for dynamic text.
  
- Ink-Overflow: Ink has no `overflow: hidden`. The only way to prevent overflow is to ensure content never exceeds the container size — truncate text, limit list items, or cap height.
  
- Ink-FlexGrow: Avoid `flexGrow={1}` on text containers inside fixed-height cards — the text will try to fill available space but Ink won't clip it if it exceeds the boundary.
  
- Ink-HeightBudget: When computing how many rows/items fit vertically, count EVERY line used by headers, footers, margins, borders, and scroll indicators. Under-reserving vertical space (e.g., `height - 8` when chrome actually uses 16 lines) causes Ink to squeeze out margins between items, making borders collapse. Always audit the actual line count.
  
- Ink-TrailingMargin: Don't apply `marginBottom` to the last item in a list — it wastes a line and can push content out of the container. Use conditional margins or container `gap`.

## Never

- Never: Hand-edit generated schemas (`crates/bharatcode-server/ui/desktop/openapi.json`, `crates/bharatcode-core/acp-schema.json`, `crates/bharatcode-core/acp-meta.json`)
- Never: Reference `ui/desktop`, `ui/sdk`, `documentation/`, or `evals/` — none of them exist in this repository
- Never: Use `goose-*` crate names (`goose-cli`, `goose-server`, `goose-test`) or the `crates/goose` path — the crates are `bharatcode-*`
- Cargo.toml: For human-authored dependency changes, use `cargo add` instead of manually editing dependency entries unless there is a specific reason not to.
- Cargo.toml: Automated dependency bump PRs are exempt; when manual edits are necessary, keep `Cargo.lock` consistent.
- Never: Skip cargo fmt
- Never: Merge without running clippy
- Never: Comment self-evident operations (`// Initialize`, `// Return result`), getters/setters, constructors, or standard Rust idioms

## Entry Points
- CLI: crates/bharatcode-cli/src/main.rs (arg surface: crates/bharatcode-cli/src/cli.rs)
- Server: crates/bharatcode-server/src/main.rs
- Agent: crates/bharatcode-core/src/agents/agent.rs
- TUI launcher: crates/bharatcode-cli/src/commands/tui.rs → ui/text/dist/tui.js
