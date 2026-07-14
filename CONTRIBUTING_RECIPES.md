# Contributing BharatCode recipes

Recipes are versioned YAML workflows. Repository-maintained examples live in
`.github/recipes/`; use them as the source of truth for the supported shape.

## Add a recipe

1. Fork [bharatcode-cli](https://github.com/arbazkhan971/bharatcode-cli/fork).
2. Add a uniquely named `.yaml` file under `.github/recipes/`.
3. Keep the recipe focused, review every extension it launches, and pin external inputs where
   the recipe format permits it.
4. Run the recipe locally before opening a pull request.
5. Explain the intended use, required credentials, and security-sensitive behavior in the PR.

A minimal recipe looks like this:

```yaml
version: "1.0.0"
title: Example review
description: Review a source tree for correctness issues.

extensions:
  - type: builtin
    name: developer

parameters:
  - key: scope
    input_type: string
    requirement: required
    description: Repository-relative path to review.

prompt: |
  Review {{ scope }} and report concrete correctness issues with file references.
```

Do not embed secrets in recipe files. Treat `stdio` extension commands as executable code:
avoid shell wrappers, document packages and arguments, and expect the runtime extension policy
to reject commands that are not trusted.

Before submitting, verify that the YAML parses, required parameters are documented, referenced
paths exist, and the recipe completes from a clean checkout. Questions and proposed format
changes belong in the pull request or a
[GitHub discussion](https://github.com/arbazkhan971/bharatcode-cli/discussions).
