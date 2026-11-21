use std::path::Path;

use crate::harness::launch::FORBIDDEN_ENV;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneShell {
    Posix,
    PowerShell,
}

impl PaneShell {
    pub fn host() -> Self {
        if cfg!(windows) {
            PaneShell::PowerShell
        } else {
            PaneShell::Posix
        }
    }

    pub fn command_line<S: AsRef<str>>(self, exe: &Path, args: &[S]) -> String {
        self.command_line_with_env(exe, &[], args)
    }

    pub fn command_line_with_env<S: AsRef<str>>(
        self,
        exe: &Path,
        env: &[(&str, &str)],
        args: &[S],
    ) -> String {
        self.line(&FORBIDDEN_ENV, exe, env, args, false)
    }

    pub fn command_line_keep_shell<S: AsRef<str>>(
        self,
        exe: &Path,
        env: &[(&str, &str)],
        args: &[S],
    ) -> String {
        self.line(&FORBIDDEN_ENV, exe, env, args, true)
    }

    fn line<S: AsRef<str>>(
        self,
        forbidden: &[&str],
        exe: &Path,
        env: &[(&str, &str)],
        args: &[S],
        keep_shell: bool,
    ) -> String {
        let mut parts = Vec::with_capacity(args.len() + 1);
        parts.push(self.quote(&exe.to_string_lossy()));
        parts.extend(args.iter().map(|a| self.quote(a.as_ref())));
        let joined = parts.join(" ");
        match self {
            PaneShell::PowerShell => {
                let removes: String = forbidden
                    .iter()
                    .map(|name| format!("Remove-Item Env:{name} -ErrorAction SilentlyContinue; "))
                    .collect();
                let sets: String = env
                    .iter()
                    .map(|(name, value)| format!("$env:{name} = {}; ", self.quote(value)))
                    .collect();
                format!("{removes}{sets}& {joined}")
            }
            PaneShell::Posix => {
                let removes: String = forbidden.iter().map(|name| format!(" -u {name}")).collect();
                let sets: String = env
                    .iter()
                    .map(|(name, value)| format!(" {}", self.quote(&format!("{name}={value}"))))
                    .collect();
                let command = format!("/usr/bin/env{removes}{sets} {joined}");
                if keep_shell {
                    let unsets: String = forbidden
                        .iter()
                        .map(|name| format!("unset {name}; "))
                        .collect();
                    format!("{unsets}{command}")
                } else {
                    format!("exec {command}")
                }
            }
        }
    }

