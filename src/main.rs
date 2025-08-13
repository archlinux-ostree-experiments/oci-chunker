mod chunking;
mod pkgdb;
mod rpm_ostree;
mod util;

use clap::{Parser, Subcommand};
use rpm_ostree::*;

use crate::{chunking::cli::GenerateChunkedOCIOpts, pkgdb::cli::BuildPackageIndexOpts};

#[derive(Debug, Subcommand)]
enum Subcommands {
    GenerateOstreeRepo(BuildChunkedOCIOpts),
    BuildPackageIndex(BuildPackageIndexOpts),
    GenerateChunkedOCI(GenerateChunkedOCIOpts),
}

impl Subcommands {
    pub(crate) fn run(self) -> Result<(), anyhow::Error> {
        match self {
            Subcommands::GenerateOstreeRepo(build_chunked_ociopts) => {
                build_chunked_ociopts.run().map(|_res| ())
            }
            Subcommands::BuildPackageIndex(build_package_index_opts) => {
                build_package_index_opts.run()
            }
            Subcommands::GenerateChunkedOCI(generate_chunked_ociopts) => {
                generate_chunked_ociopts.run()
            }
        }
    }
}

#[derive(Debug, Parser)]
#[command(version, about, long_about = None)]
#[command(propagate_version = true)]
struct CliArgs {
    #[command(subcommand)]
    subcommand: Subcommands,
}

async fn async_main() -> Result<(), anyhow::Error> {
    tokio::task::spawn_blocking(|| {
        let args = CliArgs::parse();
        args.subcommand.run()
    })
    .await?
}

fn main() -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_target(false)
        .init();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async_main())?;
    Ok(())
}
