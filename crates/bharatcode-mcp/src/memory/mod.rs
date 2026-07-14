use etcetera::{choose_app_strategy, AppStrategy};
use indoc::formatdoc;
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{
        CallToolResult, Content, ErrorCode, ErrorData, Implementation, InitializeResult, Meta,
        ServerCapabilities, ServerInfo,
    },
    schemars::JsonSchema,
    service::RequestContext,
    tool, tool_handler, tool_router, RoleServer, ServerHandler,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

const WORKING_DIR_HEADER: &str = "agent-working-dir";

const MAX_CATEGORY_LEN: usize = 64;

fn extract_working_dir_from_meta(meta: &Meta) -> Option<PathBuf> {
    meta.0
        .get(WORKING_DIR_HEADER)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

/// A category becomes a file name inside the memory directory, so it must be a single
/// flat component: no path syntax, no separators, no drive letters, ASCII only.
fn validate_category(category: &str) -> io::Result<()> {
    let is_valid = !category.is_empty()
        && category.len() <= MAX_CATEGORY_LEN
        && category
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');

    if is_valid {
        return Ok(());
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        format!(
            "Invalid memory category {category:?}: expected 1-{MAX_CATEGORY_LEN} characters from [A-Za-z0-9_-]"
        ),
    ))
}

fn containment_error(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!(
            "Refusing to follow a symlink in the memory directory: {}",
            path.display()
        ),
    )
}

