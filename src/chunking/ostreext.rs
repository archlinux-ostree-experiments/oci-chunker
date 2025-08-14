use std::num::NonZero;

use camino::Utf8Path;
use ostree_ext::chunking::ObjectMetaSized;

use crate::{
    chunking::Chunker,
    pkgdb::PackageIndex,
    rpm_ostree::{generate_mapping, open_ostree},
};

pub(crate) struct OstreeExtChunker;

impl OstreeExtChunker {
    pub fn new() -> Self {
        OstreeExtChunker {}
    }
}

impl Chunker for OstreeExtChunker {
    fn chunk(
        &mut self,
        packages: &Vec<PackageIndex>,
        _max_layers: NonZero<u32>,
        repo: &Utf8Path,
        commit: &str,
    ) -> Result<ObjectMetaSized, anyhow::Error> {
        let (repo, root, _rev) = open_ostree(repo, commit)?;
        let meta = generate_mapping(&repo, &root, packages)?;
        Ok(meta)
    }
}
