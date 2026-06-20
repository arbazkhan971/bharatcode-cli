use std::path::Path;

use bharatcode_apply_patch::apply_patch_to_disk;
use rmcp::model::{CallToolResult, Content};
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ApplyPatchParams {
    /// A patch in the apply-patch envelope format. The text must start with
    /// `*** Begin Patch` and end with `*** End Patch`, and contain one or more
    /// file hunks: `*** Add File: <path>` (followed by `+` lines),
    /// `*** Delete File: <path>`, or `*** Update File: <path>` (optionally
    /// `*** Move to: <path>`, then `@@` context headers and ` `/`-`/`+` lines).
    /// Relative paths are resolved against the working directory.
    pub patch: String,
}

/// Apply a structured apply-patch envelope to the filesystem, resolving relative
/// hunk paths against `working_dir`.
pub fn apply_patch_with_cwd(
    params: ApplyPatchParams,
    working_dir: Option<&Path>,
) -> CallToolResult {
    let cwd = working_dir
        .map(Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    match apply_patch_to_disk(&params.patch, &cwd) {
        Ok(summary) => {
            CallToolResult::success(vec![Content::text(summary.render()).with_priority(0.0)])
        }
        Err(error) => CallToolResult::error(vec![Content::text(format!(
            "Failed to apply patch: {error}"
        ))
        .with_priority(0.0)]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::RawContent;
    use std::fs;

    fn text(result: &CallToolResult) -> &str {
        match &result.content[0].raw {
            RawContent::Text(t) => &t.text,
            _ => panic!("expected text content"),
        }
    }

    #[test]
    fn applies_update_patch_in_working_dir() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("greet.txt"), "hello\nworld\n").unwrap();

        let params = ApplyPatchParams {
            patch: "*** Begin Patch\n*** Update File: greet.txt\n@@\n-world\n+there\n*** End Patch"
                .to_string(),
        };
        let result = apply_patch_with_cwd(params, Some(dir.path()));

        assert!(!result.is_error.unwrap_or(false));
        assert!(text(&result).contains("M greet.txt"));
        assert_eq!(
            fs::read_to_string(dir.path().join("greet.txt")).unwrap(),
            "hello\nthere\n"
        );
    }

    #[test]
    fn add_and_delete_round_trip() {
        let dir = tempfile::tempdir().unwrap();

        let add = apply_patch_with_cwd(
            ApplyPatchParams {
                patch: "*** Begin Patch\n*** Add File: nested/new.txt\n+line\n*** End Patch"
                    .to_string(),
            },
            Some(dir.path()),
        );
        assert!(!add.is_error.unwrap_or(false));
        assert_eq!(
            fs::read_to_string(dir.path().join("nested/new.txt")).unwrap(),
            "line\n"
        );

        let del = apply_patch_with_cwd(
            ApplyPatchParams {
                patch: "*** Begin Patch\n*** Delete File: nested/new.txt\n*** End Patch"
                    .to_string(),
            },
            Some(dir.path()),
        );
        assert!(!del.is_error.unwrap_or(false));
        assert!(!dir.path().join("nested/new.txt").exists());
    }

    #[test]
    fn invalid_patch_returns_error_result() {
        let dir = tempfile::tempdir().unwrap();
        let result = apply_patch_with_cwd(
            ApplyPatchParams {
                patch: "not a patch".to_string(),
            },
            Some(dir.path()),
        );
        assert!(result.is_error.unwrap_or(false));
        assert!(text(&result).contains("Failed to apply patch"));
    }
}