    pub(crate) fn quote(self, arg: &str) -> String {
        match self {
            PaneShell::Posix => format!("'{}'", arg.replace('\'', r"'\''")),
            PaneShell::PowerShell => format!("'{}'", arg.replace('\'', "''")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posix_command_line_quotes_every_word() {
        let line = PaneShell::Posix
            .command_line(Path::new("/usr/local/bin/horch"), &["worker", "sonnet-1"]);
        assert_eq!(
            line,
            "exec /usr/bin/env -u ANTHROPIC_API_KEY '/usr/local/bin/horch' 'worker' 'sonnet-1'"
        );
    }

    #[test]
    fn powershell_command_line_uses_the_call_operator() {
        let line = PaneShell::PowerShell.command_line(
            Path::new(r"C:\Users\First Last\horch.exe"),
            &["worker", "sonnet-1"],
        );
        assert_eq!(
            line,
            r"Remove-Item Env:ANTHROPIC_API_KEY -ErrorAction SilentlyContinue; & 'C:\Users\First Last\horch.exe' 'worker' 'sonnet-1'"
        );
    }

    #[test]
    fn every_command_line_first_removes_every_forbidden_name() {
        for name in FORBIDDEN_ENV {
            let posix = PaneShell::Posix.command_line(Path::new("/bin/horch"), &["worker"]);
            assert!(posix.starts_with("exec /usr/bin/env -u "), "{posix}");
            assert!(posix.contains(&format!(" -u {name} ")), "{posix}");
            let ps = PaneShell::PowerShell.command_line(Path::new(r"C:\horch.exe"), &["worker"]);
            let remove = format!("Remove-Item Env:{name} -ErrorAction SilentlyContinue; ");
            assert!(ps.contains(&remove), "{ps}");
            assert!(ps.find(&remove) < ps.find("& "), "{ps}");
        }
    }

    #[test]
    fn command_line_with_env_sets_each_value_for_the_binary() {
        let env = [("HORCH_DATA_DIR", "/data/it's here")];
        assert_eq!(
            PaneShell::Posix.command_line_with_env(Path::new("/bin/horch"), &env, &["worker"]),
            r"exec /usr/bin/env -u ANTHROPIC_API_KEY 'HORCH_DATA_DIR=/data/it'\''s here' '/bin/horch' 'worker'"
        );
        assert_eq!(
            PaneShell::PowerShell.command_line_with_env(
                Path::new(r"C:\horch.exe"),
                &env,
                &["worker"]
            ),
            r"Remove-Item Env:ANTHROPIC_API_KEY -ErrorAction SilentlyContinue; $env:HORCH_DATA_DIR = '/data/it''s here'; & 'C:\horch.exe' 'worker'"
        );
    }

    #[test]
    fn posix_escapes_embedded_single_quotes() {
        assert_eq!(PaneShell::Posix.quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn powershell_doubles_embedded_single_quotes() {
        assert_eq!(PaneShell::PowerShell.quote("it's"), "'it''s'");
    }

    #[test]
    fn metacharacters_are_neutralised_in_both_dialects() {
        let nasty = "a; rm -rf / && $(echo x) `id` | tee $HOME";
        for shell in [PaneShell::Posix, PaneShell::PowerShell] {
            let quoted = shell.quote(nasty);
            assert!(quoted.starts_with('\'') && quoted.ends_with('\''));
            assert!(!quoted[1..quoted.len() - 1].contains('\''));
        }
    }

    #[test]
    fn no_args_still_yields_a_runnable_line() {
        assert_eq!(
            PaneShell::Posix.command_line(Path::new("/bin/horch"), &[] as &[&str]),
            "exec /usr/bin/env -u ANTHROPIC_API_KEY '/bin/horch'"
        );
        assert_eq!(
            PaneShell::PowerShell.command_line(Path::new(r"C:\horch.exe"), &[] as &[&str]),
            r"Remove-Item Env:ANTHROPIC_API_KEY -ErrorAction SilentlyContinue; & 'C:\horch.exe'"
        );
    }

    #[test]
    fn keep_shell_line_unsets_every_forbidden_name_without_exec() {
        let env = [("HORCH_DATA_DIR", "/d")];
        assert_eq!(
            PaneShell::Posix.command_line_keep_shell(Path::new("/bin/horch"), &env, &["register"]),
            "unset ANTHROPIC_API_KEY; /usr/bin/env -u ANTHROPIC_API_KEY 'HORCH_DATA_DIR=/d' '/bin/horch' 'register'"
        );
        assert_eq!(
            PaneShell::PowerShell.command_line_keep_shell(
                Path::new(r"C:\horch.exe"),
                &env,
                &["register"]
            ),
            PaneShell::PowerShell.command_line_with_env(
                Path::new(r"C:\horch.exe"),
                &env,
                &["register"]
            ),
            "PowerShell removes the name from the pane shell itself in both lines"
        );
    }

    #[cfg(unix)]
    mod on_a_posix_shell {
        use super::*;
        use std::process::{Command, Stdio};

        const DECOY: &str = "HORCH_TEST_DECOY_FORBIDDEN";
        const PROBE: &str = r#"echo "pid=$$"; if [ -n "${HORCH_TEST_DECOY_FORBIDDEN+x}" ]; then echo decoy=set; else echo decoy=unset; fi"#;

        fn pane(line: &str) -> (u32, String) {
            let child = Command::new("/bin/sh")
                .arg("-c")
                .arg(line)
                .env(DECOY, "decoy-value")
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            let pid = child.id();
            let out = child.wait_with_output().unwrap();
            assert!(out.status.success(), "{line}");
            (pid, String::from_utf8(out.stdout).unwrap())
        }

        #[test]
        fn the_exec_line_replaces_the_shell_and_drops_the_decoy() {
            let line =
                PaneShell::Posix.line(&[DECOY], Path::new("/bin/sh"), &[], &["-c", PROBE], false);
            assert!(line.starts_with("exec "), "{line}");
            let (pid, out) = pane(&format!("{line}\necho the shell stayed"));
            assert_eq!(out, format!("pid={pid}\ndecoy=unset\n"), "{line}");
        }

        #[test]
        fn the_keep_shell_line_drops_the_decoy_from_the_shell_too() {
            let line =
                PaneShell::Posix.line(&[DECOY], Path::new("/bin/sh"), &[], &["-c", PROBE], true);
            let (pid, out) = pane(&format!("{line}; {PROBE}"));
            let lines: Vec<&str> = out.lines().collect();
            assert_eq!(lines.len(), 4, "{out}");
            assert_ne!(lines[0], format!("pid={pid}"), "exe is a child: {out}");
            assert_eq!(lines[1], "decoy=unset", "{out}");
            assert_eq!(lines[2], format!("pid={pid}"), "the shell stays: {out}");
            assert_eq!(lines[3], "decoy=unset", "{out}");
        }

        #[test]
        fn the_decoy_reaches_a_probe_that_removes_nothing() {
            let (_, out) = pane(PROBE);
            assert!(out.ends_with("decoy=set\n"), "{out}");
        }
    }
}
