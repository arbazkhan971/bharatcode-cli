# BharatCode CLI Harness Parity Iterations

Date: 2026-06-30

Goal: make the BharatCode CLI harness and TUI competitive with the strongest current coding-agent CLIs by turning research into small, verified iterations.

## Reference Set

Primary references used for this benchmark:

- OpenAI Codex CLI manual: https://developers.openai.com/codex/codex-manual.md
- OpenAI Codex CLI features: https://developers.openai.com/codex/cli/features
- OpenAI Codex slash commands: https://developers.openai.com/codex/cli/slash-commands
- OpenAI Codex approvals and sandboxing: https://developers.openai.com/codex/agent-approvals-security
- Anthropic Claude Code overview: https://docs.anthropic.com/en/docs/claude-code/overview
- Anthropic Claude Code hooks: https://docs.anthropic.com/en/docs/claude-code/hooks
- Anthropic Claude Code memory: https://docs.anthropic.com/en/docs/claude-code/memory
- Anthropic Claude Code subagents: https://docs.anthropic.com/en/docs/claude-code/sub-agents
- Aider docs: https://aider.chat/docs/
- Aider repo map: https://aider.chat/docs/repomap.html
- Aider lint/test: https://aider.chat/docs/usage/lint-test.html
- Aider git integration: https://aider.chat/docs/git.html
- Aider chat modes: https://aider.chat/docs/usage/modes.html
- OpenCode docs: https://opencode.ai/docs/
- OpenCode server architecture: https://opencode.ai/docs/server/
- OpenCode agents: https://opencode.ai/docs/agents/
- OpenCode permissions: https://opencode.ai/docs/permissions/
- OpenCode TUI: https://opencode.ai/docs/tui/
- Goose docs: https://goose-docs.ai/
- Goose CLI commands: https://goose-docs.ai/docs/guides/goose-cli-commands/
- Goose recipes: https://goose-docs.ai/docs/guides/recipes/
- Goose extensions: https://goose-docs.ai/docs/getting-started/using-extensions/
- Goose subagents: https://goose-docs.ai/docs/guides/context-engineering/subagents/
- Pi coding agent: https://github.com/earendil-works/pi/tree/main/packages/coding-agent
- Pi TUI package: https://github.com/earendil-works/pi/tree/main/packages/tui

## What "Best Harness" Means

No single CLI owns every dimension. The best target is a composite:

1. Codex CLI: strongest local safety model, slash-command breadth, session resume, app/server split, MCP/plugins/skills, review workflow, and explicit approvals.
2. Claude Code: strong memory/instruction system, hooks, subagents, permissions, IDE/cloud/terminal consistency, and managed enterprise controls.
3. Aider: pragmatic git-first workflow, repo map, automatic lint/test repair loops, explicit chat modes, and fast undo/review through commits.
4. OpenCode: clean client/server architecture, TUI attached to server state, typed SDK, multi-session support, agents/subagents, provider breadth, shareable sessions.
5. Goose: recipes as portable workflows, extensions/MCP-first design, subagents and orchestration, broad provider surface, desktop/CLI/API continuity.
6. Pi TUI: disciplined terminal rendering contract: component `render(width)`, differential rendering, synchronized output, overlays, focus management, autocomplete, and tests for width/CJK/style regressions.

## BharatCode Current Position

Observed strengths:

- Rust core with CLI, server, MCP, ACP, recipes, providers, semantic index, context compaction, permissions, session manager, and subagent execution modules.
- Existing recipe system and recipe validation.
- Existing MCP extension surface and built-in MCP servers.
- Existing ACP/Codex/Claude/Gemini/Cursor/Amp provider integration paths.
- Existing session status line, command palette, accessibility toggles, and task execution display.
- Existing `bharatcode-self-test.yaml` convention for agent self-test validation.

Observed gaps:

- `ui/text` is missing in this checkout, so `bharatcode tui` is mostly a launcher/fallback rather than a first-class local TUI.
- Terminal rendering is still mostly print/event output, not a component tree with a hard width/height contract.
- There is no virtual terminal snapshot test suite for the full TUI layout.
- Width-safe rendering exists in scattered modules; it needs to become a shared invariant.
- No top-level roadmap tracked the parity target before this file.
- Existing repo warnings prevent a clean `clippy --all-targets -- -D warnings` gate today.
- Session UX has many good components, but they are not yet composed into a Codex/OpenCode-style fullscreen interactive shell.

## Target Architecture

### TUI Layer

- Fullscreen terminal UI as the default interactive surface.
- Component interface with explicit width budget and optional height budget.
- Every rendered line must be measured and must fit its allocated columns.
- No unchecked wrapping inside fixed-height boxes.
- Message history, tool calls, diff previews, task progress, composer, footer, and overlays should be separate components.
- Virtual terminal tests should assert screenshots/frames for desktop-width, narrow-width, CJK, ANSI style reset, and long tool output cases.

### Harness Layer

