pub mod apply_patch;
pub mod build_info;
pub mod compliance;
pub mod delegate;
pub mod edit;
pub mod editor_locator;
pub mod git_advanced;
pub mod image;
pub mod read_lines;
pub mod redact;
pub mod refactor;
pub mod result_cache;
pub mod run_script;
pub mod shell;
pub mod tree;
pub mod vision_guard;
pub mod web_search;

use crate::agents::extension::PlatformExtensionContext;
use crate::agents::mcp_client::{Error, McpClientTrait};
use crate::agents::ToolCallContext;
use anyhow::Result;
use apply_patch::ApplyPatchParams;
use async_trait::async_trait;
use delegate::{DelegateParams, DelegateTool};
use edit::{EditTools, FileEditParams, FileWriteParams};
use editor_locator::EditorLocatorParams;
use git_advanced::GitAdvancedParams;
use image::{ImageReadParams, ImageTool};
use indoc::indoc;
use read_lines::{ReadLinesParams, ReadLinesTool};
use refactor::{RefactorParams, RefactorTool};
use rmcp::model::{
    CallToolResult, Content, Implementation, InitializeResult, JsonObject, ListToolsResult,
    RawContent, ServerCapabilities, Tool, ToolAnnotations,
};
use schemars::{schema_for, JsonSchema};
use serde_json::Value;
use shell::{shell_display_name, ShellOutput, ShellParams, ShellTool};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tree::{TreeParams, TreeTool};
use web_search::{WebSearchParams, WebSearchTool};

pub static EXTENSION_NAME: &str = "developer";

pub struct DeveloperClient {
    info: InitializeResult,
    shell_tool: Arc<ShellTool>,
    edit_tools: Arc<EditTools>,
    tree_tool: Arc<TreeTool>,
    read_lines_tool: Arc<ReadLinesTool>,
    image_tool: Arc<ImageTool>,
    web_search_tool: Arc<WebSearchTool>,
    refactor_tool: Arc<RefactorTool>,
    delegate_tool: Arc<DelegateTool>,
}

