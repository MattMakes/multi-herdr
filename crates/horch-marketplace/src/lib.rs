//! The skill marketplace: install a skill from a bundled, local or git
//! source through a verified, transactional pipeline, and pin it in
//! `marketplace.lock`. This crate does not depend on horch-core and does not
//! read the process environment; the caller passes every path and binary.

pub mod catalog;
pub mod error;
pub mod fsx;
pub mod git;
pub mod installer;
pub mod integrity;
pub mod lockfile;
pub mod manifest;
pub mod model;
pub mod resolver;
pub mod source;
pub mod store;

pub use catalog::{BundledFile, BundledSkill, Catalog};
pub use error::{MarketplaceError, Result};
pub use git::{GitError, GitOutput, GitRunner};
pub use installer::{FaultPoint, InstallOptions, InstalledSkill, Installer, Step};
pub use lockfile::{LockEntry, Lockfile};
pub use manifest::SkillManifest;
pub use model::{parse_source, GitRevision, SkillId, SkillSource, SkillVersion};
pub use resolver::{ResolvedOrigin, ResolvedSource, Resolver};
pub use store::Store;
