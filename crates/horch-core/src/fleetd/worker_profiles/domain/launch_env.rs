use std::path::{Path, PathBuf};

pub const PRE_PUSH_REFUSAL: &str = "fleet agents do not push; fleetd pushes the feature branch";

pub const DENIED_COMMANDS: [&[&str]; 7] = [
    &["git", "push"],
    &["gh", "pr", "merge"],
    &["gh", "release"],
    &["gh", "issue", "comment"],
    &["gh", "pr", "comment"],
    &["gh", "pr", "review"],
    &["gh", "discussion"],
];

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

const PRE_PUSH: &str = "#!/bin/sh\n\
# Written by horch (SR-3). Fleet agents do not push.\n\
echo \"fleet agents do not push; fleetd pushes the feature branch\" >&2\n\
exit 1\n";

const PASS_THROUGH: &str = "#!/bin/sh\n\
# Written by horch (SR-3). Runs the hook of the repository.\n\
name=${0##*/}\n\
dir=$(unset GIT_CONFIG_COUNT; git config --type=path --get core.hooksPath)\n\
if [ -z \"$dir\" ]; then dir=\"$(git rev-parse --git-common-dir)/hooks\"; fi\n\
hook=\"$dir/$name\"\n\
if [ -f \"$hook\" ] && [ -x \"$hook\" ] && ! [ \"$hook\" -ef \"$0\" ]; then exec \"$hook\" \"$@\"; fi\n\
exit 0\n";

pub fn hooks_dir(fleet_root: &Path) -> PathBuf {
    fleet_root.join("hooks").join("agent")
}

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

pub fn hook_files() -> impl Iterator<Item = (&'static str, &'static str)> {
    std::iter::once(("pre-push", PRE_PUSH))
        .chain(PASS_THROUGH_HOOKS.iter().map(|n| (*n, PASS_THROUGH)))
}

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

    #[cfg(unix)]
    struct SealedRepo {
        _temp: tempfile::TempDir,
        root: PathBuf,
        repo: PathBuf,
        sealed: PathBuf,
    }

    #[cfg(unix)]
    impl SealedRepo {
        fn new() -> SealedRepo {
            use std::os::unix::fs::{symlink, PermissionsExt};
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().canonicalize().unwrap();
            let sealed = root.join("sealed");
            std::fs::create_dir_all(&sealed).unwrap();
            let exec_path = std::process::Command::new("git")
                .arg("--exec-path")
                .output()
                .unwrap();
            let exec_path = String::from_utf8(exec_path.stdout).unwrap();
            symlink(Path::new(exec_path.trim()).join("git"), sealed.join("git")).unwrap();
            symlink("/bin/sh", sealed.join("sh")).unwrap();
            let hooks = hooks_dir(&root);
            std::fs::create_dir_all(&hooks).unwrap();
            for (name, text) in hook_files() {
                let path = hooks.join(name);
                std::fs::write(&path, text).unwrap();
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
            let repo = root.join("repo");
            std::fs::create_dir_all(&repo).unwrap();
            let world = SealedRepo {
                _temp: temp,
                root,
                repo,
                sealed,
            };
            let init = world.git(&["init", "-q"]).output().unwrap();
            assert!(init.status.success(), "{init:?}");
            world
        }

        fn sealed_env(&self, cmd: &mut std::process::Command) {
            cmd.env_clear()
                .current_dir(&self.repo)
                .env("PATH", &self.sealed)
                .env("HOME", &self.root)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@t");
        }

        fn git(&self, args: &[&str]) -> std::process::Command {
            let mut cmd = std::process::Command::new(self.sealed.join("git"));
            cmd.args(args);
            self.sealed_env(&mut cmd);
            cmd
        }

        fn agent_git(&self, args: &[&str]) -> std::process::Command {
            let mut cmd = self.git(args);
            cmd.envs(git_env(&self.root));
            cmd
        }
    }

    #[cfg(unix)]
    #[test]
    fn pass_through_hook_runs_the_repository_hook_with_only_git_and_sh_on_the_path() {
        use std::os::unix::fs::PermissionsExt;
        let w = SealedRepo::new();
        let mark = w.root.join("hook-ran");
        let hook = w.repo.join(".git/hooks/pre-commit");
        std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
        std::fs::write(
            &hook,
            format!("#!/bin/sh\necho ran > '{}'\nexit 1\n", mark.display()),
        )
        .unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        let commit = w
            .agent_git(&["commit", "-q", "--allow-empty", "-m", "work"])
            .output()
            .unwrap();
        assert!(
            mark.is_file(),
            "the repository hook runs: {}",
            String::from_utf8_lossy(&commit.stderr)
        );
        assert!(
            !commit.status.success(),
            "the refusal of the repository hook stops the commit"
        );
    }

    #[cfg(unix)]
    #[test]
    fn pre_push_hook_refuses_with_only_git_and_sh_on_the_path() {
        let w = SealedRepo::new();
        let mut cmd = std::process::Command::new(hooks_dir(&w.root).join("pre-push"));
        w.sealed_env(&mut cmd);
        let push = cmd.output().unwrap();
        assert_eq!(push.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&push.stderr).contains(PRE_PUSH_REFUSAL));
    }

    #[test]
    fn git_env_points_core_hooks_path_at_the_agent_folder() {
        let env = git_env(Path::new("/fleet"));
        assert_eq!(env[1].1, "core.hooksPath");
        assert_eq!(PathBuf::from(&env[2].1), Path::new("/fleet/hooks/agent"));
    }

    #[test]
    fn claude_rules_deny_push_merge_release_and_external_messages() {
        let rules = DENIED_COMMANDS.map(claude_deny_rule);
        assert_eq!(
            rules,
            [
                "Bash(git push:*)",
                "Bash(gh pr merge:*)",
                "Bash(gh release:*)",
                "Bash(gh issue comment:*)",
                "Bash(gh pr comment:*)",
                "Bash(gh pr review:*)",
                "Bash(gh discussion:*)"
            ]
        );
    }
}
