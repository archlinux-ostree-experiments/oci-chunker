use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub(crate) mod rpm;

#[derive(Debug, Serialize, Deserialize)]
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
    pub files: Vec<PathBuf>,
}

pub trait PackageDatabase {
    fn get_packages(&self) -> Result<Vec<Package>, anyhow::Error>;
}
