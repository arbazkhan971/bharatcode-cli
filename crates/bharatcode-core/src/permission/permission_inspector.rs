use crate::agents::platform_extensions::MANAGE_EXTENSIONS_TOOL_NAME_COMPLETE;
use crate::agents::types::SharedProvider;
use crate::config::permission::PermissionLevel;
use crate::config::{GooseMode, PermissionManager};
use crate::conversation::message::{Message, ToolRequest};
use crate::permission::permission_judge::{
    detect_read_only_calls, is_argument_sensitive, PermissionCheckResult,
};
use crate::tool_inspection::{InspectionAction, InspectionResult, ToolInspector};
use anyhow::Result;
use async_trait::async_trait;
use rmcp::model::Tool;
use std::collections::HashSet;
use std::sync::{Arc, RwLock};

/// Permission Inspector that handles tool permission checking
pub struct PermissionInspector {
    pub permission_manager: Arc<PermissionManager>,
    provider: SharedProvider,
    readonly_tools: RwLock<HashSet<String>>,
}

impl PermissionInspector {
    pub fn new(permission_manager: Arc<PermissionManager>, provider: SharedProvider) -> Self {
        Self {
            permission_manager,
            provider,
            readonly_tools: RwLock::new(HashSet::new()),
        }
    }

    // readonly_tools is per-agent to avoid concurrent session clobbering; write-annotated
    // tools are cached globally via PermissionManager.
    pub fn apply_tool_annotations(&self, tools: &[Tool]) {
        let mut readonly_annotated = HashSet::new();
        for tool in tools {
            let Some(anns) = &tool.annotations else {
                continue;
            };
            if anns.read_only_hint == Some(true) {
                readonly_annotated.insert(tool.name.to_string());
            }
        }
        *self.readonly_tools.write().unwrap() = readonly_annotated;
        self.permission_manager.apply_tool_annotations(tools);
    }

    pub fn is_readonly_annotated_tool(&self, tool_name: &str) -> bool {
        self.readonly_tools.read().unwrap().contains(tool_name)
    }

    /// Process inspection results into permission decisions
    /// This method takes all inspection results and converts them into a PermissionCheckResult
    /// that can be used by the agent to determine which tools to approve, deny, or ask for approval
    pub fn process_inspection_results(
        &self,
        remaining_requests: &[ToolRequest],
        inspection_results: &[InspectionResult],
    ) -> PermissionCheckResult {
        use crate::tool_inspection::apply_inspection_results_to_permissions;

        // Start with permission inspector's decisions as the baseline
        let mut permission_check_result = PermissionCheckResult {
            approved: vec![],
            needs_approval: vec![],
            denied: vec![],
        };

        // Apply permission inspector results first (baseline behavior)
        let permission_results: Vec<_> = inspection_results
            .iter()
            .filter(|result| result.inspector_name == "permission")
            .collect();

        for request in remaining_requests {
            // Find the permission decision for this request
            if let Some(permission_result) = permission_results
                .iter()
                .find(|result| result.tool_request_id == request.id)
            {
                match permission_result.action {
                    InspectionAction::Allow => {
                        permission_check_result.approved.push(request.clone());
                    }
                    InspectionAction::Deny => {
                        permission_check_result.denied.push(request.clone());
                    }
                    InspectionAction::RequireApproval(_) => {
                        permission_check_result.needs_approval.push(request.clone());
                    }
                }
            } else {
                // If no permission result found, default to needs approval for safety
                permission_check_result.needs_approval.push(request.clone());
            }
        }

        // Apply security and other inspector results as overrides
        let non_permission_results: Vec<_> = inspection_results
            .iter()
            .filter(|result| result.inspector_name != "permission")
            .cloned()
            .collect();

        if !non_permission_results.is_empty() {
            permission_check_result = apply_inspection_results_to_permissions(
                permission_check_result,
                &non_permission_results,
            );
        }

        permission_check_result
    }
}

