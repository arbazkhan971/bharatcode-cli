# Justfile
#
# This repository builds the `bharatcode` CLI and the `bharatcoded` server.
# There is no Electron desktop app and no TypeScript SDK here: the recipes that
# used to target ui/desktop, ui/sdk and documentation/ have been removed because
# none of those directories exist in this tree.

# list all tasks
default:
  @just --list

# Run all style checks and formatting (precommit validation)
check-everything:
    @echo "🔧 RUNNING ALL STYLE CHECKS..."
    @echo "  → Formatting Rust code..."
    cargo fmt --all
    @echo "  → Running clippy linting..."
    cargo clippy --all-targets -- -D warnings
    @echo "  → Validating OpenAPI schema..."
    @just check-openapi-schema
    @echo ""
    @echo "✅ All style checks passed!"

# Build the local terminal UI artifact used by `bharatcode tui`.
build-tui:
    cd ui/text && node build.js

# Run local TUI artifact tests.
test-tui:
    cd ui/text && node test.js

# Default release command: builds target/release/{bharatcode,bharatcoded}
release-binary:
    @echo "Building release version..."
    cargo build --release

# Light CLI build: no in-process inference engines, no keyring
release-binary-portable:
    cargo build --release -p bharatcode-cli --no-default-features --features portable-default

# Build the server for Windows on a Windows host (MSVC target)
[unix]
release-windows:
    @echo "just release-windows requires a Windows host: it builds the x86_64-pc-windows-msvc target."
    @exit 1

[windows]
release-windows:
    @powershell.exe -NoProfile -ExecutionPolicy Bypass -Command 'rustup target add x86_64-pc-windows-msvc; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }; cargo build --release --target x86_64-pc-windows-msvc -p bharatcode-server; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }; Write-Host "Windows executable created at ./target/x86_64-pc-windows-msvc/release/bharatcoded.exe"'

# Build for Intel Mac
release-intel:
    @echo "Building release version for Intel Mac..."
    cargo build --release --target x86_64-apple-darwin

# Run server
run-server:
    @echo "Running server..."
    cargo run -p bharatcode-server --bin bharatcoded agent

# Regenerate the OpenAPI schema from the server routes
generate-openapi:
    @echo "Generating OpenAPI schema..."
    cargo run -p bharatcode-server --bin generate_schema

# Check the committed OpenAPI schema matches a fresh, temporary generation
check-openapi-schema:
    ./scripts/check-openapi-schema.sh

# Generate ACP JSON schema from Rust types
generate-acp-schema:
    @echo "Generating ACP schema..."
    cd crates/bharatcode-core && cargo run --features code-mode,local-inference,aws-providers,telemetry,otel,rustls-tls,system-keyring --bin generate-acp-schema
    @echo "ACP schema generated: crates/bharatcode-core/acp-schema.json, crates/bharatcode-core/acp-meta.json"

# Check the committed ACP schema is up-to-date
check-acp-schema: generate-acp-schema
    #!/usr/bin/env bash
    set -e
    echo "🔍 Checking ACP schema is up-to-date..."
    if ! git diff --exit-code crates/bharatcode-core/acp-schema.json crates/bharatcode-core/acp-meta.json; then
      echo ""
      echo "❌ ACP generated files are out of date!"
      echo ""
      echo "Run 'just generate-acp-schema' locally, then commit the changes."
      exit 1
    fi
    echo "✅ ACP schema is up-to-date"

# Generate manpages for the CLI
generate-manpages:
    @echo "Generating manpages..."
    cargo run -p bharatcode-cli --bin generate_manpages
    @echo "Manpages generated at target/man/"

ensure-release-source:
    #!/usr/bin/env bash
    branch=$(git rev-parse --abbrev-ref HEAD); \
    if [[ "$branch" != "main" && ! "$branch" == release/* ]]; then \
        echo "Error: releases must be tagged from main or a release branch (current: $branch)"; \
        exit 1; \
    fi

    if [[ -n "$(git status --porcelain)" ]]; then \
        echo "Error: the working tree must be clean before tagging a release"; \
        exit 1; \
    fi

    git fetch origin "$branch" --tags --prune
    # @{u} refers to upstream branch of current branch
    if [ "$(git rev-parse HEAD)" != "$(git rev-parse @{u})" ]; then \
        echo "Error: Your branch is not up to date with the upstream branch"; \
        echo "  ensure your branch is up to date (git pull)"; \
        exit 1; \
    fi

# Validate a new public release version.
validate version:
    #!/usr/bin/env bash
    if [[ ! "{{ version }}" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z]+([.-][0-9A-Za-z]+)*)?$ ]]; then
      echo "[error]: invalid version '{{ version }}'."
      echo "  expected: semver format major.minor.patch or major.minor.patch-<suffix>"
      exit 1
    fi

    if git rev-parse --quiet --verify "refs/tags/v{{ version }}" >/dev/null; then
      echo "[error]: tag 'v{{ version }}' already exists"
      exit 1
    fi

# rebuild canonical model registry and mapping report from models.dev
build-canonical-models:
    @cargo run -p bharatcode-core --bin build_canonical_models

# Create an annotated public release tag at the synchronized source commit.
tag version: ensure-release-source
    @just validate {{ version }}
    git tag --annotate "v{{ version }}" --message "BharatCode v{{ version }}"

# Create and push a public release tag. This starts the release workflow.
tag-push version: (tag version)
    git push origin "v{{ version }}"

# generate release notes from git commits
release-notes old version:
    #!/usr/bin/env bash
    git log --pretty=format:"- %s" {{ old }}..v{{ version }}

# Make just work on Windows
set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]

build-test-tools:
  cargo build -p bharatcode-test

record-mcp-tests: build-test-tools
  BHARATCODE_RECORD_MCP=1 cargo test --package bharatcode-core --test mcp_integration_test
  git add crates/bharatcode-core/tests/mcp_replays/
