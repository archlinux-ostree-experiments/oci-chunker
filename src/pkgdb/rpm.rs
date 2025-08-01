use std::{
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    str::FromStr,
};

use crate::pkgdb::{Package, PackageDatabase};

const QUERY_FORMAT: &str = "%{nevra},%{name},%{version},%{sourcerpm},%{size}\\n";

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
    pub fn new<P: AsRef<Path>>(database: P) -> Self {
        Self {
            database: database.as_ref().to_path_buf(),
        }
    }

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

    fn query_files(&self, nevra: &str) -> Result<Vec<PathBuf>, anyhow::Error> {
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
        .map(|l| Ok(PathBuf::from_str(&l?)?))
        .collect::<Result<Vec<PathBuf>, anyhow::Error>>()?;
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
    fn into_package<F: IntoIterator<Item = PathBuf>>(self, files: F) -> Package {
        Package {
            identifier: self.identifier,
            name: self.name,
            version: self.version,
            source: self.source,
            size: self.size,
            files: files.into_iter().collect(),
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
