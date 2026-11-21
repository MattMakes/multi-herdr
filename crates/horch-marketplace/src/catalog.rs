use std::borrow::Cow;
use std::collections::BTreeMap;

use crate::model::SkillId;

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