/// The memory directory and its entries are created by this server; a symlink means
/// something else planted it to redirect reads and writes elsewhere.
fn reject_symlink(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(containment_error(path)),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Real, symlink-resolved location of `path`. Only the deepest existing ancestor can be
/// canonicalized; the remaining components are validated to be plain names (no `..`), so
/// appending them lexically cannot climb back out. Any symlink hop along the existing
/// prefix is resolved here, which is exactly what the containment check must see.
fn resolved_boundary(path: &Path) -> io::Result<PathBuf> {
    for ancestor in path.ancestors() {
        if ancestor.exists() {
            let real = fs::canonicalize(ancestor)?;
            if let Ok(tail) = path.strip_prefix(ancestor) {
                return Ok(real.join(tail));
            }
            return Ok(real);
        }
    }
    Ok(path.to_path_buf())
}

/// Rejects any `path` whose real location escapes the intended `root`, even when every
/// individual component of `path` is itself a plain entry and only an ancestor directory
/// (e.g. `.bharatcode`) is the symlink that redirects the whole tree outside `root`.
fn assert_within_root(root: &Path, path: &Path) -> io::Result<()> {
    if resolved_boundary(path)?.starts_with(resolved_boundary(root)?) {
        Ok(())
    } else {
        Err(containment_error(path))
    }
}

/// Resolves `category` to a file that is guaranteed to sit directly inside `base_dir`,
/// with the whole memory tree confined to `root` (the working dir for local storage, or
/// the global memory dir itself).
fn memory_file_in(root: &Path, base_dir: &Path, category: &str) -> io::Result<PathBuf> {
    validate_category(category)?;
    reject_symlink(base_dir)?;

    let file_path = base_dir.join(format!("{}.txt", category));
    reject_symlink(&file_path)?;
    assert_within_root(root, &file_path)?;

    Ok(file_path)
}

fn to_error_data(e: io::Error) -> ErrorData {
    let code = match e.kind() {
        io::ErrorKind::InvalidInput | io::ErrorKind::PermissionDenied => ErrorCode::INVALID_PARAMS,
        _ => ErrorCode::INTERNAL_ERROR,
    };
    ErrorData::new(code, e.to_string(), None)
}

/// Parameters for the remember_memory tool
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct RememberMemoryParams {
    /// The category to store the memory in
    pub category: String,
    /// The data to remember
    pub data: String,
    /// Optional tags for the memory
    #[serde(default)]
    pub tags: Vec<String>,
    /// Whether to store globally or locally
    pub is_global: bool,
}

/// Parameters for the retrieve_memories tool
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct RetrieveMemoriesParams {
    /// The category to retrieve memories from (use "*" for all)
    pub category: String,
    /// Whether to retrieve from global or local storage
    pub is_global: bool,
}

/// Parameters for the remove_memory_category tool
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct RemoveMemoryCategoryParams {
    /// The category to remove (use "*" for all)
    pub category: String,
    /// Whether to remove from global or local storage
    pub is_global: bool,
}

/// Parameters for the remove_specific_memory tool
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct RemoveSpecificMemoryParams {
    /// The category containing the memory
    pub category: String,
    /// The content of the memory to remove
    pub memory_content: String,
    /// Whether to remove from global or local storage
    pub is_global: bool,
}

/// Memory MCP Server using official RMCP SDK
#[derive(Clone)]
pub struct MemoryServer {
    tool_router: ToolRouter<Self>,
    instructions: String,
    global_memory_dir: PathBuf,
}

impl Default for MemoryServer {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_router(router = tool_router)]
impl MemoryServer {
    pub fn new() -> Self {
        let instructions = formatdoc! {r#"
             This extension stores and retrieves categorized information with tagging support.

             Storage:
             - Local: .bharatcode/memory/ (project-specific)
             - Global: ~/.config/bharatcode/memory/ (user-wide)

             Save proactively when users share preferences, project configurations, workflow patterns,
             or recurring commands. Always confirm with the user before saving. Suggest relevant
             categories and tags, and clarify storage scope (local vs global).

             Categories may only contain letters, digits, underscores and hyphens (max 64 characters).

             Use category "*" with retrieve_memories or remove_memory_category to access all entries.
            "#};

        let global_memory_dir = choose_app_strategy(crate::APP_STRATEGY.clone())
            .map(|strategy| strategy.in_config_dir("memory"))
            .unwrap_or_else(|_| PathBuf::from(".config/bharatcode/memory"));

        let mut memory_router = Self {
            tool_router: Self::tool_router(),
            instructions: instructions.clone(),
            global_memory_dir,
        };

        let retrieved_global_memories = memory_router.retrieve_all(true, None);

        let mut updated_instructions = instructions;

        let memories_follow_up_instructions = formatdoc! {r#"
            **Here are the user's currently saved memories:**
            Please keep this information in mind when answering future questions.
            Do not bring up memories unless relevant.
            Note: if the user has not saved any memories, this section will be empty.
            Note: if the user removes a memory that was previously loaded into the system, please remove it from the system instructions.
            "#};

        updated_instructions.push_str("\n\n");
        updated_instructions.push_str(&memories_follow_up_instructions);

        if let Ok(global_memories) = retrieved_global_memories {
            if !global_memories.is_empty() {
                updated_instructions.push_str("\n\nGlobal Memories:\n");
                for (category, memories) in global_memories {
                    updated_instructions.push_str(&format!("\nCategory: {}\n", category));
                    for memory in memories {
                        updated_instructions.push_str(&format!("- {}\n", memory));
                    }
                }
            }
        }

        memory_router.set_instructions(updated_instructions);

        memory_router
    }

    // Add a setter method for instructions
    pub fn set_instructions(&mut self, new_instructions: String) {
        self.instructions = new_instructions;
    }

    pub fn get_instructions(&self) -> &str {
        &self.instructions
    }

    /// The directory the memory tree must never escape: the working dir for local storage,
    /// or the global memory dir itself for global storage.
    fn containment_root(&self, is_global: bool, working_dir: Option<&PathBuf>) -> PathBuf {
        if is_global {
            self.global_memory_dir.clone()
        } else {
            working_dir
                .cloned()
                .or_else(|| std::env::current_dir().ok())
                .unwrap_or_else(|| PathBuf::from("."))
        }
    }

    fn base_dir(&self, is_global: bool, working_dir: Option<&PathBuf>) -> PathBuf {
        let root = self.containment_root(is_global, working_dir);
        if is_global {
            root
        } else {
            root.join(".bharatcode").join("memory")
        }
    }

    fn get_memory_file(
        &self,
        category: &str,
        is_global: bool,
        working_dir: Option<&PathBuf>,
    ) -> io::Result<PathBuf> {
        memory_file_in(
            &self.containment_root(is_global, working_dir),
            &self.base_dir(is_global, working_dir),
            category,
        )
    }

    pub fn retrieve_all(
        &self,
        is_global: bool,
        working_dir: Option<&PathBuf>,
    ) -> io::Result<HashMap<String, Vec<String>>> {
        let base_dir = self.base_dir(is_global, working_dir);
        let mut memories = HashMap::new();
        if !base_dir.exists() {
            return Ok(memories);
        }

        reject_symlink(&base_dir)?;
        assert_within_root(&self.containment_root(is_global, working_dir), &base_dir)?;

        for entry in fs::read_dir(&base_dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }

            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("txt") {
                continue;
            }

            let Some(category) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if validate_category(category).is_err() {
                continue;
            }

            let category_memories = self.retrieve(category, is_global, working_dir)?;
            memories.insert(
                category.to_string(),
                category_memories.into_iter().flat_map(|(_, v)| v).collect(),
            );
        }
        Ok(memories)
    }

    pub fn remember(
        &self,
        _context: &str,
        category: &str,
        data: &str,
        tags: &[&str],
        is_global: bool,
        working_dir: Option<&PathBuf>,
    ) -> io::Result<()> {
        let memory_file_path = self.get_memory_file(category, is_global, working_dir)?;

        if let Some(parent) = memory_file_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut file = fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&memory_file_path)?;
        if !tags.is_empty() {
            writeln!(file, "# {}", tags.join(" "))?;
        }
        writeln!(file, "{}\n", data)?;

        Ok(())
    }

    pub fn retrieve(
        &self,
        category: &str,
        is_global: bool,
        working_dir: Option<&PathBuf>,
    ) -> io::Result<HashMap<String, Vec<String>>> {
        let memory_file_path = self.get_memory_file(category, is_global, working_dir)?;
        if !memory_file_path.exists() {
            return Ok(HashMap::new());
        }

        let mut file = fs::File::open(memory_file_path)?;
        let mut content = String::new();
        file.read_to_string(&mut content)?;

        let mut memories = HashMap::new();
        for entry in content.split("\n\n") {
            let mut lines = entry.lines();
            if let Some(first_line) = lines.next() {
                if let Some(stripped) = first_line.strip_prefix('#') {
                    let tags = stripped
                        .split_whitespace()
                        .map(String::from)
                        .collect::<Vec<_>>();
                    memories.insert(tags.join(" "), lines.map(String::from).collect());
                } else {
                    let entry_data: Vec<String> = std::iter::once(first_line.to_string())
                        .chain(lines.map(String::from))
                        .collect();
                    memories
                        .entry("untagged".to_string())
                        .or_insert_with(Vec::new)
                        .extend(entry_data);
                }
            }
        }

        Ok(memories)
    }

    pub fn remove_specific_memory_internal(
        &self,
        category: &str,
        memory_content: &str,
        is_global: bool,
        working_dir: Option<&PathBuf>,
    ) -> io::Result<()> {
        let memory_file_path = self.get_memory_file(category, is_global, working_dir)?;
        if !memory_file_path.exists() {
            return Ok(());
        }

        let mut file = fs::File::open(&memory_file_path)?;
        let mut content = String::new();
        file.read_to_string(&mut content)?;

        let memories: Vec<&str> = content.split("\n\n").collect();
        let new_content: Vec<String> = memories
            .into_iter()
            .filter(|entry| !entry.contains(memory_content))
            .map(|s| s.to_string())
            .collect();

        fs::write(memory_file_path, new_content.join("\n\n"))?;

        Ok(())
    }

    pub fn clear_memory(
        &self,
        category: &str,
        is_global: bool,
        working_dir: Option<&PathBuf>,
    ) -> io::Result<()> {
        let memory_file_path = self.get_memory_file(category, is_global, working_dir)?;
        if memory_file_path.exists() {
            fs::remove_file(memory_file_path)?;
        }

        Ok(())
    }

    pub fn clear_all_global_or_local_memories(
        &self,
        is_global: bool,
        working_dir: Option<&PathBuf>,
    ) -> io::Result<()> {
        let base_dir = self.base_dir(is_global, working_dir);
        if base_dir.exists() {
            reject_symlink(&base_dir)?;
            assert_within_root(&self.containment_root(is_global, working_dir), &base_dir)?;
            fs::remove_dir_all(&base_dir)?;
        }
        Ok(())
    }

    /// Stores a memory with optional tags in a specified category
    #[tool(
        name = "remember_memory",
        description = "Stores a memory with optional tags in a specified category"
    )]
    pub async fn remember_memory(
        &self,
        params: Parameters<RememberMemoryParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let params = params.0;
        let working_dir = extract_working_dir_from_meta(&context.meta);

        if params.data.is_empty() {
            return Err(ErrorData::new(
                ErrorCode::INVALID_PARAMS,
                "Data must not be empty when remembering a memory".to_string(),
                None,
            ));
        }

        let tags: Vec<&str> = params.tags.iter().map(|s| s.as_str()).collect();
        self.remember(
            "context",
            &params.category,
            &params.data,
            &tags,
            params.is_global,
            working_dir.as_ref(),
        )
        .map_err(to_error_data)?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Stored memory in category: {}",
            params.category
        ))]))
    }

    /// Retrieves all memories from a specified category
    #[tool(
        name = "retrieve_memories",
        description = "Retrieves all memories from a specified category"
    )]
    pub async fn retrieve_memories(
        &self,
        params: Parameters<RetrieveMemoriesParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let params = params.0;
        let working_dir = extract_working_dir_from_meta(&context.meta);

        let memories = if params.category == "*" {
            self.retrieve_all(params.is_global, working_dir.as_ref())
        } else {
            self.retrieve(&params.category, params.is_global, working_dir.as_ref())
        }
        .map_err(to_error_data)?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Retrieved memories: {:?}",
            memories
        ))]))
    }

    /// Removes all memories within a specified category
    #[tool(
        name = "remove_memory_category",
        description = "Removes all memories within a specified category"
    )]
    pub async fn remove_memory_category(
        &self,
        params: Parameters<RemoveMemoryCategoryParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let params = params.0;
        let working_dir = extract_working_dir_from_meta(&context.meta);

        let message = if params.category == "*" {
            self.clear_all_global_or_local_memories(params.is_global, working_dir.as_ref())
                .map_err(to_error_data)?;
            format!(
                "Cleared all memory {} categories",
                if params.is_global { "global" } else { "local" }
            )
        } else {
            self.clear_memory(&params.category, params.is_global, working_dir.as_ref())
                .map_err(to_error_data)?;
            format!("Cleared memories in category: {}", params.category)
        };

        Ok(CallToolResult::success(vec![Content::text(message)]))
    }

    /// Removes a specific memory within a specified category
    #[tool(
        name = "remove_specific_memory",
        description = "Removes a specific memory within a specified category"
    )]
    pub async fn remove_specific_memory(
        &self,
        params: Parameters<RemoveSpecificMemoryParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let params = params.0;
        let working_dir = extract_working_dir_from_meta(&context.meta);

        self.remove_specific_memory_internal(
            &params.category,
            &params.memory_content,
            params.is_global,
            working_dir.as_ref(),
        )
        .map_err(to_error_data)?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Removed specific memory from category: {}",
            params.category
        ))]))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for MemoryServer {
    fn get_info(&self) -> ServerInfo {
        InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "bharatcode-memory",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(self.instructions.clone())
    }
}