- Event-sourced session loop: model deltas, tool calls, approvals, edits, tests, notifications, and compact events should be typed events.
- TUI should consume events; noninteractive runs should consume the same events.
- Session resume should restore transcript, plan/checklist, approvals, and pending queued input where safe.
- Server/headless mode should expose session state to TUI/web/API clients.

### Safety Layer

- Explicit approval modes with readable status in the TUI.
- Protected file patterns for secrets and generated directories.
- Permission summaries in `/status` or equivalent.
- Hook points before and after tool use, permission requests, compaction, and session stop.
- Network and filesystem policy should be visible and testable.

### Context Layer

- Repo map or semantic index enabled by default when cheap and bounded.
- File mention and fuzzy command/file picker.
- Compaction that preserves recent user intent, current diff, and active plan.
- Instructions discovery should be explicit and inspectable.

### Workflow Layer

- Slash commands for review, diff, status, model, permissions, compact, resume, recipes, MCP, skills, and goal/checklist.
- Recipes as shareable workflows with parameters, extensions, and subrecipes.
- Aider-style lint/test loops after edits, with a clear repair budget.
- Built-in review flow for working tree, staged changes, and branch diff.

## Iterations

### Iteration 1 - Local TUI Launch and Width Contracts

Status: completed locally.

Changes:

- `BHARATCODE_TUI_SCRIPT` is now honored before packaged script or npm fallback.
- Task execution display now has plain/a11y mode and terminal-width truncation.
- Task display tests cover ASCII/plain mode and width-budget invariants.
- Shared `session::terminal_width` helper added for reusable display-width truncation.

Validation:

- `cargo test -p bharatcode-cli commands::tui`
- `cargo test -p bharatcode-cli session::task_execution_display`
- `cargo test -p bharatcode-cli session::terminal_width`
- `git diff --check`

### Iteration 2 - In-Repo Research Tracker

Status: in progress.

Deliverable:

- Keep this `itrs.md` file updated after every harness/TUI parity pass.
- Add exact status, test commands, and remaining gaps after each iteration.

### Iteration 3 - First-Class Text UI Artifact

Status: completed.

Deliverable:

- Add or restore a real `ui/text` package or Rust TUI crate.
- `bharatcode tui` should run a local built artifact in development without npm fallback.
- Add build/test command for the TUI artifact.

Acceptance:

- `BHARATCODE_TUI_SCRIPT` works for local development.
- Default local checkout can build and run the TUI without downloading latest npm.
- TUI has at least one snapshot-like test (`ui/text/test.js`), with local runner entry at `just test-tui`.

### Iteration 4 - Event/Renderer Boundary

Status: completed.

Deliverable:

- Defined a small typed render event stream for session output.
- Added deterministic render variants for task progress, assistant text, tool calls, status/footer, and approvals.
- Routed task progress notifications and status footers through the typed render boundary.
- Left broader assistant streaming/tool rendering migration for the next renderer pass to avoid changing markdown/tool behavior in the same step.

Acceptance:

- Unit tests can render a deterministic event sequence into terminal lines.
- Width budget is enforced at render boundary.

Validation:

- `node ui/text/test.js`
- `CARGO_TARGET_DIR=/tmp/bharatcode-cli-target cargo test -p bharatcode-cli session::render_event`
- `CARGO_TARGET_DIR=/tmp/bharatcode-cli-target cargo test -p bharatcode-cli session::status_line`
- `CARGO_TARGET_DIR=/tmp/bharatcode-cli-target cargo test -p bharatcode-cli session::task_execution_display`
- `CARGO_TARGET_DIR=/tmp/bharatcode-cli-target cargo test -p bharatcode-cli commands::tui`

### Iteration 5 - Approval and Status UX

Status: pending.

Deliverable:

- Show active model, provider, approval mode, sandbox/permission profile, branch, and token/context pressure in one stable footer/status view.
- Add command/help entries for permission/status controls where missing.

Acceptance:

- `/status` or equivalent reports the active safety/context posture.
- Footer line never exceeds width budget.

### Iteration 6 - Recipe/Test Repair Loop

Status: pending.

Deliverable:

- Add a harness-visible edit verification loop inspired by Aider: run configured lint/test commands after agent edits and surface failures as structured repair events.
- Respect explicit user/build-test instructions from AGENTS.md.

Acceptance:

- Test failures appear as structured events.
- Repair loop has a configurable maximum retry count.

### Iteration 7 - Full TUI Snapshot Suite

Status: pending.

Deliverable:

- Virtual terminal tests for narrow terminal, long task names, long tool output, CJK/wide chars, ANSI reset, overlays, and queued input.

Acceptance:

- Render tests fail on overflow, style leaks, and layout regressions.

## Current Next Action

Keep the current patch focused:

1. Preserve the already-passing TUI launcher and task display changes.
2. Add the shared terminal-width helper and wire existing renderers to it.
3. Run targeted tests for `commands::tui`, `session::task_execution_display`, `session::terminal_width`, and `session::status_line`.
4. Update this file with validation results.
