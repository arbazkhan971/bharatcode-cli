//! Web search tool (BharatCode v27).
//!
//! A real `web_search` developer tool that lets the model fetch live search
//! results over HTTP. It talks to DuckDuckGo's keyless HTML endpoint
//! (`https://html.duckduckgo.com/html/`), extracts the result titles, links and
//! snippets, and returns a compact human-readable summary plus structured
//! content the model can reason over.
//!
//! The request is screened through the shared egress policy
//! ([`crate::offline::enforce_egress_policy`]) before any traffic leaves the
//! machine, so offline mode and the data-residency guard both apply: with a
//! strict residency posture (or offline mode) a search to a non-allowlisted host
//! is refused with a clear message instead of silently reaching out.

use std::time::Duration;

use rmcp::model::{CallToolResult, Content};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// DuckDuckGo's keyless HTML results endpoint.
const SEARCH_ENDPOINT: &str = "https://html.duckduckgo.com/html/";
/// Browser-like UA so the HTML endpoint returns the full results page.
const USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Safari/537.36";
/// Hard ceiling on how many results we will ever return regardless of `max_results`.
const RESULT_CAP: usize = 10;
/// Default number of results when the caller does not specify one.
const DEFAULT_RESULTS: usize = 5;
/// Cap on the response body we are willing to parse, in bytes.
const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;

/// Resolve a user-facing label, preferring the i18n `tr!` macro when present and
/// otherwise falling back to the supplied English string. The macro does not yet
/// exist in every build, so the fallback keeps this tool compiling and localized
/// labels can be layered in later without touching call sites.
macro_rules! label {
    ($fallback:expr) => {{
        let _ = $fallback;
        $fallback
    }};
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WebSearchParams {
    /// The search query to run against the web search engine.
    pub query: String,
    /// Maximum number of results to return (1-10). Defaults to 5.
    #[serde(default)]
    pub max_results: Option<usize>,
}

/// A single search hit returned to the model.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SearchResult {
    /// Result title.
    pub title: String,
    /// Result URL.
    pub url: String,
    /// Short snippet describing the result, when available.
    pub snippet: String,
}

pub struct WebSearchTool;

impl WebSearchTool {
    pub fn new() -> Self {
        Self
    }

    /// Run a web search and return a summarized [`CallToolResult`].
    pub async fn search(&self, params: WebSearchParams) -> CallToolResult {
        match run_search(&params).await {
            Ok(results) => {
                let summary = summarize(&params.query, &results);
                let mut result =
                    CallToolResult::success(vec![Content::text(summary).with_priority(0.0)]);
                result.structured_content = Some(json!({
                    "query": params.query,
                    "count": results.len(),
                    "results": results,
                }));
                result
            }
            Err(error) => CallToolResult::error(vec![Content::text(format!(
                "{}: {error}",
                label!("Web search failed")
            ))
            .with_priority(0.0)]),
        }
    }
}

impl Default for WebSearchTool {
    fn default() -> Self {
        Self::new()
    }
}

fn clamp_results(requested: Option<usize>) -> usize {
    requested.unwrap_or(DEFAULT_RESULTS).clamp(1, RESULT_CAP)
}

async fn run_search(params: &WebSearchParams) -> Result<Vec<SearchResult>, String> {
    let query = params.query.trim();
    if query.is_empty() {
        return Err(label!("query cannot be empty").to_string());
    }

    // Respect the residency / offline egress guard before any traffic leaves
    // the machine. In strict residency or offline mode a non-allowlisted host
    // is refused here with an actionable message.
    crate::offline::enforce_egress_policy(SEARCH_ENDPOINT).map_err(|error| error.to_string())?;

    let limit = clamp_results(params.max_results);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(USER_AGENT)
        .build()
        .map_err(|error| format!("failed to create HTTP client: {error}"))?;

    let response = client
        .get(SEARCH_ENDPOINT)
        .query(&[("q", query)])
        .send()
        .await
        .map_err(|error| format!("search request failed: {error}"))?
        .error_for_status()
        .map_err(|error| format!("search request failed: {error}"))?;

    let body = response
        .text()
        .await
        .map_err(|error| format!("failed to read search response: {error}"))?;

    if body.len() > MAX_BODY_BYTES {
        return Err(format!(
            "search response is too large: {} bytes exceeds {MAX_BODY_BYTES} byte limit",
            body.len()
        ));
    }

    let results = parse_results(&body, limit);
    Ok(results)
}

/// Parse the DuckDuckGo HTML results page into [`SearchResult`]s.
///
/// The HTML endpoint emits anchors of the form
/// `<a ... class="result__a" href="LINK">TITLE</a>` for each hit and a
/// `<a ... class="result__snippet" ...>SNIPPET</a>` for the description. We
/// scan for those markers without pulling in an HTML parser dependency, decode
/// the DuckDuckGo redirect wrapper, strip tags and HTML entities, and stop once
/// we have `limit` results.
fn parse_results(html: &str, limit: usize) -> Vec<SearchResult> {
    let mut results = Vec::new();
    let mut rest = html;

    while results.len() < limit {
        let Some(anchor_start) = rest.find("result__a") else {
            break;
        };
        rest = rest.get(anchor_start..).unwrap_or_default();

        let Some(href) = extract_attr(rest, "href=\"") else {
            break;
        };
        let url = decode_ddg_link(&href);

        let title = extract_anchor_text(rest).unwrap_or_default();
        let snippet = rest
            .find("result__snippet")
            .and_then(|idx| rest.get(idx..))
            .and_then(extract_anchor_text)
            .unwrap_or_default();

        // Advance past this anchor so the next iteration finds a fresh result.
        rest = rest.get(1..).unwrap_or_default();

        if url.is_empty() || title.is_empty() {
            continue;
        }

        results.push(SearchResult {
            title,
            url,
            snippet,
        });
    }

    results
}

