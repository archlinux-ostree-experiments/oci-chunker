use std::{collections::HashMap, fs::File};

use camino::{Utf8Path, Utf8PathBuf};
use cap_std::fs_utf8::Dir;
use clap::{Args, Parser, ValueEnum};

#[cfg(feature = "archlinux")]
use crate::pkgdb::archlinux::AlpmDb;
use crate::{
    pkgdb::{postprocessing::Postprocessing, rpm::RpmDb, PackageDatabase, PackageDatabaseWithDefaultPath, PackageIndex},
    rpm_ostree::run_with_mount,
};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub(crate) enum PackageBackend {
    Rpm,
    #[cfg(feature = "archlinux")]
    Alpm,
}

impl PackageBackend {
    pub(crate) fn get_backend(
        &self,
        sysroot: &Utf8Path,
        pkgdb_path: Option<&Utf8Path>,
    ) -> Result<Box<dyn PackageDatabase>, anyhow::Error> {
        match self {
            PackageBackend::Rpm => Ok(Box::new(RpmDb::new(
                sysroot.join(pkgdb_path.unwrap_or(RpmDb::DEFAULT_PATH.as_ref())),
            ))),
            #[cfg(feature = "archlinux")]
            PackageBackend::Alpm => Ok(Box::new(AlpmDb::new(
                sysroot,
                pkgdb_path.unwrap_or(AlpmDb::DEFAULT_PATH.as_ref()),
            )?)),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub(crate) enum ChangelogSource {
    PackageDatabase,
    PreviousIndex,
    Initialize,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub(crate) enum ChangelogResolution {
    Daily,
    Weekly,
    Monthly,
}

#[derive(Args, Debug)]
pub(crate) struct BuildPackageIndexOpts {
    #[clap(long, required = false, default_value = "rpm")]
    pub backend: PackageBackend,
    #[clap(
        long,
        required_unless_present = "image",
        help = "path to a rootfs containing the package manager database"
    )]
    pub sysroot: Option<Utf8PathBuf>,
    #[clap(
        long,
        required_unless_present = "sysroot",
        help = "path to a container image in container-storage containing the package manager database"
    )]
    pub image: Option<String>,
    #[clap(
        long,
        required = false,
        help = "path to the package manager database inside the image/rootfs"
    )]
    pub pkgdb_path: Option<Utf8PathBuf>,
    #[clap(long, required = false, default_value = "previous-index")]
    pub changelog_source: ChangelogSource,
    #[clap(long, required = false, default_value = "weekly")]
    pub changelog_resolution: ChangelogResolution,
    #[clap(long, required = false)]
    pub previous_package_index: Option<Utf8PathBuf>,
    #[clap(long, required = false)]
    pub output_package_index: Option<Utf8PathBuf>,
    #[clap(
        long,
        required = false,
        help = "YAML file with postprocessing information (add and merge packages)"
    )]
    pub postprocessing: Option<Utf8PathBuf>,
    #[clap(long, required = false)]
    pub output_ostree_ext_metadata: Option<Utf8PathBuf>,
}

impl BuildPackageIndexOpts {
    pub(crate) fn run(&self) -> Result<(), anyhow::Error> {
        run_with_mount(
            |dir| self.run_with_sysroot(dir),
            self.sysroot.clone(),
            self.image.clone(),
        )
    }

    fn run_with_sysroot(&self, sysroot: Dir) -> Result<(), anyhow::Error> {
        // We use the current build time as an ID for the changelog, if it is not populated from the package database.
        let build_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let backend = self.backend.get_backend(
            &sysroot.canonicalize(".")?,
            self.pkgdb_path.as_ref().map(|p| p.as_ref()),
        )?;
        let packages = backend.get_packages()?;
        let packages = match self.postprocessing {
            Some(ref postprocessing_path) => {
                let postprocessing = Postprocessing::new_from_toml(postprocessing_path)?;
                postprocessing.apply(packages)?
            }
            None => packages
        };
        let packages = match self.changelog_source {
            ChangelogSource::PackageDatabase => packages
                .into_iter()
                .map(|package| -> Result<PackageIndex, anyhow::Error> {
                    let changelog = backend.get_changes(&package)?;
                    let changelog_len = u32::try_from(changelog.len()).unwrap();
                    Ok(PackageIndex::new(package, changelog, changelog_len))
                })
                .collect::<Result<Vec<PackageIndex>, anyhow::Error>>()?,
            ChangelogSource::PreviousIndex => {
                let previous_package_metadata = if let Some(previous_package_metadata) =
                    &self.previous_package_index
                {
                    serde_json::from_reader::<_, Vec<PackageIndex>>(File::open(
                        previous_package_metadata,
                    )?)?
                } else {
                    anyhow::bail!(
                        "Obtaining changelog from previous index file requested, but no previous index file was specified"
                    );
                };
                let mut previous_package_metadata = previous_package_metadata
                    .into_iter()
                    .map(|package| (package.package.name.clone(), package))
                    .collect::<HashMap<String, PackageIndex>>();
                packages
                    .into_iter()
                    .map(|package| {
                        let previous_version = previous_package_metadata.remove(&package.name);
                        match previous_version {
                            Some(metadata) => PackageIndex::update_from_previous_index(
                                package, metadata, build_time,
                            ),
                            None => PackageIndex::initialize(package, build_time),
                        }
                    })
                    .collect()
            }
            ChangelogSource::Initialize => packages
                .into_iter()
                .map(|package| PackageIndex::initialize(package, build_time))
                .collect(),
        };
        Ok(())
    }
}
