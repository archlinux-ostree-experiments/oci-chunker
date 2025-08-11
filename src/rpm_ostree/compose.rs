//! Logic for server-side builds; corresponds to rpmostree-builtin-compose-tree.cxx

// SPDX-License-Identifier: Apache-2.0 OR MIT

use std::ffi::OsStr;
use std::num::NonZeroU32;
use std::os::fd::{AsFd, AsRawFd};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow};
use camino::Utf8PathBuf;
use cap_std::fs::{Dir, MetadataExt};
use clap::Parser;
use fn_error_context::context;
use oci_spec::image::ImageManifest;
use ostree::gio;
use ostree_ext::containers_image_proxy;
use ostree_ext::glib::prelude::*;
use ostree_ext::oci_spec::image::ImageConfiguration;
use ostree_ext::ostree::MutableTree;
use ostree_ext::{container as ostree_container, glib};
use ostree_ext::{oci_spec, ostree};
use tempfile::{NamedTempFile, TempDir, TempPath};

use crate::rpm_ostree::cmdutils::CommandRunExt;
use crate::rpm_ostree::containers_storage::Mount;

const SYSROOT: &str = "sysroot";
const USR: &str = "usr";
const ETC: &str = "etc";
const USR_ETC: &str = "usr/etc";

#[derive(clap::ValueEnum, Clone, Debug)]
enum OutputFormat {
    Ociarchive,
    Oci,
    Registry,
}

impl Default for OutputFormat {
    fn default() -> Self {
        Self::Ociarchive
    }
}

impl From<OutputFormat> for ostree_container::Transport {
    fn from(val: OutputFormat) -> Self {
        match val {
            OutputFormat::Ociarchive => ostree_container::Transport::OciArchive,
            OutputFormat::Oci => ostree_container::Transport::OciDir,
            OutputFormat::Registry => ostree_container::Transport::Registry,
        }
    }
}

#[derive(Debug)]
pub struct OstreeRepoFromContainerResult {
    repodir: TempDir,
    config_data: Option<TempPath>,
    commitid: String,
    manifest_data: Option<NamedTempFile>,
}

impl OstreeRepoFromContainerResult {
    pub fn repodir(&self) -> PathBuf {
        self.repodir.path().to_path_buf()
    }

    pub fn config_data(&self) -> Option<PathBuf> {
        self.config_data.as_ref().map(|t| t.to_path_buf())
    }

    pub fn commitid(&self) -> &str {
        &self.commitid
    }

    pub fn manifest_data(&self) -> Option<PathBuf> {
        self.manifest_data.as_ref().map(|s| s.path().to_path_buf())
    }
}

/// Generate a "chunked" OCI archive from an input rootfs.
#[derive(Debug, Parser)]
pub(crate) struct BuildChunkedOCIOpts {
    /// Path to the source root filesystem tree.
    #[clap(long, required_unless_present = "from")]
    rootfs: Option<Utf8PathBuf>,

    /// Use the provided image (in containers-storage).
    #[clap(long, required_unless_present = "rootfs")]
    from: Option<String>,

    /// Output image reference, in TRANSPORT:TARGET syntax.
    /// For example, `containers-storage:localhost/exampleos` or `oci:/path/to/ocidir`.
    #[clap(long, required = true)]
    output: Utf8PathBuf,
}