fn developer_instructions() -> &'static str {
    if cfg!(windows) {
        indoc! {"
            Use the developer extension to build software and operate a terminal.

            Make sure to use the tools *efficiently* - reading all the content you need in as few
            iterations as possible and then making the requested edits or running commands. You are
            responsible for managing your context window, and to minimize unnecessary turns which
            cost the user money.

            For editing software, prefer the flow of using tree to understand the codebase structure
            and file sizes. When you need to search, prefer findstr or Select-String (via shell).
            Then use type or Get-Content to gather the context you need, always reading before
            editing. Use write and edit to efficiently make changes. Test and verify as appropriate.
        "}
    } else {
        indoc! {"
            Use the developer extension to build software and operate a terminal.

            Make sure to use the tools *efficiently* - reading all the content you need in as few
            iterations as possible and then making the requested edits or running commands. You are
            responsible for managing your context window, and to minimize unnecessary turns which
            cost the user money.

            For editing software, prefer the flow of using tree to understand the codebase structure
            and file sizes. When you need to search, prefer rg which correctly respects gitignored
            content. Then use cat or sed to gather the context you need, always reading before editing.
            Use write and edit to efficiently make changes. Test and verify as appropriate.

            When running Python scripts or commands, always use `python3` instead of `python`.
        "}
    }
}

impl DeveloperClient {
    pub fn new(context: PlatformExtensionContext) -> Result<Self> {
        let info = InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(EXTENSION_NAME, "1.0.0").with_title("Developer"))
            .with_instructions(developer_instructions());

        let delegate_tool = Arc::new(DelegateTool::new(context.clone()));

        Ok(Self {
            info,
            shell_tool: Arc::new(ShellTool::new(context.use_login_shell_path)?),
            edit_tools: Arc::new(EditTools::new()),
            tree_tool: Arc::new(TreeTool::new()),
            read_lines_tool: Arc::new(ReadLinesTool::new()),
            image_tool: Arc::new(ImageTool::new()),
            web_search_tool: Arc::new(WebSearchTool::new()),
            refactor_tool: Arc::new(RefactorTool::new()),
            delegate_tool,
        })
    }

    fn schema<T: JsonSchema>() -> JsonObject {
        serde_json::to_value(schema_for!(T))
            .expect("schema serialization should succeed")
            .as_object()
            .expect("schema should serialize to an object")
            .clone()
    }

    pub fn parse_args<T: serde::de::DeserializeOwned>(
        arguments: Option<JsonObject>,
    ) -> Result<T, String> {
        let value = arguments
            .map(Value::Object)
            .ok_or_else(|| "Missing arguments".to_string())?;
        serde_json::from_value(value).map_err(|e| format!("Failed to parse arguments: {e}"))
    }

    /// Apply opt-in egress secret redaction to a shell tool result.
    ///
    /// When `BHARATCODE_REDACT` is unset (the default) the result is returned
    /// unchanged. When enabled, every text content block and the `stdout` /
    /// `stderr` fields of the structured output are scanned for high-confidence
    /// secrets, which are replaced with a `[REDACTED]` sentinel before the
    /// output reaches the model.
    fn redact_shell_result(mut result: CallToolResult) -> CallToolResult {
        if !redact::is_enabled() {
            return result;
        }

        for block in result.content.iter_mut() {
            if let RawContent::Text(text) = &mut block.raw {
                text.text = redact::redact(&text.text);
            }
        }

        if let Some(Value::Object(map)) = result.structured_content.as_mut() {
            for field in ["stdout", "stderr"] {
                if let Some(Value::String(s)) = map.get_mut(field) {
                    *s = redact::redact(s);
                }
            }
        }

        result
    }

    /// Apply the opt-in vision preflight guard to a `read_image` result.
    ///
    /// When `BHARATCODE_VISION_GUARD` is unset (the default) the result is
    /// returned unchanged. When enabled and the active model is recognised as
    /// text-only, a single advisory line is prepended to the result content so
    /// the user is warned the attached image may be ignored.
    fn guard_image_result(mut result: CallToolResult) -> CallToolResult {
        if !vision_guard::is_enabled() {
            return result;
        }

        if let Some(advisory) = vision_guard::active_model_name()
            .as_deref()
            .and_then(vision_guard::vision_advisory)
        {
            result
                .content
                .insert(0, Content::text(advisory).with_priority(0.0));
        }

        result
    }

    pub(crate) fn get_tools() -> Vec<Tool> {
        let mut tools = vec![
            Tool::new(
                "write".to_string(),
                "Create a new file or overwrite an existing file. Creates parent directories if needed.".to_string(),
                Self::schema::<FileWriteParams>(),
            )
            .annotate(ToolAnnotations::from_raw(
                Some("Write".to_string()),
                Some(false),
                Some(true),
                Some(false),
                Some(false),
            )),
            Tool::new(
                "edit".to_string(),
                "Edit a file by finding and replacing text. The before text must match exactly and uniquely. Use empty after text to delete.".to_string(),
                Self::schema::<FileEditParams>(),
            )
            .annotate(ToolAnnotations::from_raw(
                Some("Edit".to_string()),
                Some(false),
                Some(true),
                Some(false),
                Some(false),
            )),
            Tool::new(
                "apply_patch".to_string(),
                "Apply a structured multi-file patch in the apply-patch envelope format \
                 (`*** Begin Patch` ... `*** End Patch`). Supports add, update, delete, and \
                 rename hunks with fuzzy context matching. Prefer this for bulk or multi-hunk \
                 edits across one or more files."
                    .to_string(),
                Self::schema::<ApplyPatchParams>(),
            )
            .annotate(ToolAnnotations::from_raw(
                Some("Apply Patch".to_string()),
                Some(false),
                Some(true),
                Some(false),
                Some(false),
            )),
            Tool::new(
                "shell".to_string(),
                format!(
                    "Execute a shell command in the current dir. Commands run under `{shell}` \
                     (set BHARATCODE_SHELL to override) - write command strings in that shell's \
                     syntax. Returns an object with stdout and stderr as separate fields. The \
                     output of each stream is limited to up to 2000 lines, and longer outputs \
                     will be saved to a temporary file.",
                    shell = shell_display_name(),
                ),
                Self::schema::<ShellParams>(),
            )
            .with_output_schema::<ShellOutput>()
            .annotate(ToolAnnotations::from_raw(
                Some("Shell".to_string()),
                Some(false),
                Some(true),
                Some(false),
                Some(true),
            )),
            Tool::new(
                "tree".to_string(),
                "List a directory tree with line counts. Traversal respects .gitignore rules.".to_string(),
                Self::schema::<TreeParams>(),
            )
            .annotate(ToolAnnotations::from_raw(
                Some("Tree".to_string()),
                Some(true),
                Some(false),
                Some(true),
                Some(false),
            )),
            Tool::new(
                "read_lines".to_string(),
                "Read a bounded line-range window from a single file (offset + limit) so you \
                 can navigate huge files without loading the whole thing. Skips `offset` leading \
                 lines and returns up to `limit` lines (default 200, max ~2000). The slice is also \
                 capped by a hard byte budget to protect memory, so a single very long line is \
                 truncated. Reports the file's total line count and a `truncated` flag in the \
                 structured output. Read-only and byte-bounded."
                    .to_string(),
                Self::schema::<ReadLinesParams>(),
            )
            .annotate(ToolAnnotations::from_raw(
                Some("Read Lines".to_string()),
                Some(true),
                Some(false),
                Some(true),
                Some(false),
            )),
            Tool::new(
                "editor_locator".to_string(),
                "Build editor/IDE jump targets for a file path and optional 1-based line/column: \
                 a VS Code `vscode://file/...` URI and `code -g` CLI form, a JetBrains CLI form, \
                 and a generic `file:line` form, plus the resolved absolute path. Read-only: it \
                 only formats link strings and never opens an editor or reads file contents. \
                 Out-of-range line/column values are clamped."
                    .to_string(),
                Self::schema::<EditorLocatorParams>(),
            )
            .annotate(ToolAnnotations::from_raw(
                Some("Editor Locator".to_string()),
                Some(true),
                Some(false),
                Some(true),
                Some(false),
            )),
            Tool::new(
                "read_image".to_string(),
                "Read an image from a local file path or http(s) URL and return it as image content for the model to inspect. Supports png, jpeg, gif, and webp.".to_string(),
                Self::schema::<ImageReadParams>(),
            )
            .annotate(ToolAnnotations::from_raw(
                Some("Read Image".to_string()),
                Some(true),
                Some(false),
                Some(true),
                Some(false),
            )),
            Tool::new(
                "web_search".to_string(),
                "Search the web and return a list of result titles, URLs, and snippets. \
                 Use this to look up current information, documentation, or facts that may \
                 be outside your training data. Subject to the data-residency / offline egress \
                 guard."
                    .to_string(),
                Self::schema::<WebSearchParams>(),
            )
            .annotate(ToolAnnotations::from_raw(
                Some("Web Search".to_string()),
                Some(true),
                Some(false),
                Some(true),
                Some(true),
            )),
            Tool::new(
                "rename_symbol".to_string(),
                "Rename an identifier across the working tree using a safe, \
                 word-boundary (whole-word) match that respects .gitignore. \
                 Returns a per-file replacement count and a unified-diff-style \
                 preview. Defaults to a dry run (no files written); pass \
                 `dry_run: false` to apply the rename on disk."
                    .to_string(),
                Self::schema::<RefactorParams>(),
            )
            .annotate(ToolAnnotations::from_raw(
                Some("Rename Symbol".to_string()),
                Some(false),
                Some(true),
                Some(false),
                Some(false),
            )),
            Tool::new(
                "delegate".to_string(),
                "Hand off a bounded, self-contained sub-task to a fresh subagent and get back a \
                 single text result. The subagent starts with no access to this conversation, so \
                 put every detail it needs into `instructions`. Each run is hard-capped by the \
                 subagent turn budget; pass `max_turns` to lower it further. Use this to isolate \
                 a focused chunk of work without spending turns in the main conversation."
                    .to_string(),
                Self::schema::<DelegateParams>(),
            )
            .annotate(ToolAnnotations::from_raw(
                Some("Delegate".to_string()),
                Some(false),
                Some(true),
                Some(false),
                Some(true),
            )),
            Tool::new(
                "git_advanced".to_string(),
                "Read-only deep-git inspection. `op: \"worktree_list\"` lists every linked/main \
                 work tree (path, HEAD, branch, flags). `op: \"blame\"` with `file` (and optional \
                 1-based `range` like \"10,40\") returns per-line authorship as (commit, line) \
                 pairs. `op: \"pr_context\"` reports the current branch, its upstream, ahead/behind \
                 counts, and the files changed against the merge-base with that upstream. Runs only \
                 read-only git query subcommands; never mutates the repo and never touches the \
                 network."
                    .to_string(),
                Self::schema::<GitAdvancedParams>(),
            )
            .annotate(ToolAnnotations::from_raw(
                Some("Git Advanced".to_string()),
                Some(true),
                Some(false),
                Some(true),
                Some(false),
            )),
            compliance::compliance_tool(),
            build_info::build_info_tool(),
        ];

        if run_script::is_enabled() {
            tools.push(run_script::script_tool());
        }

        tools
    }
}

