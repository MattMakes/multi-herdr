use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionMode {
    #[serde(rename = "acceptEdits")]
    AcceptEdits,
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "bypassPermissions")]
    BypassPermissions,
    #[serde(rename = "manual")]
    Manual,
    #[serde(rename = "dontAsk")]
    DontAsk,
    #[serde(rename = "plan")]
    Plan,
}

impl PermissionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            PermissionMode::AcceptEdits => "acceptEdits",
            PermissionMode::Auto => "auto",
            PermissionMode::BypassPermissions => "bypassPermissions",
            PermissionMode::Manual => "manual",
            PermissionMode::DontAsk => "dontAsk",
            PermissionMode::Plan => "plan",
        }
    }

    pub(crate) fn opencode_args(self) -> Option<Vec<String>> {
        match self {
            PermissionMode::Auto | PermissionMode::BypassPermissions => {
                Some(vec!["--auto".to_string()])
            }
            PermissionMode::AcceptEdits => Some(vec!["--auto".to_string()]),
            PermissionMode::Plan | PermissionMode::Manual | PermissionMode::DontAsk => None,
        }
    }

    pub(crate) fn antigravity_args(self) -> Option<Vec<String>> {
        let args: &[&str] = match self {
            PermissionMode::Plan => &["--mode", "plan"],
            PermissionMode::AcceptEdits => &["--mode", "accept-edits"],
            PermissionMode::Auto => &["--sandbox", "--dangerously-skip-permissions"],
            PermissionMode::BypassPermissions => &["--dangerously-skip-permissions"],
            PermissionMode::Manual | PermissionMode::DontAsk => return None,
        };
        Some(args.iter().map(|s| s.to_string()).collect())
    }

    pub(crate) fn codex_args(self) -> Option<Vec<String>> {
        let pair: &[&str] = match self {
            PermissionMode::Plan => &["-s", "read-only", "-a", "on-request"],
            PermissionMode::AcceptEdits | PermissionMode::Auto => {
                &["-s", "workspace-write", "-a", "never"]
            }
            PermissionMode::BypassPermissions => &["--dangerously-bypass-approvals-and-sandbox"],
            PermissionMode::Manual | PermissionMode::DontAsk => return None,
        };
        Some(pair.iter().map(|s| s.to_string()).collect())
    }
}