impl BuildChunkedOCIOpts {
    pub(crate) fn run(self) -> Result<OstreeRepoFromContainerResult> {
        enum FileSource {
            Rootfs(Utf8PathBuf),
            Podman(Mount),
        }

        //let existing_manifest = self.check_existing_image(&self.output)?;

        let rootfs_source = if let Some(rootfs) = self.rootfs {
            FileSource::Rootfs(rootfs)
        } else {
            let image = self.from.as_deref().unwrap();
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
        let rootfs = Dir::open_ambient_dir(rootfs, cap_std::ambient_authority())
            .with_context(|| format!("Opening {}", rootfs))?;

        // If we're deriving from an existing image, be sure to preserve its metadata (labels, creation time, etc.)
        // by default.
        let image_config: oci_spec::image::ImageConfiguration =
            if let Some(image) = self.from.as_deref() {
                let img_transport = format!("containers-storage:{image}");
                Command::new("skopeo")
                    .args(["inspect", "--config", img_transport.as_str()])
                    .run_and_parse_json()
                    .context("Invoking skopeo to inspect config")?
            } else {
                // If we're not deriving, then we take the timestamp of the root
                // directory as a creation timestamp.
                let toplevel_ts = rootfs.dir_metadata()?.modified()?.into_std();
                let toplevel_ts = chrono::DateTime::<chrono::Utc>::from(toplevel_ts)
                    .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
                let mut config = ImageConfiguration::default();
                config.set_created(Some(toplevel_ts));
                config
            };
        let arch = image_config.architecture();
        let creation_timestamp = image_config
            .created()
            .as_deref()
            .map(chrono::DateTime::parse_from_rfc3339)
            .transpose()?;

        let repo = ostree::Repo::create_at(
            libc::AT_FDCWD,
            self.output.as_str(),
            ostree::RepoMode::BareUser,
            None,
            gio::Cancellable::NONE,
        )?;

        println!("Generating commit...");
        // It's only the tests that override
        let modifier =
            ostree::RepoCommitModifier::new(ostree::RepoCommitModifierFlags::empty(), None);
        // Process the filesystem, generating an ostree commit
        let commitid =
            generate_commit_from_rootfs(&repo, &rootfs, modifier, creation_timestamp.as_ref())?;

        let label_arg = ["--label", "containers.bootc=1"].as_slice();
        let base_config = image_config
            .config()
            .as_ref()
            .filter(|_| self.from.is_some());
        let config_data = if let Some(config) = base_config {
            let mut tmpf = tempfile::NamedTempFile::new()?;
            serde_json::to_writer(&mut tmpf, &config)?;
            Some(tmpf.into_temp_path())
        } else {
            None
        };

        // let manifest_data_tmpfile = if let Some(manifest) = existing_manifest.as_ref() {
        //     let mut tmpf = tempfile::NamedTempFile::new()?;
        //     serde_json::to_writer(&mut tmpf, &manifest)?;
        //     Some(tmpf)
        // } else {
        //     None
        // };


        drop(rootfs);
        match rootfs_source {
            FileSource::Rootfs(_) => {}
            FileSource::Podman(mnt) => {
                mnt.unmount().context("Final mount cleanup")?;
            }
        }

        Ok(OstreeRepoFromContainerResult {
            repodir: TempDir::new().unwrap(),
            config_data,
            commitid,
            manifest_data: None,
        })
    }

    /// Check if there's already an image at the target location and if it's chunked
    fn check_existing_image(&self, output: &str) -> Result<Option<oci_spec::image::ImageManifest>> {
        // Parse the output reference to determine transport and location
        let (_transport, _location) = output
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("Invalid output format, expected TRANSPORT:TARGET"))?;

        let handle = tokio::runtime::Handle::current();
        let result: Option<oci_spec::image::ImageManifest> = handle.block_on(async {
            // Create image proxy without specific authfile config since BuildChunkedOCIOpts doesn't have authfile field
            // The proxy will use default authentication sources (e.g., $XDG_RUNTIME_DIR/containers/auth.json)
            let proxy = containers_image_proxy::ImageProxy::new().await?;

            tracing::debug!("Open Image: {}", output);
            // Try to open the image
            let img = proxy
                .open_image_optional(output)
                .await
                .map_err(|e| tracing::warn!("Failed to open output image: {}", e))
                .ok()
                .flatten();

            if let Some(opened_image) = img {
                // Fetch the manifest
                let (_, manifest) = proxy.fetch_manifest(&opened_image).await?;
                anyhow::Ok(Some(manifest))
            } else {
                // Image doesn't exist
                anyhow::Ok(None)
            }
        })?;

        if let Some(manifest) = result {
            // Check if all layers have ostree.components annotation (skip first layer)
            let is_chunked = manifest.layers().iter().skip(1).all(|layer| {
                layer
                    .annotations()
                    .as_ref()
                    .and_then(|annotations| annotations.get("ostree.components"))
                    .is_some()
            });

            if is_chunked {
                println!("Found existing chunked image at target, will use as baseline");
                Ok(Some(manifest))
            } else {
                println!(
                    "Found existing image at target but it's not chunked, will create new image"
                );
                Ok(None)
            }
        } else {
            // Image doesn't exist, return None
            Ok(None)
        }
    }
}

fn label_to_xattrs(label: Option<&str>) -> Option<glib::Variant> {
    let xattrs = label.map(|label| {
        let mut label: Vec<_> = label.to_owned().into();
        label.push(0);
        vec![(c"security.selinux".to_bytes_with_nul(), label)]
    });
    xattrs.map(|x| x.to_variant())
}

fn create_root_dirmeta(root: &Dir, policy: &ostree::SePolicy) -> Result<glib::Variant> {
    let finfo = gio::FileInfo::new();
    let meta = root.dir_metadata()?;
    finfo.set_attribute_uint32("unix::uid", 0);
    finfo.set_attribute_uint32("unix::gid", 0);
    finfo.set_attribute_uint32("unix::mode", libc::S_IFDIR | meta.mode());
    let label = policy.label("/", 0o777 | libc::S_IFDIR, gio::Cancellable::NONE)?;
    let xattrs = label_to_xattrs(label.as_deref());
    let r = ostree::create_directory_metadata(&finfo, xattrs.as_ref());
    Ok(r)
}

enum MtreeEntry {
    #[allow(dead_code)]
    Leaf(String),
    Directory(MutableTree),
}

impl MtreeEntry {
    fn require_dir(self) -> Result<MutableTree> {
        match self {
            MtreeEntry::Leaf(_) => anyhow::bail!("Expected a directory"),
            MtreeEntry::Directory(t) => Ok(t),
        }
    }
}

