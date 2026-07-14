use serde::{Deserialize, Serialize};
use strum::{Display, EnumMessage, EnumString, IntoStaticStr, VariantNames};
use utoipa::ToSchema;

#[derive(
    Copy,
    Clone,
    Debug,
    Default,
    Eq,
    Hash,
    PartialEq,
    Serialize,
    Deserialize,
    Display,
    EnumMessage,
    EnumString,
    IntoStaticStr,
    VariantNames,
    ToSchema,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum GooseMode {
    #[strum(message = "Automatically approve tool calls")]
    Auto,
    #[strum(message = "Ask before every tool call")]
    Approve,
    /// Safe default: an absent/unset mode never grants unattended write access.
    /// Auto is opt-in only (config, `--yolo`, or an explicit API mode).
    #[default]
    #[strum(message = "Ask only for sensitive tool calls")]
    SmartApprove,
    #[strum(message = "Chat only, no tool calls")]
    Chat,
}

impl GooseMode {
    /// Whether the mode auto-approves every tool call without asking.
    pub fn auto_approves_everything(self) -> bool {
        matches!(self, GooseMode::Auto)
    }

    /// Whether the mode can pause a tool call to ask the user for approval.
    /// These modes are unusable wherever an approval prompt cannot be answered.
    pub fn requires_approval_channel(self) -> bool {
        matches!(self, GooseMode::Approve | GooseMode::SmartApprove)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_mode_never_auto_approves() {
        assert_eq!(GooseMode::default(), GooseMode::SmartApprove);
        assert!(!GooseMode::default().auto_approves_everything());
    }

    #[test]
    fn auto_stays_available_when_requested_explicitly() {
        assert_eq!("auto".parse::<GooseMode>().unwrap(), GooseMode::Auto);
        assert!(GooseMode::Auto.auto_approves_everything());
        assert_eq!(
            serde_json::from_str::<GooseMode>("\"auto\"").unwrap(),
            GooseMode::Auto
        );
    }

    #[test]
    fn approval_modes_need_a_confirmation_channel() {
        assert!(GooseMode::Approve.requires_approval_channel());
        assert!(GooseMode::SmartApprove.requires_approval_channel());
        assert!(!GooseMode::Auto.requires_approval_channel());
        assert!(!GooseMode::Chat.requires_approval_channel());
    }
}
