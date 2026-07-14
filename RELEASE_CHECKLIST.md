# BharatCode Release Checklist

## Version: `vX.Y.Z`

Record the release manager, date, target commit, and links to the successful CI and release workflow runs.

## Source and automated gates

- [ ] The target commit is on `main` and matches `origin/main`.
- [ ] The working tree is clean.
- [ ] `cargo fmt --all -- --check` passes.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes.
- [ ] `cargo test --workspace` passes.
- [ ] `./scripts/check-openapi-schema.sh` passes.
- [ ] The ACP schema generator is deterministic and committed output is current.
- [ ] Relevant non-Rust service and UI tests pass.
- [ ] Added dependencies, actions, and release containers are pinned and reviewed.
- [ ] The outgoing diff contains no credentials, private keys, generated scratch files, or unexpected binaries.

## Release build smoke test

- [ ] Build the distributed CLI feature profile:

  ```bash
  cargo build --release -p bharatcode-cli --bin bharatcode \
    --no-default-features --features rustls-tls
  ```

- [ ] `target/release/bharatcode --version` succeeds.
- [ ] `target/release/bharatcode --help` succeeds and lists the expected commands.
- [ ] Start an interactive session and complete a basic prompt with a configured provider.
- [ ] Verify sensitive tool use asks for approval by default.
- [ ] Verify unattended execution denies approval-required actions.
- [ ] Add and invoke a trusted MCP extension.
- [ ] Confirm an untrusted executable extension is rejected until explicitly trusted.
- [ ] Start the server on loopback and verify authenticated health/API access.
- [ ] Confirm a non-loopback server bind without the required security configuration is rejected.

## Data and compatibility

- [ ] Create, resume, list, and export a session.
- [ ] Verify sessions with legacy and RFC 3339 timestamps sort correctly.
- [ ] Load representative existing recipes and configuration files.
- [ ] Run database integrity and migration checks against a disposable copy of existing user data.
- [ ] Confirm installer and updater asset names match the release workflow contract.

## Publish and post-publish verification

- [ ] The new semantic-version tag does not already exist locally or remotely.
- [ ] Push the annotated tag with `just tag-push X.Y.Z`.
- [ ] The `Release` workflow completes successfully for every platform matrix entry.
- [ ] The versioned release contains `checksums.txt`, both installer scripts, and every expected archive.
- [ ] Build-provenance attestations exist for published assets.
- [ ] A prerelease does not update `stable`; a final release does.
- [ ] Install the pinned version into a temporary directory on at least one supported platform.
- [ ] The installed binary reports the public tag version.
- [ ] Checksum verification fails closed when the manifest or archive is tampered with.
- [ ] The updater discovers the final release and verifies its checksum before replacement.

## Sign-off

- Release manager:
- Date:
- Target commit:
- CI run:
- Release run:
- Notes or follow-up issues:
