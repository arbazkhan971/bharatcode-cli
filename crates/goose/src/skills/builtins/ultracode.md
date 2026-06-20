---
name: ultracode
description: |
  Structured dynamic workflows for large, multi-step engineering tasks. Use this skill when the user types "$ultracode" / "use ultracode" / "ultracode this", OR asks to take on work too big for a single pass — repo-wide audits, migrations, feature work that needs discovery + edits + tests, security reviews, or anything where the answer must be comprehensive and verified rather than guessed. It teaches you to decompose the task (plan → split → run independent work → check results → integrate → verify), to delegate independent pieces to parallel subagents when your harness supports them, to keep integration and final decisions in the parent session, and to back every claim with reproducible evidence instead of confidence. Default to lightweight Direct mode for small tasks; only spin up workflow artifacts when the task is genuinely large.
---

# Ultracode — structured dynamic workflows

Ultracode is a *procedure*, not a runner. It scales you from "one prompt, one pass" to
"plan → split → run independent work → check results → integrate → verify" so large tasks
land complete, correct, and auditable. Nothing here requires a special binary: you drive it
with whatever orchestration your host gives you (parallel subagents if available, otherwise
sequential planned steps).

## Pick the mode FIRST (do not over-engineer)

Choose the smallest mode that fits. Most requests are Direct.

1. **Direct mode** — single-pass, no artifacts. Use for typo fixes, single-file reads,
   one-line answers, anything you can finish correctly in one shot. Just do it. Do NOT
   create a `.workflow/` directory for these.
2. **Workflow mode** — planned, multi-step, *no* parallel agents. Use when the task has
   several dependent steps but the environment has no subagents (or splitting adds no value).
   Write the plan + state artifacts, then execute the steps yourself in order, checking off
   each one.
3. **Delegated mode** — split independent work across parallel subagents. Use when the task
   decomposes into pieces that don't depend on each other (e.g. "review these 9 modules",
   "port these 6 files", "search for X across the repo five different ways"), AND your
   harness can spawn subagents/tasks. The parent session keeps integration and final calls.

If unsure between Workflow and Delegated: if the pieces are independent and there are ≥3 of
them, prefer Delegated.

## The sequence (Workflow / Delegated)

> **plan → split → run independent work → check results → integrate → verify**

### 1. Plan
Restate the goal in one sentence. List the deliverables and the definition of done
(build passes? tests green? a written report? a merged patch?). Identify the unknowns that
must be discovered before editing.

### 2. Split
Break the work into **independent packets**. A packet = a self-contained unit of work with
(a) a crisp objective, (b) the exact files/area it owns, (c) what it must return. Packets in
the same wave **must be disjoint** — no two may edit the same file, or they will clobber each
other when run in parallel. Shared/contended files get assigned to **at most one** packet per
wave; everything else should be new files.

### 3. Run independent work
- **Explorers** (read-only) inspect independent areas and return findings *with citations*
  (`file:line`), never opinions without sources.
- **Workers** make edits strictly within their packet's owned files. A worker that needs to
  touch a file outside its scope must STOP and report, not reach across the boundary.
- Run packets in the same wave concurrently when delegating; otherwise execute them in order.

### 4. Check results
For each returned packet: did it meet its objective? Are its claims backed by evidence you
can re-derive (a quoted line, a command output, a test name)? Reject and re-run packets whose
results are unsupported. Do **not** accept "I think it works."

### 5. Integrate (parent session only)
The parent — never a subagent — merges the packet results into the real tree. Resolve
conflicts by **going back to the source** (read the actual code/diff), not by majority vote
among agents. Record every conflict and its resolution. Anything you cannot resolve from
evidence goes into a "Verification still needed" list and stays there — you do not guess.

### 6. Verify
Run the real gates: build, tests, linters, and a manual smoke of the changed surface. The
task is done only when the definition-of-done from step 1 is objectively met. State the
evidence (test counts, command output) plainly; if a gate fails, say so.

## Artifacts (Workflow / Delegated only)

For non-trivial runs, keep a durable trail so the run is resumable and auditable:

```
.workflow/ultracode/<run-slug>/
  ├── plan.md            # goal, deliverables, definition-of-done, unknowns
  ├── orchestration.md   # the packets, their owned files, the wave grouping
  ├── state.json         # progress: per-packet status (pending|running|done|rejected)
  ├── packets/           # one brief per packet (objective + scope + expected return)
  ├── results/           # one result per packet (findings/edits + evidence)
  ├── integration.md     # merge decisions, conflicts + how each was resolved
  └── final-report.md    # what changed, verification evidence, "verification still needed"
```

`<run-slug>` is a short kebab-case name for the task (e.g. `audit-auth-paths`).

## Rules that keep it correct

- **Evidence over confidence.** Every finding and every handoff cites a source (`file:line`,
  a command's output, a test name). Unsupported claims are rejected.
- **Disjoint waves.** Parallel packets never share a file. Contended files → one owner per wave.
- **Parent owns integration.** Subagents propose; the parent decides and merges.
- **Conflicts resolved by source, not vote.** Re-read the truth; don't average opinions.
- **Honest gaps.** Unresolved items live in "Verification still needed" — never papered over.
- **Right-size it.** Small task → Direct mode. Don't manufacture ceremony.

## Invocation

The user opts in with `$ultracode`, "use ultracode", or by asking for large end-to-end work
with parallel agents + verification. Example:

> Use $ultracode to implement this feature end to end and verify it. Use parallel agents when
> the task can be split, keep integration in the parent session, and verify the final patch.

When invoked, announce the mode you picked and why, then proceed through the sequence.
