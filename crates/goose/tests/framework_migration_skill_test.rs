//! Verifies the builtin `framework-migration` skill is embedded via
//! `include_dir!` and surfaced through the public skills discovery API.
//!
//! Wiring is automatic: `skills/builtin.rs` globs every `.md` under
//! `src/skills/builtins/` into the binary, and `discover_skills()` parses each
//! one's front-matter and exposes it as `builtin://skills/<name>`. No shared
//! file is edited to register a new skill — dropping the `.md` is enough — so
//! this test guards that the new file is genuinely reachable in the built
//! binary rather than dead.

use goose::skills::discover_skills;

const SKILL_NAME: &str = "framework-migration";

/// The skill is embedded and its front-matter survives parsing into a surfaced
/// `SourceEntry`. The entry only exists at all if `parse_skill_content`
/// accepted the YAML and extracted a `name`, so matching on the parsed
/// `name` proves "embedded + valid front-matter (name) + surfaced".
///
/// Note: `discover_skills` strips the front-matter block out of `content`, so
/// the parsed `name` field — not a raw `content.contains("name: ...")` — is the
/// faithful public-API witness that the front-matter `name:` is present.
#[test]
fn framework_migration_skill_is_embedded_and_surfaced() {
    let skills = discover_skills(None);

    assert!(
        skills.iter().any(|s| s.name == SKILL_NAME),
        "expected a builtin skill whose front-matter declares name: {SKILL_NAME}"
    );
}

/// The skill is discoverable by its parsed name, marked as a built-in, and
/// reachable under the synthetic `builtin://skills/<name>` path that the
/// runtime uses to load it.
#[test]
fn framework_migration_skill_has_builtin_identity() {
    let skills = discover_skills(None);

    let entry = skills
        .iter()
        .find(|s| s.name == SKILL_NAME)
        .unwrap_or_else(|| panic!("builtin skill `{SKILL_NAME}` was not discovered"));

    assert_eq!(
        entry.path,
        format!("builtin://skills/{SKILL_NAME}"),
        "builtin skill should be surfaced under its synthetic builtin path"
    );
    assert!(
        !entry.description.trim().is_empty(),
        "skill front-matter must carry a non-empty description (the trigger text)"
    );
}

/// The description is the trigger surface the agent matches against, so it must
/// actually mention framework/library/version migration to fire on the right
/// tasks (Express -> Fastify, React class -> hooks, Flask -> FastAPI, etc.).
#[test]
fn framework_migration_skill_description_covers_migration_triggers() {
    let skills = discover_skills(None);

    let entry = skills
        .iter()
        .find(|s| s.name == SKILL_NAME)
        .unwrap_or_else(|| panic!("builtin skill `{SKILL_NAME}` was not discovered"));

    let description = entry.description.to_lowercase();
    assert!(
        description.contains("migrat"),
        "description should describe a migration trigger; got: {}",
        entry.description
    );
    assert!(
        description.contains("framework")
            || description.contains("library")
            || description.contains("version"),
        "description should mention frameworks/libraries/versions; got: {}",
        entry.description
    );
}
