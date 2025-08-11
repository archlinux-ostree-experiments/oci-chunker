mod chunking;
mod pkgdb;
mod rpm_ostree;

use clap::{Parser, Subcommand};
use rpm_ostree::*;

use crate::pkgdb::cli::BuildPackageIndexOpts;

#[derive(Debug, Subcommand)]
enum Subcommands {
    BuildChunkedOCI(BuildChunkedOCIOpts),
    BuildPackageIndex(BuildPackageIndexOpts),
}

#[derive(Debug, Parser)]
#[command(version, about, long_about = None)]
#[command(propagate_version = true)]
struct CliArgs {
    #[command(subcommand)]
    subcommand: Subcommands,
}

async fn async_main() -> Result<(), tokio::task::JoinError> {
    tokio::task::spawn_blocking(|| {
        let args = CliArgs::parse();
        println!("{:#?}", args);

        let mut s = String::new();
        std::io::stdin()
            .read_line(&mut s)
            .expect("Did not enter a correct string");
    })
    .await
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
