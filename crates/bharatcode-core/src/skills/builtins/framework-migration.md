---
name: framework-migration
description: |
  Disciplined helper for migrating a codebase from one framework, library, or major version to another. Use this skill whenever the task is to port, migrate, convert, swap, or upgrade between two technologies that share a problem domain — e.g. Express → Fastify, React class components → hooks, Flask → FastAPI, Mocha → Vitest, Diesel → SeaORM, Webpack → Vite, Moment → Day.js, Enzyme → Testing Library, Redux → Zustand, requests → httpx, JUnit 4 → JUnit 5, or a major-version bump with breaking API changes (React 17 → 18, Angular n → n+1). It enforces a deterministic procedure: detect and pin the source + target versions, inventory the source API surface actually used (grep imports and call sites), build an explicit old → new mapping table, migrate leaf modules first while running the project's own build and tests between batches, and emit a MIGRATION_REPORT.md of what is done and what is blocked. Works entirely from the local checkout — no network access or hosted service is required.
---

# Framework migration helper

A repeatable, build-gated procedure for moving a codebase from a **source** framework/library/version
to a **target** one without losing track of what changed or breaking the build halfway through.

Migrations fail when they are done all-at-once and ad-hoc: the build stays red for days, the diff is
unreviewable, and nobody can tell which APIs still need porting. This skill replaces that with a
**measured, leaf-first, build-gated** loop driven by an explicit mapping table.

## When to use

Use this when the task is to **migrate between two things that solve the same problem**:

- HTTP frameworks — Express → Fastify, Flask → FastAPI, Koa → Hono.
- UI patterns — React class components → hooks, Options API → Composition API.
- Test runners — Mocha → Vitest, Jest → Vitest, JUnit 4 → JUnit 5, Enzyme → Testing Library.
- ORMs / DB layers — Diesel → SeaORM, SQLAlchemy core → ORM, raw SQL → query builder.
- Build/tooling — Webpack → Vite, Babel → SWC, npm → pnpm.
- Libraries — Moment → Day.js, requests → httpx, lodash → native, Redux → Zustand.
- **Major-version upgrades** with breaking changes — React 17 → 18, Angular n → n+1, Python 2 → 3 idioms.

Do NOT use this for: greenfield code with no source to migrate from, or a pure dependency-version
bump with no API changes (just bump the manifest and run the build).

## Procedure (do these in order)

### 1. Detect source + target and pin versions

- Read the dependency manifest (`Cargo.toml`, `package.json`, `pyproject.toml` / `requirements.txt`,
  `pom.xml`, `go.mod`) to confirm the **source** is actually present and at which version.
- Confirm the **target** name and the exact version you intend to land on. Pin both — record
  `source@X.Y` → `target@A.B`. Migrating to a moving target makes the mapping table rot.
- If either side is ambiguous, ask one clarifying question before touching code.

### 2. Enumerate the source API surface actually used

Do not migrate the whole API of the source framework — only the parts this repo uses.

- Grep for import / require / `use` statements that pull in the source:
  - JS/TS: `import .* from ['"]express['"]`, `require\(['"]express['"]\)`
  - Python: `^\s*(from|import)\s+flask`
  - Rust: `use\s+diesel(::|;)`
- Grep for call sites of the source's symbols (router methods, decorators, macros, hooks).
- Produce a deduplicated **inventory** list: every source symbol used, with a count and the files
  that touch it. Sort by frequency — high-frequency symbols define the bulk of the work, rare ones
  are where surprises hide.

### 3. Build an explicit old → new mapping table

For every inventoried symbol, write one row mapping the source construct to its target equivalent:

| Source (old) | Target (new) | Notes / semantic gap |
| --- | --- | --- |
| `app.get(path, handler)` | `fastify.get(path, handler)` | reply object differs; `res.send` → `reply.send` |
| `req.body` | `request.body` | Fastify parses JSON by default |
| `class C extends React.Component` | `function C()` | `this.state` → `useState`, lifecycle → `useEffect` |
| `@app.route("/x")` | `@app.get("/x")` | FastAPI splits verbs; add response model |

- Mark rows with **no clean equivalent** as `BLOCKED` and note the workaround or open question.
- This table is the contract for the migration. Keep it in the report (step 5).

### 4. Migrate leaf-first, with a build + test gate between batches

- Topologically order the modules: **leaves first** (files nothing else imports), then their
  dependents, working up toward entry points. Leaves are the safest, smallest blast radius.
- Migrate **one batch** (a leaf module or a small cluster) at a time using the mapping table.
- After each batch, run the **project's own** build and test commands — discover them, do not assume:
  - `cargo build && cargo test` / `npm run build && npm test` / `pytest` / `go build ./... && go test ./...`
  - Follow the repo's existing verify and exec-policy conventions; run only the project's declared
    commands, prefer the narrowest test scope that covers the batch, and never invent network steps.
- **Gate:** do not start the next batch until the current one builds and its tests pass. If a batch
  fails, fix it or revert it before moving on — never let red accumulate across batches.
- Keep each batch a reviewable, self-contained commit.

### 5. Emit MIGRATION_REPORT.md

Write a `MIGRATION_REPORT.md` at the repo root summarizing:

- **Pinned versions** — `source@X.Y` → `target@A.B`.
- **Mapping table** — the full old → new table from step 3.
- **Done** — modules migrated, with the build/test result for each batch.
- **Blocked** — symbols/modules with no clean equivalent, the reason, and the proposed follow-up.
- **Remaining** — inventoried symbols not yet migrated, in suggested order.

## Principles

- **Local-first.** Everything here runs against the local checkout: grep, the repo's build, the
  repo's tests. No hosted service, no network calls, no external upload of code.
- **Build-gated.** The build/test gate between batches is non-negotiable — it is what keeps a long
  migration reviewable and bisectable.
- **Inventory before edits.** Never migrate by intuition; migrate what the grep inventory proves is
  used, and track every symbol through the mapping table to done-or-blocked.
- **Leaf-first.** Smallest blast radius first; entry points last, when their dependencies are ready.
