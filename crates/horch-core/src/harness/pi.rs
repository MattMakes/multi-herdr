//! pi: `pi --model <pattern> --thinking <level> -- "<prompt>"`.
//!
//! pi's `--session-id` creates the session if it is missing, so horch mints
//! the id and nothing is discovered after launch.
//!
//! Each launch also loads horch's compaction extension
//! (`assets/pi/horch-compact.ts`, X3) with `--extension`: pi's summary is
//! then built from the session, with no model call, and keeps horch's
//! keep-list. The operator's `~/.pi/agent` is not read or written.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context as _, Result};
use sha2::{Digest as _, Sha256};

use super::launch::model_for;
use super::{CommandSpec, Harness, HarnessKind, LaunchEnv, PrepareRequest, Prepared, Session};
use crate::roster::Teammate;
use crate::runtime::RuntimeContext;
use crate::skills::Bundle;

/// The fleet compaction extension, as this horch build ships it.
pub(crate) const COMPACT_EXTENSION: &str = include_str!("../../assets/pi/horch-compact.ts");

/// Where a launch keeps the extension: under the state root, named by the
/// first 12 hex digits of its SHA-256. A new horch version writes a new
/// file, so a live pane's file never changes under it.
pub(crate) fn compact_extension_path(state_root: &Path) -> PathBuf {
    let digest = Sha256::digest(COMPACT_EXTENSION.as_bytes());
    let hex: String = digest[..6].iter().map(|b| format!("{b:02x}")).collect();
    state_root
        .join("pi")
        .join("extensions")
        .join(format!("horch-compact-{hex}.ts"))
}

/// Make the extension file hold exactly the embedded bytes, mode 0600, in
/// dirs of mode 0700. A Codex pane can write the state root (W21), so an
/// existing file is compared byte for byte, and anything else - other
/// bytes, a link, another mode - is replaced (temp name, then rename).
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

/// A regular file (not a link) with mode 0600 and the embedded bytes.
fn is_intact(path: &Path) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o777 != 0o600 {
            return false;
        }
    }
    std::fs::read(path).is_ok_and(|bytes| bytes == COMPACT_EXTENSION.as_bytes())
}

pub(crate) struct Pi;

impl Harness for Pi {
    fn kind(&self) -> HarnessKind {
        HarnessKind::Pi
    }

    /// `--extension <path>` for the fleet compaction extension. pi loads an
    /// explicit path also under `--no-extensions`. When the file cannot be
    /// written, the pane starts without it and pi writes its own summary.
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

/// pi and Prime Agent read skills from `--skill <dir>`. One flag names the
/// bundle's `skills/` directory, which holds exactly the activated skills; it
/// goes into `args`, before the `--` that fences off the prompt.
pub(super) fn with_skill_flag(teammate: &Teammate, skills: &Bundle) -> Teammate {
    let mut adjusted = teammate.clone();
    adjusted.args.extend([
        "--skill".to_owned(),
        skills.skills_dir().to_string_lossy().into_owned(),
    ]);
    adjusted
}

/// pi and Prime Agent: `<bin> --model <pattern> --thinking <level> -- "<prompt>"`.
///
/// Neither has an approval gate to bypass: their tools run, which is what makes
/// them usable in a pane nobody is watching. `permission_mode` therefore has
/// nothing to map onto, and the roster check rejects it rather than pretending.
pub(super) fn pi_family_command(
    bin: std::path::PathBuf,
    teammate: &Teammate,
    session: Session<'_>,
    prompt: &str,
    model_override: Option<&str>,
) -> Result<Command> {
    let mut cmd = Command::new(bin);
    cmd.arg("--model").arg(model_for(teammate, model_override)?);

    if let Some(effort) = &teammate.effort {
        cmd.arg("--thinking").arg(effort);
    }
    if let Some(tools) = &teammate.tools {
        // `Some([])` means "no tools at all", which is its own flag here rather
        // than an empty list.
        if tools.is_empty() {
            cmd.arg("--no-tools");
        } else {
            cmd.arg("--tools").arg(tools.join(","));
        }
    }
    // `--exclude-tools` is pi's; Prime 0.9.4 has no denylist, and the roster
    // check rejects `disallowed_tools` on a Prime teammate rather than dropping
    // it here, so this only ever fires for pi.
    if !teammate.disallowed_tools.is_empty() && teammate.agent.capabilities().tool_denylist {
        cmd.arg("--exclude-tools")
            .arg(teammate.disallowed_tools.join(","));
    }
    // Discovery of everything the repo or the operator might have lying around.
    // Nothing here is on by default in a fleet: a worker with one narrow job
    // should not inherit a project's extensions or the operator's skills.
    if !teammate.inherit_plugins {
        cmd.arg("--no-extensions")
            .arg("--no-skills")
            .arg("--no-prompt-templates")
            .arg("--no-themes");
    }

    match session {
        // pi's `--session-id` creates the session if it does not exist, so a
        // fresh launch and a resume are the same flag with a different id.
        // Prime has no such flag: `horch worker` gives it a `--session-dir` it
        // owns instead, and reads back whatever session appears there.
        Session::Fresh(id) if teammate.agent.capabilities().caller_minted_session => {
            cmd.arg("--session-id").arg(id);
        }
        Session::Resume(id) => {
            if teammate.agent.capabilities().caller_minted_session {
                cmd.arg("--session-id").arg(id);
            } else {
                cmd.arg("--resume").arg(id);
            }
        }
        Session::Fresh(_) | Session::Unmanaged => {}
    }
    cmd.args(&teammate.args);
    // `--` ends option parsing: without it a prompt beginning with a dash would
    // be read as a flag.
    cmd.arg("--").arg(prompt);
    Ok(cmd)
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

    use super::*;
    use crate::runtime::MapEnv;

    /// Prepare one pi pane with `tmp/home` as `$HOME` and `tmp/state` as
    /// the state root.
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

        // The flag goes into the teammate's args: before the `--` that
        // fences off the prompt, and after `--no-extensions`.
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

    #[test]
    fn x3_an_intact_file_is_left_as_it_is() {
        let tmp = tempfile::tempdir().unwrap();
        let (ctx, _) = prepare_in(tmp.path());
        let path = compact_extension_path(&ctx.paths.state_root);
        let before = std::fs::metadata(&path).unwrap().ino();
        prepare_in(tmp.path());
        assert_eq!(std::fs::metadata(&path).unwrap().ino(), before);
    }

    /// A Codex pane can write the state root (W21): a changed file, a link,
    /// or a loose mode is written again before pi loads it.
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

        // A link to a file with the same bytes is still replaced, and its
        // target is not touched.
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
