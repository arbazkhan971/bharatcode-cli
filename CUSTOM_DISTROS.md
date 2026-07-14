# Custom BharatCode distributions

BharatCode is Apache-2.0 licensed and can be embedded or redistributed under a different product
name. Keep the license and third-party notices, avoid upstream trademarks, and clearly identify
your changes.

## Supported customization points

- Providers: implement the provider contract in `crates/bharatcode-providers/src/base.rs` and
  register the integration under `crates/bharatcode-core/src/providers/`.
- Built-in tools: add MCP functionality under `crates/bharatcode-mcp/` or a platform extension
  under `crates/bharatcode-core/src/agents/platform_extensions/`.
- Server integrations: build against the routes in `crates/bharatcode-server/src/routes/`; treat
  the generated OpenAPI schema as an artifact, not hand-authored source.
- Terminal experience: the CLI surface is in `crates/bharatcode-cli/src/cli.rs`; the optional
  launcher artifact is under `ui/text/`.
- Recipes: distribute reviewed YAML workflows and explicitly document every external command,
  credential, and network destination they require.

## Security invariants to preserve

- Keep SmartApprove as the default; make unrestricted execution an explicit user choice.
- Require authentication on non-loopback server binds and reject empty secrets.
- Do not weaken path containment, subprocess cancellation, extension command policy, SSRF
  controls, or release checksum/provenance verification.
- Store tokens and secrets in private files or a platform keyring; keep payload logging opt-in.
- Use the `BHARATCODE_*` environment namespace consistently in a renamed distribution.

## Release checklist

1. Update package metadata, binary names, repository URLs, container labels, and generated
   schemas together.
2. Run the workspace tests, formatter, clippy, OpenAPI check, installer self-test, terminal UI
   test, and any service-specific tests.
3. Publish checksums and GitHub build provenance for every archive and container digest.
4. Test install, update, rollback, authenticated server startup, and the default approval flow
   from a clean user profile.

See [BUILDING_DOCKER.md](BUILDING_DOCKER.md) for the maintained container workflow and
[CONTRIBUTING.md](CONTRIBUTING.md) for the development commands.
