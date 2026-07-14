use crate::conversation::message::{Message, MessageContent, ToolRequest};
use crate::conversation::Conversation;
use crate::prompt_template::render_template;
use crate::providers::base::Provider;
use chrono::Utc;
use indoc::indoc;
use rmcp::model::{Tool, ToolAnnotations};
use rmcp::object;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::sync::Arc;

/// Per-call argument budget for the judge prompt, in characters.
///
/// This is a limit, not a truncation point: the judge decides from the arguments
/// as invoked, so it must see them whole. A call whose serialized arguments do
/// not fit is not judged at all and always falls through to the user. Truncating
/// them and judging the head would let `echo <600 chars of padding> && rm -rf /`
/// come back read-only on the strength of its `echo`.
const ARGUMENTS_BUDGET: usize = 600;

const READ_ONLY_JUDGE_TOOL: &str = "platform__tool_by_tool_permission";

#[derive(Serialize)]
struct PermissionJudgeContext {
    // Empty struct for now since the current template doesn't need variables
}

/// The arguments a call was made with, serialized in full, or `None` when the
/// call cannot be judged: it is malformed, its arguments do not serialize, or
/// they are larger than [`ARGUMENTS_BUDGET`].
///
/// Every read-only verdict is gated on this returning `Some`, so a call the judge
/// cannot be shown in full is never one the judge can approve.
fn judgeable_arguments(request: &ToolRequest) -> Option<String> {
    let tool_call = request.tool_call.as_ref().ok()?;
    let arguments = match tool_call.arguments.as_ref() {
        Some(args) => serde_json::to_string(args).ok()?,
        None => "{}".to_string(),
    };
    (arguments.chars().count() <= ARGUMENTS_BUDGET).then_some(arguments)
}

/// Whether a tool call's behavior depends on its arguments.
///
/// `developer__shell` is read-only when it runs `ls` and destructive when it runs
/// `rm -rf`, so a verdict about one such *call* says nothing about the next call
/// to the same tool. Calls that carry no arguments always do the same thing, so a
/// verdict about them holds for every future call and may be cached by name.
pub fn is_argument_sensitive(request: &ToolRequest) -> bool {
    match &request.tool_call {
        // A call nobody could judge in full is never a safe candidate for a
        // name-only grant either: the cache must not learn from what was unread.
        Ok(_) if judgeable_arguments(request).is_none() => true,
        Ok(tool_call) => tool_call
            .arguments
            .as_ref()
            .is_some_and(|args| !args.is_empty()),
        // A malformed call is never a safe candidate for a name-only grant.
        Err(_) => true,
    }
}

