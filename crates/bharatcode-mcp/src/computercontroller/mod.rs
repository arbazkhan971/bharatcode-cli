#[cfg(not(windows))]
use crate::subprocess::merged_path;
use crate::subprocess::SubprocessExt;
#[cfg(target_os = "macos")]
use base64::Engine;
use etcetera::{choose_app_strategy, AppStrategy};
use indoc::{formatdoc, indoc};
use reqwest::{header::LOCATION, Client, Response, StatusCode, Url};
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{
        AnnotateAble, CallToolResult, Content, ErrorCode, ErrorData, Implementation,
        InitializeResult, ListResourcesResult, PaginatedRequestParams, RawResource,
        ReadResourceRequestParams, ReadResourceResult, Resource, ResourceContents,
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
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    path::{Component, Path, PathBuf},
    sync::Arc,
    sync::Mutex,
    time::Duration,
};
use tokio::process::Command;
use url::Host;

#[cfg(target_os = "macos")]
use rmcp::model::Role;
#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

mod docx_tool;
mod pdf_tool;
mod xlsx_tool;

mod platform;
use platform::{create_system_automation, SystemAutomation};

/// Enum for save_as parameter in web_scrape tool
#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, Default)]
#[serde(rename_all = "lowercase")]
pub enum SaveAsFormat {
    /// Save as text (for HTML pages)
    #[default]
    Text,
    /// Save as JSON (for API responses)
    Json,
    /// Save as binary (for images and other files)
    Binary,
}

/// Parameters for the web_scrape tool
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct WebScrapeParams {
    /// The URL to fetch content from
    pub url: String,
    /// Format of the response.
    #[serde(default)]
    pub save_as: SaveAsFormat,
}

/// Enum for language parameter in automation_script tool
#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(rename_all = "lowercase")]
pub enum ScriptLanguage {
    /// Shell/Bash script
    Shell,
    /// Batch script (Windows)
    Batch,
    /// Ruby script
    Ruby,
    /// PowerShell script
    Powershell,
}

/// Enum for command parameter in cache tool
#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(rename_all = "lowercase")]
pub enum CacheCommand {
    /// List all cached files
    List,
    /// View content of a cached file
    View,
    /// Delete a cached file
    Delete,
    /// Clear all cached files
    Clear,
}

/// Parameters for the automation_script tool
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct AutomationScriptParams {
    /// The scripting language to use
    #[serde(rename = "language")]
    pub language: ScriptLanguage,
    /// The script content
    pub script: String,
    /// Whether to save the script output to a file
    #[serde(default)]
    pub save_output: bool,
}

/// Parameters for the computer_control tool (Windows, Linux)
#[cfg(not(target_os = "macos"))]
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ComputerControlParams {
    /// The automation script content (PowerShell for Windows, shell for Linux)
    pub script: String,
    /// Whether to save the script output to a file
    #[serde(default)]
    pub save_output: bool,
}

/// Parameters for the computer_control tool (macOS — Peekaboo CLI passthrough)
#[cfg(target_os = "macos")]
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ComputerControlParams {
    /// The peekaboo subcommand and arguments as a single string.
    /// Examples:
    ///   "see --app Safari --annotate"
    ///   "click --on B1"
    ///   "type --text \"hello\" --return"
    ///   "hotkey --keys cmd,c"
    ///   "app launch Safari --open https://example.com"
    ///   "window list --app Safari --json"
    ///   "press tab --count 3"
    ///   "clipboard --action get"
    pub command: String,
    /// Whether to capture and return a screenshot as part of the result.
    /// Useful after click/type actions to see the updated UI state.
    #[serde(default)]
    pub capture_screenshot: bool,
}

/// Parameters for the cache tool
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CacheParams {
    /// The command to perform
    pub command: CacheCommand,
    /// Identifier of the cached entry for view/delete commands, as reported by the
    /// list command. Only entries inside the cache directory can be addressed.
    pub path: Option<String>,
}

/// Parameters for the pdf_tool
/// Enum for operation parameter in pdf_tool
#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(rename_all = "snake_case")]
pub enum PdfOperation {
    /// Extract all text content from the PDF
    ExtractText,
    /// Extract and save embedded images to PNG files
    ExtractImages,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct PdfToolParams {
    /// Path to the PDF file
    pub path: String,
    /// Operation to perform on the PDF
    pub operation: PdfOperation,
}

/// Enum for operation parameter in docx_tool
#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(rename_all = "snake_case")]
pub enum DocxOperation {
    /// Extract all text content and structure from the DOCX
    ExtractText,
    /// Create a new DOCX or update existing one with provided content
    UpdateDoc,
}

/// Enum for update mode in docx_tool params
#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, Default)]
#[serde(rename_all = "snake_case")]
pub enum DocxUpdateMode {
    /// Add content to end of document (default)
    #[default]
    Append,
    /// Replace specific text with new content
    Replace,
    /// Add content with specific heading level and styling
    Structured,
    /// Add an image to the document (with optional caption)
    AddImage,
}

/// Enum for text alignment in docx_tool params
#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(rename_all = "lowercase")]
pub enum TextAlignment {
    /// Left alignment
    Left,
    /// Center alignment
    Center,
    /// Right alignment
    Right,
    /// Justified alignment
    Justified,
}

/// Styling options for text in docx_tool
#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, Default)]
pub struct DocxTextStyle {
    /// Make text bold
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    /// Make text italic
    #[serde(skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    /// Make text underlined
    #[serde(skip_serializing_if = "Option::is_none")]
    pub underline: Option<bool>,
    /// Font size in points
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u32>,
    /// Text color in hex format (e.g., 'FF0000' for red)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Text alignment
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alignment: Option<TextAlignment>,
}

/// Additional parameters for update_doc operation
#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone, Default)]
pub struct DocxUpdateParams {
    /// Update mode (default: append)
    #[serde(default)]
    pub mode: DocxUpdateMode,
    /// Text to replace (required for replace mode)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_text: Option<String>,
    /// Heading level for structured mode (e.g., 'Heading1', 'Heading2')
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    /// Path to the image file (required for add_image mode)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_path: Option<String>,
    /// Image width in pixels (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    /// Image height in pixels (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    /// Styling options for the text
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<DocxTextStyle>,
}

/// Parameters for the docx_tool
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct DocxToolParams {
    /// Path to the DOCX file
    pub path: String,
    /// Operation to perform on the DOCX
    pub operation: DocxOperation,
    /// Content to write (required for update_doc operation)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Additional parameters for update_doc operation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<DocxUpdateParams>,
}

/// Parameters for the xlsx_tool
/// Enum for operation parameter in xlsx_tool
#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(rename_all = "snake_case")]
pub enum XlsxOperation {
    /// List all worksheets in the workbook
    ListWorksheets,
    /// Get column names from a worksheet
    GetColumns,
    /// Get values and formulas from a cell range
    GetRange,
    /// Search for text in a worksheet
    FindText,
    /// Update a single cell's value
    UpdateCell,
    /// Get value and formula from a specific cell
    GetCell,
    /// Save changes back to the file
    Save,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct XlsxToolParams {
    /// Path to the XLSX file
    pub path: String,
    /// Operation to perform on the XLSX file
    pub operation: XlsxOperation,
    /// Worksheet name (if not provided, uses first worksheet)
    pub worksheet: Option<String>,
    /// Cell range in A1 notation (e.g., 'A1:C10') for get_range operation
    pub range: Option<String>,
    /// Text to search for in find_text operation
    pub search_text: Option<String>,
    /// Whether search should be case-sensitive
    #[serde(default)]
    pub case_sensitive: bool,
    /// Row number for update_cell and get_cell operations
    pub row: Option<u64>,
    /// Column number for update_cell and get_cell operations
    pub col: Option<u64>,
    /// New value for update_cell operation
    pub value: Option<String>,
}

/// ComputerController MCP Server using official RMCP SDK
#[derive(Clone)]
pub struct ComputerControllerServer {
    tool_router: ToolRouter<Self>,
    cache_dir: PathBuf,
    active_resources: Arc<Mutex<HashMap<String, ResourceContents>>>,
    instructions: String,
    system_automation: Arc<Box<dyn SystemAutomation + Send + Sync>>,
    #[cfg(target_os = "macos")]
    peekaboo_installed: Arc<AtomicBool>,
}

impl Default for ComputerControllerServer {
    fn default() -> Self {
        Self::new()
    }
}

fn cache_entry_rejected(requested: &str, reason: &str) -> ErrorData {
    ErrorData::new(
        ErrorCode::INVALID_PARAMS,
        format!(
            "Refusing to access '{}': {}. Use an entry name reported by the cache list command.",
            requested, reason
        ),
        None,
    )
}

/// Limits applied to every web_scrape fetch.
///
/// Production always uses [`FetchPolicy::default`]; the fields exist so the tests can shrink the
/// limits and exempt a single loopback listener without weakening the shipped defaults.
#[derive(Clone, Debug)]
struct FetchPolicy {
    max_bytes: usize,
    max_redirects: usize,
    connect_timeout: Duration,
    read_timeout: Duration,
    request_timeout: Duration,
    total_timeout: Duration,
    dns_timeout: Duration,
    bypass_proxy: bool,
    exempt_addr: Option<SocketAddr>,
}

impl Default for FetchPolicy {
    fn default() -> Self {
        Self {
            max_bytes: 10 * 1024 * 1024,
            max_redirects: 5,
            connect_timeout: Duration::from_secs(10),
            read_timeout: Duration::from_secs(20),
            request_timeout: Duration::from_secs(60),
            total_timeout: Duration::from_secs(120),
            dns_timeout: Duration::from_secs(5),
            bypass_proxy: false,
            exempt_addr: None,
        }
    }
}

fn scrape_rejected(message: String) -> ErrorData {
    ErrorData::new(ErrorCode::INVALID_PARAMS, message, None)
}

fn scrape_failed(message: String) -> ErrorData {
    ErrorData::new(ErrorCode::INTERNAL_ERROR, message, None)
}

fn is_blocked_ipv4(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_multicast()
        || a == 0
        || (a == 100 && (64..128).contains(&b))
        || (a == 192 && b == 0)
        || (a == 198 && (b & 0xfe) == 18)
        || a >= 240
}

fn is_blocked_ipv6(ip: Ipv6Addr) -> bool {
    if let Some(mapped) = ip.to_ipv4_mapped().or_else(|| ip.to_ipv4()) {
        return is_blocked_ipv4(mapped);
    }
    let segments = ip.segments();
    ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_multicast()
        || (segments[0] & 0xfe00) == 0xfc00
        || (segments[0] & 0xffc0) == 0xfe80
        || (segments[0] == 0x0064 && segments[1] == 0xff9b)
        || (segments[0] == 0x2001 && segments[1] == 0x0db8)
}

fn is_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_blocked_ipv4(ip),
        IpAddr::V6(ip) => is_blocked_ipv6(ip),
    }
}

