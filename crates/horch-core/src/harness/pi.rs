use horch_platform::start::Command;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use sha2::{Digest as _, Sha256};

use super::capabilities::Capabilities;
use super::launch::model_for;
use super::{CommandSpec, Harness, HarnessKind, LaunchEnv, PrepareRequest, Prepared, Session};
use crate::fleetd::worker_profiles::domain::operator_boundary::{Boundary, NotSupported};
use crate::roster::Teammate;
use crate::runtime::RuntimeContext;
use crate::skills::Bundle;

pub(crate) const COMPACT_EXTENSION: &str = include_str!("../../assets/pi/horch-compact.ts");

pub(crate) fn compact_extension_path(state_root: &Path) -> PathBuf {
    let digest = Sha256::digest(COMPACT_EXTENSION.as_bytes());
    let hex: String = digest[..6].iter().map(|b| format!("{b:02x}")).collect();
    state_root
        .join("pi")
        .join("extensions")
        .join(format!("horch-compact-{hex}.ts"))
}

pub(crate) fn install_compact_extension(state_root: &Path) -> Result<PathBuf> {
    let path = compact_extension_path(state_root);
    let dir = path.parent().expect("the path has a parent");
    crate::fsx::ensure_private_dir(&state_root.join("pi"))?;
    crate::fsx::ensure_private_dir(dir)?;
    if !is_intact(&path) {
        crate::fsx::write_atomic(&path, COMPACT_EXTENSION.as_bytes(), 0o600)
            .with_context(|| format!("write {}", path.display()))?;
    }
    Ok(path)
}

fn is_intact(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file() && private_mode(&m))
        && std::fs::read(path).is_ok_and(|bytes| bytes == COMPACT_EXTENSION.as_bytes())
}

fn private_mode(meta: &std::fs::Metadata) -> bool {
    horch_platform::fs::mode_of(meta).map_or(true, |mode| mode & 0o777 == 0o600)
}

pub(crate) struct Pi;

impl Harness for Pi {
    fn kind(&self) -> HarnessKind {
        HarnessKind::Pi
    }

    fn prepare(&self, ctx: &RuntimeContext, req: &PrepareRequest<'_>) -> Result<Prepared> {
        let mut prepared = Prepared::default();
        match install_compact_extension(&ctx.paths.state_root) {
            Ok(path) => prepared.extra_args.extend([
                "--extension".to_owned(),
                path.to_string_lossy().into_owned(),
            ]),
            Err(e) => eprintln!(
                "horch[{}]: pi compaction extension not installed, pi summarizes with its model: {e:#}",
                req.role
            ),
        }
        Ok(prepared)
    }

    fn expose_skills(
        &self,
        teammate: &Teammate,
        skills: &Bundle,
        _home: Option<&Path>,
    ) -> Result<Teammate> {
        Ok(with_skill_flag(teammate, skills))
    }

    fn command(&self, env: &LaunchEnv, spec: &CommandSpec<'_>) -> Result<Command> {
        pi_family_command(
            env.bins.pi.clone(),
            spec.teammate,
            spec.session,
            spec.prompt,
            spec.model_override,
        )
    }
}

pub(super) fn with_skill_flag(teammate: &Teammate, skills: &Bundle) -> Teammate {
    let mut adjusted = teammate.clone();
    adjusted.args.extend([
        "--skill".to_owned(),
        skills.skills_dir().to_string_lossy().into_owned(),
    ]);
    adjusted
}

pub(super) fn pi_family_command(
    bin: std::path::PathBuf,
    teammate: &Teammate,
    session: Session<'_>,
    prompt: &str,
    model_override: Option<&str>,
) -> Result<Command> {
    let mut cmd = horch_platform::start::command(bin);
    cmd.arg("--model").arg(model_for(teammate, model_override)?);

    if let Some(effort) = &teammate.effort {
        cmd.arg("--thinking").arg(effort);
    }
    let caps = teammate.agent.capabilities();
    push_tool_args(&mut cmd, teammate, caps);
    push_isolation_args(&mut cmd, teammate, caps);
    push_session_args(&mut cmd, session, caps);
    cmd.args(&teammate.args);
    cmd.arg("--").arg(prompt);
    Ok(cmd)
}

