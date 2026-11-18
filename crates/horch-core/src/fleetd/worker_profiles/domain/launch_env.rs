//! The launch environment of an agent, so that no agent pushes (ticket 22).
//!
//! SR-3: no agent pushes, deploys or messages outside the fleet. In Phase 4
//! no agent pushes at all; each push is the push job of fleetd
//! (`docs/tickets/fleetd-p4/MAP.md`, Q13). 2 guards hold it:
//!
//! - The environment of each worker sets `core.hooksPath` to
//!   `<fleet root>/hooks/agent` through `GIT_CONFIG_COUNT`, and the
//!   `pre-push` hook there refuses. The other hooks there pass each call
//!   on to the hook of the repository, so the project hooks still run.
//! - The profiles of Claude Code and Codex deny [`DENIED_COMMANDS`]. They
//!   also stop `git push --no-verify`, which runs no hook.
//!
//! horch's own git calls and the jobs of fleetd set
//! `-c core.hooksPath=/dev/null`, and a `-c` value wins over the
//! `GIT_CONFIG_*` values of the environment. So their pushes still work.
//!
//! This module does no I/O: the launch writes [`hook_files`].

use std::path::{Path, PathBuf};

/// The message of the `pre-push` hook of an agent.
pub const PRE_PUSH_REFUSAL: &str = "fleet agents do not push; fleetd pushes the feature branch";

/// The commands that the profile of an agent denies, as words.
pub const DENIED_COMMANDS: [&[&str]; 3] =
    [&["git", "push"], &["gh", "pr", "merge"], &["gh", "release"]];

/// The hook names of git (`githooks(5)`) other than `pre-push`. The agent
/// hooks folder has a pass-through hook for each.
const PASS_THROUGH_HOOKS: [&str; 26] = [
    "applypatch-msg",
    "pre-applypatch",
    "post-applypatch",
    "pre-commit",
    "pre-merge-commit",
    "prepare-commit-msg",
    "commit-msg",
    "post-commit",
    "pre-rebase",
    "post-checkout",
    "post-merge",
    "pre-receive",
    "update",
    "proc-receive",
    "post-receive",
    "post-update",
    "reference-transaction",
    "push-to-checkout",
    "pre-auto-gc",
    "post-rewrite",
    "sendemail-validate",
    "p4-changelist",
    "p4-prepare-changelist",
    "p4-post-changelist",
    "p4-pre-submit",
    "post-index-change",
];

/// The `pre-push` hook: it refuses each push.
const PRE_PUSH: &str = "#!/bin/sh\n\
# Written by horch (SR-3). Fleet agents do not push.\n\
echo \"fleet agents do not push; fleetd pushes the feature branch\" >&2\n\
exit 1\n";

/// A pass-through hook: it runs the hook of the same name of the
/// repository, from the hooks path the repository has without the agent
/// environment, else from `<common git dir>/hooks`. It gives the arguments,
/// the standard input and the exit code of that hook. A missing hook, or
/// a hooks path that is this folder, passes.
const PASS_THROUGH: &str = "#!/bin/sh\n\
# Written by horch (SR-3). Runs the hook of the repository.\n\
name=$(basename \"$0\")\n\
dir=$(unset GIT_CONFIG_COUNT; git config --type=path --get core.hooksPath)\n\
if [ -z \"$dir\" ]; then dir=\"$(git rev-parse --git-common-dir)/hooks\"; fi\n\
hook=\"$dir/$name\"\n\
if [ -f \"$hook\" ] && [ -x \"$hook\" ] && ! [ \"$hook\" -ef \"$0\" ]; then exec \"$hook\" \"$@\"; fi\n\
exit 0\n";

/// The hooks folder of the agents of `fleet_root`.
pub fn hooks_dir(fleet_root: &Path) -> PathBuf {
    fleet_root.join("hooks").join("agent")
}

/// The variables that give an agent the hooks folder of `fleet_root`.
pub fn git_env(fleet_root: &Path) -> [(String, String); 3] {
    [
        ("GIT_CONFIG_COUNT".into(), "1".into()),
        ("GIT_CONFIG_KEY_0".into(), "core.hooksPath".into()),
        (
            "GIT_CONFIG_VALUE_0".into(),
            hooks_dir(fleet_root).to_string_lossy().into_owned(),
        ),
    ]
}

/// Each file of the hooks folder: its name and its text. Each is a POSIX
/// shell script with mode 0755. On Windows git runs it with its own sh.
pub fn hook_files() -> impl Iterator<Item = (&'static str, &'static str)> {
    std::iter::once(("pre-push", PRE_PUSH))
        .chain(PASS_THROUGH_HOOKS.iter().map(|n| (*n, PASS_THROUGH)))
}

/// The Claude Code `permissions.deny` rule of 1 of [`DENIED_COMMANDS`].
pub fn claude_deny_rule(words: &[&str]) -> String {
    format!("Bash({}:*)", words.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pre_push_hook_prints_the_refusal() {
        assert!(PRE_PUSH.contains(PRE_PUSH_REFUSAL));
    }

    #[test]
    fn each_hook_name_has_1_file_and_pre_push_refuses() {
        let names: Vec<&str> = hook_files().map(|(n, _)| n).collect();
        let mut unique = names.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), names.len());
        assert_eq!(names.len(), 27);
        for (name, text) in hook_files() {
            assert!(text.starts_with("#!/bin/sh\n"), "{name}");
            assert_eq!(text == PRE_PUSH, name == "pre-push", "{name}");
        }
    }

    #[test]
    fn git_env_points_core_hooks_path_at_the_agent_folder() {
        let env = git_env(Path::new("/fleet"));
        assert_eq!(env[1].1, "core.hooksPath");
        assert_eq!(PathBuf::from(&env[2].1), Path::new("/fleet/hooks/agent"));
    }

    #[test]
    fn claude_rules_deny_push_merge_and_release() {
        let rules = DENIED_COMMANDS.map(claude_deny_rule);
        assert_eq!(
            rules,
            [
                "Bash(git push:*)",
                "Bash(gh pr merge:*)",
                "Bash(gh release:*)"
            ]
        );
    }
}
