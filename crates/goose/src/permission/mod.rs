pub mod approval_mode;
pub mod permission_inspector;
pub mod permission_judge;
pub mod permission_store;

pub use approval_mode::{resolve_mode, ApprovalDecision, ApprovalMode, APPROVAL_CONFIG_KEY};
pub use goose_providers::permission::{Permission, PermissionConfirmation};
pub mod permission_confirmation {
    pub use goose_providers::permission::PrincipalType;
}
pub use permission_inspector::PermissionInspector;
pub use permission_store::ToolPermissionStore;
