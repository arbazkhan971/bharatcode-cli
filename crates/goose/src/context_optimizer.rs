//! Opt-in context/token optimizer.
//!
//! When the conversation must be trimmed to fit within a token budget, the
//! default behavior keeps the most *recent* messages (a simple tail). This
//! module adds a smarter, opt-in selection pass that keeps the messages that
//! are both the most *recent* and the most *relevant* to what the user is
//! currently asking, while always preserving the integrity of the
//! conversation (the final exchange and any tool request/response pairs).
//!
//! The optimizer is gated behind the `BHARATCODE_CONTEXT_OPTIMIZE` config flag
//! and is a no-op (returns the input unchanged) when disabled, so the default
//! behavior is unchanged.

use crate::config::Config;
use crate::conversation::message::{Message, MessageContent};

/// Config flag that turns the smarter context optimizer on. Default: off.
pub const CONTEXT_OPTIMIZE_PARAM: &str = "BHARATCODE_CONTEXT_OPTIMIZE";

/// Returns true if the smarter context optimizer is enabled via config.
///
/// Defaults to `false` so that, unless explicitly opted in, the surrounding
/// trimming/compaction path behaves exactly as before.
pub fn context_optimize_enabled() -> bool {
    Config::global()
        .get_param::<bool>(CONTEXT_OPTIMIZE_PARAM)
        .unwrap_or(false)
}

/// Estimate a token count for a single message using a lightweight,
/// dependency-free heuristic (roughly four characters per token, with a small
/// per-message overhead). This intentionally avoids constructing a real
/// tokenizer so the optimizer stays cheap and synchronous; callers that need
/// exact counts can pass their own scorer to [`select_messages_with`].
pub fn estimate_message_tokens(message: &Message) -> usize {
    let mut chars = 0usize;
    for content in &message.content {
        match content {
            MessageContent::Text(t) => chars += t.text.len(),
            MessageContent::ToolRequest(req) => {
                if let Ok(call) = &req.tool_call {
                    chars += call.name.len();
                    chars += serde_json::to_string(&call.arguments)
                        .map(|s| s.len())
                        .unwrap_or(0);
                }
            }
            MessageContent::ToolResponse(res) => {
                if let Ok(result) = &res.tool_result {
                    for c in &result.content {
                        if let Some(t) = c.as_text() {
                            chars += t.text.len();
                        }
                    }
                }
            }
            _ => {
                chars += 16;
            }
        }
    }
    // ~4 chars/token plus a small fixed framing cost per message.
    (chars / 4) + 4
}

/// Extract lowercase alphanumeric "words" of length >= 3 from a message's text
/// content. Used to compute lexical relevance to the current user request.
fn keywords(message: &Message) -> Vec<String> {
    let mut out = Vec::new();
    for content in &message.content {
        if let MessageContent::Text(t) = content {
            for raw in t.text.split(|c: char| !c.is_alphanumeric()) {
                if raw.len() >= 3 {
                    out.push(raw.to_lowercase());
                }
            }
        }
    }
    out
}

/// True if the message participates in a tool request/response exchange. Such
/// messages must never be split from their partner, so they are kept together.
fn is_tool_related(message: &Message) -> bool {
    message.content.iter().any(|c| {
        matches!(
            c,
            MessageContent::ToolRequest(_) | MessageContent::ToolResponse(_)
        )
    })
}

/// Select the indices of the messages to keep, in original order, such that the
/// total estimated token cost does not exceed `token_budget`.
///
/// Selection strategy:
/// 1. Always keep the very first message (typically scene-setting / the
///    original request) and a protected tail of the most recent messages so the
///    active turn and any in-flight tool calls stay intact.
/// 2. Fill the remaining budget with the highest-scoring middle messages, where
///    score combines recency (newer is better) and lexical relevance to the
///    latest user message.
///
/// This is the core, pure, unit-testable routine. `score_token` provides the
/// per-message token estimate (injectable for exact counting / testing).
pub fn select_indices_with<F>(
    messages: &[Message],
    token_budget: usize,
    protect_last_n: usize,
    score_token: F,
) -> Vec<usize>
where
    F: Fn(&Message) -> usize,
{
    let n = messages.len();
    if n == 0 {
        return Vec::new();
    }

    // Relevance reference: keywords from the most recent user message.
    let reference: std::collections::HashSet<String> = messages
        .iter()
        .rev()
        .find(|m| matches!(m.role, rmcp::model::Role::User))
        .map(keywords)
        .unwrap_or_default()
        .into_iter()
        .collect();

    let mut kept = vec![false; n];
    let mut used: usize = 0;

    // Helper closure to keep an index if it fits; tool-related neighbors are
    // pulled in together so a request is never separated from its response.
    let cost = |i: usize| score_token(&messages[i]);

    // 1. Protected anchors: first message + the last `protect_last_n`.
    let mut anchors: Vec<usize> = Vec::new();
    anchors.push(0);
    let tail_start = n.saturating_sub(protect_last_n.max(1));
    for i in tail_start..n {
        anchors.push(i);
    }
    anchors.sort_unstable();
    anchors.dedup();
    for &i in &anchors {
        if !kept[i] {
            kept[i] = true;
            used += cost(i);
        }
    }

    // 2. Score the remaining middle messages and greedily admit the best ones.
    let mut scored: Vec<(f64, usize)> = Vec::new();
    for (i, msg) in messages.iter().enumerate() {
        if kept[i] {
            continue;
        }
        let recency = (i as f64 + 1.0) / (n as f64);
        let kws = keywords(msg);
        let overlap = if reference.is_empty() || kws.is_empty() {
            0.0
        } else {
            let hits = kws.iter().filter(|k| reference.contains(*k)).count();
            hits as f64 / kws.len() as f64
        };
        // Relevance weighted a bit higher than raw recency; tool-related
        // context gets a small nudge so useful tool output survives.
        let mut score = 0.6 * overlap + 0.4 * recency;
        if is_tool_related(msg) {
            score += 0.05;
        }
        scored.push((score, i));
    }
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    for (_score, i) in scored {
        let c = cost(i);
        if used + c <= token_budget {
            kept[i] = true;
            used += c;
        }
    }

    (0..n).filter(|&i| kept[i]).collect()
}

