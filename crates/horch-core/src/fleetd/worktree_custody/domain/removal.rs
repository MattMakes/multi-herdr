use std::path::{Path, PathBuf};

use super::rescue;

pub fn quarantine_path(tree: &Path, id: &str) -> PathBuf {
    let parent = tree.parent().unwrap_or(Path::new(""));
    parent.join(format!("{QUARANTINE_PREFIX}{id}"))
}

pub const QUARANTINE_PREFIX: &str = ".horch-removing-";

pub const NEEDS_YOU_KIND: &str = "remove_failed";

pub fn needs_you_key(id: &str) -> String {
    format!("{NEEDS_YOU_KIND}:{id}")
}

pub const OWNER_LIVE: &str = "a process of the owner attempt is live";

pub const IN_THE_TREE: &str = "have their working folder in the tree";

pub fn transient(reason: &str) -> bool {
    reason.contains(OWNER_LIVE) || reason.contains(IN_THE_TREE)
}

pub type Fact<T> = Result<T, String>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Basis {
    Landed { landed_commit: String },
    Rescued { rescue_ref: String },
}

impl Basis {
    pub fn force(&self) -> bool {
        matches!(self, Basis::Rescued { .. })
    }
}

pub fn removable(id: &str, state: &str) -> Result<(), String> {
    match state {
        "landed" | "rescued" => Ok(()),
        _ => Err(format!(
            "worktree {id} is {state}: only a landed or rescued tree is removed"
        )),
    }
}

pub fn basis(
    id: &str,
    state: &str,
    landed_commit: Option<&str>,
    rescue_ref: Option<&str>,
) -> Result<Basis, String> {
    removable(id, state)?;
    let nonempty = |s: &&str| !s.is_empty();
    if state == "landed" {
        return landed_commit
            .filter(nonempty)
            .map(|c| Basis::Landed {
                landed_commit: c.to_string(),
            })
            .ok_or_else(|| format!("worktree {id} is landed but has no landed commit"));
    }
    if !rescue::valid_id(id) {
        return Err(format!(
            "the worktree id {id:?} cannot name a quarantine folder"
        ));
    }
    rescue_ref
        .filter(|r| r.starts_with(&rescue::ref_folder(id)))
        .map(|r| Basis::Rescued {
            rescue_ref: r.to_string(),
        })
        .ok_or_else(|| format!("worktree {id} is rescued but has no rescue ref of its own"))
}

