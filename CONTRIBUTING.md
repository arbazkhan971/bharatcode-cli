# Contribution Guide

BharatCode is open source!

We welcome pull requests for general contributions! In these days of AI it is easier than ever to contribute, but
there are some pitfalls to avoid. This document describes the best practices for new and experienced contributors
to get work landed as smoothly as possible.

> [!TIP]
> Beyond code, check out [other ways to contribute](#other-ways-to-contribute)

---

## Getting Started

Your first contribution should probably be a small bug fix. Maintainers have a lot of incoming PRs to review, and
the reputation of the author is an important signal. While contributions are generally of remarkably high quality,
we do get our fair share of AI slop. When a first-time contributor opens a 3k line PR touching 20 files, we have no
easy way to tell whether it's thoughtful work or blindly AI-generated without doing a deep dive.

So please start small to establish trust and work your way up from there. A small bug fix or performance improvement
is a good start. Linking your fix to an existing issue shows that you are responding to a community need.

If your first PR gets closed with a link to this section, please don't take it personally.
It just means the change was too large for a first contribution. Start with something smaller and try again.

## Discussions, Issues and PRs

### Issues

If you spot a bug or have a concrete proposal for a feature, please open an issue. This shows the community and
the maintainers the direction of your thinking.

For bugs, describe how to reproduce the problem as clearly as possible. If the issue involves an interaction
with an LLM, include a diagnostics report if possible.

### Discussions

If you have an idea but are not yet sure how it should work, open a discussion instead. Discussions are a good
place to explore design questions, alternatives, and whether something fits the goals of the project.

If a change is large or touches multiple parts of the codebase, please start with a discussion before opening a PR.
This helps us align on direction before you spend time implementing something.

### Pull Requests

Open a PR when you have a concrete change ready. For first contributions we strongly recommend starting small
(see [Getting Started](#getting-started)). Don't open many PRs in quick succession. Submit them in order of 
your preference and wait for them to land before opening more. 

If the code is still evolving but useful for discussion, open the PR in draft mode. Draft PRs are for discussion, 
not just unfinished work. If it's not ready yet, keep the branch local.

### Feature Requests

Before proposing a new feature, consider whether it is something broadly useful or mainly a personal preference.
Adding features is easy; maintaining them is a long-term cost, so we may decline features that add complexity
without clear general benefit.


## Code Reviews

PRs are reviewed by maintainers and must pass the repository's automated checks. Address every review comment by
either updating the code or explaining why a change is not appropriate. Draft PRs are welcome when early feedback
would help shape the implementation.

## Quick Responsible AI Tips

There's no need to tell us you used AI in your work. You are contributing to an agent, it would be odd if 
you had not. Our general thinking is, use AI any way you want, but until the robot revolution comes, you
are responsible for the final code. Before submitting a PR for review, make sure you have reviewed it yourself.
We'll close any vibe coded submissions that obviously skip this step.

You can use whatever agent and whatever methodology you like as long as you stick to that principle. One thing to
watch out for is LLM eagerness. They like to please and are in a hurry.

   * **Think first**. Agents tend to jump straight to code writing. Explain the architecture you want first to 
      avoid this behavior, based on your own understanding of the code, or have the agent explore the code first and
      suggest approaches. If the first implementation doesn't look quite right, just start over and use
      what you learned to do better next time.
   * **Spot the laziness**. LLMs will make their job easy. They'll write trivial tests, make types wide and
      optional so the compiler doesn't complain, catch exceptions and just log instead of handling errors
      and copy local patterns whether appropriate or not. Push back!
   * **Spot the uncertainty**. As much as the bots declare I see the issue now clearly, they often do not. Call
      them on it, if you see the agent flailing. Another telltale sign is if the agent starts listing the
      number of ways it fixed an issue or starts writing overly defensive code.
   * **Spot the bloat**. Agents like to insert redundant comments or worse, commenting on the change at hand,
     not the resulting code. They create loads of tests that don't really test anything and if they do,
     test the implementation, not the intention. They also like to log anything, just in case.
   
## Prerequisites

BharatCode is a Rust workspace: the `bharatcode` CLI and the `bharatcoded` server. The only UI is a terminal UI
artifact under `ui/text/`, launched by `bharatcode tui`. There is no desktop app in this repository.

We use [Hermit][hermit] to manage development dependencies (Rust, Node, pnpm, just, etc.).
Activate Hermit when entering the project:

```bash
source bin/activate-hermit
```

Or add [shell hook auto-activation](https://cashapp.github.io/hermit/usage/shell/#shell-hooks) so Hermit activates automatically when you `cd` into the project (recommended).

We provide a shortcut to standard commands using [just][just] in our `Justfile`.

### Windows Subsystem for Linux

For WSL users, you might need to install `build-essential` and `libxcb` otherwise you might run into `cc` linking errors (cc stands for C Compiler).
Install them by running these commands:

```
sudo apt update                   # Refreshes package list (no installs yet)
sudo apt install build-essential  # build-essential is a package that installs all core tools
sudo apt install libxcb1-dev      # libxcb1-dev is the development package for the X C Binding (XCB) library on Linux
```

## Getting Started

### Rust

First let's compile BharatCode and try it out. Since BharatCode requires Hermit for managing dependencies, let's
activate hermit.

```
cd bharatcode-cli
source ./bin/activate-hermit
cargo build
```

When that completes, debug builds of the binaries are available, including the CLI:

```
./target/debug/bharatcode --help
```

A plain `cargo build` uses the default features, which include the in-process inference engines. For a much faster
build that drops them:

```
cargo build -p bharatcode-cli --no-default-features --features portable-default
```

For first-time setup, run the configure command:

```
./target/debug/bharatcode configure
```

Once a connection to an LLM provider is working, start a session:

```
./target/debug/bharatcode session
```

These same commands can be recompiled and immediately run using `cargo run -p bharatcode-cli` for iteration.
When making changes to the Rust code, test them on the CLI or run checks, tests, and the linter:

```
cargo check  # verify changes compile
cargo test  # run tests with changes
cargo fmt   # format code
cargo clippy --all-targets -- -D warnings # run the linter
```

`just check-everything` runs fmt, clippy and the OpenAPI schema check in one go.

### Terminal UI

The terminal UI artifact lives in `ui/text/`. `bharatcode tui` launches the current binary
without downloading code. `BHARATCODE_TUI_SCRIPT` can select an existing local launcher.

To build and test it:

```
just build-tui   # copies ui/text/src/tui.js to ui/text/dist/tui.js
just test-tui
```

### Regenerating the OpenAPI schema

The server's OpenAPI schema is generated by the `generate_schema` binary in `crates/bharatcode-server`:

```
just generate-openapi
```

API changes should be made in the Rust source under `crates/bharatcode-server/src/`.

The generated schema is committed at `crates/bharatcode-server/ui/desktop/openapi.json`.
Run `just check-openapi-schema` to compare it with a fresh temporary generation. Do not
hand-edit the generated JSON.

### Debugging

To debug the server, run it from an IDE. The configuration will depend on the IDE. The command to run is:

```
export BHARATCODE_SERVER__SECRET_KEY=test
cargo run --package bharatcode-server --bin bharatcoded -- agent   # or: `just run-server`
```

The server listens on port `3000` by default; this can be changed by setting the
`BHARATCODE_PORT` environment variable.

## Creating a fork

To fork the repository:

1. Go to https://github.com/arbazkhan971/bharatcode-cli and click "Fork" (top-right corner).
2. This creates https://github.com/<your-username>/bharatcode-cli under your GitHub account.
3. Clone your fork (not the main repo):

```
git clone https://github.com/<your-username>/bharatcode-cli.git
cd bharatcode-cli
```

4. Add the main repository as upstream:

```
git remote add upstream https://github.com/arbazkhan971/bharatcode-cli.git
```

5. Create a branch in your fork for your changes:

```
git checkout -b my-feature-branch
```

6. Sync your fork with the main repo:

```
git fetch upstream

# Merge them into your local branch (e.g., 'main' or 'my-feature-branch')
git checkout main
git merge upstream/main
```

7. Push to your fork. Because you're the owner of the fork, you have permission to push here.

```
git push origin my-feature-branch
```

8. Open a Pull Request from your branch on your fork to the main branch upstream.

## Keeping Your Fork Up-to-Date

To ensure a smooth integration of your contributions, it's important that your fork is kept up-to-date with the main 
repository. This helps avoid conflicts and allows us to merge your pull requests more quickly. Here's how you can sync your fork:

### Syncing Your Fork with the Main Repository

1. **Add the Main Repository as a Remote** (Skip if you have already set this up):

   ```bash
   git remote add upstream https://github.com/arbazkhan971/bharatcode-cli.git
   ```

2. **Fetch the Latest Changes from the Main Repository**:

   ```bash
   git fetch upstream
   ```

3. **Checkout Your Development Branch**:

   ```bash
   git checkout your-branch-name
   ```

4. **Merge Changes from the Main Branch into Your Branch**:

   ```bash
   git merge upstream/main
   ```

   Resolve any conflicts that arise and commit the changes.

5. **Push the Merged Changes to Your Fork**:

   ```bash
   git push origin your-branch-name
   ```

This process will help you keep your branch aligned with the ongoing changes in the main repository, minimizing integration issues when it comes time to merge!

### Before Submitting a Pull Request

Before you submit a pull request, please ensure your fork is synchronized as described above. This check ensures your changes are compatible with the latest in the main repository and streamlines the review process.

If you encounter any issues during this process or have any questions, please reach out by [opening an issue][issues], and we'll be happy to help.

## Env Vars

You may want to make more frequent changes to your provider setup or similar to test things out
as a developer. You can use environment variables to change things on the fly without redoing
your configuration.

The live environment namespace is `BHARATCODE_*`. You can change the provider BharatCode points to via
`BHARATCODE_PROVIDER` (and the model via `BHARATCODE_MODEL`). If you already have a credential for that provider in
your keychain from previously setting up, it should reuse it. For things like automations or to test without doing
official setup, you can also set the relevant env vars for that provider, for example `ANTHROPIC_API_KEY`,
`OPENAI_API_KEY`, or `DATABRICKS_HOST`. Refer to the provider details for more info on required keys.

See the README for the user-facing `BHARATCODE_*` options (residency, offline, budget, sandbox, theme, and so on).

### Isolating Test Environments

When testing changes or running multiple BharatCode configurations, use `BHARATCODE_PATH_ROOT` to isolate your data:

```bash
# Test with a clean environment
export BHARATCODE_PATH_ROOT="/tmp/bharatcode-test"
./target/debug/bharatcode session

# Or for a single command
BHARATCODE_PATH_ROOT="/tmp/bharatcode-dev" cargo run -p bharatcode-cli -- session
```

This creates isolated `config/`, `data/`, and `state/` directories under the specified path, preventing your test
sessions from affecting your main BharatCode installation.

## Enable traces with [locally hosted Langfuse](https://langfuse.com/docs/deployment/self-host)

- [Start a local Langfuse using the docs](https://langfuse.com/self-hosting/docker-compose). Create an organization and project and create API credentials.
- Set the environment variables so that BharatCode can connect to the langfuse server:

```
export LANGFUSE_INIT_PROJECT_PUBLIC_KEY=publickey-local
export LANGFUSE_INIT_PROJECT_SECRET_KEY=secretkey-local
```

Then you can view your traces at http://localhost:3000

## Conventional Commits

This project follows the [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/) specification for PR titles. Conventional Commits make it easier to understand the history of a project and facilitate automation around versioning and changelog generation.

[issues]: https://github.com/arbazkhan971/bharatcode-cli/issues
[hermit]: https://cashapp.github.io/hermit/
[just]: https://github.com/casey/just?tab=readme-ov-file#installation

## Other Ways to Contribute

There are numerous ways to be an open source contributor. Here are some suggestions to get started:

- **Stars on GitHub:** If you resonate with the project and find it valuable, consider starring it on GitHub! 🌟
- **Ask Questions:** Your questions not only help us improve but also benefit the community. If you have a question, [open a discussion](https://github.com/arbazkhan971/bharatcode-cli/discussions).
- **Give Feedback:** Have a feature you want to see or encounter an issue, [open an issue](https://github.com/arbazkhan971/bharatcode-cli/issues/new/choose).
- **Improve Documentation:** Good documentation is key to the success of any project. You can help improve the quality of our existing docs.
- **Help Other Members:** See another community member stuck? Or a contributor blocked by a question you know the answer to? Reply to threads or do a code review for others to help.
