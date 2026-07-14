# Making a Release

BharatCode releases are built and published by GitHub Actions from an explicit semantic-version tag. The public
release version comes from the tag; the workspace package version remains available for internal compatibility.
The release build injects the public version into every distributed binary.

## Release types

- Use a patch release such as `1.0.4` for compatible fixes and hardening.
- Use a minor release such as `1.1.0` for compatible features.
- Use a major release for incompatible public behavior.
- Add a SemVer suffix such as `1.1.0-rc.1` for a prerelease. Prereleases never update `stable`.

## Before tagging

1. Confirm the intended commit is on `main` and `main` is synchronized with `origin/main`.
2. Confirm CI is green for that commit.
3. Complete [RELEASE_CHECKLIST.md](RELEASE_CHECKLIST.md).
4. Confirm the target tag and GitHub release do not already exist.

## Publish

From a clean, synchronized `main` checkout, run:

```bash
just tag-push 1.1.0
```

The recipe validates the version, creates an annotated `v1.1.0` tag at `HEAD`, and pushes only that tag. The
tag-triggered `release.yml` workflow then:

1. validates the tag and derives the binary version;
2. builds every supported CLI target;
3. generates SHA-256 checksums and build-provenance attestations;
4. publishes the versioned GitHub release; and
5. updates the `stable` release only for a final version.

Do not create release tags through the GitHub web interface. A local annotated tag makes the source commit explicit
and lets the release recipe verify that the checkout is clean and synchronized first.

## Verify the published release

Watch the release workflow to completion, then verify:

```bash
gh run list --workflow release.yml --limit 1
gh release view v1.1.0
```

Check that `checksums.txt`, both installer scripts, and all expected platform archives are present. Install into a
temporary directory with a pinned `BHARATCODE_VERSION`, run `bharatcode --version`, and confirm it reports the tag's
version before announcing the release.

If a release job fails, fix the source on `main` and publish a new version. Do not move or overwrite an existing
version tag.
