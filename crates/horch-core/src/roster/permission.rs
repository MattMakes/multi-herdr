//! How much a teammate may do without stopping to ask.

use serde::{Deserialize, Serialize};

/// Codex's flag for workspace-write with approvals sent to its automatic
/// reviewer (Codex 0.147+). It conflicts with `-s` and
/// `--dangerously-bypass-approvals-and-sandbox`.
pub(crate) const CODEX_APPROVE_FOR_ME: &str = "--approve-for-me";

/// The `-c` pair that turns on Codex's automatic approval reviewer, which
/// `--approve-for-me` sets along with its sandbox.
pub(crate) const CODEX_AUTO_REVIEWER: &str = "approvals_reviewer=\"auto_review\"";

/// How much a teammate may do without stopping to ask.
///
/// Claude takes these directly. Codex has no single equivalent, so
/// `PermissionMode::codex_args` maps them onto its sandbox/approval pair.
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

    /// OpenCode's single approval flag for this mode.
    ///
    /// OpenCode has one lever - `--auto`, "auto-approve permissions that are not
    /// explicitly denied" - so the modes collapse into "pass it" or "do not".
    /// `Plan` and the two ask-shaped modes have no analogue at all: there is no
    /// read-only mode to put it in, and a worker left to prompt in a pane nobody
    /// is watching stalls forever. Returning `None` makes that an error.
    pub(crate) fn opencode_args(self) -> Option<Vec<String>> {
        match self {
            PermissionMode::Auto | PermissionMode::BypassPermissions => {
                Some(vec!["--auto".to_string()])
            }
            // OpenCode still asks about anything explicitly denied, which is the
            // closest thing it has to "accept edits, ask for the rest".
            PermissionMode::AcceptEdits => Some(vec!["--auto".to_string()]),
            PermissionMode::Plan | PermissionMode::Manual | PermissionMode::DontAsk => None,
        }
    }

    /// Antigravity CLI (`agy`) flags for this mode.
    ///
    /// `agy` has `--mode plan|accept-edits` and one bypass,
    /// `--dangerously-skip-permissions`. `Auto` adds `--sandbox`, which
    /// confines terminal commands, to the bypass: approve everything, inside
    /// the sandbox. `Manual` and `DontAsk` have no analogue.
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

    /// Codex sandbox + approval flags for this mode.
    ///
    /// The write modes use `--approve-for-me`: workspace-write, and approval
    /// requests go to Codex's automatic reviewer instead of a person or a
    /// refusal. The flag always means workspace-write, so `Plan` keeps
    /// read-only and turns on the same reviewer with its config key.
    ///
    /// `Manual` and `DontAsk` have no codex analogue. Returning `None` lets the
    /// caller raise an error rather than silently downgrade a teammate to
    /// something more permissive than its file asked for.
    pub(crate) fn codex_args(self) -> Option<Vec<String>> {
        let pair: &[&str] = match self {
            PermissionMode::Plan => &[
                "-s",
                "read-only",
                "-a",
                "on-request",
                "-c",
                CODEX_AUTO_REVIEWER,
            ],
            PermissionMode::AcceptEdits | PermissionMode::Auto => &[CODEX_APPROVE_FOR_ME],
            PermissionMode::BypassPermissions => &["--dangerously-bypass-approvals-and-sandbox"],
            PermissionMode::Manual | PermissionMode::DontAsk => return None,
        };
        Some(pair.iter().map(|s| s.to_string()).collect())
    }
}