fn is_blocked_addr(addr: SocketAddr, policy: &FetchPolicy) -> bool {
    if policy.exempt_addr == Some(addr) {
        return false;
    }
    is_blocked_ip(addr.ip())
}

/// Validate one hop and resolve it to the exact addresses the request is allowed to reach.
///
/// Returns the domain to pin (None for IP literals, which need no resolution) alongside the
/// vetted addresses. Anything uncertain - an unknown scheme, a name that will not resolve, a
/// single blocked answer in a multi-address record - is rejected rather than attempted.
async fn resolve_scrape_target(
    url: &Url,
    policy: &FetchPolicy,
) -> Result<(Option<String>, Vec<SocketAddr>), ErrorData> {
    match url.scheme() {
        "http" | "https" => {}
        scheme => {
            return Err(scrape_rejected(format!(
                "Refusing to fetch '{}': only http and https URLs can be scraped, not '{}'",
                url, scheme
            )))
        }
    }

    if !url.username().is_empty() || url.password().is_some() {
        return Err(scrape_rejected(format!(
            "Refusing to fetch '{}': URLs carrying credentials are not allowed",
            url
        )));
    }

    let host = url
        .host()
        .ok_or_else(|| {
            scrape_rejected(format!("Refusing to fetch '{}': the URL has no host", url))
        })?
        .to_owned();
    let port = url.port_or_known_default().ok_or_else(|| {
        scrape_rejected(format!("Refusing to fetch '{}': the URL has no port", url))
    })?;

    let (domain, addrs) = match host {
        Host::Ipv4(ip) => (None, vec![SocketAddr::new(IpAddr::V4(ip), port)]),
        Host::Ipv6(ip) => (None, vec![SocketAddr::new(IpAddr::V6(ip), port)]),
        Host::Domain(domain) => {
            let resolved = tokio::time::timeout(
                policy.dns_timeout,
                tokio::net::lookup_host((domain.as_str(), port)),
            )
            .await
            .map_err(|_| {
                scrape_rejected(format!("Refusing to fetch '{}': DNS lookup timed out", url))
            })?
            .map_err(|e| {
                scrape_rejected(format!(
                    "Refusing to fetch '{}': DNS lookup failed: {}",
                    url, e
                ))
            })?;
            let addresses = resolved.collect::<Vec<_>>();
            (Some(domain), addresses)
        }
    };

    if addrs.is_empty() {
        return Err(scrape_rejected(format!(
            "Refusing to fetch '{}': the host did not resolve to any address",
            url
        )));
    }

    if let Some(blocked) = addrs.iter().find(|addr| is_blocked_addr(**addr, policy)) {
        return Err(scrape_rejected(format!(
            "Refusing to fetch '{}': it resolves to {}, which is a loopback, private, link-local or otherwise non-public address",
            url,
            blocked.ip()
        )));
    }

    Ok((domain, addrs))
}

/// Build a client for a single hop, pinned to the addresses that were just vetted so a name
/// cannot resolve to a public address during validation and a private one at connect time.
fn build_scrape_client(
    domain: Option<&str>,
    addrs: &[SocketAddr],
    policy: &FetchPolicy,
) -> Result<Client, ErrorData> {
    let mut builder = Client::builder()
        .user_agent("bharatcode/1.0")
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(policy.connect_timeout)
        .read_timeout(policy.read_timeout)
        .timeout(policy.request_timeout);

    if let Some(domain) = domain {
        builder = builder.resolve_to_addrs(domain, addrs);
    }
    if policy.bypass_proxy {
        builder = builder.no_proxy();
    }

    builder
        .build()
        .map_err(|e| scrape_failed(format!("Failed to build HTTP client: {}", e)))
}

fn is_redirect(status: StatusCode) -> bool {
    matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308)
}

async fn read_bounded(mut response: Response, max_bytes: usize) -> Result<Vec<u8>, ErrorData> {
    let too_large = || {
        scrape_failed(format!(
            "Response body exceeds the {} byte limit",
            max_bytes
        ))
    };

    if response
        .content_length()
        .is_some_and(|len| len > max_bytes as u64)
    {
        return Err(too_large());
    }

    let mut body: Vec<u8> = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| scrape_failed(format!("Failed to read response body: {}", e)))?
    {
        if body.len() + chunk.len() > max_bytes {
            return Err(too_large());
        }
        body.extend_from_slice(&chunk);
    }

    Ok(body)
}

/// Fetch a URL under [`FetchPolicy`]: every hop is revalidated, redirects are followed by hand so
/// they cannot pivot to an internal host, and the body is streamed under a hard byte cap.
async fn fetch_bounded(url: &str, policy: &FetchPolicy) -> Result<Vec<u8>, ErrorData> {
    tokio::time::timeout(policy.total_timeout, fetch_bounded_inner(url, policy))
        .await
        .map_err(|_| scrape_failed(format!("Timed out fetching '{}'", url)))?
}

async fn fetch_bounded_inner(url: &str, policy: &FetchPolicy) -> Result<Vec<u8>, ErrorData> {
    let mut target =
        Url::parse(url).map_err(|e| scrape_rejected(format!("Invalid URL '{}': {}", url, e)))?;
    let mut redirects = 0;

    loop {
        let (domain, addrs) = resolve_scrape_target(&target, policy).await?;
        let client = build_scrape_client(domain.as_deref(), &addrs, policy)?;

        let response = client
            .get(target.clone())
            .header("Accept", "text/markdown, */*")
            .send()
            .await
            .map_err(|e| scrape_failed(format!("Failed to fetch URL: {}", e)))?;

        let status = response.status();
        if is_redirect(status) {
            if redirects == policy.max_redirects {
                return Err(scrape_failed(format!(
                    "Refusing to follow more than {} redirects for '{}'",
                    policy.max_redirects, url
                )));
            }
            let location = response
                .headers()
                .get(LOCATION)
                .and_then(|location| location.to_str().ok())
                .ok_or_else(|| {
                    scrape_failed(format!(
                        "Redirect status {} without a Location header",
                        status
                    ))
                })?;
            target = target.join(location).map_err(|e| {
                scrape_rejected(format!("Invalid redirect target '{}': {}", location, e))
            })?;
            redirects += 1;
            continue;
        }

        if !status.is_success() {
            return Err(scrape_failed(format!(
                "HTTP request failed with status: {}",
                status
            )));
        }

        return read_bounded(response, policy.max_bytes).await;
    }
}