/// Extract the value of an attribute (e.g. `href="`) starting from `slice`.
fn extract_attr(slice: &str, attr: &str) -> Option<String> {
    let start = slice.find(attr)? + attr.len();
    let tail = slice.get(start..)?;
    let end = tail.find('"')?;
    Some(tail.get(..end)?.to_string())
}

/// Extract the visible text of the next anchor (`>text</a>`) in `slice`.
fn extract_anchor_text(slice: &str) -> Option<String> {
    let gt = slice.find('>')? + 1;
    let tail = slice.get(gt..)?;
    let end = tail.find("</a>")?;
    let text = strip_tags(tail.get(..end)?);
    let text = decode_entities(&text);
    let trimmed = text.trim().to_string();
    (!trimmed.is_empty()).then_some(trimmed)
}

/// DuckDuckGo wraps result links in a redirect of the form
/// `//duckduckgo.com/l/?uddg=ENCODED&...`. Decode the real target when present;
/// otherwise normalize a protocol-relative link to https.
fn decode_ddg_link(raw: &str) -> String {
    if let Some(idx) = raw.find("uddg=") {
        let after = raw.get(idx + "uddg=".len()..).unwrap_or_default();
        let encoded = after.split('&').next().unwrap_or(after);
        if let Ok(decoded) = urlencoding::decode(encoded) {
            return decoded.into_owned();
        }
    }
    if let Some(stripped) = raw.strip_prefix("//") {
        return format!("https://{stripped}");
    }
    raw.to_string()
}

/// Remove any `<...>` tags from a fragment, keeping the text between them.
fn strip_tags(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

/// Decode the handful of HTML entities DuckDuckGo emits in result text.
fn decode_entities(input: &str) -> String {
    input
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

/// Build the human-readable summary returned alongside the structured results.
fn summarize(query: &str, results: &[SearchResult]) -> String {
    if results.is_empty() {
        return format!("{} \"{query}\".", label!("No web search results found for"));
    }

    let header = format!(
        "{} {} {} \"{query}\":",
        label!("Found"),
        results.len(),
        label!("web search result(s) for")
    );

    let body = results
        .iter()
        .enumerate()
        .map(|(idx, r)| {
            let snippet = if r.snippet.is_empty() {
                String::new()
            } else {
                format!("\n   {}", r.snippet)
            };
            format!("{}. {}\n   {}{snippet}", idx + 1, r.title, r.url)
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    format!("{header}\n\n{body}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_results_applies_bounds_and_default() {
        assert_eq!(clamp_results(None), DEFAULT_RESULTS);
        assert_eq!(clamp_results(Some(0)), 1);
        assert_eq!(clamp_results(Some(3)), 3);
        assert_eq!(clamp_results(Some(50)), RESULT_CAP);
    }

    #[test]
    fn strip_tags_and_entities() {
        assert_eq!(strip_tags("<b>hello</b> world"), "hello world");
        assert_eq!(decode_entities("a &amp; b &#39;c&#39;"), "a & b 'c'");
    }

    #[test]
    fn decode_ddg_link_unwraps_redirect() {
        let raw = "//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fpage&rut=abc";
        assert_eq!(decode_ddg_link(raw), "https://example.com/page");
    }

    #[test]
    fn decode_ddg_link_normalizes_protocol_relative() {
        assert_eq!(
            decode_ddg_link("//example.com/foo"),
            "https://example.com/foo"
        );
    }

    #[test]
    fn parse_results_extracts_title_url_and_snippet() {
        let html = r##"
            <a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Frust-lang.org%2F&rut=x">The Rust Programming Language</a>
            <a class="result__snippet" href="#">A language empowering everyone &amp; more.</a>
        "##;
        let results = parse_results(html, 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "The Rust Programming Language");
        assert_eq!(results[0].url, "https://rust-lang.org/");
        assert_eq!(results[0].snippet, "A language empowering everyone & more.");
    }

    #[test]
    fn parse_results_honors_limit() {
        let one =
            r#"<a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fa.com%2F">A</a>"#;
        let html = one.repeat(5);
        let results = parse_results(&html, 2);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn summarize_handles_empty_and_nonempty() {
        assert!(summarize("rust", &[]).contains("rust"));
        let results = vec![SearchResult {
            title: "Title".into(),
            url: "https://example.com".into(),
            snippet: "Snippet".into(),
        }];
        let summary = summarize("rust", &results);
        assert!(summary.contains("Title"));
        assert!(summary.contains("https://example.com"));
        assert!(summary.contains("Snippet"));
    }
}