#[async_trait]
impl ToolInspector for PermissionInspector {
    fn name(&self) -> &'static str {
        "permission"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn inspect(
        &self,
        session_id: &str,
        tool_requests: &[ToolRequest],
        _messages: &[Message],
        goose_mode: GooseMode,
    ) -> Result<Vec<InspectionResult>> {
        let mut results = Vec::new();
        let permission_manager = &self.permission_manager;
        let mut llm_detect_candidates: Vec<&ToolRequest> = Vec::new();

        for request in tool_requests {
            if let Ok(tool_call) = &request.tool_call {
                let tool_name = &tool_call.name;

                let action = match goose_mode {
                    GooseMode::Chat => continue,
                    GooseMode::Auto => InspectionAction::Allow,
                    GooseMode::Approve | GooseMode::SmartApprove => {
                        // 1. An explicit, user-set permission always wins.
                        if let Some(level) = permission_manager.get_user_permission(tool_name) {
                            match level {
                                PermissionLevel::AlwaysAllow => InspectionAction::Allow,
                                PermissionLevel::NeverAllow => InspectionAction::Deny,
                                PermissionLevel::AskBefore => {
                                    InspectionAction::RequireApproval(None)
                                }
                            }
                        // 2. Extension management is never auto-approved.
                        } else if tool_name == MANAGE_EXTENSIONS_TOOL_NAME_COMPLETE {
                            InspectionAction::RequireApproval(Some(
                                "Extension management requires approval for security".to_string(),
                            ))
                        // 3. Approve means ask before *every* tool call: no read-only
                        //    annotation or smart-approve verdict may short-circuit it.
                        } else if goose_mode == GooseMode::Approve {
                            InspectionAction::RequireApproval(None)
                        // 4. A read-only annotation speaks for the tool as a whole.
                        } else if self.is_readonly_annotated_tool(tool_name) {
                            InspectionAction::Allow
                        } else {
                            match permission_manager.get_smart_approve_permission(tool_name) {
                                Some(PermissionLevel::NeverAllow) => InspectionAction::Deny,
                                Some(PermissionLevel::AskBefore) => {
                                    InspectionAction::RequireApproval(None)
                                }
                                // A cached allow is keyed by tool name only, so it may
                                // speak for a call whose behavior cannot vary with its
                                // arguments. Anything else is judged per call below —
                                // `shell(ls)` must not stand in for `shell(rm -rf /)`.
                                Some(PermissionLevel::AlwaysAllow)
                                    if !is_argument_sensitive(request) =>
                                {
                                    InspectionAction::Allow
                                }
                                _ => {
                                    llm_detect_candidates.push(request);
                                    continue;
                                }
                            }
                        }
                    }
                };

                let reason = match &action {
                    InspectionAction::Allow => {
                        if goose_mode == GooseMode::Auto {
                            "Auto mode - all tools approved".to_string()
                        } else if self.is_readonly_annotated_tool(tool_name) {
                            "Tool annotated as read-only".to_string()
                        } else if goose_mode == GooseMode::SmartApprove {
                            "SmartApprove cached as read-only".to_string()
                        } else {
                            "User permission allows this tool".to_string()
                        }
                    }
                    InspectionAction::Deny => "User permission denies this tool".to_string(),
                    InspectionAction::RequireApproval(_) => {
                        if tool_name == MANAGE_EXTENSIONS_TOOL_NAME_COMPLETE {
                            "Extension management requires user approval".to_string()
                        } else {
                            "Tool requires user approval".to_string()
                        }
                    }
                };

                results.push(InspectionResult {
                    tool_request_id: request.id.clone(),
                    action,
                    reason,
                    confidence: 1.0, // Permission decisions are definitive
                    inspector_name: self.name().to_string(),
                    finding_id: None,
                });
            }
        }

        // LLM-based read-only detection for deferred SmartApprove candidates. The
        // judge sees each call with its arguments and answers per call, so the
        // verdict below is about this invocation only.
        if !llm_detect_candidates.is_empty() {
            let detected: HashSet<String> = match self.provider.lock().await.clone() {
                Some(provider) => {
                    detect_read_only_calls(provider, session_id, &llm_detect_candidates).await
                }
                None => Default::default(),
            };

            for candidate in &llm_detect_candidates {
                let is_readonly = detected.contains(&candidate.id);

                // The cache is keyed by tool name, so only a verdict that holds for
                // every future call of that name may be written: caching
                // `shell(ls) is read-only` would hand `shell(rm -rf /)` a standing,
                // on-disk grant. Argument-sensitive calls are judged per call instead.
                if !is_argument_sensitive(candidate) {
                    if let Ok(tc) = &candidate.tool_call {
                        let level = if is_readonly {
                            PermissionLevel::AlwaysAllow
                        } else {
                            PermissionLevel::AskBefore
                        };
                        permission_manager.update_smart_approve_permission(&tc.name, level);
                    }
                }

                results.push(InspectionResult {
                    tool_request_id: candidate.id.clone(),
                    action: if is_readonly {
                        InspectionAction::Allow
                    } else {
                        InspectionAction::RequireApproval(None)
                    },
                    reason: if is_readonly {
                        "LLM detected as read-only".to_string()
                    } else {
                        "Tool requires user approval".to_string()
                    },
                    confidence: 1.0, // Permission decisions are definitive
                    inspector_name: self.name().to_string(),
                    finding_id: None,
                });
            }
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::CallToolRequestParams;
    use rmcp::object;
    use std::sync::Arc;
    use test_case::test_case;
    use tokio::sync::Mutex;

    fn request(name: &str, arguments: serde_json::Map<String, serde_json::Value>) -> ToolRequest {
        ToolRequest {
            id: "req".into(),
            tool_call: Ok(CallToolRequestParams::new(name.to_string()).with_arguments(arguments)),
            metadata: None,
            tool_meta: None,
        }
    }

    #[test_case(GooseMode::Auto, false, None, InspectionAction::Allow; "auto_allows")]
    #[test_case(GooseMode::SmartApprove, true, None, InspectionAction::Allow; "smart_approve_annotation_allows")]
    #[test_case(GooseMode::SmartApprove, false, Some(PermissionLevel::AlwaysAllow), InspectionAction::Allow; "smart_approve_cached_allow")]
    #[test_case(GooseMode::SmartApprove, false, Some(PermissionLevel::AskBefore), InspectionAction::RequireApproval(None); "smart_approve_cached_ask")]
    #[test_case(GooseMode::SmartApprove, false, None, InspectionAction::RequireApproval(None); "smart_approve_unknown_defers")]
    #[test_case(GooseMode::Approve, false, None, InspectionAction::RequireApproval(None); "approve_requires_approval")]
    #[test_case(GooseMode::Approve, false, Some(PermissionLevel::AlwaysAllow), InspectionAction::RequireApproval(None); "approve_ignores_cache")]
    #[test_case(GooseMode::Approve, true, None, InspectionAction::RequireApproval(None); "approve_ignores_readonly_annotation")]
    #[tokio::test]
    async fn test_inspect_action(
        mode: GooseMode,
        smart_approved: bool,
        cache: Option<PermissionLevel>,
        expected: InspectionAction,
    ) {
        let pm = Arc::new(PermissionManager::new(tempfile::tempdir().unwrap().keep()));
        if let Some(level) = cache {
            pm.update_smart_approve_permission("tool", level);
        }
        let inspector = PermissionInspector::new(pm, Arc::new(Mutex::new(None)));
        if smart_approved {
            *inspector.readonly_tools.write().unwrap() = ["tool".to_string()].into_iter().collect();
        }
        let results = inspector
            .inspect(
                bharatcode_test_support::TEST_SESSION_ID,
                &[request("tool", object!({}))],
                &[],
                mode,
            )
            .await
            .unwrap();
        assert_eq!(results[0].action, expected);
    }

    /// A cached read-only verdict is stored under the tool's *name*. For a tool
    /// whose behavior depends on its arguments, that grant must not carry over to
    /// the next call: one approved `shell(ls)` cannot silently approve
    /// `shell(rm -rf /)`. Such a call is re-judged instead (and with no provider
    /// wired up here, an unjudged call falls back to asking).
    #[tokio::test]
    async fn cached_allow_does_not_cover_a_different_argument() {
        let pm = Arc::new(PermissionManager::new(tempfile::tempdir().unwrap().keep()));
        pm.update_smart_approve_permission("developer__shell", PermissionLevel::AlwaysAllow);
        let inspector = PermissionInspector::new(pm, Arc::new(Mutex::new(None)));

        let results = inspector
            .inspect(
                bharatcode_test_support::TEST_SESSION_ID,
                &[request(
                    "developer__shell",
                    object!({"command": "rm -rf /tmp/x"}),
                )],
                &[],
                GooseMode::SmartApprove,
            )
            .await
            .unwrap();

        assert_eq!(results[0].action, InspectionAction::RequireApproval(None));
    }

    /// The smart-approve cache is a name-only grant, so a verdict on an
    /// argument-bearing call must never be written to it.
    #[tokio::test]
    async fn judging_an_argument_bearing_call_writes_no_standing_grant() {
        let pm = Arc::new(PermissionManager::new(tempfile::tempdir().unwrap().keep()));
        let inspector = PermissionInspector::new(pm.clone(), Arc::new(Mutex::new(None)));

        inspector
            .inspect(
                bharatcode_test_support::TEST_SESSION_ID,
                &[request("developer__shell", object!({"command": "ls"}))],
                &[],
                GooseMode::SmartApprove,
            )
            .await
            .unwrap();

        assert_eq!(pm.get_smart_approve_permission("developer__shell"), None);
    }

    /// A call that takes no arguments always does the same thing, so its verdict
    /// is safe to cache by name — that is what keeps SmartApprove from re-judging
    /// every listing call forever.
    #[tokio::test]
    async fn judging_an_argumentless_call_still_caches() {
        let pm = Arc::new(PermissionManager::new(tempfile::tempdir().unwrap().keep()));
        let inspector = PermissionInspector::new(pm.clone(), Arc::new(Mutex::new(None)));

        inspector
            .inspect(
                bharatcode_test_support::TEST_SESSION_ID,
                &[request("developer__list_windows", object!({}))],
                &[],
                GooseMode::SmartApprove,
            )
            .await
            .unwrap();

        // No provider is configured, so nothing is detected as read-only and the
        // call is cached on the safe side.
        assert_eq!(
            pm.get_smart_approve_permission("developer__list_windows"),
            Some(PermissionLevel::AskBefore)
        );
    }
}
