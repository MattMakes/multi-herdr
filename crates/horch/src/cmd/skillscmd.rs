//! `horch skills` - the skill catalog: the legacy listing, and the
//! marketplace install, update and check commands.
//!
//! `horch skills [list] [--phase <p>] [--json]` prints exactly what it
//! printed before the marketplace existed (the A0 oracle).

use std::process::ExitCode;

use anyhow::{anyhow, Context, Result};
use clap::Subcommand;
use horch_core::roster::Phase;
use horch_core::runtime::RuntimeContext;
use horch_core::skills::catalog::marketplace::{parse_source, InstallOptions, Lockfile, Store};
use horch_core::skills::catalog::{self, StoreState};
use horch_core::skills::{CatalogSource, SkillCatalog};
use serde_json::json;

use crate::output;

#[derive(Subcommand)]
pub enum SkillsCommand {
    /// The bundled catalog and its estimated context cost (the default).
    List {
        #[arg(long)]
        phase: Option<Phase>,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// One skill: id, version, source, digest, description, copied files
    /// and install path.
    Show {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Install a skill from `owner/repo[@rev]`, an `https://` or `file://`
    /// git URL `[@rev]`, an absolute directory, or `bundled:<id>`, and pin
    /// it in the marketplace lock.
    Install {
        #[arg(value_name = "SOURCE[@REV]")]
        source: String,
        /// The skill directory inside a git repository.
        #[arg(long, value_name = "SUBDIR")]
        path: Option<String>,
    },
    /// Re-resolve the requested revision of every installed skill, or of
    /// one, and install what changed.
    Update { id: Option<String> },
    /// Verify every locked skill's files against its digest. Exit 1 on a
    /// missing or changed version.
    Doctor,
}

/// Run `horch skills`. `phase` and `json` are the group's own flags, for
/// the legacy form without a subcommand.
pub fn run(
    ctx: &RuntimeContext,
    command: Option<SkillsCommand>,
    phase: Option<Phase>,
    json: bool,
) -> Result<ExitCode> {
    match command {
        None => list(phase, json)?,
        Some(SkillsCommand::List { phase, json }) => list(phase, json)?,
        Some(SkillsCommand::Show { id, json }) => show(ctx, &id, json)?,
        Some(SkillsCommand::Install { source, path }) => install(ctx, &source, path.as_deref())?,
        Some(SkillsCommand::Update { id }) => update(ctx, id.as_deref())?,
        Some(SkillsCommand::Doctor) => return doctor(ctx),
    }
    Ok(ExitCode::SUCCESS)
}

/// `horch skills show <id>`. JSON keys: `id`, `version`, `source`,
/// `digest`, `description`, `copied` (null unless a bundled skill has an
/// entry in `copied.json`) and `install_path` (null for a
/// compiled-in skill).
fn show(ctx: &RuntimeContext, id: &str, json: bool) -> Result<()> {
    let catalog = SkillCatalog::installed(&ctx.paths.data_root)?;
    let entry = catalog
        .lookup(id)
        .with_context(|| format!("no skill '{id}' in the catalog"))?;
    let path = catalog.store_dir(entry)?;
    let view = json!({
        "id": entry.id.as_str(),
        "version": entry.version.0,
        "source": entry.source_label(),
        "digest": entry.digest.to_string(),
        "description": entry.description,
        "copied": entry.copied,
        "install_path": path.as_deref().map(|p| p.display().to_string()),
    });
    if json {
        output::println(&serde_json::to_string_pretty(&view)?);
        return Ok(());
    }
    output::println(&format!("id:           {}", entry.id));
    output::println(&format!("version:      {}", entry.version));
    output::println(&format!("source:       {}", entry.source_label()));
    output::println(&format!("digest:       {}", entry.digest));
    output::println(&format!("description:  {}", printable(&entry.description)));
    if let Some(c) = &entry.copied {
        if c.copied_files.is_empty() {
            output::println("copied:       none (own text)");
        }
        for file in &c.copied_files {
            output::println(&format!("copied:       {file}"));
        }
        if c.verbatim {
            output::println("verbatim:     true");
        }
        if let Some(reason) = &c.budget_exempt {
            output::println(&format!("budget:       exempt ({})", printable(reason)));
        }
    }
    output::println(&format!(
        "install path: {}",
        match (&entry.source, &path) {
            (_, Some(p)) => p.display().to_string(),
            (CatalogSource::Bundled, None) => "compiled in".into(),
            _ => "-".into(),
        }
    ));
    Ok(())
}

/// `horch skills install <source>[@rev] [--path <subdir>]`.
fn install(ctx: &RuntimeContext, spec: &str, subdir: Option<&str>) -> Result<()> {
    let mut source = parse_source(spec).map_err(|e| anyhow!("{e}"))?;
    if let Some(dir) = subdir {
        source = source.with_subdir(dir);
    }
    let installer = catalog::installer(&ctx.paths.data_root, &ctx.bins.harness.git)?;
    let before = read_lock(&ctx.paths.data_root)?;
    let mut opts = InstallOptions::default();
    let fault = "abort-after-materialize-before-lock";
    if ctx.settings.faults.has(fault) {
        opts.fault = horch_core::skills::catalog::marketplace::FaultPoint::parse(fault);
    }
    let entry = installer
        .install(&source, &opts, None)
        // Not the spec: a rejected one can hold a credential.
        .map_err(|e| anyhow!("install: {e}"))?;
    let unchanged = before
        .get(&entry.id)
        .is_some_and(|old| old.version == entry.version && old.digest == entry.digest);
    output::println(&format!(
        "{} {} {}{}",
        if unchanged { "unchanged" } else { "installed" },
        entry.id,
        entry.version,
        entry
            .resolved_commit
            .as_deref()
            .map(|c| format!(" (commit {c})"))
            .unwrap_or_default()
    ));
    Ok(())
}

/// `horch skills update [<id>]`: one `<id> <old> -> <new>` line per skill.
fn update(ctx: &RuntimeContext, id: Option<&str>) -> Result<()> {
    let installer = catalog::installer(&ctx.paths.data_root, &ctx.bins.harness.git)?;
    let before = read_lock(&ctx.paths.data_root)?;
    let updated = installer.update(id).map_err(|e| anyhow!("update: {e}"))?;
    if updated.is_empty() {
        output::println("no installed skills");
    }
    for entry in updated {
        let old = before
            .get(&entry.id)
            .map(|e| e.version.as_str())
            .unwrap_or("-");
        output::println(&format!("{} {old} -> {}", entry.id, entry.version));
    }
    Ok(())
}

/// `horch skills doctor`: one line per lock entry; exit 1 on any problem.
fn doctor(ctx: &RuntimeContext) -> Result<ExitCode> {
    let checks = catalog::check_store(&ctx.paths.data_root)?;
    let mut problems = 0;
    for (entry, dir, state) in &checks {
        let line = match state {
            StoreState::Ok => format!("ok       {} {}", entry.id, entry.version),
            StoreState::Missing => format!(
                "missing  {} {}: {} does not exist (run `horch marketplace refresh`)",
                entry.id,
                entry.version,
                dir.display()
            ),
            StoreState::Tampered { actual } => format!(
                "changed  {} {}: locked {}, on disk {actual}",
                entry.id, entry.version, entry.digest
            ),
            StoreState::Invalid(why) => format!("invalid  {}: {why}", entry.id),
        };
        if *state != StoreState::Ok {
            problems += 1;
        }
        output::println(&line);
    }
    output::println(&format!(
        "{} skill(s) checked, {problems} problem(s); store {}",
        checks.len(),
        ctx.paths.data_root.display()
    ));
    Ok(if problems == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

/// SKILL.md text from an installed source, safe for a terminal: control
/// characters become `?`.
fn printable(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

fn read_lock(data_root: &std::path::Path) -> Result<Lockfile> {
    let path = Store::new(data_root).lock_path();
    Lockfile::read(&path).map_err(|e| anyhow!("{}: {e}", path.display()))
}

/// The legacy listing, byte for byte.
fn list(phase: Option<Phase>, json: bool) -> Result<()> {
    let catalog = horch_core::skills::describe(phase)?;
    if json {
        output::println(&serde_json::to_string_pretty(&catalog)?);
        return Ok(());
    }
    output::println(&format!(
        "Phase: {}",
        phase.map(|p| p.to_string()).unwrap_or_else(|| "all".into())
    ));
    for skill in catalog["skills"].as_array().expect("catalog skills array") {
        output::println(&format!(
            "  {:<18} {}",
            skill["name"].as_str().unwrap(),
            skill["description"].as_str().unwrap()
        ));
    }
    output::println(&format!(
        "Metadata: {} bytes (~{} tokens, estimate only); workflows load on demand.",
        catalog["metadata_bytes"], catalog["metadata_tokens_estimate"]
    ));
    Ok(())
}