#[tool_router(router = tool_router)]
impl ComputerControllerServer {
    pub fn new() -> Self {
        // choose_app_strategy().cache_dir()
        // - macOS/Linux: ~/.cache/goose/computer_controller/
        // - Windows:     ~\AppData\Local\Block\goose\cache\computer_controller\
        // keep previous behavior of defaulting to /tmp/
        let cache_dir = choose_app_strategy(crate::APP_STRATEGY.clone())
            .map(|strategy| strategy.in_cache_dir("computer_controller"))
            .unwrap_or_else(|_| create_system_automation().get_temp_path());

        fs::create_dir_all(&cache_dir).unwrap_or_else(|_| {
            println!(
                "Warning: Failed to create cache directory at {:?}",
                cache_dir
            )
        });

        let system_automation: Arc<Box<dyn SystemAutomation + Send + Sync>> =
            Arc::new(create_system_automation());

        let has_display = system_automation.has_display();

        let os_specific_instructions = match (std::env::consts::OS, has_display) {
            ("windows", _) => indoc! {r#"
            Here are some extra tools:
            automation_script
              - Create and run PowerShell or Batch scripts
              - PowerShell is recommended for most tasks
              - Scripts can save their output to files
              - Windows-specific features:
                - PowerShell for system automation and UI control
                - Windows Management Instrumentation (WMI)
                - Registry access and system settings
              - Use the screenshot tool if needed to help with tasks

            computer_control
              - System automation using PowerShell
              - Consider the screenshot tool to work out what is on screen and what to do to help with the control task.
            "#},
            ("macos", _) => indoc! {r#"
            Here are some extra tools:
            automation_script
              - Create and run Shell, Ruby, or AppleScript scripts
              - Scripts can save their output to files

            computer_control — Peekaboo CLI for macOS UI automation (auto-installed via Homebrew).
              Peekaboo captures/inspects screens, targets UI elements, drives input, and manages
              apps/windows/menus. Pass a peekaboo subcommand string as the `command` parameter.
              Set `capture_screenshot: true` to capture the screen after actions (click, type, etc.).
              Commands support `--json`/`-j` for structured output. Run `peekaboo <cmd> --help` for
              full flags if needed.

              Quickstart (most reliable flow):
                1. command: "see --app Safari --annotate"    — get annotated screenshot with element IDs
                2. command: "click --on B3 --app Safari"     — click element B3
                3. command: "type \"user@example.com\" --app Safari"  — type text
                4. command: "press tab --count 1 --app Safari"       — press tab
                5. command: "type \"password\" --app Safari --return" — type and press enter

              Vision:
              - see — annotated UI map with element IDs and optional AI analysis
                `see --app Safari --annotate`, `see --mode screen --screen-index 0`
                `see --app Notes --analyze "describe what's on screen"`
              - image — capture screenshots without annotation
                `image --mode frontmost`, `image --mode screen --screen-index 1 --retina`
                `image --app Safari --window-title "Dashboard" --analyze "Summarize KPIs"`
              - capture — live motion-aware capture
                `capture live --mode region --region 100,100,800,600 --duration 30`

              Interaction:
              - click — by element ID, query, or coordinates with smart waits
                `click --on B1`, `click --coords 100,200`, `click --on B1 --double`, `click --on B1 --right`
              - type — text input with optional control keys
                `type "hello" --return`, `type "text" --clear --app Notes`, `type "slow" --wpm 80`
              - press — special key sequences with repeats
                `press tab --count 3`, `press escape`, `press return`, `press space`
              - hotkey — modifier key combos (comma-separated)
                `hotkey --keys cmd,c`, `hotkey --keys cmd,shift,t`, `hotkey --keys cmd,a`
              - paste — set clipboard then paste (more reliable than type for long text)
                `paste --text "long multi-line content"`
              - scroll — directional scrolling with optional targeting
                `scroll --direction down --amount 5 --smooth`, `scroll --direction up --amount 3`
              - drag — drag between elements or coordinates
                `drag --from B1 --to T2`, `drag --from-coords 100,100 --to-coords 500,300`
              - swipe — gesture-style drags
                `swipe --from-coords 100,500 --to-coords 100,200 --duration 800`
              - move — cursor positioning
                `move 500,300 --smooth`

              Apps & Windows:
              - app — launch, quit, switch, list applications
                `app launch Safari --open https://example.com`, `app quit Safari`
                `app switch Safari`, `app list`, `app hide Safari`, `app unhide Safari`
              - window — manage window position, size, focus, list
                `window list --app Safari --json`, `window focus --app Safari`
                `window set-bounds --app Safari --x 50 --y 50 --width 1200 --height 800`
                `window close --app Safari`, `window minimize --app Safari`
              - list — enumerate apps, windows, screens
                `list apps --json`, `list windows --json`, `list screens --json`
              - space — macOS Spaces (virtual desktops)
                `space list`, `space switch --index 2`

              Menus & System:
              - menu — click application menu items
                `menu click --app Safari --item "New Window"`
                `menu click --app TextEdit --path "Format > Font > Show Fonts"`
              - menubar — status bar / menu extras
                `menubar list --json`, `menubar click --title "WiFi"`
              - dock — Dock items
                `dock launch Safari`, `dock list --json`
              - dialog — system dialogs and alerts
                `dialog click --button "OK"`, `dialog list`
              - clipboard — read/write clipboard
                `clipboard --action get`, `clipboard --action set --text "content"`
              - open — open URLs or files with app targeting
                `open https://example.com --app Safari`
              - permissions — check Screen Recording / Accessibility status
                `permissions status`

              Common targeting parameters (work across most commands):
              - App/window: `--app Name`, `--pid 1234`, `--window-title "title"`, `--window-id 5678`, `--window-index 0`
              - Elements: `--on B1` (element ID from see), `--coords 100,200`
              - Snapshot reuse: `--snapshot <id>` (reuse a previous see result without re-capturing)
              - Focus: `--no-auto-focus`, `--space-switch`, `--bring-to-current-space`

              Tips:
              - Always `see --annotate` first to identify element IDs before clicking
              - Use `--json` for structured output on list/query commands
              - Use `paste` over `type` for long or multi-line text
              - Use `--screen-index` for multi-monitor setups
              - If something fails, check `permissions status` for missing permissions
              - Use `capture_screenshot: true` on click/type/press actions to verify the result
            "#},
            (_, true) => indoc! {r#"
            Here are some extra tools:
            automation_script
              - Create and run Shell scripts
              - Shell (bash) is recommended for most tasks
              - Scripts can save their output to files
              - Linux-specific features:
                - System automation through shell scripting
                - X11/Wayland window management
                - D-Bus system services integration
                - Desktop environment control
              - Use the screenshot tool if needed to help with tasks

            computer_control
              - System automation using shell commands and system tools
              - Desktop environment automation (GNOME, KDE, etc.)
              - Consider the screenshot tool to work out what is on screen and what to do to help with the control task.

            When you need to interact with websites or web applications, consider using tools like xdotool or wmctrl for:
              - Window management
              - Simulating keyboard/mouse input
              - Automating UI interactions
              - Desktop environment control
            "#},
            (_, false) => indoc! {r#"
            Here are some extra tools:
            automation_script
              - Create and run Shell scripts
              - Shell (bash) is recommended for most tasks
              - Scripts can save their output to files
              - Linux-specific features:
                - System automation through shell scripting
                - D-Bus system services integration

            Note: No display server detected (headless mode). The computer_control tool
            is not available in this environment. Use automation_script for shell-based tasks.
            "#},
        };

        let instructions = formatdoc! {r#"
            You are a helpful assistant to a power user who is not a professional developer, but you may use development tools to help assist them.
            The user may not know how to break down tasks, so you will need to ensure that you do, and run things in batches as needed.
            The ComputerControllerExtension helps you with common tasks like web scraping,
            data processing, and automation without requiring programming expertise.

            You can use scripting as needed to work with text files of data, such as csvs, json, or text files etc.
            Using the developer extension is allowed for more sophisticated tasks or instructed to (js or py can be helpful for more complex tasks if tools are available).

            Accessing web sites, even apis, may be common (you can use scripting to do this) without troubling them too much (they won't know what limits are).
            Try to do your best to find ways to complete a task without too many questions or offering options unless it is really unclear, find a way if you can.
            You can also guide them steps if they can help out as you go along.

            There is already a screenshot tool available you can use if needed to see what is on screen.

            {os_instructions}

            web_scrape
              - Fetch content from html websites and APIs
              - Save as text, JSON, or binary files
              - Content is cached locally for later use
              - This is not optimised for complex websites, so don't use this as the first tool.
            cache
              - Manage your cached files
              - List, view, delete files
              - Clear all cached data
            The extension automatically manages:
            - Cache directory: {cache_dir}
            - File organization and cleanup
            "#,
            os_instructions = os_specific_instructions,
            cache_dir = cache_dir.display()
        };

        let mut tool_router = Self::tool_router();
        if !has_display {
            tool_router.remove_route("computer_control");
        }

        Self {
            tool_router,
            cache_dir,
            active_resources: Arc::new(Mutex::new(HashMap::new())),
            instructions,
            system_automation,
            #[cfg(target_os = "macos")]
            peekaboo_installed: Arc::new(AtomicBool::new(crate::peekaboo::is_peekaboo_installed())),
        }
    }

    // Helper function to generate a cache file path
    fn get_cache_path(&self, prefix: &str, extension: &str) -> PathBuf {
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        self.cache_dir
            .join(format!("{}_{}.{}", prefix, timestamp, extension))
    }

    /// Resolve a caller supplied cache identifier to a file inside the cache directory.
    ///
    /// An identifier is an entry name reported by the list command; the absolute paths the
    /// save helpers report are also accepted as long as they point back into the cache
    /// directory. Traversal segments, absolute paths elsewhere on the filesystem, symlinks
    /// and anything that is not a regular file are rejected, so view/delete can never reach
    /// outside the cache.
    fn resolve_cache_entry(&self, requested: &str) -> Result<PathBuf, ErrorData> {
        let canonical_root = self.cache_dir.canonicalize().map_err(|e| {
            ErrorData::new(
                ErrorCode::INTERNAL_ERROR,
                format!("Failed to resolve cache directory: {}", e),
                None,
            )
        })?;

        let requested = requested.trim();
        let candidate = Path::new(requested);
        let relative = candidate
            .strip_prefix(&canonical_root)
            .or_else(|_| candidate.strip_prefix(&self.cache_dir))
            .unwrap_or(candidate);

        let mut resolved = self.cache_dir.clone();
        let mut file_type = None;
        for component in relative.components() {
            let Component::Normal(name) = component else {
                return Err(cache_entry_rejected(
                    requested,
                    "only entry names inside the cache directory can be addressed",
                ));
            };
            resolved.push(name);

            let metadata = fs::symlink_metadata(&resolved)
                .map_err(|_| cache_entry_rejected(requested, "no such cache entry"))?;
            if metadata.file_type().is_symlink() {
                return Err(cache_entry_rejected(requested, "cache entry is a symlink"));
            }
            file_type = Some(metadata.file_type());
        }

        match file_type {
            Some(file_type) if file_type.is_file() => {}
            Some(_) => {
                return Err(cache_entry_rejected(
                    requested,
                    "cache entry is not a regular file",
                ))
            }
            None => {
                return Err(cache_entry_rejected(
                    requested,
                    "expected the name of a cache entry",
                ))
            }
        }

        let canonical = resolved
            .canonicalize()
            .map_err(|_| cache_entry_rejected(requested, "no such cache entry"))?;
        if !canonical.starts_with(&canonical_root) {
            return Err(cache_entry_rejected(
                requested,
                "resolves outside the cache directory",
            ));
        }

        Ok(resolved)
    }

    /// Entry names, relative to the cache directory, usable as view/delete identifiers.
    fn list_cache_entries(&self) -> Result<Vec<String>, ErrorData> {
        let mut entries = Vec::new();
        let mut pending = vec![self.cache_dir.clone()];

        while let Some(dir) = pending.pop() {
            let read_dir = fs::read_dir(&dir).map_err(|e| {
                ErrorData::new(
                    ErrorCode::INTERNAL_ERROR,
                    format!("Failed to read cache directory: {}", e),
                    None,
                )
            })?;

            for entry in read_dir {
                let entry = entry.map_err(|e| {
                    ErrorData::new(
                        ErrorCode::INTERNAL_ERROR,
                        format!("Failed to read directory entry: {}", e),
                        None,
                    )
                })?;
                let file_type = entry.file_type().map_err(|e| {
                    ErrorData::new(
                        ErrorCode::INTERNAL_ERROR,
                        format!("Failed to read directory entry: {}", e),
                        None,
                    )
                })?;

                if file_type.is_symlink() {
                    continue;
                }
                let path = entry.path();
                if file_type.is_dir() {
                    pending.push(path);
                } else if file_type.is_file() {
                    if let Ok(relative) = path.strip_prefix(&self.cache_dir) {
                        entries.push(relative.display().to_string());
                    }
                }
            }
        }

        entries.sort();
        Ok(entries)
    }

    // Helper function to save content to cache
    async fn save_to_cache(
        &self,
        content: &[u8],
        prefix: &str,
        extension: &str,
    ) -> Result<PathBuf, ErrorData> {
        let cache_path = self.get_cache_path(prefix, extension);
        fs::write(&cache_path, content).map_err(|e| {
            ErrorData::new(
                ErrorCode::INTERNAL_ERROR,
                format!("Failed to write to cache: {}", e),
                None,
            )
        })?;
        Ok(cache_path)
    }

    // Helper function to register a file as a resource
    fn register_as_resource(&self, cache_path: &PathBuf, mime_type: &str) -> Result<(), ErrorData> {
        let uri = Url::from_file_path(cache_path)
            .map_err(|_| {
                ErrorData::new(
                    ErrorCode::INTERNAL_ERROR,
                    "Invalid cache path".to_string(),
                    None,
                )
            })?
            .to_string();

        let resource = ResourceContents::TextResourceContents {
            uri: uri.clone(),
            text: String::new(), // We'll read it when needed
            mime_type: Some(mime_type.to_string()),
            meta: None,
        };

        self.active_resources.lock().unwrap().insert(uri, resource);
        Ok(())
    }

    /// Fetch and save content from a web page
    #[tool(
        name = "web_scrape",
        description = "
            Fetch and save content from a web page. The content can be saved as:
            - text (for HTML pages)
            - json (for API responses)
            - binary (for images and other files)
            Returns 'Content saved to: <path>'. Use cache to read the content.
        "
    )]
    pub async fn web_scrape(
        &self,
        params: Parameters<WebScrapeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let params = params.0;
        let save_as = params.save_as;

        let body = fetch_bounded(&params.url, &FetchPolicy::default()).await?;

        let (content, extension, mime_type) = match save_as {
            SaveAsFormat::Text => {
                let text = String::from_utf8_lossy(&body).into_owned();
                (text.into_bytes(), "txt", "text/plain")
            }
            SaveAsFormat::Json => {
                let text = String::from_utf8_lossy(&body).into_owned();
                serde_json::from_str::<serde_json::Value>(&text).map_err(|e| {
                    ErrorData::new(
                        ErrorCode::INTERNAL_ERROR,
                        format!("Invalid JSON response: {}", e),
                        None,
                    )
                })?;
                (text.into_bytes(), "json", "application/json")
            }
            SaveAsFormat::Binary => (body, "bin", "application/octet-stream"),
        };

        // Save to cache
        let cache_path = self.save_to_cache(&content, "web", extension).await?;

        // Register as a resource
        self.register_as_resource(&cache_path, mime_type)?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Content saved to: {}",
            cache_path.display()
        ))]))
    }

    /// Create and run small scripts for automation tasks
    #[cfg(target_os = "windows")]
    #[tool(
        name = "automation_script",
        description = "
            Create and run small PowerShell or Batch scripts for automation tasks.
            PowerShell is recommended for most tasks.

            The script is saved to a temporary file and executed.
            Some examples:
            - Sort unique lines: Get-Content file.txt | Sort-Object -Unique
            - Extract CSV column: Import-Csv file.csv | Select-Object -ExpandProperty Column2
            - Find text: Select-String -Pattern 'pattern' -Path file.txt
        "
    )]
    pub async fn automation_script(
        &self,
        params: Parameters<AutomationScriptParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.automation_script_impl(params).await
    }

    /// Create and run small scripts for automation tasks
    #[cfg(target_os = "macos")]
    #[tool(
        name = "automation_script",
        description = "
            Create and run Shell, Ruby, or AppleScript (via osascript) scripts.
            Use shell (bash) for most tasks. AppleScript for app scripting and system settings.
            Examples:
                - sort file.txt | uniq
                - awk -F ',' '{ print $2}' file.csv
                - osascript -e 'tell app \"Finder\" to get name of every window'
        "
    )]
    pub async fn automation_script(
        &self,
        params: Parameters<AutomationScriptParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.automation_script_impl(params).await
    }

    /// Create and run small scripts for automation tasks
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    #[tool(
        name = "automation_script",
        description = "
            Create and run Shell scripts for automation tasks.
            Consider using shell script (bash) for most simple tasks first.
            Examples:
                - sort file.txt | uniq
                - awk -F ',' '{ print $2}' file.csv
                - grep pattern file.txt
        "
    )]
    pub async fn automation_script(
        &self,
        params: Parameters<AutomationScriptParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.automation_script_impl(params).await
    }

    #[allow(clippy::too_many_lines)]
    async fn automation_script_impl(
        &self,
        params: Parameters<AutomationScriptParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let params = params.0;
        let language = params.language;
        let script = &params.script;
        let save_output = params.save_output;

        // Opt-in exec policy gate (default off). The developer shell tool screens
        // its commands against `BHARATCODE_EXEC_POLICY`; this tool spawns a shell
        // too, so it must honour the same gate instead of silently bypassing it.
        if let Err(reason) = exec_policy_gate::check_command(script) {
            return Ok(CallToolResult::error(vec![Content::text(reason)]));
        }

        // Create a temporary directory for the script
        let script_dir = tempfile::tempdir().map_err(|e| {
            ErrorData::new(
                ErrorCode::INTERNAL_ERROR,
                format!("Failed to create temporary directory: {}", e),
                None,
            )
        })?;

        let (shell, shell_arg) = self.system_automation.get_shell_command();

        let command = match language {
            ScriptLanguage::Shell | ScriptLanguage::Batch => {
                let script_path = script_dir.path().join(format!(
                    "script.{}",
                    if cfg!(windows) { "bat" } else { "sh" }
                ));
                fs::write(&script_path, script).map_err(|e| {
                    ErrorData::new(
                        ErrorCode::INTERNAL_ERROR,
                        format!("Failed to write script: {}", e),
                        None,
                    )
                })?;

                // Set execute permissions on Unix systems
                #[cfg(unix)]
                {
                    let mut perms = fs::metadata(&script_path)
                        .map_err(|e| {
                            ErrorData::new(
                                ErrorCode::INTERNAL_ERROR,
                                format!("Failed to get file metadata: {}", e),
                                None,
                            )
                        })?
                        .permissions();
                    perms.set_mode(0o755); // rwxr-xr-x
                    fs::set_permissions(&script_path, perms).map_err(|e| {
                        ErrorData::new(
                            ErrorCode::INTERNAL_ERROR,
                            format!("Failed to set execute permissions: {}", e),
                            None,
                        )
                    })?;
                }

                script_path.display().to_string()
            }
            ScriptLanguage::Ruby => {
                let script_path = script_dir.path().join("script.rb");
                fs::write(&script_path, script).map_err(|e| {
                    ErrorData::new(
                        ErrorCode::INTERNAL_ERROR,
                        format!("Failed to write script: {}", e),
                        None,
                    )
                })?;

                format!("ruby {}", script_path.display())
            }
            ScriptLanguage::Powershell => {
                let script_path = script_dir.path().join("script.ps1");
                fs::write(&script_path, script).map_err(|e| {
                    ErrorData::new(
                        ErrorCode::INTERNAL_ERROR,
                        format!("Failed to write script: {}", e),
                        None,
                    )
                })?;

                script_path.display().to_string()
            }
        };

        // Run the script
        let output = match language {
            ScriptLanguage::Powershell => {
                // For PowerShell, we need to use -File instead of -Command
                Command::new("powershell")
                    .arg("-NoProfile")
                    .arg("-NonInteractive")
                    .arg("-File")
                    .arg(&command)
                    .env("BHARATCODE_TERMINAL", "1")
                    .env("AGENT", "bharatcode")
                    .set_no_window()
                    .output()
                    .await
                    .map_err(|e| {
                        ErrorData::new(
                            ErrorCode::INTERNAL_ERROR,
                            format!("Failed to run script: {}", e),
                            None,
                        )
                    })?
            }
            _ => {
                let mut cmd = Command::new(shell);
                cmd.arg(shell_arg)
                    .arg(&command)
                    .env("BHARATCODE_TERMINAL", "1")
                    .env("AGENT", "bharatcode");
                #[cfg(not(windows))]
                if let Some(path) = merged_path() {
                    cmd.env("PATH", path);
                }
                cmd.set_no_window().output().await.map_err(|e| {
                    ErrorData::new(
                        ErrorCode::INTERNAL_ERROR,
                        format!("Failed to run script: {}", e),
                        None,
                    )
                })?
            }
        };

        let output_str = String::from_utf8_lossy(&output.stdout).into_owned();
        let error_str = String::from_utf8_lossy(&output.stderr).into_owned();

        let mut result = if output.status.success() {
            format!("Script completed successfully.\n\nOutput:\n{}", output_str)
        } else {
            format!(
                "Script failed with error code {}.\n\nError:\n{}\nOutput:\n{}",
                output.status, error_str, output_str
            )
        };

        // Save output if requested
        if save_output && !output_str.is_empty() {
            let cache_path = self
                .save_to_cache(output_str.as_bytes(), "script_output", "txt")
                .await?;
            result.push_str(&format!("\n\nOutput saved to: {}", cache_path.display()));

            // Register as a resource
            self.register_as_resource(&cache_path, "text")?;
        }

        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    /// Control the computer using system automation
    #[cfg(target_os = "windows")]
    #[tool(
        name = "computer_control",
        description = "
            Control the computer using Windows system automation.

            Features available:
            - PowerShell automation for system control
            - UI automation through PowerShell
            - File and system management
            - Windows-specific features and settings

            Can be combined with screenshot tool for visual task assistance.
        "
    )]
    pub async fn computer_control(
        &self,
        params: Parameters<ComputerControlParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.computer_control_impl(params).await
    }

    /// Control the computer using Peekaboo CLI for macOS GUI automation.
    /// Auto-installs via Homebrew on first use.
    #[cfg(target_os = "macos")]
    #[tool(
        name = "computer_control",
        description = "
            macOS UI automation via Peekaboo CLI. Pass a subcommand string as `command`.

            Core workflow: see → click → type
            1. see --app Safari --annotate  (get annotated screenshot with element IDs)
            2. click --on B3               (click element by ID)
            3. type \"hello\" --return       (type text, press enter)

            Key commands: see, image, click, type, press, hotkey, paste, scroll, drag,
            swipe, move, app, window, list, menu, menubar, dock, dialog, clipboard,
            space, open, permissions.

            Targeting: --app Name, --window-title, --window-id, --on ID, --coords x,y
            Set capture_screenshot=true to verify UI state after actions.
            See extension instructions for full command reference and examples.
        "
    )]
    pub async fn computer_control(
        &self,
        params: Parameters<ComputerControlParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.peekaboo_impl(params).await
    }

    /// Control the computer using system automation
    #[cfg(target_os = "linux")]
    #[tool(
        name = "computer_control",
        description = "
            Control the computer using Linux system automation.

            Features available:
            - Shell scripting for system control
            - X11/Wayland window management
            - D-Bus for system services
            - File and system management
            - Desktop environment control (GNOME, KDE, etc.)
            - Process management and monitoring
            - System settings and configurations

            Can be combined with screenshot tool for visual task assistance.
        "
    )]
    pub async fn computer_control(
        &self,
        params: Parameters<ComputerControlParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.computer_control_impl(params).await
    }

    /// Control the computer using system automation (fallback for other OS)
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    #[tool(
        name = "computer_control",
        description = "Control the computer using system automation. Features available depend on your operating system. Can be combined with screenshot tool for visual task assistance."
    )]
    pub async fn computer_control(
        &self,
        params: Parameters<ComputerControlParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.computer_control_impl(params).await
    }

    #[cfg(not(target_os = "macos"))]
    async fn computer_control_impl(
        &self,
        params: Parameters<ComputerControlParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let params = params.0;
        let script = &params.script;
        let save_output = params.save_output;

        // Use platform-specific automation
        let output = self
            .system_automation
            .execute_system_script(script)
            .map_err(|e| {
                ErrorData::new(
                    ErrorCode::INTERNAL_ERROR,
                    format!("Failed to execute script: {}", e),
                    None,
                )
            })?;

        let mut result = format!("Script completed successfully.\n\nOutput:\n{}", output);

        // Save output if requested
        if save_output && !output.is_empty() {
            let cache_path = self
                .save_to_cache(output.as_bytes(), "automation_output", "txt")
                .await?;
            result.push_str(&format!("\n\nOutput saved to: {}", cache_path.display()));

            // Register as a resource
            self.register_as_resource(&cache_path, "text")?;
        }

        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    #[cfg(target_os = "macos")]
    fn ensure_peekaboo(&self) -> Result<(), ErrorData> {
        if self.peekaboo_installed.load(Ordering::Relaxed) {
            return Ok(());
        }
        if crate::peekaboo::is_peekaboo_installed() {
            self.peekaboo_installed.store(true, Ordering::Relaxed);
            return Ok(());
        }
        tracing::info!("Peekaboo not found, attempting auto-install via brew");
        match crate::peekaboo::auto_install_peekaboo() {
            Ok(()) => {
                self.peekaboo_installed.store(true, Ordering::Relaxed);
                tracing::info!("Peekaboo installed successfully");
                Ok(())
            }
            Err(msg) => Err(ErrorData::new(
                ErrorCode::INTERNAL_ERROR,
                format!(
                    "Peekaboo is not installed and auto-install failed: {}\n\
                     Install manually with: brew install steipete/tap/peekaboo\n\
                     Peekaboo requires macOS 15+ (Sequoia) with Screen Recording and Accessibility permissions.",
                    msg
                ),
                None,
            )),
        }
    }

    #[cfg(target_os = "macos")]
    fn run_peekaboo_cmd(&self, args: &[&str]) -> Result<String, ErrorData> {
        let mut cmd = std::process::Command::new("peekaboo");
        cmd.args(args);
        if let Some(path) = merged_path() {
            cmd.env("PATH", path);
        }
        let output = cmd.output().map_err(|e| {
            ErrorData::new(
                ErrorCode::INTERNAL_ERROR,
                format!("Failed to run peekaboo: {}", e),
                None,
            )
        })?;
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        if !output.status.success() {
            return Err(ErrorData::new(
                ErrorCode::INTERNAL_ERROR,
                format!(
                    "peekaboo {} failed (exit {}):\n{}\n{}",
                    args.first().unwrap_or(&""),
                    output.status,
                    stderr.trim(),
                    stdout.trim()
                ),
                None,
            ));
        }
        Ok(stdout)
    }

    #[cfg(target_os = "macos")]
    async fn peekaboo_impl(
        &self,
        params: Parameters<ComputerControlParams>,
    ) -> Result<CallToolResult, ErrorData> {
        self.ensure_peekaboo()?;
        let params = params.0;

        let args = shell_words::split(&params.command).map_err(|e| {
            ErrorData::new(
                ErrorCode::INVALID_PARAMS,
                format!("Failed to parse command: {}", e),
                None,
            )
        })?;
        if args.is_empty() {
            return Err(ErrorData::new(
                ErrorCode::INVALID_PARAMS,
                "Command cannot be empty".to_string(),
                None,
            ));
        }

        let is_see = args[0] == "see";
        let is_image = args[0] == "image";
        let screenshot_path = if is_see || is_image {
            Some(self.get_cache_path(&args[0], "png"))
        } else {
            None
        };

        let mut full_args: Vec<String> = args.clone();

        if let Some(ref path) = screenshot_path {
            if !full_args.iter().any(|a| a == "--path") {
                full_args.push("--path".to_string());
                full_args.push(path.to_string_lossy().to_string());
            }
        }
        if is_see && !full_args.iter().any(|a| a == "--json-output") {
            full_args.push("--json-output".to_string());
        }

        let wants_json = matches!(
            args[0].as_str(),
            "list" | "window" | "menubar" | "permissions" | "clipboard"
        );
        if wants_json
            && !full_args.iter().any(|a| a == "--json" || a == "-j")
            && !full_args.iter().any(|a| a == "--json-output")
        {
            full_args.push("--json".to_string());
        }

        let arg_refs: Vec<&str> = full_args.iter().map(|s| s.as_str()).collect();
        let stdout = self.run_peekaboo_cmd(&arg_refs)?;

        let mut contents = Vec::new();

        if let Some(ref path) = screenshot_path {
            let annotated = path.to_string_lossy().replace(".png", "_annotated.png");
            let image_path = if is_see && std::path::Path::new(&annotated).exists() {
                PathBuf::from(&annotated)
            } else {
                path.clone()
            };
            if image_path.exists() {
                if let Ok(bytes) = fs::read(&image_path) {
                    let data = base64::prelude::BASE64_STANDARD.encode(&bytes);
                    contents.push(Content::image(data, "image/png").with_priority(0.0));
                }
            }
        }

        if params.capture_screenshot && screenshot_path.is_none() {
            let cap_path = self.get_cache_path("peekaboo_capture", "png");
            let cap_path_str = cap_path.to_string_lossy().to_string();
            if self
                .run_peekaboo_cmd(&["image", "--mode", "frontmost", "--path", &cap_path_str])
                .is_ok()
                && cap_path.exists()
            {
                if let Ok(bytes) = fs::read(&cap_path) {
                    let data = base64::prelude::BASE64_STANDARD.encode(&bytes);
                    contents.push(Content::image(data, "image/png").with_priority(0.0));
                }
            }
        }

        let text = if stdout.len() > 12000 {
            let truncated: String = stdout.chars().take(12000).collect();
            format!(
                "{}\n\n[Output truncated. {} total chars.]",
                truncated,
                stdout.len()
            )
        } else {
            stdout
        };

        contents.insert(0, Content::text(&text).with_audience(vec![Role::Assistant]));

        Ok(CallToolResult::success(contents))
    }

    /// Process Excel (XLSX) files to read and manipulate spreadsheet data
    #[tool(
        name = "xlsx_tool",
        description = "
            Process Excel (XLSX) files to read and manipulate spreadsheet data.
            Supports operations:
            - list_worksheets: List all worksheets in the workbook (returns name, index, column_count, row_count)
            - get_columns: Get column names from a worksheet (returns values from the first row)
            - get_range: Get values and formulas from a cell range (e.g., 'A1:C10') (returns a 2D array organized as [row][column])
            - find_text: Search for text in a worksheet (returns a list of (row, column) coordinates)
            - update_cell: Update a single cell's value (returns confirmation message)
            - get_cell: Get value and formula from a specific cell (returns both value and formula if present)
            - save: Save changes back to the file (returns confirmation message)

            Use this when working with Excel spreadsheets to analyze or modify data.
        "
    )]
    pub async fn xlsx_tool(
        &self,
        params: Parameters<XlsxToolParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let params = params.0;
        let path = &params.path;
        let operation = params.operation;

        match operation {
            XlsxOperation::ListWorksheets => {
                let xlsx = xlsx_tool::XlsxTool::new(path)
                    .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?;
                let worksheets = xlsx
                    .list_worksheets()
                    .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?;
                Ok(CallToolResult::success(vec![Content::text(format!(
                    "{:#?}",
                    worksheets
                ))]))
            }
            XlsxOperation::GetColumns => {
                let xlsx = xlsx_tool::XlsxTool::new(path)
                    .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?;
                let worksheet = if let Some(name) = &params.worksheet {
                    xlsx.get_worksheet_by_name(name).map_err(|e| {
                        ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None)
                    })?
                } else {
                    xlsx.get_worksheet_by_index(0).map_err(|e| {
                        ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None)
                    })?
                };
                let columns = xlsx
                    .get_column_names(worksheet)
                    .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?;
                Ok(CallToolResult::success(vec![Content::text(format!(
                    "{:#?}",
                    columns
                ))]))
            }
            XlsxOperation::GetRange => {
                let range = params.range.as_ref().ok_or_else(|| {
                    ErrorData::new(
                        ErrorCode::INVALID_PARAMS,
                        "Missing 'range' parameter".to_string(),
                        None,
                    )
                })?;

                let xlsx = xlsx_tool::XlsxTool::new(path)
                    .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?;
                let worksheet = if let Some(name) = &params.worksheet {
                    xlsx.get_worksheet_by_name(name).map_err(|e| {
                        ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None)
                    })?
                } else {
                    xlsx.get_worksheet_by_index(0).map_err(|e| {
                        ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None)
                    })?
                };
                let range_data = xlsx
                    .get_range(worksheet, range)
                    .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?;
                Ok(CallToolResult::success(vec![Content::text(format!(
                    "{:#?}",
                    range_data
                ))]))
            }
            XlsxOperation::FindText => {
                let search_text = params.search_text.as_ref().ok_or_else(|| {
                    ErrorData::new(
                        ErrorCode::INVALID_PARAMS,
                        "Missing 'search_text' parameter".to_string(),
                        None,
                    )
                })?;

                let case_sensitive = params.case_sensitive;

                let xlsx = xlsx_tool::XlsxTool::new(path)
                    .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?;
                let worksheet = if let Some(name) = &params.worksheet {
                    xlsx.get_worksheet_by_name(name).map_err(|e| {
                        ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None)
                    })?
                } else {
                    xlsx.get_worksheet_by_index(0).map_err(|e| {
                        ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None)
                    })?
                };
                let matches = xlsx
                    .find_in_worksheet(worksheet, search_text, case_sensitive)
                    .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?;
                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Found matches at: {:#?}",
                    matches
                ))]))
            }
            XlsxOperation::UpdateCell => {
                let row = params.row.ok_or_else(|| {
                    ErrorData::new(
                        ErrorCode::INVALID_PARAMS,
                        "Missing 'row' parameter".to_string(),
                        None,
                    )
                })?;
                let col = params.col.ok_or_else(|| {
                    ErrorData::new(
                        ErrorCode::INVALID_PARAMS,
                        "Missing 'col' parameter".to_string(),
                        None,
                    )
                })?;
                let value = params.value.as_ref().ok_or_else(|| {
                    ErrorData::new(
                        ErrorCode::INVALID_PARAMS,
                        "Missing 'value' parameter".to_string(),
                        None,
                    )
                })?;

                let worksheet_name = params.worksheet.as_deref().unwrap_or("Sheet1");

                let mut xlsx = xlsx_tool::XlsxTool::new(path)
                    .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?;
                xlsx.update_cell(worksheet_name, row as u32, col as u32, value)
                    .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?;
                xlsx.save(path)
                    .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?;
                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Updated cell ({}, {}) to '{}' in worksheet '{}'",
                    row, col, value, worksheet_name
                ))]))
            }
            XlsxOperation::Save => {
                let xlsx = xlsx_tool::XlsxTool::new(path)
                    .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?;
                xlsx.save(path)
                    .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?;
                Ok(CallToolResult::success(vec![Content::text(
                    "File saved successfully.",
                )]))
            }
            XlsxOperation::GetCell => {
                let row = params.row.ok_or_else(|| {
                    ErrorData::new(
                        ErrorCode::INVALID_PARAMS,
                        "Missing 'row' parameter".to_string(),
                        None,
                    )
                })?;

                let col = params.col.ok_or_else(|| {
                    ErrorData::new(
                        ErrorCode::INVALID_PARAMS,
                        "Missing 'col' parameter".to_string(),
                        None,
                    )
                })?;

                let xlsx = xlsx_tool::XlsxTool::new(path)
                    .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?;
                let worksheet = if let Some(name) = &params.worksheet {
                    xlsx.get_worksheet_by_name(name).map_err(|e| {
                        ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None)
                    })?
                } else {
                    xlsx.get_worksheet_by_index(0).map_err(|e| {
                        ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None)
                    })?
                };
                let cell_value = xlsx
                    .get_cell_value(worksheet, row as u32, col as u32)
                    .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?;
                Ok(CallToolResult::success(vec![Content::text(format!(
                    "{:#?}",
                    cell_value
                ))]))
            }
        }
    }

    /// Process DOCX files to extract text and create/update documents
    #[tool(
        name = "docx_tool",
        description = "
            Process DOCX files to extract text and create/update documents.
            Supports operations:
            - extract_text: Extract all text content and structure (headings, TOC) from the DOCX
            - update_doc: Create a new DOCX or update existing one with provided content
              Modes:
              - append: Add content to end of document (default)
              - replace: Replace specific text with new content
              - structured: Add content with specific heading level and styling
              - add_image: Add an image to the document (with optional caption)

            Use this when there is a .docx file that needs to be processed or created.
        "
    )]
    pub async fn docx_tool(
        &self,
        params: Parameters<DocxToolParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let params = params.0;
        let path = &params.path;
        let operation = params.operation;

        // Convert enum to string for the existing implementation
        let operation_str = match operation {
            DocxOperation::ExtractText => "extract_text",
            DocxOperation::UpdateDoc => "update_doc",
        };

        // Convert typed params back to JSON for the internal docx_tool impl
        let json_params = params
            .params
            .as_ref()
            .map(|p| serde_json::to_value(p).unwrap_or(serde_json::Value::Null));

        let result = crate::computercontroller::docx_tool::docx_tool(
            path,
            operation_str,
            params.content.as_deref(),
            json_params.as_ref(),
        )
        .await
        .map_err(|e| ErrorData::new(e.code, e.message, e.data))?;

        Ok(CallToolResult::success(result))
    }

    /// Process PDF files to extract text and images
    #[tool(
        name = "pdf_tool",
        description = "
            Process PDF files to extract text and images.
            Supports operations:
            - extract_text: Extract all text content from the PDF
            - extract_images: Extract and save embedded images to PNG files

            Use this when there is a .pdf file or files that need to be processed.
        "
    )]
    pub async fn pdf_tool(
        &self,
        params: Parameters<PdfToolParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let params = params.0;
        let path = &params.path;
        let operation = params.operation;

        // Convert enum to string for the existing implementation
        let operation_str = match operation {
            PdfOperation::ExtractText => "extract_text",
            PdfOperation::ExtractImages => "extract_images",
        };

        let result =
            crate::computercontroller::pdf_tool::pdf_tool(path, operation_str, &self.cache_dir)
                .await
                .map_err(|e| ErrorData::new(e.code, e.message, e.data))?;

        Ok(CallToolResult::success(result))
    }

    /// Manage cached files and data
    #[tool(
        name = "cache",
        description = "
            Manage cached files and data:
            - list: List all cached files
            - view: View content of a cached file, by the entry name reported by list
            - delete: Delete a cached file, by the entry name reported by list
            - clear: Clear all cached files
            Only entries inside the cache directory can be viewed or deleted.
        "
    )]
    pub async fn cache(
        &self,
        params: Parameters<CacheParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let command = params.0.command;
        let path = params.0.path.as_deref();

        match command {
            CacheCommand::List => {
                let entries = self.list_cache_entries()?;
                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Cached files:\n{}",
                    entries.join("\n")
                ))]))
            }
            CacheCommand::View => {
                let path = path.ok_or_else(|| {
                    ErrorData::new(
                        ErrorCode::INVALID_PARAMS,
                        "Missing 'path' parameter for view".to_string(),
                        None,
                    )
                })?;
                let entry = self.resolve_cache_entry(path)?;

                let content = fs::read_to_string(&entry).map_err(|e| {
                    ErrorData::new(
                        ErrorCode::INTERNAL_ERROR,
                        format!("Failed to read file: {}", e),
                        None,
                    )
                })?;

                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Content of {}:\n\n{}",
                    path, content
                ))]))
            }
            CacheCommand::Delete => {
                let path = path.ok_or_else(|| {
                    ErrorData::new(
                        ErrorCode::INVALID_PARAMS,
                        "Missing 'path' parameter for delete".to_string(),
                        None,
                    )
                })?;
                let entry = self.resolve_cache_entry(path)?;

                fs::remove_file(&entry).map_err(|e| {
                    ErrorData::new(
                        ErrorCode::INTERNAL_ERROR,
                        format!("Failed to delete file: {}", e),
                        None,
                    )
                })?;

                // Remove from active resources if present
                if let Ok(url) = Url::from_file_path(&entry) {
                    self.active_resources
                        .lock()
                        .unwrap()
                        .remove(&url.to_string());
                }

                Ok(CallToolResult::success(vec![Content::text(format!(
                    "Deleted file: {}",
                    path
                ))]))
            }
            CacheCommand::Clear => {
                fs::remove_dir_all(&self.cache_dir).map_err(|e| {
                    ErrorData::new(
                        ErrorCode::INTERNAL_ERROR,
                        format!("Failed to clear cache directory: {}", e),
                        None,
                    )
                })?;
                fs::create_dir_all(&self.cache_dir).map_err(|e| {
                    ErrorData::new(
                        ErrorCode::INTERNAL_ERROR,
                        format!("Failed to recreate cache directory: {}", e),
                        None,
                    )
                })?;

                // Clear active resources
                self.active_resources.lock().unwrap().clear();

                Ok(CallToolResult::success(vec![Content::text(
                    "Cache cleared successfully.",
                )]))
            }
        }
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for ComputerControllerServer {
    fn get_info(&self) -> ServerInfo {
        InitializeResult::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_server_info(Implementation::new(
            "bharatcode-computercontroller",
            env!("CARGO_PKG_VERSION"),
        ))
        .with_instructions(self.instructions.clone())
    }

    async fn list_resources(
        &self,
        _pagination: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        let active_resources = self.active_resources.lock().unwrap();
        let resources: Vec<Resource> = active_resources
            .keys()
            .map(|uri| {
                RawResource::new(
                    uri.clone(),
                    uri.split('/').next_back().unwrap_or("").to_string(),
                )
                .no_annotation()
            })
            .collect();
        Ok(ListResourcesResult {
            resources,
            next_cursor: None,
            meta: None,
        })
    }

    async fn read_resource(
        &self,
        params: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, ErrorData> {
        let active_resources = self.active_resources.lock().unwrap();
        let resource = active_resources.get(&params.uri).ok_or_else(|| {
            ErrorData::new(
                ErrorCode::INVALID_REQUEST,
                format!("Resource not found: {}", params.uri),
                None,
            )
        })?;

        // Clone the resource to return
        Ok(ReadResourceResult::new(vec![resource.clone()]))
    }
}

/// Self-contained mirror of `bharatcode_core::exec_policy` for the computercontroller's
/// shell / PowerShell spawn.
///
/// `goose` depends on `goose-mcp`, so this crate cannot call
/// `bharatcode_core::exec_policy::check_command` without creating a dependency cycle.
/// Rather than let `automation_script` bypass the opt-in
/// `BHARATCODE_EXEC_POLICY` gate that the developer shell tool already enforces,
/// the same allow/deny screening is reimplemented here against the same
/// environment variable and JSON policy format, so one policy file governs both
/// execution paths. Keep this in sync with `crates/bharatcode-core/src/exec_policy.rs`.
mod exec_policy_gate {
    use serde::Deserialize;
    use std::path::{Path, PathBuf};

    const ENV_VAR: &str = "BHARATCODE_EXEC_POLICY";

    #[derive(Debug, Default, Deserialize)]
    struct ExecPolicy {
        #[serde(default)]
        allow: Vec<String>,
        #[serde(default)]
        deny: Vec<String>,
    }

    /// Screen `command_line` against the active policy, returning `Err(reason)`
    /// when it must be blocked. Returns `Ok(())` when the policy is disabled
    /// (the default); when enabled but unreadable, the command is denied so an
    /// opted-in restriction never fails open.
    pub fn check_command(command_line: &str) -> Result<(), String> {
        let Some(path) = policy_path() else {
            return Ok(());
        };
        let policy = load_policy(&path).map_err(|error| {
            format!(
                "Command blocked: exec policy is enabled ({ENV_VAR}) but could not be loaded: {error}"
            )
        })?;
        check(&policy, command_line)
    }

    fn policy_path() -> Option<PathBuf> {
        let value = std::env::var(ENV_VAR).ok()?;
        let trimmed = value.trim();
        if trimmed.is_empty()
            || trimmed.eq_ignore_ascii_case("off")
            || trimmed.eq_ignore_ascii_case("false")
            || trimmed == "0"
        {
            return None;
        }
        Some(PathBuf::from(trimmed))
    }

    fn load_policy(path: &Path) -> Result<ExecPolicy, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("could not read {}: {error}", path.display()))?;
        serde_json::from_str(&text).map_err(|error| format!("invalid exec policy JSON: {error}"))
    }

    fn check(policy: &ExecPolicy, command_line: &str) -> Result<(), String> {
        for segment in split_segments(command_line) {
            let tokens = tokenize(&segment);
            if tokens.is_empty() {
                continue;
            }
            if let Some(prefix) = matched_prefix(&policy.deny, &tokens) {
                return Err(format!(
                    "Command blocked by exec policy ({ENV_VAR}): `{}` matches denied prefix `{}`.",
                    segment.trim(),
                    prefix
                ));
            }
            if !policy.allow.is_empty() && matched_prefix(&policy.allow, &tokens).is_none() {
                return Err(format!(
                    "Command blocked by exec policy ({ENV_VAR}): `{}` is not in the allowed-command list.",
                    segment.trim()
                ));
            }
        }
        Ok(())
    }

    fn tokenize(segment: &str) -> Vec<String> {
        segment.split_whitespace().map(str::to_string).collect()
    }

    fn matched_prefix(prefixes: &[String], tokens: &[String]) -> Option<String> {
        for prefix in prefixes {
            let prefix_tokens = tokenize(prefix);
            if prefix_tokens.is_empty() || prefix_tokens.len() > tokens.len() {
                continue;
            }
            if tokens[..prefix_tokens.len()] == prefix_tokens[..] {
                return Some(prefix_tokens.join(" "));
            }
        }
        None
    }

    fn split_segments(command_line: &str) -> Vec<String> {
        let chars: Vec<char> = command_line.chars().collect();
        let mut segments = Vec::new();
        let mut current = String::new();
        let mut in_single = false;
        let mut in_double = false;
        let mut i = 0;

        while i < chars.len() {
            let c = chars[i];

            if in_single {
                if c == '\'' {
                    in_single = false;
                }
                current.push(c);
                i += 1;
                continue;
            }
            if in_double {
                if c == '"' {
                    in_double = false;
                }
                current.push(c);
                i += 1;
                continue;
            }

            match c {
                '\'' => {
                    in_single = true;
                    current.push(c);
                    i += 1;
                }
                '"' => {
                    in_double = true;
                    current.push(c);
                    i += 1;
                }
                ';' | '\n' => {
                    segments.push(std::mem::take(&mut current));
                    i += 1;
                }
                '&' => {
                    segments.push(std::mem::take(&mut current));
                    i += if i + 1 < chars.len() && chars[i + 1] == '&' {
                        2
                    } else {
                        1
                    };
                }
                '|' => {
                    segments.push(std::mem::take(&mut current));
                    i += if i + 1 < chars.len() && chars[i + 1] == '|' {
                        2
                    } else {
                        1
                    };
                }
                // Command substitution and subshell/group boundaries, so a denied
                // command inside `$(...)`, backticks, `(...)` or `{ ...; }` is not
                // hidden from screening. `${...}` parameter expansion (the
                // `$`-guarded `{`) is left intact.
                '`' | '(' | ')' => {
                    segments.push(std::mem::take(&mut current));
                    i += 1;
                }
                '{' if !current.ends_with('$') => {
                    segments.push(std::mem::take(&mut current));
                    i += 1;
                }
                _ => {
                    current.push(c);
                    i += 1;
                }
            }
        }

        segments.push(current);
        segments
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn policy(allow: &[&str], deny: &[&str]) -> ExecPolicy {
            ExecPolicy {
                allow: allow.iter().map(|s| s.to_string()).collect(),
                deny: deny.iter().map(|s| s.to_string()).collect(),
            }
        }

        #[test]
        fn deny_blocks_matching_prefix() {
            let p = policy(&[], &["rm -rf"]);
            assert!(check(&p, "rm -rf /tmp/x").is_err());
            assert!(check(&p, "ls -la").is_ok());
        }

        #[test]
        fn denied_command_inside_substitution_or_subshell_is_caught() {
            let p = policy(&[], &["rm -rf"]);
            assert!(check(&p, "echo $(rm -rf x)").is_err());
            assert!(check(&p, "echo `rm -rf x`").is_err());
            assert!(check(&p, "(cd /tmp && rm -rf x)").is_err());
            assert!(check(&p, "{ rm -rf x; }").is_err());
        }

        #[test]
        fn parameter_expansion_is_not_fragmented() {
            let p = policy(&["echo"], &[]);
            assert!(check(&p, "echo ${HOME}/bin").is_ok());
        }

        #[test]
        fn allow_list_still_screens_substituted_command() {
            let p = policy(&["echo"], &[]);
            assert!(check(&p, "echo $(curl evil.test)").is_err());
        }
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;
    use tempfile::TempDir;

    const ENTRY: &str = "web_20240101_000000.txt";

    struct Fixture {
        server: ComputerControllerServer,
        cache_dir: PathBuf,
        outside_secret: PathBuf,
        sibling_secret: PathBuf,
        _root: TempDir,
    }

    fn fixture() -> Fixture {
        let root = tempfile::tempdir().unwrap();
        let cache_dir = root.path().join("cache");
        fs::create_dir_all(&cache_dir).unwrap();
        fs::write(cache_dir.join(ENTRY), "cached page").unwrap();

        let outside_secret = root.path().join("secret.txt");
        fs::write(&outside_secret, "top secret").unwrap();

        let sibling_dir = root.path().join("sibling");
        fs::create_dir_all(&sibling_dir).unwrap();
        let sibling_secret = sibling_dir.join("notes.txt");
        fs::write(&sibling_secret, "sibling secret").unwrap();

        let mut server = ComputerControllerServer::new();
        server.cache_dir = cache_dir.clone();

        Fixture {
            server,
            cache_dir,
            outside_secret,
            sibling_secret,
            _root: root,
        }
    }

    async fn run(
        server: &ComputerControllerServer,
        command: CacheCommand,
        path: Option<&str>,
    ) -> Result<CallToolResult, ErrorData> {
        server
            .cache(Parameters(CacheParams {
                command,
                path: path.map(str::to_string),
            }))
            .await
    }

    fn text(result: &CallToolResult) -> String {
        result.content[0].as_text().unwrap().text.clone()
    }

    fn assert_rejected(result: Result<CallToolResult, ErrorData>) {
        let error = result.expect_err("cache access outside the cache directory must be rejected");
        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    }

    #[tokio::test]
    async fn list_reports_entry_names_usable_for_view() {
        let f = fixture();

        let listed = text(&run(&f.server, CacheCommand::List, None).await.unwrap());
        assert!(listed.contains(ENTRY));
        assert!(!listed.contains(&f.cache_dir.display().to_string()));

        let viewed = text(
            &run(&f.server, CacheCommand::View, Some(ENTRY))
                .await
                .unwrap(),
        );
        assert!(viewed.contains("cached page"));
    }

    #[tokio::test]
    async fn view_accepts_full_path_inside_cache_dir() {
        let f = fixture();
        let full_path = f.cache_dir.join(ENTRY);

        let viewed = text(
            &run(
                &f.server,
                CacheCommand::View,
                Some(full_path.to_str().unwrap()),
            )
            .await
            .unwrap(),
        );
        assert!(viewed.contains("cached page"));
    }

    #[tokio::test]
    async fn view_accepts_nested_entry() {
        let f = fixture();
        let nested_dir = f.cache_dir.join("pdf_images");
        fs::create_dir_all(&nested_dir).unwrap();
        fs::write(nested_dir.join("page_1.txt"), "extracted").unwrap();

        let listed = text(&run(&f.server, CacheCommand::List, None).await.unwrap());
        assert!(listed.contains("page_1.txt"));

        let nested_id = format!("pdf_images{}page_1.txt", std::path::MAIN_SEPARATOR);
        let viewed = text(
            &run(&f.server, CacheCommand::View, Some(nested_id.as_str()))
                .await
                .unwrap(),
        );
        assert!(viewed.contains("extracted"));
    }

    #[tokio::test]
    async fn view_rejects_absolute_path_outside_cache() {
        let f = fixture();

        assert_rejected(
            run(
                &f.server,
                CacheCommand::View,
                Some(f.outside_secret.to_str().unwrap()),
            )
            .await,
        );
        assert_rejected(run(&f.server, CacheCommand::View, Some("/etc/passwd")).await);
        assert!(f.outside_secret.exists());
    }

    #[tokio::test]
    async fn view_rejects_traversal() {
        let f = fixture();
        let escaping = f.cache_dir.join("..").join("secret.txt");

        assert_rejected(run(&f.server, CacheCommand::View, Some("../secret.txt")).await);
        assert_rejected(run(&f.server, CacheCommand::View, Some("../../etc/passwd")).await);
        assert_rejected(
            run(
                &f.server,
                CacheCommand::View,
                Some(escaping.to_str().unwrap()),
            )
            .await,
        );
        assert!(f.outside_secret.exists());
    }

    #[tokio::test]
    async fn view_rejects_sibling_directory_entry() {
        let f = fixture();

        assert_rejected(
            run(
                &f.server,
                CacheCommand::View,
                Some(f.sibling_secret.to_str().unwrap()),
            )
            .await,
        );
        assert_rejected(run(&f.server, CacheCommand::View, Some("../sibling/notes.txt")).await);
        assert!(f.sibling_secret.exists());
    }

    #[tokio::test]
    async fn view_rejects_non_regular_entry() {
        let f = fixture();
        fs::create_dir_all(f.cache_dir.join("subdir")).unwrap();

        assert_rejected(run(&f.server, CacheCommand::View, Some("subdir")).await);
        assert_rejected(run(&f.server, CacheCommand::Delete, Some("subdir")).await);
        assert!(f.cache_dir.join("subdir").is_dir());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn symlinked_entry_is_rejected_and_unlisted() {
        let f = fixture();
        std::os::unix::fs::symlink(&f.outside_secret, f.cache_dir.join("link.txt")).unwrap();

        let listed = text(&run(&f.server, CacheCommand::List, None).await.unwrap());
        assert!(!listed.contains("link.txt"));

        assert_rejected(run(&f.server, CacheCommand::View, Some("link.txt")).await);
        assert_rejected(run(&f.server, CacheCommand::Delete, Some("link.txt")).await);
        assert!(f.outside_secret.exists());
        assert!(f.cache_dir.join("link.txt").symlink_metadata().is_ok());
    }

    #[tokio::test]
    async fn delete_removes_listed_entry_only() {
        let f = fixture();

        run(&f.server, CacheCommand::Delete, Some(ENTRY))
            .await
            .unwrap();
        assert!(!f.cache_dir.join(ENTRY).exists());

        let listed = text(&run(&f.server, CacheCommand::List, None).await.unwrap());
        assert!(!listed.contains(ENTRY));
        assert_rejected(run(&f.server, CacheCommand::View, Some(ENTRY)).await);
    }

    #[tokio::test]
    async fn delete_rejects_targets_outside_cache() {
        let f = fixture();

        assert_rejected(
            run(
                &f.server,
                CacheCommand::Delete,
                Some(f.outside_secret.to_str().unwrap()),
            )
            .await,
        );
        assert_rejected(run(&f.server, CacheCommand::Delete, Some("../secret.txt")).await);
        assert_rejected(
            run(
                &f.server,
                CacheCommand::Delete,
                Some(f.sibling_secret.to_str().unwrap()),
            )
            .await,
        );

        assert!(f.outside_secret.exists());
        assert!(f.sibling_secret.exists());
    }
}

#[cfg(test)]
mod web_scrape_tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;
    use tempfile::TempDir;

    enum Behavior {
        Body(&'static str),
        Chunked(usize),
        DeclaredLength(u64),
        RedirectTo(String),
        SelfRedirect,
        Stall,
    }

    /// A loopback listener that counts connections, so a test can prove a blocked target was
    /// never even reached rather than merely that the call returned an error.
    struct TestServer {
        addr: SocketAddr,
        hits: Arc<AtomicUsize>,
    }

    impl TestServer {
        fn spawn(behavior: Behavior) -> Self {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            let addr = listener.local_addr().unwrap();
            let hits = Arc::new(AtomicUsize::new(0));
            let counter = Arc::clone(&hits);

            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(mut stream) = stream else { break };
                    counter.fetch_add(1, Ordering::SeqCst);
                    let _ = stream.read(&mut [0u8; 2048]);

                    match &behavior {
                        Behavior::Body(body) => {
                            let _ = write!(
                                stream,
                                "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",
                                body.len(),
                                body
                            );
                        }
                        Behavior::Chunked(total) => {
                            let _ = stream.write_all(
                                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n",
                            );
                            let chunk = [b'a'; 256];
                            let mut sent = 0;
                            while sent < *total {
                                if write!(stream, "{:x}\r\n", chunk.len()).is_err()
                                    || stream.write_all(&chunk).is_err()
                                    || stream.write_all(b"\r\n").is_err()
                                {
                                    break;
                                }
                                sent += chunk.len();
                            }
                            let _ = stream.write_all(b"0\r\n\r\n");
                        }
                        Behavior::DeclaredLength(len) => {
                            let _ = write!(
                                stream,
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                                len
                            );
                        }
                        Behavior::RedirectTo(location) => {
                            let _ = write!(
                                stream,
                                "HTTP/1.1 302 Found\r\nLocation: {}\r\nContent-Length: 0\r\n\r\n",
                                location
                            );
                        }
                        Behavior::SelfRedirect => {
                            let _ = stream.write_all(
                                b"HTTP/1.1 302 Found\r\nLocation: /next\r\nContent-Length: 0\r\n\r\n",
                            );
                        }
                        Behavior::Stall => {
                            let _ =
                                stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n");
                            let _ = stream.flush();
                            std::thread::sleep(Duration::from_secs(30));
                        }
                    }
                    let _ = stream.flush();
                }
            });

            Self { addr, hits }
        }

        fn url(&self) -> String {
            format!("http://{}/", self.addr)
        }

        fn hits(&self) -> usize {
            self.hits.load(Ordering::SeqCst)
        }

        /// A policy that reaches this server but keeps every other destination guarded.
        fn policy(&self) -> FetchPolicy {
            FetchPolicy {
                max_bytes: 1024,
                max_redirects: 3,
                connect_timeout: Duration::from_secs(2),
                read_timeout: Duration::from_millis(300),
                request_timeout: Duration::from_secs(5),
                total_timeout: Duration::from_secs(10),
                dns_timeout: Duration::from_secs(2),
                bypass_proxy: true,
                exempt_addr: Some(self.addr),
            }
        }
    }

    fn server_with_cache() -> (ComputerControllerServer, TempDir) {
        let root = tempfile::tempdir().unwrap();
        let mut server = ComputerControllerServer::new();
        server.cache_dir = root.path().to_path_buf();
        (server, root)
    }

    async fn scrape(url: &str) -> Result<CallToolResult, ErrorData> {
        let (server, _root) = server_with_cache();
        server
            .web_scrape(Parameters(WebScrapeParams {
                url: url.to_string(),
                save_as: SaveAsFormat::Text,
            }))
            .await
    }

    fn assert_rejected(result: Result<CallToolResult, ErrorData>, expected: &str) {
        let error = result.expect_err("web_scrape should have refused this target");
        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
        let message = error.message.to_lowercase();
        assert!(
            message.contains(expected),
            "expected {:?} to mention {:?}",
            message,
            expected
        );
    }

    #[test]
    fn non_public_addresses_are_blocked() {
        for ip in [
            "127.0.0.1",
            "0.0.0.0",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.100.100.200",
            "224.0.0.1",
            "::1",
            "::",
            "fd00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
        ] {
            assert!(
                is_blocked_ip(ip.parse().unwrap()),
                "{} should be treated as non-public",
                ip
            );
        }

        for ip in ["1.1.1.1", "93.184.216.34", "2606:4700:4700::1111"] {
            assert!(
                !is_blocked_ip(ip.parse().unwrap()),
                "{} should be allowed",
                ip
            );
        }
    }

    #[tokio::test]
    async fn rejects_non_http_schemes() {
        assert_rejected(scrape("file:///etc/passwd").await, "http and https");
        assert_rejected(scrape("ftp://example.com/secrets").await, "http and https");
        assert_rejected(scrape("javascript:alert(1)").await, "http and https");
    }

    #[tokio::test]
    async fn rejects_embedded_credentials() {
        assert_rejected(scrape("http://user:pass@example.com/").await, "credentials");
    }

    #[tokio::test]
    async fn rejects_loopback_without_connecting() {
        let server = TestServer::spawn(Behavior::Body("internal secret"));

        assert_rejected(scrape(&server.url()).await, "loopback");
        assert_eq!(
            server.hits(),
            0,
            "the loopback service must never be dialled"
        );
    }

    #[tokio::test]
    async fn rejects_metadata_and_private_literals() {
        for url in [
            "http://169.254.169.254/latest/meta-data/",
            "http://[fd00::1]/",
            "http://10.0.0.5/admin",
            "http://192.168.1.1/",
            "http://[::1]:8080/",
        ] {
            assert_rejected(scrape(url).await, "non-public");
        }
    }

    #[tokio::test]
    async fn unresolvable_host_fails_closed() {
        let policy = FetchPolicy {
            dns_timeout: Duration::ZERO,
            bypass_proxy: true,
            ..FetchPolicy::default()
        };

        let error = fetch_bounded("http://example.com/", &policy)
            .await
            .expect_err("a name we could not resolve must not be fetched");
        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
        assert!(error.message.contains("DNS"), "{}", error.message);
    }

    #[tokio::test]
    async fn rejects_redirect_pivot_to_loopback() {
        let internal = TestServer::spawn(Behavior::Body("internal secret"));
        let entry = TestServer::spawn(Behavior::RedirectTo(internal.url()));

        let error = fetch_bounded(&entry.url(), &entry.policy())
            .await
            .expect_err("a redirect into loopback must be refused");
        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
        assert_eq!(entry.hits(), 1);
        assert_eq!(
            internal.hits(),
            0,
            "the redirect target must never be dialled"
        );
    }

    #[tokio::test]
    async fn rejects_redirect_pivot_to_metadata_and_file() {
        for location in [
            "http://169.254.169.254/latest/meta-data/",
            "file:///etc/passwd",
        ] {
            let entry = TestServer::spawn(Behavior::RedirectTo(location.to_string()));

            let error = fetch_bounded(&entry.url(), &entry.policy())
                .await
                .expect_err("redirect target must be revalidated");
            assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
        }
    }

    #[tokio::test]
    async fn caps_redirect_chains() {
        let entry = TestServer::spawn(Behavior::SelfRedirect);
        let policy = entry.policy();

        let error = fetch_bounded(&entry.url(), &policy)
            .await
            .expect_err("an endless redirect chain must terminate");
        assert!(error.message.contains("redirects"), "{}", error.message);
        assert_eq!(entry.hits(), policy.max_redirects + 1);
    }

    #[tokio::test]
    async fn rejects_streamed_body_over_limit() {
        let server = TestServer::spawn(Behavior::Chunked(64 * 1024));

        let error = fetch_bounded(&server.url(), &server.policy())
            .await
            .expect_err("an oversized body must not be buffered");
        assert!(error.message.contains("byte limit"), "{}", error.message);
    }

    #[tokio::test]
    async fn rejects_declared_body_over_limit() {
        let server = TestServer::spawn(Behavior::DeclaredLength(1_000_000));

        let error = fetch_bounded(&server.url(), &server.policy())
            .await
            .expect_err("an oversized content-length must be refused up front");
        assert!(error.message.contains("byte limit"), "{}", error.message);
    }

    #[tokio::test]
    async fn stalled_response_times_out() {
        let server = TestServer::spawn(Behavior::Stall);
        let policy = server.policy();

        let started = Instant::now();
        fetch_bounded(&server.url(), &policy)
            .await
            .expect_err("a stalled body must not hang the fetch");
        assert!(
            started.elapsed() < policy.request_timeout,
            "the read timeout should fire well before the request timeout"
        );
    }

    #[tokio::test]
    async fn fetches_an_ordinary_page() {
        let server = TestServer::spawn(Behavior::Body("hello world"));

        let body = fetch_bounded(&server.url(), &server.policy())
            .await
            .unwrap();
        assert_eq!(body, b"hello world");
        assert_eq!(server.hits(), 1);
    }
}
