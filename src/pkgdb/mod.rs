use std::{hash::Hash, rc::Rc};

use camino::Utf8PathBuf;
use ostree_ext::{chunking::ObjectSourceMetaSized, objectsource::ObjectSourceMeta};
use serde::{Deserialize, Serialize};

#[cfg(feature = "archlinux")]
pub(crate) mod archlinux;
pub(crate) mod rpm;

pub(crate) mod cli;
pub(crate) mod postprocessing;

pub(crate) const MAXIMUM_CHANGES: usize = 100;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Package {
    // Unique package identifier
    pub identifier: String,

    // Package name, should stay stable across updates
    pub name: String,

    // Package version
    pub version: String,

    // Package source (e.g. srpm, identifier for packages originating from a single source)
    pub source: String,

    // Size in bytes
    pub size: u64,

    // List of files
    pub files: Vec<Utf8PathBuf>,
}

impl Hash for Package {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.identifier.hash(state);
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PackageIndex {
    pub package: Package,
    pub changes: Vec<u64>,
    pub total_updates: u32,
}

impl PackageIndex {
    pub fn update_from_previous_index(
        package: Package,
        previous_index: PackageIndex,
        current_change: u64,
    ) -> Self {
        assert_eq!(package.name, previous_index.package.name);
        let mut changes = previous_index.changes;
        let is_new_version = package.version != previous_index.package.version
            || package.identifier != previous_index.package.identifier;
        let is_new_build = changes
            .last()
            .map(|last| *last != current_change)
            .unwrap_or(true);
        if is_new_version && is_new_build {
            changes.push(current_change);
        }
        let total_updates = if is_new_build {
            previous_index.total_updates + 1
        } else {
            previous_index.total_updates
        };
        if changes.len() > MAXIMUM_CHANGES {
            let remove = changes.len() - MAXIMUM_CHANGES;
            let _ = changes.drain(0..remove);
        }
        Self {
            package,
            changes,
            total_updates,
        }
    }

    pub fn initialize(package: Package, current_change: u64) -> Self {
        Self {
            package,
            changes: vec![current_change],
            total_updates: 1,
        }
    }

    pub fn new(package: Package, changes: Vec<u64>, total_updates: u32) -> Self {
        Self {
            package,
            changes,
            total_updates,
        }
    }
}

pub trait PackageDatabase {
    /// Get a list of all installed packages according to the database.
    fn get_packages(&self) -> Result<Vec<Package>, anyhow::Error>;
    /// If the package system supports it, return the unix timestamps of every package update/changelog entry.
    fn get_changes(&self, package: &Package) -> Result<Vec<u64>, anyhow::Error>;
}

pub trait PackageDatabaseWithDefaultPath: PackageDatabase {
    const DEFAULT_PATH: &'static str;
}

impl From<Package> for ObjectSourceMetaSized {
    fn from(value: Package) -> Self {
        ObjectSourceMetaSized {
            meta: ObjectSourceMeta {
                identifier: Rc::from(value.identifier),
                name: Rc::from(value.name),
                srcid: Rc::from(value.source),
                change_time_offset: 1,
                change_frequency: 0,
            },
            size: value.size,
        }
    }
}

impl From<PackageIndex> for ObjectSourceMetaSized {
    fn from(value: PackageIndex) -> Self {
        ObjectSourceMetaSized {
            meta: ObjectSourceMeta {
                identifier: Rc::from(value.package.identifier),
                name: Rc::from(value.package.name),
                srcid: Rc::from(value.package.source),
                change_time_offset: if value.changes.len() >= 2 {
                    // Safety depends on the concrete measure of time, i.e. if u64 unix timestamps are used,
                    // this could overflow. However, we're talking about time differences, so this shouldn't be problematic.
                    // Even when measured in seconds, the delta is > 60 years, so this is going to be someone else's problem ;-)
                    u32::try_from(
                        value.changes[value.changes.len() - 1]
                            - value.changes[value.changes.len() - 2],
                    )
                    .unwrap()
                } else {
                    0
                },
                change_frequency: u32::try_from(value.changes.len()).unwrap(),
            },
            size: value.package.size,
        }
    }
}