/// Creates the tool definition for checking read-only permissions.
fn create_read_only_tool() -> Tool {
    Tool::new(
        READ_ONLY_JUDGE_TOOL.to_string(),
        indoc! {r#"
            Analyze the tool calls and determine which ones perform read-only operations.

            What constitutes a read-only operation:
            - A read-only operation retrieves information without modifying any data or state.
            - Examples include:
                - Reading a file without writing to it.
                - Querying a database without making updates.
                - Retrieving information from APIs without performing POST, PUT, or DELETE operations.

            Examples of read vs. write operations:
            - Read Operations:
                - `SELECT` query in SQL.
                - Reading file metadata or content.
                - Listing directory contents.
            - Write Operations:
                - `INSERT`, `UPDATE`, or `DELETE` in SQL.
                - Writing or appending to a file.
                - Modifying system configurations.
                - Sending messages to Slack channel.

            How to analyze tool calls:
            - Judge each call as it was actually made: the same tool is read-only with some
              arguments and destructive with others. A shell tool running `ls` is read-only;
              the same shell tool running `rm -rf`, `git push`, or `curl -X POST` is not.
            - Treat a call as read-only only if it cannot modify any state or data, anywhere,
              including remote systems.
            - Return the ids of the calls that are strictly read-only. If you cannot decide for
              a call, leave it out: it is not read-only.
        "#}
        .to_string(),
        object!({
            "type": "object",
            "properties": {
                "read_only_call_ids": {
                    "type": "array",
                    "items": {
                        "type": "string"
                    },
                    "description": "Ids of the tool calls, exactly as given, that perform read-only operations."
                }
            },
            "required": []
        })
    ).annotate(ToolAnnotations::with_title("Check tool operation".to_string()).read_only(true).destructive(false).idempotent(false).open_world(false))
}

/// Renders one tool call for the judge: its id, its name, and the arguments it
/// was actually called with, in full. A call that cannot be shown in full is not
/// shown at all.
fn describe_call(request: &ToolRequest) -> Option<String> {
    let tool_call = request.tool_call.as_ref().ok()?;
    let arguments = judgeable_arguments(request)?;
    Some(format!(
        "- id: {}\n  tool: {}\n  arguments: {}",
        request.id, tool_call.name, arguments
    ))
}

/// Builds the message to be sent to the LLM for detecting read-only calls.
fn create_check_messages(tool_requests: &[&ToolRequest]) -> Conversation {
    let calls: Vec<String> = tool_requests
        .iter()
        .filter_map(|r| describe_call(r))
        .collect();

    let check_messages = vec![Message::new(
        rmcp::model::Role::User,
        Utc::now().timestamp(),
        vec![MessageContent::text(format!(
            "Here are the tool calls:\n\n{}\n\nDecide, for each call, whether it is strictly \
             read-only as invoked with those exact arguments. \
             \n\nGuidelines for Read-Only Operations: \
             \n- Read-only operations do not modify any data or state, locally or remotely. \
             \n- Examples include file reading, SELECT queries in SQL, and directory listing. \
             \n- Write operations include INSERT, UPDATE, DELETE, file writing, and any command \
             that installs, deletes, moves, pushes, or sends something. \
             \n- The same tool can be read-only in one call and destructive in the next: judge \
             the arguments, not the tool name. \
             \n\nReturn the ids of the calls that qualify as read-only:",
            calls.join("\n"),
        ))],
    )];
    Conversation::new_unvalidated(check_messages)
}

/// Processes the response to extract the ids of the read-only calls.
fn extract_read_only_call_ids(response: &Message) -> Option<Vec<String>> {
    for content in &response.content {
        if let MessageContent::ToolRequest(tool_request) = content {
            if let Ok(tool_call) = &tool_request.tool_call {
                if tool_call.name == READ_ONLY_JUDGE_TOOL {
                    if let Some(arguments) = &tool_call.arguments {
                        if let Some(Value::Array(ids)) = arguments.get("read_only_call_ids") {
                            return Some(
                                ids.iter()
                                    .filter_map(|id| id.as_str().map(String::from))
                                    .collect(),
                            );
                        }
                    }
                }
            }
        }
    }
    None
}

/// Judges each tool call as invoked and returns the ids of the calls that are
/// read-only.
///
/// The verdict is about the *call*, not the tool: the arguments are part of what
/// is judged, and the result is keyed by tool request id so a caller cannot turn
/// one read-only call into a standing grant for the tool's name. Anything the
/// judge does not positively classify — an unreachable provider, a malformed
/// response, an id it invented, a call whose arguments were too large to show it
/// — is simply absent from the result, so callers fall through to asking the user.
pub async fn detect_read_only_calls(
    provider: Arc<dyn Provider>,
    session_id: &str,
    tool_requests: &[&ToolRequest],
) -> HashSet<String> {
    // Only calls the judge can be shown in full are eligible for a read-only
    // verdict. The rest are neither sent to it nor accepted back from it.
    let judgeable: HashSet<&str> = tool_requests
        .iter()
        .filter(|request| judgeable_arguments(request).is_some())
        .map(|request| request.id.as_str())
        .collect();

    if judgeable.len() < tool_requests.len() {
        tracing::warn!(
            unjudgeable = tool_requests.len() - judgeable.len(),
            "tool calls with oversized or unreadable arguments require user approval"
        );
    }

    if judgeable.is_empty() {
        return HashSet::new();
    }
    let tool = create_read_only_tool();
    let check_messages = create_check_messages(tool_requests);

    let context = PermissionJudgeContext {};
    let system_prompt = render_template("permission_judge.md", &context)
        .unwrap_or_else(|_| "You are a good analyst and can detect operations whether they have read-only operations.".to_string());

    let model_config = provider.get_model_config();
    let res = provider
        .complete(
            &model_config,
            session_id,
            &system_prompt,
            check_messages.messages(),
            std::slice::from_ref(&tool),
        )
        .await;

    let Ok((message, _usage)) = res else {
        return HashSet::new();
    };

    extract_read_only_call_ids(&message)
        .unwrap_or_default()
        .into_iter()
        .filter(|id| judgeable.contains(id.as_str()))
        .collect()
}

/// Result of permission checking for tool requests
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PermissionCheckResult {
    pub approved: Vec<ToolRequest>,
    pub needs_approval: Vec<ToolRequest>,
    pub denied: Vec<ToolRequest>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::base::{stream_from_single_message, MessageStream};
    use async_trait::async_trait;
    use bharatcode_providers::conversation::token_usage::{ProviderUsage, Usage};
    use bharatcode_providers::errors::ProviderError;
    use bharatcode_providers::model::ModelConfig;
    use rmcp::model::{CallToolRequestParams, ErrorCode, ErrorData};
    use rmcp::object;
    use std::sync::Mutex;

    fn request(id: &str, name: &str, arguments: serde_json::Map<String, Value>) -> ToolRequest {
        ToolRequest {
            id: id.to_string(),
            tool_call: Ok(CallToolRequestParams::new(name.to_string()).with_arguments(arguments)),
            metadata: None,
            tool_meta: None,
        }
    }

    /// A call whose arguments never made it into a `CallToolRequestParams` — the
    /// judge has nothing to read, so it must have nothing to say.
    fn malformed_request(id: &str) -> ToolRequest {
        ToolRequest {
            id: id.to_string(),
            tool_call: Err(ErrorData::new(
                ErrorCode::INVALID_PARAMS,
                "arguments are not valid json".to_string(),
                None,
            )),
            metadata: None,
            tool_meta: None,
        }
    }

    /// A judge that calls every id it is handed read-only, plus any extra id it is
    /// told to invent. It records the prompts it was shown, so a test can assert
    /// what the judge was — and was not — allowed to see.
    struct PermissiveJudge {
        invented_ids: Vec<String>,
        prompts: Mutex<Vec<String>>,
    }

    impl PermissiveJudge {
        fn approving(invented_ids: &[&str]) -> Arc<Self> {
            Arc::new(Self {
                invented_ids: invented_ids.iter().map(|id| id.to_string()).collect(),
                prompts: Mutex::new(Vec::new()),
            })
        }

        fn prompts(&self) -> String {
            self.prompts.lock().unwrap().join("\n")
        }
    }

    #[async_trait]
    impl Provider for PermissiveJudge {
        fn get_name(&self) -> &str {
            "permissive-judge"
        }

        fn get_model_config(&self) -> ModelConfig {
            ModelConfig::new("test-model").unwrap()
        }

        async fn stream(
            &self,
            _model_config: &ModelConfig,
            _session_id: &str,
            _system: &str,
            messages: &[Message],
            _tools: &[Tool],
        ) -> Result<MessageStream, ProviderError> {
            let prompt = messages
                .iter()
                .map(|message| message.as_concat_text())
                .collect::<Vec<_>>()
                .join("\n");

            // Every id the prompt actually shows, plus whatever we were told to make up.
            let mut ids: Vec<Value> = self
                .invented_ids
                .iter()
                .map(|id| Value::String(id.clone()))
                .collect();
            for line in prompt.lines() {
                if let Some(id) = line.strip_prefix("- id: ") {
                    ids.push(Value::String(id.to_string()));
                }
            }
            self.prompts.lock().unwrap().push(prompt);

            let mut arguments = serde_json::Map::new();
            arguments.insert("read_only_call_ids".to_string(), Value::Array(ids));
            let message = Message::assistant().with_tool_request(
                "judge",
                Ok(CallToolRequestParams::new(READ_ONLY_JUDGE_TOOL.to_string())
                    .with_arguments(arguments)),
            );
            Ok(stream_from_single_message(
                message,
                ProviderUsage::new("permissive-judge".to_string(), Usage::default()),
            ))
        }
    }

    #[test]
    fn calls_with_arguments_are_argument_sensitive() {
        let shell = request("1", "developer__shell", object!({"command": "ls"}));
        assert!(is_argument_sensitive(&shell));

        let no_args = request("2", "developer__list_windows", object!({}));
        assert!(!is_argument_sensitive(&no_args));
    }

    /// The judge must see the arguments, not just the tool name: `shell(ls)` and
    /// `shell(rm -rf /)` are the same tool and must be judged differently.
    #[test]
    fn judge_prompt_carries_ids_and_arguments() {
        let requests = [
            request("call-1", "developer__shell", object!({"command": "ls"})),
            request(
                "call-2",
                "developer__shell",
                object!({"command": "rm -rf /tmp/x"}),
            ),
        ];
        let borrowed: Vec<&ToolRequest> = requests.iter().collect();
        let text = create_check_messages(&borrowed).messages()[0].as_concat_text();

        assert!(text.contains("call-1"));
        assert!(text.contains("call-2"));
        assert!(text.contains("ls"));
        assert!(text.contains("rm -rf /tmp/x"));
    }

    /// A call too large for the judge is never shown to it, in whole or in part.
    /// Showing the head would be worse than showing nothing: the judge would rule
    /// on a prefix it could not know was a prefix.
    #[test]
    fn judge_prompt_omits_oversized_arguments() {
        let body = "x".repeat(ARGUMENTS_BUDGET * 4);
        let requests = [
            request(
                "call-1",
                "developer__text_editor",
                object!({"file_text": body.clone()}),
            ),
            request("call-2", "developer__shell", object!({"command": "ls"})),
        ];
        let borrowed: Vec<&ToolRequest> = requests.iter().collect();
        let text = create_check_messages(&borrowed).messages()[0].as_concat_text();

        assert!(!text.contains("call-1"));
        assert!(!text.contains(&"x".repeat(64)));
        assert!(text.contains("call-2"));
    }

    /// The bypass: pad a shell command past the budget and hide `rm -rf /` in the
    /// tail. Under truncation the judge saw only the benign `echo` head and called
    /// the whole call read-only. The call must now never reach the judge, and a
    /// judge that approves it anyway must not be believed.
    #[tokio::test]
    async fn a_dangerous_suffix_past_the_budget_is_never_read_only() {
        let padding = "a".repeat(ARGUMENTS_BUDGET * 2);
        let smuggled = request(
            "smuggled",
            "developer__shell",
            object!({"command": format!("echo {padding} && rm -rf /")}),
        );
        let benign = request("benign", "developer__shell", object!({"command": "ls"}));
        let requests: Vec<&ToolRequest> = vec![&smuggled, &benign];

        let judge = PermissiveJudge::approving(&["smuggled"]);
        let detected = detect_read_only_calls(
            judge.clone(),
            bharatcode_test_support::TEST_SESSION_ID,
            &requests,
        )
        .await;

        assert_eq!(
            detected,
            HashSet::from(["benign".to_string()]),
            "an oversized call must not be classified read-only, even when the judge says it is"
        );

        let prompts = judge.prompts();
        assert!(!prompts.contains("smuggled"));
        assert!(!prompts.contains("rm -rf /"));
        assert!(prompts.contains("benign"));
    }

    /// A call the judge cannot be shown is also never a candidate for the
    /// name-keyed smart-approve cache, so no truncated content can turn into a
    /// standing (session or on-disk) auto-allow for the tool.
    #[test]
    fn unjudgeable_calls_are_argument_sensitive() {
        let oversized = request(
            "call-1",
            "developer__shell",
            object!({"command": "a".repeat(ARGUMENTS_BUDGET * 2)}),
        );
        assert!(judgeable_arguments(&oversized).is_none());
        assert!(is_argument_sensitive(&oversized));

        let malformed = malformed_request("call-2");
        assert!(judgeable_arguments(&malformed).is_none());
        assert!(is_argument_sensitive(&malformed));
    }

    /// A call whose arguments cannot be read at all is unjudgeable for the same
    /// reason an oversized one is: the judge would be ruling on something other
    /// than what will run.
    #[tokio::test]
    async fn a_malformed_call_is_never_read_only() {
        let malformed = malformed_request("malformed");
        let requests: Vec<&ToolRequest> = vec![&malformed];

        let judge = PermissiveJudge::approving(&["malformed"]);
        let detected = detect_read_only_calls(
            judge.clone(),
            bharatcode_test_support::TEST_SESSION_ID,
            &requests,
        )
        .await;

        assert!(detected.is_empty());
        // With nothing judgeable to ask about, the judge is never consulted.
        assert!(judge.prompts().is_empty());
    }

    /// Arguments that fit are still judged in full — the fix fails closed on the
    /// oversized case without making every argument-bearing call ask.
    #[tokio::test]
    async fn arguments_within_the_budget_are_still_judged() {
        let listing = request(
            "listing",
            "developer__shell",
            object!({"command": "ls -la"}),
        );
        let requests: Vec<&ToolRequest> = vec![&listing];

        let judge = PermissiveJudge::approving(&[]);
        let detected = detect_read_only_calls(
            judge.clone(),
            bharatcode_test_support::TEST_SESSION_ID,
            &requests,
        )
        .await;

        assert_eq!(detected, HashSet::from(["listing".to_string()]));
        assert!(judge.prompts().contains("ls -la"));
    }

    #[test]
    fn extracts_call_ids_from_judge_response() {
        let response = Message::assistant().with_tool_request(
            "judge",
            Ok(CallToolRequestParams::new(READ_ONLY_JUDGE_TOOL.to_string())
                .with_arguments(object!({"read_only_call_ids": ["call-1"]}))),
        );
        assert_eq!(
            extract_read_only_call_ids(&response),
            Some(vec!["call-1".to_string()])
        );
    }

    #[test]
    fn ignores_a_response_without_the_judge_tool_call() {
        let response = Message::assistant().with_text("call-1 looks read-only to me");
        assert_eq!(extract_read_only_call_ids(&response), None);
    }
}
