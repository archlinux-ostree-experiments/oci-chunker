//! This module contains code adapted from the `rpm-ostree` project
//! https://github.com/coreos/rpm-ostree
// SPDX-License-Identifier: Apache-2.0 OR MIT

mod cmdutils;
mod compose;
mod containers_storage;

use camino::{Utf8Path, Utf8PathBuf};
use containers_storage::Mount;

pub(crate) use compose::BuildChunkedOCIOpts;

pub fn run_with_mount<F: FnOnce(&Utf8Path) -> Result<T, anyhow::Error>, T>(
    run_with_mount: F,
    rootfs: Option<Utf8PathBuf>,
    from: Option<String>,
) -> Result<T, anyhow::Error> {
    enum FileSource {
        Rootfs(Utf8PathBuf),
        Podman(Mount),
    }

    let rootfs_source = if let Some(rootfs) = rootfs {
        FileSource::Rootfs(rootfs)
    } else {
        let image = from.as_deref().unwrap();
        // TODO: Fix running this inside unprivileged podman too. We'll likely need
        // to refactor things into a two-step process where we do the mount+ostree repo commit
        // in a subprocess that has the "unshare", and then the secondary main process
        // just reads/operates on that.
        // Note that this would all be a lot saner with a composefs-native container storage
        // as we could cleanly operate on that, asking c/storage to synthesize one for us.
        // crate::containers_storage::reexec_if_needed()?;
        FileSource::Podman(Mount::new_for_image(image)?)
    };
    let rootfs = match &rootfs_source {
        FileSource::Rootfs(p) => p.as_path(),
        FileSource::Podman(mnt) => mnt.path(),
    };

    let result = run_with_mount(rootfs)?;

    drop(rootfs_source);
    Ok(result)
}
