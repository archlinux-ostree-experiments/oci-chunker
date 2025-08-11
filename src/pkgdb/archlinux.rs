use std::{io::Read, path::PathBuf, str::FromStr};

use alpm::{Alpm, Db};
use camino::{Utf8Path, Utf8PathBuf};

use crate::pkgdb::{Package, PackageDatabase, PackageDatabaseWithDefaultPath};

pub(crate) struct AlpmDb {
    handle: Alpm,
}

impl AlpmDb {
    pub(crate) fn new(sysroot: &Utf8Path, db_path: &Utf8Path) -> Result<Self, anyhow::Error> {
        let handle = Alpm::new(sysroot.as_str(), db_path.as_str())?;
        Ok(Self { handle })
    }

    pub fn db(&self) -> &Db {
        &self.handle.localdb()
    }
}

impl PackageDatabase for AlpmDb {
    fn get_packages(&self) -> Result<Vec<Package>, anyhow::Error> {
        Ok(self
            .db()
            .pkgs()
            .iter()
            .map(|pkg| Package {
                identifier: format!("{}-{}-{}", pkg.name(), pkg.version(), pkg.build_date()),
                name: pkg.name().to_string(),
                version: pkg.version().to_string(),
                source: pkg.name().to_string(),
                size: u64::try_from(pkg.isize()).unwrap(),
                files: pkg
                    .files()
                    .files()
                    .iter()
                    .map(|f| Utf8PathBuf::from_str(&format!("/{}", f.name())).unwrap())
                    .collect(),
            })
            .collect())
    }

    fn get_changes(&self, _package: &Package) -> Result<Vec<u64>, anyhow::Error> {
        anyhow::bail!("Changes not implemented for AlpmDb");
    }

    fn default_path(&self) -> Utf8PathBuf {
        Utf8PathBuf::from_str(Self::DEFAULT_PATH).unwrap()
    }
}

impl PackageDatabaseWithDefaultPath for AlpmDb {
    const DEFAULT_PATH: &'static str = "/var/lib/pacman";
}

#[cfg(test)]
mod tests {
    use std::{fs::File, str::FromStr};

    use camino::Utf8PathBuf;

    use crate::pkgdb::{PackageDatabase, archlinux::AlpmDb};

    #[test]
    fn testit() {
        let test = AlpmDb::new(
            &Utf8PathBuf::from_str("/").unwrap(),
            &Utf8PathBuf::from_str("/var/lib/pacman").unwrap(),
        )
        .unwrap();
        let packages = test.get_packages().unwrap();
        for package in &packages {
            println!("Package: {}", package.name);
            test.get_changes(package).unwrap();
        }
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let filename = format!("pkg-{}.json", timestamp);
        let out = File::create(&filename).unwrap();
        serde_json::to_writer(out, &packages).unwrap();
    }
}