#[async_trait]
impl McpClientTrait for DeveloperClient {
    async fn list_tools(
        &self,
        _session_id: &str,
        _next_cursor: Option<String>,
        _cancellation_token: CancellationToken,
    ) -> Result<ListToolsResult, Error> {
        Ok(ListToolsResult {
            tools: Self::get_tools(),
            next_cursor: None,
            meta: None,
        })
    }

    async fn call_tool(
        &self,
        ctx: &ToolCallContext,
        name: &str,
        arguments: Option<JsonObject>,
        _cancel_token: CancellationToken,
    ) -> Result<CallToolResult, Error> {
        let working_dir = ctx.working_dir.as_deref();
        match name {
            "shell" => {
                let cache_args = arguments.clone();
                if let Some(hit) = result_cache::lookup(name, &cache_args) {
                    return Ok(hit);
                }
                match Self::parse_args::<ShellParams>(arguments) {
                    Ok(params) => {
                        let mut advisory: Option<String> = None;
                        if crate::security::shell_harden::mode()
                            != crate::security::shell_harden::Mode::Off
                        {
                            let harden_cwd = working_dir
                                .map(std::path::Path::to_path_buf)
                                .unwrap_or_else(|| {
                                    std::env::current_dir()
                                        .unwrap_or_else(|_| std::path::PathBuf::from("."))
                                });
                            match crate::security::shell_harden::assess(
                                &params.command,
                                &harden_cwd,
                            ) {
                                crate::security::shell_harden::Risk::Block(reason) => {
                                    return Ok(ShellTool::error_result(
                                        &format!(
                                            "Command blocked by security pre-flight: {reason}. \
                                             Set BHARATCODE_HARDEN=warn to downgrade to an \
                                             advisory, or BHARATCODE_HARDEN=off to disable."
                                        ),
                                        None,
                                    ));
                                }
                                crate::security::shell_harden::Risk::Warn(reason) => {
                                    advisory = Some(format!("Security advisory: {reason}."));
                                }
                                crate::security::shell_harden::Risk::Safe => {}
                            }
                        }
                        let result = self.shell_tool.shell_with_cwd(params, working_dir).await;
                        let mut result = Self::redact_shell_result(result);
                        if let Some(note) = advisory {
                            result
                                .content
                                .insert(0, Content::text(note).with_priority(0.0));
                        }
                        if result_cache::arguments_are_read_only(&cache_args) {
                            result_cache::store_if_read_only(name, &cache_args, &result);
                        } else {
                            result_cache::invalidate_all();
                        }
                        Ok(result)
                    }
                    Err(error) => Ok(ShellTool::error_result(&format!("Error: {error}"), None)),
                }
            }
            "write" => match Self::parse_args::<FileWriteParams>(arguments) {
                Ok(params) => {
                    result_cache::invalidate_all();
                    Ok(self.edit_tools.file_write_with_cwd(params, working_dir))
                }
                Err(error) => Ok(CallToolResult::error(vec![Content::text(format!(
                    "Error: {error}"
                ))
                .with_priority(0.0)])),
            },
            "edit" => match Self::parse_args::<FileEditParams>(arguments) {
                Ok(params) => {
                    result_cache::invalidate_all();
                    Ok(self.edit_tools.file_edit_with_cwd(params, working_dir))
                }
                Err(error) => Ok(CallToolResult::error(vec![Content::text(format!(
                    "Error: {error}"
                ))
                .with_priority(0.0)])),
            },
            "apply_patch" => match Self::parse_args::<ApplyPatchParams>(arguments) {
                Ok(params) => {
                    result_cache::invalidate_all();
                    Ok(apply_patch::apply_patch_with_cwd(params, working_dir))
                }
                Err(error) => Ok(CallToolResult::error(vec![Content::text(format!(
                    "Error: {error}"
                ))
                .with_priority(0.0)])),
            },
            "tree" => match Self::parse_args::<TreeParams>(arguments) {
                Ok(params) => Ok(self.tree_tool.tree_with_cwd(params, working_dir)),
                Err(error) => Ok(CallToolResult::error(vec![Content::text(format!(
                    "Error: {error}"
                ))
                .with_priority(0.0)])),
            },
            "read_lines" => match Self::parse_args::<ReadLinesParams>(arguments) {
                Ok(params) => match self
                    .read_lines_tool
                    .read_lines_with_cwd(params, working_dir)
                {
                    Ok(result) => Ok(result),
                    Err(error) => Ok(CallToolResult::error(vec![Content::text(format!(
                        "Error: {}",
                        error.message
                    ))
                    .with_priority(0.0)])),
                },
                Err(error) => Ok(CallToolResult::error(vec![Content::text(format!(
                    "Error: {error}"
                ))
                .with_priority(0.0)])),
            },
            "editor_locator" => {
                let params = arguments.map(Value::Object).unwrap_or(Value::Null);
                match editor_locator::editor_locator(params, working_dir) {
                    Ok(result) => Ok(result),
                    Err(error) => Ok(CallToolResult::error(vec![Content::text(format!(
                        "Error: {}",
                        error.message
                    ))
                    .with_priority(0.0)])),
                }
            }
            "read_image" => match Self::parse_args::<ImageReadParams>(arguments) {
                Ok(params) => {
                    let result = self
                        .image_tool
                        .image_read_with_cwd(params, working_dir)
                        .await;
                    Ok(Self::guard_image_result(result))
                }
                Err(error) => Ok(CallToolResult::error(vec![Content::text(format!(
                    "Error: {error}"
                ))
                .with_priority(0.0)])),
            },
            "web_search" => match Self::parse_args::<WebSearchParams>(arguments) {
                Ok(params) => Ok(self.web_search_tool.search(params).await),
                Err(error) => Ok(CallToolResult::error(vec![Content::text(format!(
                    "Error: {error}"
                ))
                .with_priority(0.0)])),
            },
            "rename_symbol" => match Self::parse_args::<RefactorParams>(arguments) {
                Ok(params) => Ok(self.refactor_tool.rename_symbol(params, working_dir).await),
                Err(error) => Ok(CallToolResult::error(vec![Content::text(format!(
                    "Error: {error}"
                ))
                .with_priority(0.0)])),
            },
            "delegate" => match Self::parse_args::<DelegateParams>(arguments) {
                Ok(params) => Ok(self.delegate_tool.delegate(params, &ctx.session_id).await),
                Err(error) => Ok(CallToolResult::error(vec![Content::text(format!(
                    "Error: {error}"
                ))
                .with_priority(0.0)])),
            },
            "git_advanced" => match Self::parse_args::<GitAdvancedParams>(arguments) {
                Ok(params) => match git_advanced::run_git_advanced(params, working_dir) {
                    Ok(result) => Ok(result),
                    Err(error) => Ok(CallToolResult::error(vec![Content::text(format!(
                        "Error: {}",
                        error.message
                    ))
                    .with_priority(0.0)])),
                },
                Err(error) => Ok(CallToolResult::error(vec![Content::text(format!(
                    "Error: {error}"
                ))
                .with_priority(0.0)])),
            },
            "verify_compliance" => match compliance::run(arguments, working_dir) {
                Ok(result) => Ok(result),
                Err(error) => Ok(CallToolResult::error(vec![Content::text(format!(
                    "Error: {}",
                    error.message
                ))
                .with_priority(0.0)])),
            },
            "run_script" => match Self::parse_args::<run_script::RunScriptParams>(arguments) {
                Ok(params) => Ok(run_script::run(params, working_dir).await),
                Err(error) => Ok(CallToolResult::error(vec![Content::text(format!(
                    "Error: {error}"
                ))
                .with_priority(0.0)])),
            },
            "build_info" => Ok(build_info::run()),
            _ => Ok(CallToolResult::error(vec![Content::text(format!(
                "Error: Unknown tool: {name}"
            ))
            .with_priority(0.0)])),
        }
    }

