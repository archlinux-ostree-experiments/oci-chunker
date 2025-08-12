use std::{
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    str::FromStr,
};

use camino::{Utf8Path, Utf8PathBuf};

use crate::pkgdb::{Package, PackageDatabase, PackageDatabaseWithDefaultPath};

const QUERY_FORMAT: &str = "%{nevra},%{name},%{version},%{sourcerpm},%{size}\\n";

/// Parses RPM query output into a `PackageRpmQa` struct.
///
/// Expects an iterator of strings that represent lines from `rpm -qa` output
/// in the format: `nevra,name,version,sourcerpm,size`
fn get_components<I: Iterator<Item = String>>(mut it: I) -> Result<PackageRpmQa, anyhow::Error> {
    let unexpected_rpm_output = || anyhow::Error::msg("unexpected rpm output");
    let (identifier, name, version, source, size) = (
        it.next().ok_or_else(unexpected_rpm_output)?,
        it.next().ok_or_else(unexpected_rpm_output)?,
        it.next().ok_or_else(unexpected_rpm_output)?,
        it.next().ok_or_else(unexpected_rpm_output)?,
        it.next().ok_or_else(unexpected_rpm_output)?,
    );
    Ok(PackageRpmQa {
        identifier,
        name,
        version,
        source,
        size: size.parse()?,
    })
}

pub struct RpmDb {
    database: PathBuf,
}

impl RpmDb {
    /// Creates a new `RpmDb` instance pointing to the specified database path.
    pub fn new<P: AsRef<Path>>(database: P) -> Self {
        tracing::trace!("Initialize RPM package database at path {:?}", database.as_ref());
        Self {
            database: database.as_ref().to_path_buf(),
        }
    }

    /// Queries metadata for all installed packages in the RPM database.
    ///
    /// This method uses the `rpm` command to query package information and
    /// returns a vector of `PackageRpmQa` structs containing all the
    /// needed information with the exception of the file list.
    fn query_metadata(&self) -> Result<Vec<PackageRpmQa>, anyhow::Error> {
        let child = Command::new("/usr/bin/rpm")
            .arg("--dbpath")
            .arg(self.database.clone())
            .arg("-q")
            .arg("--queryformat")
            .arg(QUERY_FORMAT)
            .arg("-a")
            .stdout(Stdio::piped())
            .spawn()?;
        let packages = BufReader::new(
            child
                .stdout
                .ok_or(anyhow::Error::msg("rpm command had no stdout"))?,
        )
        .lines()
        .map(|l| get_components(l?.split(",").map(|s| s.to_string())))
        .collect::<Result<Vec<PackageRpmQa>, anyhow::Error>>()?;

        Ok(packages)
    }

    /// Queries the file list for a specific package identified by its NEVRA (Name-Epoch-Version-Release-Architecture).
    fn query_files(&self, nevra: &str) -> Result<Vec<Utf8PathBuf>, anyhow::Error> {
        let child = Command::new("/usr/bin/rpm")
            .arg("--dbpath")
            .arg(self.database.clone())
            .arg("-ql")
            .arg(nevra)
            .stdout(Stdio::piped())
            .spawn()?;
        let files = BufReader::new(
            child
                .stdout
                .ok_or(anyhow::Error::msg("rpm command had no stdout"))?,
        )
        .lines()
        .map(|l| Ok(Utf8PathBuf::from_str(&l?)?))
        .collect::<Result<Vec<Utf8PathBuf>, anyhow::Error>>()?;
        Ok(files)
    }
}

// Package information that can be obtained by a single call to `rpm -qa`
struct PackageRpmQa {
    // Unique package identifier
    identifier: String,

    // Package name, should stay stable across updates
    name: String,

    // Package version
    version: String,

    // Package source (e.g. srpm, identifier for packages originating from a single source)
    source: String,

    // Size in bytes
    size: u64,
}

impl PackageRpmQa {
    fn into_package<F: IntoIterator<Item = P>, P: AsRef<Utf8Path>>(self, files: F) -> Package {
        Package {
            identifier: self.identifier,
            name: self.name,
            version: self.version,
            source: self.source,
            size: self.size,
            files: files
                .into_iter()
                .map(|p| p.as_ref().to_path_buf())
                .collect(),
        }
    }
}

impl PackageDatabase for RpmDb {
    fn get_packages(&self) -> Result<Vec<Package>, anyhow::Error> {
        self.query_metadata()?
            .into_iter()
            .map(|meta| {
                let files = self.query_files(&meta.identifier)?;
                Ok(meta.into_package(files))
            })
            .collect()
    }

    fn get_changes(&self, _package: &Package) -> Result<Vec<u64>, anyhow::Error> {
        todo!()
    }
}

impl PackageDatabaseWithDefaultPath for RpmDb {
    const DEFAULT_PATH: &'static str = "/usr/share/rpm";
}

#[cfg(test)]
mod test {
    use std::fs::File;

    use crate::pkgdb::{PackageDatabase, rpm::RpmDb};

    #[test]
    fn test() {
        let test = RpmDb::new("/usr/lib/sysimage/rpm");
        let packages = test.get_packages().unwrap();
        let out = File::create("./pkg.json").unwrap();
        serde_json::to_writer(out, &packages).unwrap();
    }
}