// Remove the old MemoryArgs struct since we're using the new parameter structs

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_lazy_directory_creation() {
        let temp_dir = tempdir().unwrap();
        let memory_base = temp_dir.path().join("test_memory");
        let working_dir = memory_base.join("working");

        let router = MemoryServer {
            tool_router: ToolRouter::new(),
            instructions: String::new(),
            global_memory_dir: memory_base.join("global"),
        };

        let local_memory_dir = working_dir.join(".bharatcode").join("memory");

        assert!(!router.global_memory_dir.exists());
        assert!(!local_memory_dir.exists());

        router
            .remember(
                "test_context",
                "test_category",
                "test_data",
                &["tag1"],
                false,
                Some(&working_dir),
            )
            .unwrap();

        assert!(local_memory_dir.exists());
        assert!(!router.global_memory_dir.exists());

        router
            .remember(
                "test_context",
                "global_category",
                "global_data",
                &["global_tag"],
                true,
                None,
            )
            .unwrap();

        assert!(router.global_memory_dir.exists());
    }

    #[test]
    fn test_clear_nonexistent_directories() {
        let temp_dir = tempdir().unwrap();
        let memory_base = temp_dir.path().join("nonexistent_memory");
        let working_dir = memory_base.join("working");

        let router = MemoryServer {
            tool_router: ToolRouter::new(),
            instructions: String::new(),
            global_memory_dir: memory_base.join("global"),
        };

        assert!(router
            .clear_all_global_or_local_memories(false, Some(&working_dir))
            .is_ok());
        assert!(router
            .clear_all_global_or_local_memories(true, None)
            .is_ok());
    }

    #[test]
    fn test_remember_retrieve_clear_workflow() {
        let temp_dir = tempdir().unwrap();
        let memory_base = temp_dir.path().join("workflow_test");
        let working_dir = memory_base.join("working");

        let router = MemoryServer {
            tool_router: ToolRouter::new(),
            instructions: String::new(),
            global_memory_dir: memory_base.join("global"),
        };

        router
            .remember(
                "context",
                "test_category",
                "test_data_content",
                &["test_tag"],
                false,
                Some(&working_dir),
            )
            .unwrap();

        let memories = router
            .retrieve("test_category", false, Some(&working_dir))
            .unwrap();
        assert!(!memories.is_empty());

        let has_content = memories.values().any(|v| {
            v.iter()
                .any(|content| content.contains("test_data_content"))
        });
        assert!(has_content);

        router
            .clear_memory("test_category", false, Some(&working_dir))
            .unwrap();

        let memories_after_clear = router
            .retrieve("test_category", false, Some(&working_dir))
            .unwrap();
        assert!(memories_after_clear.is_empty());
    }

    #[test]
    fn test_directory_creation_on_write() {
        let temp_dir = tempdir().unwrap();
        let memory_base = temp_dir.path().join("write_test");
        let working_dir = memory_base.join("working");

        let router = MemoryServer {
            tool_router: ToolRouter::new(),
            instructions: String::new(),
            global_memory_dir: memory_base.join("global"),
        };

        let local_memory_dir = working_dir.join(".bharatcode").join("memory");
        assert!(!local_memory_dir.exists());

        router
            .remember(
                "context",
                "category",
                "data",
                &[],
                false,
                Some(&working_dir),
            )
            .unwrap();

        assert!(local_memory_dir.exists());
        assert!(local_memory_dir.join("category.txt").exists());
    }

    #[test]
    fn test_remove_specific_memory() {
        let temp_dir = tempdir().unwrap();
        let memory_base = temp_dir.path().join("remove_test");
        let working_dir = memory_base.join("working");

        let router = MemoryServer {
            tool_router: ToolRouter::new(),
            instructions: String::new(),
            global_memory_dir: memory_base.join("global"),
        };

        router
            .remember(
                "context",
                "category",
                "keep_this",
                &[],
                false,
                Some(&working_dir),
            )
            .unwrap();
        router
            .remember(
                "context",
                "category",
                "remove_this",
                &[],
                false,
                Some(&working_dir),
            )
            .unwrap();

        let memories = router
            .retrieve("category", false, Some(&working_dir))
            .unwrap();
        assert_eq!(memories.len(), 1);

        router
            .remove_specific_memory_internal("category", "remove_this", false, Some(&working_dir))
            .unwrap();

        let memories_after = router
            .retrieve("category", false, Some(&working_dir))
            .unwrap();
        let has_removed = memories_after
            .values()
            .any(|v| v.iter().any(|content| content.contains("remove_this")));
        assert!(!has_removed);

        let has_kept = memories_after
            .values()
            .any(|v| v.iter().any(|content| content.contains("keep_this")));
        assert!(has_kept);
    }

    fn test_router(memory_base: &std::path::Path) -> MemoryServer {
        MemoryServer {
            tool_router: ToolRouter::new(),
            instructions: String::new(),
            global_memory_dir: memory_base.join("global"),
        }
    }

    #[test]
    fn test_valid_categories_are_preserved() {
        for category in [
            "notes",
            "test_category",
            "kebab-case",
            "MixedCase",
            "with123digits",
            "_leading_underscore",
            "9",
            &"a".repeat(MAX_CATEGORY_LEN),
        ] {
            assert!(
                validate_category(category).is_ok(),
                "expected {category:?} to be a valid category"
            );
        }
    }

    #[test]
    fn test_traversal_categories_are_rejected() {
        for category in [
            "..",
            ".",
            "../etc/passwd",
            "../../../../etc/cron.d/payload",
            "..\\..\\windows\\system32",
            "notes/../../../escape",
            "....//escape",
            "%2e%2e%2fescape",
        ] {
            assert!(
                validate_category(category).is_err(),
                "expected traversal category {category:?} to be rejected"
            );
        }
    }

    #[test]
    fn test_absolute_path_categories_are_rejected() {
        for category in [
            "/etc/passwd",
            "/tmp/pwned",
            "//server/share/file",
            "C:\\Windows\\System32\\drivers\\etc\\hosts",
            "~/.ssh/authorized_keys",
        ] {
            assert!(
                validate_category(category).is_err(),
                "expected absolute category {category:?} to be rejected"
            );
        }
    }

    #[test]
    fn test_separator_and_other_unsafe_categories_are_rejected() {
        for category in [
            "",
            "*",
            "a/b",
            "a\\b",
            "nested/dir/notes",
            "trailing/",
            "with space",
            "with\nnewline",
            "with\0null",
            "dotted.name",
            "unicodé",
            &"a".repeat(MAX_CATEGORY_LEN + 1),
        ] {
            assert!(
                validate_category(category).is_err(),
                "expected unsafe category {category:?} to be rejected"
            );
        }
    }

    #[test]
    fn test_traversal_category_cannot_write_outside_memory_dir() {
        let temp_dir = tempdir().unwrap();
        let memory_base = temp_dir.path().join("traversal");
        let working_dir = memory_base.join("working");
        let router = test_router(&memory_base);

        let escaped = temp_dir.path().join("escaped.txt");
        let relative_escape = format!(
            "../../../{}",
            escaped.file_stem().unwrap().to_str().unwrap()
        );

        let err = router
            .remember(
                "context",
                &relative_escape,
                "pwned",
                &[],
                false,
                Some(&working_dir),
            )
            .unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(!escaped.exists());
    }

    #[test]
    fn test_absolute_category_cannot_write_outside_memory_dir() {
        let temp_dir = tempdir().unwrap();
        let memory_base = temp_dir.path().join("absolute");
        let working_dir = memory_base.join("working");
        let router = test_router(&memory_base);

        let target = temp_dir.path().join("absolute_escape");
        let category = target.to_str().unwrap().to_string();

        let err = router
            .remember(
                "context",
                &category,
                "pwned",
                &[],
                false,
                Some(&working_dir),
            )
            .unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(!target.with_extension("txt").exists());
        assert!(!working_dir.join(".bharatcode").join("memory").exists());
    }

    #[test]
    fn test_invalid_category_rejected_on_every_operation() {
        let temp_dir = tempdir().unwrap();
        let memory_base = temp_dir.path().join("all_ops");
        let working_dir = memory_base.join("working");
        let router = test_router(&memory_base);

        let category = "../../escape";

        assert!(router
            .remember("context", category, "data", &[], false, Some(&working_dir))
            .is_err());
        assert!(router
            .retrieve(category, false, Some(&working_dir))
            .is_err());
        assert!(router
            .clear_memory(category, false, Some(&working_dir))
            .is_err());
        assert!(router
            .remove_specific_memory_internal(category, "data", false, Some(&working_dir))
            .is_err());
    }

    #[cfg(unix)]
    #[test]
    fn test_symlinked_memory_file_is_not_followed() {
        use std::os::unix::fs::symlink;

        let temp_dir = tempdir().unwrap();
        let memory_base = temp_dir.path().join("symlink_file");
        let working_dir = memory_base.join("working");
        let router = test_router(&memory_base);

        let memory_dir = working_dir.join(".bharatcode").join("memory");
        fs::create_dir_all(&memory_dir).unwrap();

        let secret = temp_dir.path().join("secret.txt");
        fs::write(&secret, "original secret\n").unwrap();
        symlink(&secret, memory_dir.join("notes.txt")).unwrap();

        let write_err = router
            .remember("context", "notes", "pwned", &[], false, Some(&working_dir))
            .unwrap_err();
        assert_eq!(write_err.kind(), io::ErrorKind::PermissionDenied);

        let read_err = router
            .retrieve("notes", false, Some(&working_dir))
            .unwrap_err();
        assert_eq!(read_err.kind(), io::ErrorKind::PermissionDenied);

        let clear_err = router
            .clear_memory("notes", false, Some(&working_dir))
            .unwrap_err();
        assert_eq!(clear_err.kind(), io::ErrorKind::PermissionDenied);

        assert_eq!(fs::read_to_string(&secret).unwrap(), "original secret\n");
    }

    #[cfg(unix)]
    #[test]
    fn test_symlinked_memory_dir_is_not_followed() {
        use std::os::unix::fs::symlink;

        let temp_dir = tempdir().unwrap();
        let memory_base = temp_dir.path().join("symlink_dir");
        let working_dir = memory_base.join("working");
        let router = test_router(&memory_base);

        let outside = temp_dir.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::create_dir_all(working_dir.join(".bharatcode")).unwrap();
        symlink(&outside, working_dir.join(".bharatcode").join("memory")).unwrap();

        let err = router
            .remember("context", "notes", "pwned", &[], false, Some(&working_dir))
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);

        assert!(!outside.join("notes.txt").exists());
        assert!(router
            .clear_all_global_or_local_memories(false, Some(&working_dir))
            .is_err());
        assert!(outside.exists());
    }

    #[cfg(unix)]
    #[test]
    fn test_symlinked_bharatcode_parent_cannot_escape_working_dir() {
        use std::os::unix::fs::symlink;

        let temp_dir = tempdir().unwrap();
        let memory_base = temp_dir.path().join("symlink_parent");
        let working_dir = memory_base.join("working");
        let router = test_router(&memory_base);

        // The outside tree the attacker wants to reach. `.bharatcode/memory/notes.txt`
        // below are all ordinary entries; only `.bharatcode` itself is the symlink that
        // redirects the whole memory tree outside `working_dir`.
        let outside = temp_dir.path().join("outside");
        let outside_memory = outside.join("memory");
        fs::create_dir_all(&outside_memory).unwrap();
        fs::write(outside_memory.join("notes.txt"), "outside secret\n").unwrap();

        fs::create_dir_all(&working_dir).unwrap();
        symlink(&outside, working_dir.join(".bharatcode")).unwrap();

        let write_err = router
            .remember("context", "notes", "pwned", &[], false, Some(&working_dir))
            .unwrap_err();
        assert_eq!(write_err.kind(), io::ErrorKind::PermissionDenied);

        let read_err = router
            .retrieve("notes", false, Some(&working_dir))
            .unwrap_err();
        assert_eq!(read_err.kind(), io::ErrorKind::PermissionDenied);

        let remove_err = router
            .remove_specific_memory_internal("notes", "secret", false, Some(&working_dir))
            .unwrap_err();
        assert_eq!(remove_err.kind(), io::ErrorKind::PermissionDenied);

        let clear_err = router
            .clear_memory("notes", false, Some(&working_dir))
            .unwrap_err();
        assert_eq!(clear_err.kind(), io::ErrorKind::PermissionDenied);

        assert!(router.retrieve_all(false, Some(&working_dir)).is_err());

        let clear_all_err = router
            .clear_all_global_or_local_memories(false, Some(&working_dir))
            .unwrap_err();
        assert_eq!(clear_all_err.kind(), io::ErrorKind::PermissionDenied);

        // The outside target is neither exposed, modified, nor deleted.
        assert_eq!(
            fs::read_to_string(outside_memory.join("notes.txt")).unwrap(),
            "outside secret\n"
        );
        assert!(outside_memory.exists());
    }

    #[cfg(unix)]
    #[test]
    fn test_retrieve_all_skips_symlinked_and_invalid_entries() {
        use std::os::unix::fs::symlink;

        let temp_dir = tempdir().unwrap();
        let memory_base = temp_dir.path().join("retrieve_all");
        let working_dir = memory_base.join("working");
        let router = test_router(&memory_base);

        router
            .remember(
                "context",
                "notes",
                "real_memory",
                &[],
                false,
                Some(&working_dir),
            )
            .unwrap();

        let memory_dir = working_dir.join(".bharatcode").join("memory");
        let secret = temp_dir.path().join("secret.txt");
        fs::write(&secret, "secret contents\n").unwrap();
        symlink(&secret, memory_dir.join("linked.txt")).unwrap();
        fs::write(memory_dir.join("not a category.txt"), "ignored\n").unwrap();

        let memories = router.retrieve_all(false, Some(&working_dir)).unwrap();

        assert_eq!(memories.len(), 1);
        assert!(memories.contains_key("notes"));
        let flattened = format!("{:?}", memories);
        assert!(flattened.contains("real_memory"));
        assert!(!flattened.contains("secret contents"));
    }
}
