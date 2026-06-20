//! Opt-in framework-migration advisory.
//!
//! When the `BHARATCODE_MIGRATE` environment variable is set to a `from:to`
//! pair (e.g. `express:fastify`), a compact migration-strategy instruction
//! block is injected into the session prompt at build time so the agent plans
//! the migration consistently. Default behaviour (variable unset) leaves the
//! session prompt unchanged.

use std::collections::BTreeMap;
use std::sync::LazyLock;

const MIGRATE_ENV: &str = "BHARATCODE_MIGRATE";

/// A parsed source/target framework pair for an advisory migration plan.
pub struct MigrationSpec {
    pub from: String,
    pub to: String,
}

/// Parse [`MIGRATE_ENV`] (`from:to`) into a [`MigrationSpec`].
///
/// Returns `None` when the variable is unset, empty, or malformed (missing
/// exactly one separator, or an empty side).
pub fn from_env() -> Option<MigrationSpec> {
    let raw = std::env::var(MIGRATE_ENV).ok()?;
    parse(&raw)
}

fn parse(raw: &str) -> Option<MigrationSpec> {
    let mut parts = raw.splitn(2, ':');
    let from = parts.next()?.trim();
    let to = parts.next()?.trim();
    if from.is_empty() || to.is_empty() || to.contains(':') {
        return None;
    }
    Some(MigrationSpec {
        from: from.to_string(),
        to: to.to_string(),
    })
}

/// Curated, pair-specific migration notes keyed by lowercase `(from, to)`.
static STRATEGY_NOTES: LazyLock<BTreeMap<(&'static str, &'static str), &'static str>> =
    LazyLock::new(|| {
        let mut m = BTreeMap::new();
        m.insert(
            ("express", "fastify"),
            "- Replace `app.use`/middleware chains with Fastify plugins and hooks (`onRequest`, `preHandler`).\n\
             - Convert `(req, res)` handlers to `async (request, reply)`; return values instead of `res.send`.\n\
             - Move body parsing, CORS, and validation to Fastify schemas and registered plugins.\n\
             - Port error handling to `setErrorHandler`; map `res.status(n).json(...)` to `reply.code(n).send(...)`.",
        );
        m.insert(
            ("flask", "fastapi"),
            "- Replace `@app.route` with typed `@app.get`/`@app.post` and Pydantic request/response models.\n\
             - Swap synchronous view functions for `async def` endpoints where I/O-bound.\n\
             - Move `request.args`/`request.json` access to typed path, query, and body parameters.\n\
             - Replace Flask blueprints with `APIRouter`; port error handlers to exception handlers.",
        );
        m.insert(
            ("jest", "vitest"),
            "- Swap `jest.mock`/`jest.fn` for `vi.mock`/`vi.fn`; import test globals from `vitest` or enable `globals`.\n\
             - Replace `jest.config.js` with a `test` block in `vite.config.ts`/`vitest.config.ts`.\n\
             - Port timer and module mocks (`vi.useFakeTimers`, `vi.importActual`); update snapshot serializers.\n\
             - Update CI scripts from `jest` to `vitest run` and verify coverage provider settings.",
        );
        m
    });

/// Build the advisory instruction block for `spec`.
///
/// Uses a curated, pair-specific note when one exists; otherwise falls back to
/// a generic-but-useful migration checklist. The returned block always names
/// both the source and target framework.
pub fn advisory_block(spec: &MigrationSpec) -> String {
    let from = &spec.from;
    let to = &spec.to;
    let key = (from.to_lowercase(), to.to_lowercase());
    let notes = STRATEGY_NOTES
        .get(&(key.0.as_str(), key.1.as_str()))
        .copied()
        .map(str::to_string)
        .unwrap_or_else(|| generic_checklist(from, to));

    format!(
        "## Framework migration: {from} -> {to}\n\
         You are assisting an incremental migration from {from} to {to}. Plan and \
         execute it consistently:\n\
         {notes}\n\
         - Migrate in small, independently testable slices; keep the build green at every step.\n\
         - Preserve existing behaviour and public contracts unless the user asks to change them.\n\
         - Update and run tests for each slice before moving on; do not delete coverage.\n\
         - Call out {from}-specific idioms that have no direct {to} equivalent and propose alternatives."
    )
}

fn generic_checklist(from: &str, to: &str) -> String {
    format!(
        "- Inventory the {from} surface area (entry points, routing, config, middleware/plugins) before changing code.\n\
         - Map each {from} concept to its closest {to} equivalent; flag gaps explicitly.\n\
         - Introduce {to} alongside {from} and migrate module by module rather than in one sweep.\n\
         - Port dependencies and build/test tooling, then remove {from} only once nothing references it."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_pair() {
        let spec = parse("express:fastify").expect("valid pair should parse");
        assert_eq!(spec.from, "express");
        assert_eq!(spec.to, "fastify");
    }

    #[test]
    fn parse_trims_whitespace() {
        let spec = parse("  flask : fastapi  ").expect("padded pair should parse");
        assert_eq!(spec.from, "flask");
        assert_eq!(spec.to, "fastapi");
    }

    #[test]
    fn parse_rejects_malformed() {
        assert!(parse("").is_none());
        assert!(parse("express").is_none());
        assert!(parse(":fastify").is_none());
        assert!(parse("express:").is_none());
        assert!(parse("a:b:c").is_none());
    }

    #[test]
    fn from_env_unset_is_none() {
        std::env::remove_var(MIGRATE_ENV);
        assert!(from_env().is_none());
    }

    #[test]
    fn advisory_known_pair_names_both_frameworks() {
        let spec = MigrationSpec {
            from: "express".to_string(),
            to: "fastify".to_string(),
        };
        let block = advisory_block(&spec);
        assert!(block.contains("express"));
        assert!(block.contains("fastify"));
        assert!(block.contains("Fastify plugins"));
    }

    #[test]
    fn advisory_unknown_pair_uses_generic_checklist() {
        let spec = MigrationSpec {
            from: "django".to_string(),
            to: "rails".to_string(),
        };
        let block = advisory_block(&spec);
        assert!(block.contains("django"));
        assert!(block.contains("rails"));
        assert!(block.contains("Inventory the django"));
    }
}
