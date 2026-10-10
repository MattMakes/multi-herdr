//! The skills this crate knows before any fetch: the bundled skills the
//! caller hands in, and the installed skills recorded in the lock.

use std::borrow::Cow;
use std::collections::BTreeMap;

use crate::model::SkillId;

/// One file of a bundled skill. `path` is relative to the skill directory,
/// `/`-separated; it is checked before anything is written. `executable`
/// files are written with mode 755 on Unix; the mode is not in the digest.
#[derive(Debug, Clone)]
pub struct BundledFile {
    pub path: String,
    pub bytes: Cow<'static, [u8]>,
    pub executable: bool,
}

#[derive(Debug, Clone)]
pub struct BundledSkill {
    pub id: SkillId,
    pub files: Vec<BundledFile>,
}

/// The bundled skills. This crate does not embed them; the caller (core,
/// from its build-time skill table) supplies them.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    bundled: BTreeMap<SkillId, BundledSkill>,
}

impl Catalog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_bundled(mut self, skill: BundledSkill) -> Self {
        self.bundled.insert(skill.id.clone(), skill);
        self
    }

    pub fn bundled(&self, id: &SkillId) -> Option<&BundledSkill> {
        self.bundled.get(id)
    }

    pub fn bundled_ids(&self) -> impl Iterator<Item = &SkillId> {
        self.bundled.keys()
    }
}