fn push_tool_args(cmd: &mut Command, teammate: &Teammate, caps: &Capabilities) {
    if !caps.tool_selection {
        return;
    }
    if let Some(tools) = &teammate.tools {
        if tools.is_empty() {
            cmd.arg("--no-tools");
        } else {
            cmd.arg("--tools").arg(tools.join(","));
        }
    }
    if !teammate.disallowed_tools.is_empty() && caps.tool_denylist {
        cmd.arg("--exclude-tools")
            .arg(teammate.disallowed_tools.join(","));
    }
}

fn push_isolation_args(cmd: &mut Command, teammate: &Teammate, caps: &Capabilities) {
    if teammate.inherit_plugins {
        return;
    }
    if caps.extensions_flag {
        cmd.arg("--no-extensions");
    }
    cmd.arg("--no-skills")
        .arg("--no-prompt-templates")
        .arg("--no-themes");
}

fn push_session_args(cmd: &mut Command, session: Session<'_>, caps: &Capabilities) {
    let (flag, id) = match (session, caps.caller_minted_session) {
        (Session::Fresh(id) | Session::Resume(id), true) => ("--session-id", id),
        (Session::Resume(id), false) => ("--resume", id),
        _ => return,
    };
    cmd.arg(flag).arg(id);
}

pub fn operator_boundary(
    _teammate: &Teammate,
    _fleet_root: &Path,
) -> Result<Boundary, NotSupported> {
    Err(NotSupported {
        adapter: "pi",
        os: std::env::consts::OS.into(),
        reason: "pi runs its tools with no sandbox and has no deny rule for a path or a socket",
    })
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

    use super::*;
    use crate::runtime::MapEnv;

    fn prepare_in(tmp: &Path) -> (RuntimeContext, Prepared) {
        let home = tmp.join("home");
        std::fs::create_dir_all(&home).unwrap();
        let env = MapEnv::new(tmp)
            .with("HOME", &home.to_string_lossy())
            .with("HORCH_STATE_DIR", &tmp.join("state").to_string_lossy());
        let ctx = RuntimeContext::from_env(&env).unwrap();
        let teammate = Teammate::default();
        let prepared = Pi
            .prepare(
                &ctx,
                &PrepareRequest {
                    role: "pi-1",
                    exec_rules: &[],
                    skills: None,
                    compact_window: None,
                    teammate: &teammate,
                    model: "ollama/qwen3.8",
                    workdir: tmp,
                    shell_env: &[],
                },
            )
            .unwrap();
        (ctx, prepared)
    }

    fn mode(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn x3_each_launch_loads_the_compaction_extension_from_the_state_root() {
        let tmp = tempfile::tempdir().unwrap();
        let (ctx, prepared) = prepare_in(tmp.path());
        let path = compact_extension_path(&ctx.paths.state_root);
        assert!(path.starts_with(tmp.path().join("state/pi/extensions")));
        assert_eq!(
            prepared.extra_args,
            vec![
                "--extension".to_string(),
                path.to_string_lossy().into_owned()
            ]
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), COMPACT_EXTENSION);
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
        assert_eq!(mode(&tmp.path().join("state/pi")), 0o700);

        let mut teammate = Teammate {
            agent: HarnessKind::Pi,
            model: Some("ollama/qwen3.8".into()),
            inherit_plugins: false,
            ..Teammate::default()
        };
        teammate.args.extend(prepared.extra_args.iter().cloned());
        let cmd =
            pi_family_command("pi".into(), &teammate, Session::Fresh("s"), "PROMPT", None).unwrap();
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let at = |flag: &str| args.iter().position(|a| a == flag).unwrap();
        assert!(at("--no-extensions") < at("--extension"));
        assert!(at("--extension") < at("--"));
        assert_eq!(args[at("--extension") + 1], path.to_string_lossy());
    }

    fn command_args(teammate: &Teammate) -> Vec<String> {
        pi_family_command("bin".into(), teammate, Session::Fresh("s"), "PROMPT", None)
            .unwrap()
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    const PRIME_REFUSED: [&str; 6] = [
        "--no-extensions",
        "-e",
        "--extension",
        "--tools",
        "--no-tools",
        "--exclude-tools",
    ];

    #[test]
    fn arc_10_a_prime_command_has_no_flag_prime_refuses() {
        for tools in [Some(vec![]), Some(vec!["read".to_string()])] {
            let prime = Teammate {
                agent: HarnessKind::Prime,
                model: Some("anthropic/claude-opus-5-5".into()),
                inherit_plugins: false,
                tools: tools.clone(),
                disallowed_tools: vec!["bash".into()],
                ..Teammate::default()
            };
            let args = command_args(&prime);
            for flag in PRIME_REFUSED {
                assert!(!args.iter().any(|a| a == flag), "{flag} in {args:?}");
            }
            for flag in ["--no-skills", "--no-prompt-templates", "--no-themes"] {
                assert!(args.iter().any(|a| a == flag), "{flag} missing: {args:?}");
            }

            let pi = Teammate {
                agent: HarnessKind::Pi,
                ..prime.clone()
            };
            let args = command_args(&pi);
            let tool_flag = if tools.as_ref().is_some_and(Vec::is_empty) {
                "--no-tools"
            } else {
                "--tools"
            };
            for flag in ["--no-extensions", tool_flag, "--exclude-tools"] {
                assert!(args.iter().any(|a| a == flag), "{flag} missing: {args:?}");
            }
        }
    }

    fn full_args(teammate: &Teammate, session: Session<'_>) -> Vec<String> {
        pi_family_command("bin".into(), teammate, session, "PROMPT", None)
            .unwrap()
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    fn pi_with(f: impl FnOnce(&mut Teammate)) -> Teammate {
        let mut t = Teammate {
            agent: HarnessKind::Pi,
            model: Some("ollama/qwen3.8".into()),
            inherit_plugins: true,
            args: vec!["--skill".into(), "/s".into()],
            ..Teammate::default()
        };
        f(&mut t);
        t
    }

    #[test]
    fn pi_flags_per_tool_capability() {
        let session = Session::Unmanaged;
        let none = full_args(&pi_with(|_| {}), session);
        assert_eq!(
            none,
            ["--model", "ollama/qwen3.8", "--skill", "/s", "--", "PROMPT"]
        );

        let empty = full_args(&pi_with(|t| t.tools = Some(vec![])), session);
        assert_eq!(empty[2], "--no-tools");
        assert!(!empty.iter().any(|a| a == "--tools"), "{empty:?}");

        let some = full_args(
            &pi_with(|t| t.tools = Some(vec!["read".into(), "grep".into()])),
            session,
        );
        assert_eq!(some[2..4], ["--tools", "read,grep"]);

        let deny = full_args(
            &pi_with(|t| t.disallowed_tools = vec!["bash".into(), "edit".into()]),
            session,
        );
        assert_eq!(deny[2..4], ["--exclude-tools", "bash,edit"]);

        let all = full_args(
            &pi_with(|t| {
                t.effort = Some("high".into());
                t.tools = Some(vec!["read".into()]);
                t.disallowed_tools = vec!["bash".into()];
                t.inherit_plugins = false;
            }),
            session,
        );
        assert_eq!(
            all,
            [
                "--model",
                "ollama/qwen3.8",
                "--thinking",
                "high",
                "--tools",
                "read",
                "--exclude-tools",
                "bash",
                "--no-extensions",
                "--no-skills",
                "--no-prompt-templates",
                "--no-themes",
                "--skill",
                "/s",
                "--",
                "PROMPT"
            ]
        );
    }

    #[test]
    fn prime_flags_skip_tool_and_extension_flags() {
        let prime = Teammate {
            agent: HarnessKind::Prime,
            model: Some("anthropic/claude-opus-5-5".into()),
            inherit_plugins: false,
            tools: Some(vec![]),
            disallowed_tools: vec!["bash".into()],
            ..Teammate::default()
        };
        assert_eq!(
            full_args(&prime, Session::Unmanaged),
            [
                "--model",
                "anthropic/claude-opus-5-5",
                "--no-skills",
                "--no-prompt-templates",
                "--no-themes",
                "--",
                "PROMPT"
            ]
        );
    }

    #[test]
    fn session_flag_follows_who_mints_the_id() {
        let pi = pi_with(|_| {});
        let tail = |t: &Teammate, s| full_args(t, s)[2..].to_vec();
        assert_eq!(
            tail(&pi, Session::Fresh("s1")),
            ["--session-id", "s1", "--skill", "/s", "--", "PROMPT"]
        );
        assert_eq!(
            tail(&pi, Session::Resume("s1")),
            ["--session-id", "s1", "--skill", "/s", "--", "PROMPT"]
        );
        assert_eq!(
            tail(&pi, Session::Unmanaged),
            ["--skill", "/s", "--", "PROMPT"]
        );

        let prime = Teammate {
            agent: HarnessKind::Prime,
            model: Some("m".into()),
            inherit_plugins: true,
            ..Teammate::default()
        };
        assert_eq!(tail(&prime, Session::Fresh("s1")), ["--", "PROMPT"]);
        assert_eq!(
            tail(&prime, Session::Resume("s1")),
            ["--resume", "s1", "--", "PROMPT"]
        );
        assert_eq!(tail(&prime, Session::Unmanaged), ["--", "PROMPT"]);
    }

    #[test]
    fn is_intact_rejects_missing_dir_link_and_wrong_mode() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("ext.ts");
        assert!(!is_intact(&file), "missing file");
        std::fs::create_dir(&file).unwrap();
        assert!(!is_intact(&file), "directory");
        std::fs::remove_dir(&file).unwrap();

        std::fs::write(&file, COMPACT_EXTENSION).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(!is_intact(&file), "wrong mode");
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(is_intact(&file));

        std::fs::write(&file, "other").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(!is_intact(&file), "other bytes");

        let link = tmp.path().join("link.ts");
        std::fs::write(&file, COMPACT_EXTENSION).unwrap();
        std::os::unix::fs::symlink(&file, &link).unwrap();
        assert!(!is_intact(&link), "symlink");
    }

    #[test]
    fn x3_an_intact_file_is_left_as_it_is() {
        let tmp = tempfile::tempdir().unwrap();
        let (ctx, _) = prepare_in(tmp.path());
        let path = compact_extension_path(&ctx.paths.state_root);
        let before = std::fs::metadata(&path).unwrap().ino();
        prepare_in(tmp.path());
        assert_eq!(std::fs::metadata(&path).unwrap().ino(), before);
    }

    #[test]
    fn x3_a_changed_extension_file_is_written_again() {
        let tmp = tempfile::tempdir().unwrap();
        let (ctx, _) = prepare_in(tmp.path());
        let path = compact_extension_path(&ctx.paths.state_root);

        std::fs::write(&path, "export default (pi) => { /* changed */ };").unwrap();
        prepare_in(tmp.path());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), COMPACT_EXTENSION);
        assert_eq!(mode(&path), 0o600);

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
        prepare_in(tmp.path());
        assert_eq!(mode(&path), 0o600);

        let elsewhere = tmp.path().join("elsewhere.ts");
        std::fs::write(&elsewhere, COMPACT_EXTENSION).unwrap();
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &path).unwrap();
        prepare_in(tmp.path());
        assert!(!std::fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), COMPACT_EXTENSION);
        assert_eq!(
            std::fs::read_to_string(&elsewhere).unwrap(),
            COMPACT_EXTENSION
        );
    }

    #[test]
    fn x3_the_operators_pi_agent_dir_is_untouched() {
        let tmp = tempfile::tempdir().unwrap();
        let agent = tmp.path().join("home/.pi/agent");
        std::fs::create_dir_all(agent.join("extensions")).unwrap();
        std::fs::write(agent.join("settings.json"), r#"{"theme":"dark"}"#).unwrap();
        let listing = |dir: &Path| {
            let mut names: Vec<_> = std::fs::read_dir(dir)
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect();
            names.sort();
            names
        };
        let before = (listing(&agent), listing(&agent.join("extensions")));
        let (_, prepared) = prepare_in(tmp.path());
        assert!(
            prepared.env.is_empty(),
            "no PI_CODING_AGENT_DIR: {:?}",
            prepared.env
        );
        assert_eq!(
            (listing(&agent), listing(&agent.join("extensions"))),
            before
        );
        assert_eq!(
            std::fs::read_to_string(agent.join("settings.json")).unwrap(),
            r#"{"theme":"dark"}"#
        );
        assert!(!tmp
            .path()
            .join("home/.pi/agent/pi-vcc-config.json")
            .exists());
    }
}
