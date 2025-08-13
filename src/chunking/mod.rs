use camino::Utf8Path;
use ostree_ext::chunking::ObjectMetaSized;

use crate::pkgdb::PackageIndex;

pub(crate) mod cli;
pub(crate) mod ostreext;

pub(crate) trait Chunker {
    fn chunk(
        &mut self,
        packages: &Vec<PackageIndex>,
        max_layers: usize,
        repo: &Utf8Path,
        commit: &str,
    ) -> Result<ObjectMetaSized, anyhow::Error>;
}
