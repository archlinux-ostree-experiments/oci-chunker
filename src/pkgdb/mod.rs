use std::{
    collections::{BTreeSet, HashSet},
    hash::Hash,
    ops::Div,
    rc::Rc,
};

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
    pub files: HashSet<Utf8PathBuf>,
}

impl Hash for Package {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.identifier.hash(state);
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PackageIndex {
    pub package: Package,
    pub changes: BTreeSet<u64>,
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
            changes.insert(current_change);
        }
        while changes.len() > MAXIMUM_CHANGES {
            let _ = changes.pop_first();
        }
        Self { package, changes }
    }

    pub fn initialize(package: Package, current_change: u64) -> Self {
        Self {
            package,
            changes: BTreeSet::from_iter([current_change]),
        }
    }

    pub fn new<I: IntoIterator<Item = u64>>(package: Package, changes: I) -> Self {
        Self {
            package,
            changes: BTreeSet::from_iter(changes),
        }
    }

    pub fn change_frequency(&self) -> u32 {
        if self.changes.len() < 2 {
            return 0;
        }

        // Sum over the time differences of changes
        let diff = self
            .changes
            .iter()
            .fold((0u64, None), |(sum, last_element), current| {
                if let Some(last) = last_element {
                    (sum + (current - last), Some(current))
                } else {
                    (0, Some(current))
                }
            })
            .0;

        // Calculate the avg. time differences between package updates
        // Safety: Updates are expensive enough, that no one will want to have more than u64::MAX of them over an entire lifetime.
        // Actually, when updating from a previous index, it can never get larger than `MAXIMUM_CHANGES`.
        let avg = diff.div(u64::try_from(self.changes.len() - 1).unwrap());

        // Safety: We assume that the "average package" updates more frequently than every 60+ years.
        u32::try_from(avg).unwrap()
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