pub fn names_branch(entry: &str, branch: &str) -> bool {
    entry.contains(&format!("On {branch}:")) || entry.contains(&format!("WIP on {branch}:"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Saved {
    Landed {
        landed_commit: String,
        dirty: Fact<bool>,
    },
    Rescued {
        rescue_ref: String,
        read_back: Fact<String>,
        commit_tree: Fact<String>,
        commit_parent: Fact<String>,
        owner_live: Fact<bool>,
        cwd_pids: Fact<Vec<u32>>,
        disk_tree: Fact<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub state: String,
    pub owner_attempt: Option<String>,
    pub rescue_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    pub record_before: Record,
    pub head_before: Fact<String>,
    pub stashes: Fact<Vec<String>>,
    pub ignored_files: Fact<u64>,
    pub saved: Saved,
    pub head_after: Fact<String>,
    pub record_after: Fact<Record>,
}

fn known<'a, T>(what: &str, fact: &'a Fact<T>) -> Result<&'a T, String> {
    fact.as_ref()
        .map_err(|e| format!("the fact {what} is unknown: {e}"))
}

pub fn decide(f: &Facts) -> Result<(), String> {
    let head = known("the HEAD at the start of the checks", &f.head_before)?;
    let after = known("the HEAD at the end of the checks", &f.head_after)?;
    let stashes = known("the stash entries of the branch", &f.stashes)?;
    known("the count of ignored files", &f.ignored_files)?;
    let record = known("the ledger row at the end of the checks", &f.record_after)?;
    if *record != f.record_before {
        return Err(format!(
            "the ledger row of the tree changed during the checks: {:?} -> {record:?}",
            f.record_before
        ));
    }
    if head != after {
        return Err(format!(
            "the HEAD moved during the checks: {head} -> {after}"
        ));
    }
    match &f.saved {
        Saved::Landed {
            landed_commit,
            dirty,
        } => landed(
            head,
            landed_commit,
            *known("whether the tree is dirty", dirty)?,
        )?,
        Saved::Rescued {
            rescue_ref,
            read_back,
            commit_tree,
            commit_parent,
            owner_live,
            cwd_pids,
            disk_tree,
        } => {
            let commit = known("the read-back of the rescue ref", read_back)?;
            let commit_tree = known("the tree of the rescue commit", commit_tree)?;
            let parent = known("the parent of the rescue commit", commit_parent)?;
            let owner_live = *known("whether the owner attempt is live", owner_live)?;
            let cwd_pids = known("the processes in the tree", cwd_pids)?;
            let disk_tree = known("the tree of the folder now", disk_tree)?;
            if parent != head {
                return Err(format!(
                    "the HEAD {head} is not the parent of the rescue commit {commit}: \
                     the tree has commits after the rescue"
                ));
            }
            if disk_tree != commit_tree {
                return Err(format!(
                    "the folder differs from the rescue ref {rescue_ref}: \
                     a file changed after the rescue"
                ));
            }
            if owner_live {
                return Err(OWNER_LIVE.to_string());
            }
            if !cwd_pids.is_empty() {
                let pids: Vec<String> = cwd_pids.iter().map(u32::to_string).collect();
                return Err(format!("the processes {} {IN_THE_TREE}", pids.join(", ")));
            }
        }
    }
    if let Some(entry) = stashes.first() {
        return Err(format!("a stash entry names the branch: {entry}"));
    }
    Ok(())
}

fn landed(head: &str, landed_commit: &str, dirty: bool) -> Result<(), String> {
    if head != landed_commit {
        return Err(format!(
            "the HEAD {head} is not the landed commit {landed_commit}: \
             the tree has commits that are not landed"
        ));
    }
    if dirty {
        return Err("the tree has a modified or untracked file".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_process_in_the_tree_is_a_refusal_that_can_go_away() {
        assert!(transient(
            "the processes 7, 9 have their working folder in the tree"
        ));
        assert!(transient(
            "in quarantine, a process of the owner attempt is live"
        ));
        for reason in [
            "the folder differs from the rescue ref refs/horch/rescue/a/1: a file changed after the rescue",
            "the tree has a modified or untracked file",
            "a stash entry names the branch: stash@{0}: On b: x",
            "the HEAD moved during the checks: a -> b",
        ] {
            assert!(!transient(reason), "{reason}");
        }
    }

    fn ok<T>(v: T) -> Fact<T> {
        Ok(v)
    }

    fn record() -> Record {
        Record {
            state: "rescued".into(),
            owner_attempt: None,
            rescue_ref: Some("refs/horch/rescue/wt1/1".into()),
        }
    }

    fn rescued() -> Facts {
        Facts {
            record_before: record(),
            record_after: ok(record()),
            head_before: ok("h".into()),
            saved: Saved::Rescued {
                rescue_ref: "refs/horch/rescue/wt1/1".into(),
                read_back: ok("c".into()),
                commit_tree: ok("t".into()),
                commit_parent: ok("h".into()),
                owner_live: ok(false),
                cwd_pids: ok(Vec::new()),
                disk_tree: ok("t".into()),
            },
            stashes: ok(Vec::new()),
            ignored_files: ok(0),
            head_after: ok("h".into()),
        }
    }

    fn landed_facts(dirty: bool) -> Facts {
        Facts {
            saved: Saved::Landed {
                landed_commit: "h".into(),
                dirty: ok(dirty),
            },
            ..rescued()
        }
    }

    #[test]
    fn a_saved_tree_with_no_stash_and_a_still_head_is_removed() {
        assert_eq!(decide(&rescued()), Ok(()));
        assert_eq!(decide(&landed_facts(false)), Ok(()));
    }

    #[test]
    fn each_unknown_fact_refuses() {
        let unknown = || Err::<String, _>("git failed".to_string());
        let mut cases = vec![
            Facts {
                head_before: unknown(),
                ..rescued()
            },
            Facts {
                head_after: unknown(),
                ..rescued()
            },
            Facts {
                stashes: Err("git failed".into()),
                ..rescued()
            },
            Facts {
                ignored_files: Err("git failed".into()),
                ..rescued()
            },
            Facts {
                record_after: Err("no row".into()),
                ..rescued()
            },
            Facts {
                saved: Saved::Landed {
                    landed_commit: "h".into(),
                    dirty: Err("git failed".into()),
                },
                ..rescued()
            },
        ];
        for field in 0..6 {
            let mut f = rescued();
            if let Saved::Rescued {
                read_back,
                commit_tree,
                commit_parent,
                owner_live,
                cwd_pids,
                disk_tree,
                ..
            } = &mut f.saved
            {
                match field {
                    0 => *read_back = unknown(),
                    1 => *commit_tree = unknown(),
                    2 => *commit_parent = unknown(),
                    3 => *owner_live = Err("no pid".into()),
                    4 => *cwd_pids = Err("not supported".into()),
                    _ => *disk_tree = unknown(),
                }
            }
            cases.push(f);
        }
        for f in cases {
            let why = decide(&f).expect_err("an unknown fact refuses");
            assert!(why.contains("is unknown"), "{why}");
        }
    }

    #[test]
    fn each_unsaved_state_refuses() {
        let set = |change: &dyn Fn(&mut Facts)| {
            let mut f = rescued();
            change(&mut f);
            decide(&f).expect_err("a refusal")
        };
        assert!(set(&|f| f.head_after = ok("h2".into())).contains("moved during the checks"));
        let adopted = set(&|f| {
            f.record_after = ok(Record {
                state: "active".into(),
                owner_attempt: Some("A2".into()),
                ..record()
            })
        });
        assert!(
            adopted.contains("ledger row of the tree changed"),
            "{adopted}"
        );
        assert!(set(&|f| f.stashes = ok(vec!["On b: x".into()])).contains("stash entry"));
        let rescued_field = |change: &dyn Fn(&mut Saved)| set(&|f| change(&mut f.saved));
        let parent = rescued_field(&|s| {
            if let Saved::Rescued { commit_parent, .. } = s {
                *commit_parent = ok("other".into());
            }
        });
        assert!(parent.contains("not the parent"), "{parent}");
        let disk = rescued_field(&|s| {
            if let Saved::Rescued { disk_tree, .. } = s {
                *disk_tree = ok("other".into());
            }
        });
        assert!(disk.contains("a file changed after the rescue"), "{disk}");
        let owner = rescued_field(&|s| {
            if let Saved::Rescued { owner_live, .. } = s {
                *owner_live = ok(true);
            }
        });
        assert!(owner.contains("owner attempt is live"), "{owner}");
        let cwd = rescued_field(&|s| {
            if let Saved::Rescued { cwd_pids, .. } = s {
                *cwd_pids = ok(vec![7, 9]);
            }
        });
        assert!(cwd.contains("7, 9"), "{cwd}");
        let dirty = decide(&landed_facts(true)).unwrap_err();
        assert!(dirty.contains("modified or untracked"), "{dirty}");
        let mut f = landed_facts(false);
        f.saved = Saved::Landed {
            landed_commit: "other".into(),
            dirty: ok(false),
        };
        assert!(decide(&f).unwrap_err().contains("not the landed commit"));
    }

    #[test]
    fn only_a_landed_or_rescued_row_with_its_record_has_a_basis() {
        assert_eq!(
            basis("wt1", "landed", Some("c"), None),
            Ok(Basis::Landed {
                landed_commit: "c".into()
            })
        );
        assert_eq!(
            basis("wt1", "rescued", None, Some("refs/horch/rescue/wt1/2")),
            Ok(Basis::Rescued {
                rescue_ref: "refs/horch/rescue/wt1/2".into()
            })
        );
        assert!(basis("wt1", "landed", None, None).is_err());
        assert!(basis("wt1", "landed", Some(""), None).is_err());
        assert!(basis("wt1", "rescued", None, None).is_err());
        assert!(basis("wt1", "rescued", None, Some("refs/horch/rescue/wt2/1")).is_err());
        assert!(basis("wt1", "rescued", None, Some("refs/heads/main")).is_err());
        for state in [
            "reserved", "created", "active", "landing", "orphaned", "removed",
        ] {
            let why = basis("wt1", state, Some("c"), Some("refs/horch/rescue/wt1/1")).unwrap_err();
            assert!(why.contains(state), "{why}");
        }
        assert!(!Basis::Landed {
            landed_commit: "c".into()
        }
        .force());
        assert!(Basis::Rescued {
            rescue_ref: "r".into()
        }
        .force());
    }

    #[test]
    fn the_quarantine_folder_is_beside_the_tree() {
        assert_eq!(
            quarantine_path(Path::new("/p/.worktrees/T1-x"), "wt1"),
            Path::new("/p/.worktrees/.horch-removing-wt1")
        );
        assert!(basis("../x", "rescued", None, Some("refs/horch/rescue/../x/1")).is_err());
    }

    #[test]
    fn a_stash_entry_names_a_branch_as_today() {
        assert!(names_branch("stash@{0}: On fleet/t1: keep", "fleet/t1"));
        assert!(names_branch(
            "stash@{1}: WIP on fleet/t1: abc work",
            "fleet/t1"
        ));
        assert!(!names_branch("stash@{0}: On fleet/t10: keep", "fleet/t1"));
        assert!(!names_branch("stash@{0}: On main: keep", "fleet/t1"));
    }
}