/// The two returns value in C are mutually exclusive; also map "not found" to None.
fn mtree_lookup(t: &ostree::MutableTree, path: &str) -> Result<Option<MtreeEntry>> {
    let r = match t.lookup(path) {
        Ok((Some(leaf), None)) => Some(MtreeEntry::Leaf(leaf.into())),
        Ok((_, Some(subdir))) => Some(MtreeEntry::Directory(subdir)),
        Ok((None, None)) => unreachable!(),
        Err(e) if e.matches(gio::IOErrorEnum::NotFound) => None,
        Err(e) => return Err(e.into()),
    };
    Ok(r)
}

// Given a root filesystem, perform some in-memory postprocessing.
// At the moment, that's just ensuring /etc is /usr/etc.
#[context("Postprocessing commit")]
fn postprocess_mtree(repo: &ostree::Repo, rootfs: &ostree::MutableTree) -> Result<()> {
    let etc_subdir = mtree_lookup(rootfs, ETC)?
        .map(|e| e.require_dir().context("/etc"))
        .transpose()?;
    let usr_etc_subdir = mtree_lookup(rootfs, USR_ETC)?
        .map(|e| e.require_dir().context("/usr/etc"))
        .transpose()?;
    match (etc_subdir, usr_etc_subdir) {
        (None, None) => {
            // No /etc? We'll let you try it.
        }
        (None, Some(_)) => {
            // Having just /usr/etc is the expected ostree default.
        }
        (Some(etc), None) => {
            // We need to write the etc dir now to generate checksums,
            // then move it.
            repo.write_mtree(&etc, gio::Cancellable::NONE)?;
            let usr = rootfs
                .lookup(USR)?
                .1
                .ok_or_else(|| anyhow!("Missing /usr"))?;
            let usretc = usr.ensure_dir(ETC)?;
            usretc.set_contents_checksum(&etc.contents_checksum());
            usretc.set_metadata_checksum(&etc.metadata_checksum());
            rootfs.remove(ETC, false)?;
        }
        (Some(_), Some(_)) => {
            anyhow::bail!("Found both /etc and /usr/etc");
        }
    }
    Ok(())
}

#[context("Generating commit from rootfs")]
fn generate_commit_from_rootfs(
    repo: &ostree::Repo,
    rootfs: &Dir,
    modifier: ostree::RepoCommitModifier,
    creation_time: Option<&chrono::DateTime<chrono::FixedOffset>>,
) -> Result<String> {
    let root_mtree = ostree::MutableTree::new();
    let cancellable = gio::Cancellable::NONE;
    let tx = repo.auto_transaction(cancellable)?;

    let policy = ostree::SePolicy::new_at(rootfs.as_fd().as_raw_fd(), cancellable)?;
    modifier.set_sepolicy(Some(&policy));

    let root_dirmeta = create_root_dirmeta(rootfs, &policy)?;
    let root_metachecksum = repo
        .write_metadata(
            ostree::ObjectType::DirMeta,
            None,
            &root_dirmeta,
            cancellable,
        )
        .context("Writing root dirmeta")?;
    root_mtree.set_metadata_checksum(&root_metachecksum.to_hex());

    for ent in rootfs.entries_utf8()? {
        let ent = ent?;
        let name = ent.file_name()?;

        let ftype = ent.file_type()?;
        // Skip the contents of the sysroot
        if ftype.is_dir() && name == SYSROOT {
            let child_mtree = root_mtree.ensure_dir(&name)?;
            child_mtree.set_metadata_checksum(&root_metachecksum.to_hex());
        } else if ftype.is_dir() {
            let child_mtree = root_mtree.ensure_dir(&name)?;
            let child = ent.open_dir()?;
            repo.write_dfd_to_mtree(
                child.as_raw_fd(),
                ".",
                &child_mtree,
                Some(&modifier),
                cancellable,
            )
            .with_context(|| format!("Processing dir {name}"))?;
        } else if ftype.is_symlink() {
            let contents: Utf8PathBuf = rootfs
                .read_link_contents(&name)
                .with_context(|| format!("Reading {name}"))?
                .try_into()?;
            // Label lookups need to be absolute
            let selabel_path = format!("/{name}");
            let label = policy.label(selabel_path.as_str(), 0o777 | libc::S_IFLNK, cancellable)?;
            let xattrs = label_to_xattrs(label.as_deref());
            let link_checksum = repo
                .write_symlink(None, 0, 0, xattrs.as_ref(), contents.as_str(), cancellable)
                .with_context(|| format!("Processing symlink {selabel_path}"))?;
            root_mtree.replace_file(&name, &link_checksum)?;
        } else {
            // Yes we could support this but it's a surprising amount of typing
            anyhow::bail!("Unsupported regular file {name} at toplevel");
        }
    }

    postprocess_mtree(repo, &root_mtree)?;

    let ostree_root = repo.write_mtree(&root_mtree, cancellable)?;
    let ostree_root = ostree_root.downcast_ref::<ostree::RepoFile>().unwrap();
    let creation_time: u64 = creation_time
        .as_ref()
        .map(|t| t.timestamp())
        .unwrap_or_default()
        .try_into()
        .context("Parsing creation time")?;
    let commit = repo.write_commit_with_time(
        None,
        None,
        None,
        None,
        ostree_root,
        creation_time,
        cancellable,
    )?;

    tx.commit(cancellable)?;
    Ok(commit.into())
}
