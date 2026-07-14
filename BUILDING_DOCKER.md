# Building and running BharatCode with Docker

The repository ships a multi-stage image for the `bharatcode` CLI. Both the Rust builder
and runtime base images are digest-pinned, the final process runs as an unprivileged user,
and release images are published with GitHub build provenance.

## Pull the published image

```bash
docker pull ghcr.io/arbazkhan971/bharatcode:latest
docker run --rm ghcr.io/arbazkhan971/bharatcode:latest --version
```

Pass provider credentials and mount the working tree only when needed:

```bash
docker run --rm -it \
  -e BHARATCODE_PROVIDER=openai \
  -e BHARATCODE_MODEL=gpt-4o \
  -e OPENAI_API_KEY \
  -v "$PWD:/workspace" \
  -w /workspace \
  ghcr.io/arbazkhan971/bharatcode:latest \
  run -t "Review this repository"
```

Avoid baking API keys into an image or passing them as Docker build arguments. Prefer a
runtime secret mechanism in CI and a read-only source mount when the agent does not need to
edit files.

## Build locally

```bash
git clone https://github.com/arbazkhan971/bharatcode-cli.git
cd bharatcode-cli
docker build --pull -t bharatcode:local .
docker run --rm bharatcode:local --help
```

For a multi-platform image:

```bash
docker buildx build --platform linux/amd64,linux/arm64 -t bharatcode:multi .
```

## Persist configuration

The runtime user is `bharatcode` (UID 1000), with home directory `/home/bharatcode`.

```bash
docker volume create bharatcode-config
docker run --rm -it \
  -v bharatcode-config:/home/bharatcode/.config/bharatcode \
  bharatcode:local configure
```

The default image entrypoint is `/usr/local/bin/bharatcode`; arguments after the image name
are passed directly to the CLI.

## Verify a published image

The publish workflow attaches GitHub artifact attestations to the registry digest. With the
GitHub CLI installed, verify that the image was built by this repository:

```bash
gh attestation verify \
  oci://ghcr.io/arbazkhan971/bharatcode:latest \
  --repo arbazkhan971/bharatcode-cli
```