    fn get_info(&self) -> Option<&InitializeResult> {
        Some(&self.info)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::SessionManager;
    use rmcp::model::RawContent;
    use rmcp::object;
    use std::fs;

    #[test]
    fn developer_tools_are_flat() {
        // The opt-in `run_script` tool is gated on BHARATCODE_SCRIPTS and must be
        // absent from the default tool list. Share run_script's env lock so this
        // never races the `BHARATCODE_SCRIPTS`-mutating tests in that module.
        let _lock = run_script::SCRIPTS_ENV_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        std::env::remove_var(run_script::SCRIPTS_ENV);

        let names: Vec<String> = DeveloperClient::get_tools()
            .into_iter()
            .map(|t| t.name.to_string())
            .collect();

        assert_eq!(
            names,
            vec![
                "write",
                "edit",
                "apply_patch",
                "shell",
                "tree",
                "read_lines",
                "editor_locator",
                "read_image",
                "web_search",
                "rename_symbol",
                "delegate",
                "git_advanced",
                "verify_compliance",
                "build_info"
            ]
        );
        assert!(!names.iter().any(|n| n == "run_script"));
    }

    #[test]
    fn run_script_tool_present_when_enabled() {
        let _lock = run_script::SCRIPTS_ENV_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        std::env::set_var(run_script::SCRIPTS_ENV, "1");
        let names: Vec<String> = DeveloperClient::get_tools()
            .into_iter()
            .map(|t| t.name.to_string())
            .collect();
        std::env::remove_var(run_script::SCRIPTS_ENV);

        assert!(names.iter().any(|n| n == "run_script"));
    }

    fn test_context(data_dir: std::path::PathBuf) -> PlatformExtensionContext {
        PlatformExtensionContext {
            extension_manager: None,
            session_manager: Arc::new(SessionManager::new(data_dir)),
            session: None,
            use_login_shell_path: false,
        }
    }

    fn first_text(result: &CallToolResult) -> &str {
        match &result.content[0].raw {
            RawContent::Text(text) => &text.text,
            _ => panic!("expected text content"),
        }
    }

    #[tokio::test]
    async fn developer_client_uses_working_dir_for_file_tools() {
        let temp = tempfile::tempdir().unwrap();
        let client = DeveloperClient::new(test_context(temp.path().join("sessions"))).unwrap();
        let cwd = temp.path().join("workspace");
        fs::create_dir_all(&cwd).unwrap();

        let ctx = ToolCallContext::new("session".to_owned(), Some(cwd.clone()), None);
        let write = client
            .call_tool(
                &ctx,
                "write",
                Some(object!({
                    "path": "notes.txt",
                    "content": "first line"
                })),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(write.is_error, Some(false));
        assert_eq!(
            fs::read_to_string(cwd.join("notes.txt")).unwrap(),
            "first line"
        );

        let edit = client
            .call_tool(
                &ctx,
                "edit",
                Some(object!({
                    "path": "notes.txt",
                    "before": "first",
                    "after": "updated"
                })),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(edit.is_error, Some(false));
        assert_eq!(
            fs::read_to_string(cwd.join("notes.txt")).unwrap(),
            "updated line"
        );
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn developer_client_uses_working_dir_for_shell_tool() {
        let temp = tempfile::tempdir().unwrap();
        let client = DeveloperClient::new(test_context(temp.path().join("sessions"))).unwrap();
        let cwd = temp.path().join("workspace");
        fs::create_dir_all(&cwd).unwrap();

        let ctx = ToolCallContext::new("session".to_owned(), Some(cwd.clone()), None);
        let result = client
            .call_tool(
                &ctx,
                "shell",
                Some(object!({
                    "command": "pwd"
                })),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(result.is_error, Some(false));
        let observed = std::fs::canonicalize(first_text(&result)).unwrap();
        let expected = std::fs::canonicalize(&cwd).unwrap();
        assert_eq!(observed, expected);
    }
}
