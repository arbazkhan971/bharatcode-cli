//! `build_info` developer tool (BharatCode v96).
//!
//! A read-only, always-available agent tool that reports the running binary's
//! identity from compile-time facts: the crate version, the build profile
//! (debug/release), the target triple, and the set of gated Cargo features that
//! were compiled in. Performs no I/O and alters no existing default behaviour.

use rmcp::model::{CallToolResult, Content, JsonObject, Tool, ToolAnnotations};
use schemars::{schema_for, JsonSchema};
use serde::Deserialize;
use serde_json::json;

/// Local English-fallback label helper. The `tr!` macro lives in the CLI crate,
/// not here, so user-facing labels are inlined; mirrors the sibling pattern in
/// `refactor.rs` / `web_search.rs`.
macro_rules! label {
    ($_key:expr, $default:expr) => {
        $default
    };
}

/// The `build_info` tool takes no arguments.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct BuildInfoParams {}

/// Resolve the build profile from compile-time facts. The goose crate has no
/// `build.rs` to inject Cargo's `PROFILE`, so this is derived from
/// `cfg!(debug_assertions)`.
fn profile() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    }
}

/// Assemble an approximate target triple from `std::env::consts` plus the ABI
/// suffix, avoiding a build-script `TARGET` dependency.
fn target_triple() -> String {
    let arch = std::env::consts::ARCH;
    let os = std::env::consts::OS;
    let env = if cfg!(target_env = "gnu") {
        "-gnu"
    } else if cfg!(target_env = "musl") {
        "-musl"
    } else if cfg!(target_env = "msvc") {
        "-msvc"
    } else {
        ""
    };
    // A vendor segment keeps the shape triple-like (arch-vendor-os[-abi]).
    let vendor = if os == "macos" {
        "apple"
    } else if os == "windows" {
        "pc"
    } else {
        "unknown"
    };
    format!("{arch}-{vendor}-{os}{env}")
}

/// Probe the gated Cargo features that were compiled into this binary, in a
/// stable order mirroring `[features]` in `Cargo.toml`.
fn features() -> Vec<&'static str> {
    let mut out = Vec::new();
    if cfg!(feature = "rustls-tls") {
        out.push("rustls-tls");
    }
    if cfg!(feature = "native-tls") {
        out.push("native-tls");
    }
    out
}

/// Entry point for the `build_info` tool. Returns a text summary plus structured
/// content carrying the version, profile, target triple, and feature list.
pub fn run() -> CallToolResult {
    let version = env!("CARGO_PKG_VERSION");
    let profile = profile();
    let triple = target_triple();
    let feats = features();

    let summary = format!(
        "{label} v{version} {profile} build for {triple}",
        label = label!("build_info.summary", "Build"),
    );

    let structured = json!({
        "version": version,
        "profile": profile,
        "target_triple": triple,
        "features": feats,
    });

    let mut result = CallToolResult::success(vec![Content::text(summary).with_priority(0.0)]);
    result.structured_content = Some(structured);
    result
}

/// Build the `Tool` descriptor for `build_info`.
pub fn build_info_tool() -> Tool {
    Tool::new(
        "build_info".to_string(),
        label!(
            "build_info.description",
            "Read-only build identity of the running agent binary: crate version, \
             build profile (debug/release), target triple, and compiled-in feature \
             flags. Reports compile-time facts only; performs no I/O and changes \
             nothing."
        )
        .to_string(),
        schema_object::<BuildInfoParams>(),
    )
    .annotate(ToolAnnotations::from_raw(
        Some(label!("build_info.title", "Build Info").to_string()),
        Some(true),
        Some(false),
        Some(true),
        Some(false),
    ))
}

/// Serialize a JsonSchema type into the object form `Tool::new` expects.
fn schema_object<T: JsonSchema>() -> JsonObject {
    serde_json::to_value(schema_for!(T))
        .expect("schema serialization should succeed")
        .as_object()
        .expect("schema should serialize to an object")
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::RawContent;

    fn text_of(result: &CallToolResult) -> String {
        result
            .content
            .as_ref()
            .and_then(|items| items.first())
            .and_then(|c| match &c.raw {
                RawContent::Text(t) => Some(t.text.clone()),
                _ => None,
            })
            .unwrap_or_default()
    }

    #[test]
    fn payload_contains_crate_version() {
        let result = run();
        let text = text_of(&result);
        assert!(
            text.contains(env!("CARGO_PKG_VERSION")),
            "missing version: {text}"
        );
    }

    #[test]
    fn profile_is_debug_or_release() {
        let sc = run().structured_content.expect("structured");
        let p = sc["profile"].as_str().unwrap();
        assert!(p == "debug" || p == "release", "got: {p}");
    }

    #[test]
    fn target_triple_looks_like_a_triple() {
        let sc = run().structured_content.expect("structured");
        let triple = sc["target_triple"].as_str().unwrap();
        assert!(triple.matches('-').count() >= 2, "got: {triple}");
    }

    #[test]
    fn features_are_a_string_list() {
        let sc = run().structured_content.expect("structured");
        assert!(sc["features"].is_array(), "features not an array");
    }

    #[test]
    fn result_is_not_an_error() {
        let result = run();
        assert_ne!(result.is_error, Some(true));
    }

    #[test]
    fn descriptor_is_read_only() {
        let tool = build_info_tool();
        let ann = tool.annotations.expect("annotations");
        assert_eq!(ann.read_only_hint, Some(true));
        assert_eq!(ann.destructive_hint, Some(false));
    }

    #[test]
    fn payload_is_brand_free() {
        let needle_a = ["go", "ose"].concat();
        let needle_b = ["bl", "ock"].concat();
        let text = text_of(&run()).to_lowercase();
        assert!(!text.contains(&needle_a), "brand leak: {text}");
        assert!(!text.contains(&needle_b), "brand leak: {text}");
        let sc = run().structured_content.unwrap().to_string().to_lowercase();
        assert!(!sc.contains(&needle_a), "brand leak: {sc}");
        assert!(!sc.contains(&needle_b), "brand leak: {sc}");
    }
}
