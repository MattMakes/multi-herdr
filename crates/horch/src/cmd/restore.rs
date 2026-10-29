//! herdr's native agent session restore (#125).
//!
//! After a herdr server restart, herdr resumes agent panes by itself
//! (`[session] resume_agents_on_restore`, on by default). A resumed fleet
//! agent runs without `horch worker`: it has no `HORCH_ROLE`, no record id
//! and no relay, and it starts from the herdr server's environment, which
//! horch does not scrub. This module warns when the restore is on, and lets
//! a resumed worker pane act as its registered role again.
//!
//! Nothing here reads a forbidden variable: the warning names it only.

use std::path::{Path, PathBuf};

use horch_core::harness::launch::FORBIDDEN_ENV;
use horch_core::messaging::mailbox::Mailbox;
use horch_core::runtime::{RuntimeContext, WorkerEnv};
use horch_core::workspace::herdr::Herdr;

/// herdr's config file: `HERDR_CONFIG_PATH`, else the platform default.
fn herdr_config_path(ctx: &RuntimeContext) -> PathBuf {
    if let Some(p) = std::env::var_os("HERDR_CONFIG_PATH").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    if cfg!(windows) {
        if let Some(app) = std::env::var_os("APPDATA").filter(|p| !p.is_empty()) {
            return Path::new(&app).join("herdr").join("config.toml");
        }
    }
    ctx.paths
        .home
        .join(".config")
        .join("herdr")
        .join("config.toml")
}

/// Whether herdr's `config` text leaves `resume_agents_on_restore` on. It is
/// on unless the `[session]` table (or a dotted `session.` key) sets it to
/// `false`; a later line wins, as in TOML.
fn resumes_on_restore(config: &str) -> bool {
    let mut table = String::new();
    let mut on = true;
    for raw in config.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            table = name.trim().to_string();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let ours = (table == "session" && key == "resume_agents_on_restore")
            || (table.is_empty() && key == "session.resume_agents_on_restore");
        if ours {
            on = value.trim() != "false";
        }
    }
    on
}

/// The warning for a herdr that resumes agents on restore, or `None` when
/// the config at `path` turns it off. A missing file is herdr's default: on.
fn resume_warning(path: &Path, config: Option<&str>) -> Option<String> {
    if config.is_some_and(|c| !resumes_on_restore(c)) {
        return None;
    }
    Some(format!(
        "warning: herdr resumes agent panes by itself after a herdr server restart: [session] resume_agents_on_restore is not false in {}. \
         A resumed fleet agent runs without its horch worker supervisor, so it loses its role, its reports and its relay. \
         It also starts from the herdr server's environment, which horch does not scrub, so it can bill {} instead of the claude.ai login. \
         Update herdr with `herdr update --handoff`, and do not restart the herdr server while a fleet runs. \
         To turn the restore off, set resume_agents_on_restore = false under [session] in that file.",
        path.display(),
        FORBIDDEN_ENV.join(", ")
    ))
}

/// Print [`resume_warning`] for this machine's herdr config.
pub fn warn_if_resume_on(ctx: &RuntimeContext) {
    let path = herdr_config_path(ctx);
    let config = std::fs::read_to_string(&path).ok();
    if let Some(w) = resume_warning(&path, config.as_deref()) {
        eprintln!("{w}");
    }
}

/// A pane that herdr resumed runs a registered worker's agent with no
/// `HORCH_ROLE`. When this process runs in such a pane, give it that role's
/// identity from the workspace registry and its brief, and say once that
/// the pane runs outside its supervisor.
///
/// Only a pane that herdr shows running an agent counts: an operator's
/// shell that got a stale pane id is not a worker. The orchestrator's role
/// is never taken: an untagged orchestrator line is the operator's.
pub fn adopt_restored_identity(ctx: &mut RuntimeContext) {
    if ctx.worker.as_ref().is_some_and(|w| w.role.is_some()) {
        return;
    }
    let Some(pane_env) = ctx.herdr.pane.as_ref().map(|p| p.as_str().to_string()) else {
        return;
    };
    let Ok(pane) = Herdr::with_bin(&ctx.bins.harness.herdr).pane_get(&pane_env) else {
        return;
    };
    if pane.agent.is_none() {
        return;
    }
    let Some(ws) = pane
        .workspace_id
        .clone()
        .or_else(|| ctx.herdr.workspace.as_ref().map(|w| w.as_str().to_string()))
    else {
        return;
    };
    let mailbox = Mailbox::in_context(ctx, &ws);
    let Some((role, _)) = mailbox
        .roles()
        .into_iter()
        .find(|(role, id)| *id == pane.pane_id && role != "orchestrator")
    else {
        return;
    };
    let Ok(brief) = mailbox.read_brief(&role) else {
        return;
    };
    eprintln!(
        "horch: warning: pane {} runs {role} outside its horch worker supervisor: herdr resumed it after a server restart. \
         This command uses the registered identity of {role}. Close the pane, and spawn the worker again with \
         horch spawn --resume {}.",
        pane.pane_id, brief.record_id
    );
    ctx.worker = Some(WorkerEnv {
        role: Some(role),
        teammate: Some(brief.teammate.clone()),
        agent: Some(brief.agent.clone()),
        model: Some(brief.model.clone()),
        record_id: Some(brief.record_id.clone()),
        worker_id: brief.worker_id.clone(),
        ..Default::default()
    });
    if let Ok(id) = horch_core::ids::WorkspaceId::new(&ws) {
        ctx.herdr.workspace = Some(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_restore_is_on_unless_the_session_table_turns_it_off() {
        assert!(resumes_on_restore(""));
        assert!(resumes_on_restore(
            "[session]\nresume_agents_on_restore = true\n"
        ));
        assert!(!resumes_on_restore(
            "[session]\nresume_agents_on_restore = false\n"
        ));
        assert!(!resumes_on_restore(
            "[ui]\ntheme = \"x\"\n[session]\n  resume_agents_on_restore=false  # fleet\n"
        ));
        assert!(!resumes_on_restore(
            "session.resume_agents_on_restore = false\n"
        ));
        // The key of another table, and a commented-out line, do not count.
        assert!(resumes_on_restore(
            "[other]\nresume_agents_on_restore = false\n"
        ));
        assert!(resumes_on_restore(
            "[session]\n# resume_agents_on_restore = false\n"
        ));
        // A later line wins.
        assert!(resumes_on_restore(
            "[session]\nresume_agents_on_restore = false\nresume_agents_on_restore = true\n"
        ));
    }

    #[test]
    fn the_warning_names_the_setting_the_key_risk_and_the_safe_update() {
        let path = Path::new("/h/.config/herdr/config.toml");
        let w = resume_warning(path, None).expect("a missing config is herdr's default: on");
        assert!(
            w.contains("resume_agents_on_restore is not false in /h/.config/herdr/config.toml"),
            "{w}"
        );
        assert!(w.contains("without its horch worker supervisor"), "{w}");
        assert!(
            w.contains(&format!("it can bill {}", FORBIDDEN_ENV[0])),
            "{w}"
        );
        assert!(w.contains("`herdr update --handoff`"), "{w}");
        assert_eq!(
            resume_warning(path, Some("[session]\nresume_agents_on_restore = false\n")),
            None
        );
    }
}