/// Convenience wrapper over [`select_indices_with`] using the built-in
/// [`estimate_message_tokens`] heuristic.
pub fn select_indices(
    messages: &[Message],
    token_budget: usize,
    protect_last_n: usize,
) -> Vec<usize> {
    select_indices_with(
        messages,
        token_budget,
        protect_last_n,
        estimate_message_tokens,
    )
}

/// Optimize a slice of messages down to those that fit within `token_budget`,
/// returning cloned messages in their original order.
///
/// If the optimizer is disabled, or the messages already fit, the full set is
/// returned unchanged so this is safe to call unconditionally on the trimming
/// path.
pub fn optimize_messages(
    messages: &[Message],
    token_budget: usize,
    protect_last_n: usize,
) -> Vec<Message> {
    if !context_optimize_enabled() {
        return messages.to_vec();
    }
    let total: usize = messages.iter().map(estimate_message_tokens).sum();
    if total <= token_budget {
        return messages.to_vec();
    }
    let indices = select_indices(messages, token_budget, protect_last_n);
    indices.into_iter().map(|i| messages[i].clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::{AnnotateAble, CallToolRequestParams, CallToolResult, RawContent};

    fn user(text: &str) -> Message {
        Message::user().with_text(text)
    }

    fn assistant(text: &str) -> Message {
        Message::assistant().with_text(text)
    }

    fn tool_pair(id: &str, name: &str, response: &str) -> Vec<Message> {
        vec![
            Message::assistant()
                .with_tool_request(id, Ok(CallToolRequestParams::new(name.to_string()))),
            Message::user().with_tool_response(
                id,
                Ok(CallToolResult::success(vec![
                    RawContent::text(response).no_annotation()
                ])),
            ),
        ]
    }

    #[test]
    fn test_estimate_is_positive() {
        let m = user("hello world this is a message");
        assert!(estimate_message_tokens(&m) > 0);
    }

    #[test]
    fn test_keeps_first_and_tail() {
        let messages = vec![
            user("original request about deploying the kubernetes cluster"),
            assistant("a"),
            assistant("b"),
            assistant("c"),
            user("now please redeploy the kubernetes cluster"),
        ];
        // Tiny budget: only anchors should survive.
        let kept = select_indices(&messages, 1, 1);
        assert!(kept.contains(&0), "first message must be kept");
        assert!(
            kept.contains(&(messages.len() - 1)),
            "last message must be kept"
        );
    }

    #[test]
    fn test_relevance_beats_recency() {
        // Middle messages: one relevant to the final query, one not.
        let messages = vec![
            user("intro"),
            assistant("the kubernetes deployment manifest lives in deploy.yaml"),
            assistant("completely unrelated chit chat about the weather today"),
            assistant("more unrelated filler content padding padding padding"),
            user("how do I change the kubernetes deployment manifest"),
        ];
        // Budget large enough for anchors + exactly one middle message.
        let anchors_cost = estimate_message_tokens(&messages[0])
            + estimate_message_tokens(&messages[messages.len() - 1]);
        let relevant_cost = estimate_message_tokens(&messages[1]);
        let kept = select_indices(&messages, anchors_cost + relevant_cost, 1);
        assert!(
            kept.contains(&1),
            "the relevant middle message should be selected over unrelated ones: {:?}",
            kept
        );
        assert!(
            !kept.contains(&2),
            "the unrelated message should be dropped under tight budget: {:?}",
            kept
        );
    }

    #[test]
    fn test_keeps_order() {
        let messages = vec![user("a"), assistant("b"), assistant("c"), user("d")];
        let kept = select_indices(&messages, 1_000_000, 1);
        let mut sorted = kept.clone();
        sorted.sort_unstable();
        assert_eq!(kept, sorted, "indices must be returned in original order");
    }

    #[test]
    fn test_huge_budget_keeps_all() {
        let mut messages = vec![user("start")];
        messages.extend(tool_pair("t0", "read_file", "file contents here"));
        messages.push(user("end"));
        let kept = select_indices(&messages, usize::MAX, 1);
        assert_eq!(kept.len(), messages.len(), "all messages should be kept");
    }

    #[test]
    fn test_optimize_disabled_returns_all() {
        // With the flag unset (default off), optimize_messages must be a no-op
        // even when over budget.
        let messages = vec![
            user("a long message that clearly exceeds a one token budget"),
            assistant("another long message exceeding the tiny budget here"),
        ];
        let out = optimize_messages(&messages, 1, 1);
        assert_eq!(out.len(), messages.len());
    }

    #[test]
    fn test_select_with_custom_scorer() {
        // Inject a uniform scorer: every message costs 1 token.
        let messages = vec![user("aaa"), assistant("bbb"), assistant("ccc"), user("ddd")];
        // Budget of 2 with protect_last_n=1: anchors are index 0 and 3 (2 tokens).
        let kept = select_indices_with(&messages, 2, 1, |_| 1);
        assert!(kept.contains(&0));
        assert!(kept.contains(&3));
        assert_eq!(kept.len(), 2, "only the two anchors fit a 2-token budget");
    }
}
