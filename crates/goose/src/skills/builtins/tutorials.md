---
name: tutorials
description: |
  Guided, offline walkthroughs of core BharatCode workflows. Use this skill when a user is new, asks "how do I get started", "walk me through setup", "show me a tutorial", "how do I run offline / fully local", "how do I track or cap my cost / token spend", or "how do I switch the interface to Hindi or Tamil". It exposes a small embedded registry of step-by-step guides — getting-started, going-offline, controlling-cost, hindi-tamil-ui — each referencing real BharatCode commands. The onboarding wizard lists these tutorials; the agent can read the matching guide aloud and adapt the steps to the user's situation. Everything ships inside the binary: no network access and no files are written, so the guides work identically on a fresh install and on air-gapped machines.
---

# Interactive tutorials

A curated set of short, brand-neutral walkthroughs for the core BharatCode
workflows. Each one is embedded in the binary and references real commands, so
you can guide a user end to end without any network access.

Use this skill when the user is onboarding or asks how to do one of the
workflows below. Pick the tutorial whose topic matches the request, walk the
user through its steps in order, and adapt to what they have already done — do
not dump all four guides at once.

## Available tutorials

| id | When to use it |
| --- | --- |
| `getting-started` | First time using BharatCode: configure a provider/model and run an interactive session. |
| `going-offline` | Run fully local with a self-hosted model and privacy mode; useful for air-gapped or sensitive work. |
| `controlling-cost` | See token spend per session and set a budget guardrail before the bill arrives. |
| `hindi-tamil-ui` | Switch the interface language to Hindi or Tamil (untranslated strings fall back to English). |

## How to run a tutorial with the user

1. **Identify the right guide.** Map the user's goal to one of the four ids
   above. If they are brand new and unsure, start with `getting-started`.
2. **Walk the steps in order.** Each guide is a short numbered sequence built
   around real commands (`configure`, `session`, `cost`, `budget`, `privacy`,
   `doctor`). Present one step, confirm it worked, then move on.
3. **Adapt, don't recite.** Skip steps the user has already completed, and fill
   in the specifics they ask about (which provider, which model, where the
   config lives) rather than reading the guide verbatim.
4. **Offer the next guide.** When a workflow is done, suggest the logical
   follow-up (for example, after `getting-started`, offer `controlling-cost`).

The same registry is enumerated by the onboarding wizard, so the ids here match
exactly what a user sees when they list available tutorials.

## Principles

- **Offline-first.** Every guide is embedded; nothing here requires the network
  and nothing is written to disk. The guides render the same on a fresh install
  and in air-gapped environments.
- **Real commands only.** Reference the actual BharatCode subcommands and
  environment variables; never invent flags. If you are unsure of a command's
  exact options, say so rather than guessing.
- **Locale-aware.** Titles and summaries are translated where a locale entry
  exists and fall back to English otherwise, so guidance stays readable in every
  supported language (English, Hindi, Tamil).
- **One workflow at a time.** Solve the user's immediate goal with the single
  matching tutorial before offering the next one.
