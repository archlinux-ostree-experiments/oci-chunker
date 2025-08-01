mod pkgdb;
mod rpm_ostree;

use clap::Parser;
use pkgdb::*;
use rpm_ostree::*;

// #[tokio::main]
// async fn main() -> Result<(), anyhow::Error> {
//     tracing_subscriber::fmt()
//         .with_max_level(tracing::Level::TRACE)
//         .with_target(false)
//         .init();
//     let builder = BuildChunkedOCIOpts::parse();
//     {
//         let proxy = containers_image_proxy::ImageProxy::new().await?;
//         let image = proxy.open_image("docker://ghcr.io/archlinux-ostree-experiments/archlinux-bootc:main").await?;
//         println!("Image: {:?}", image);
//         drop(image);
//         drop(proxy);
//     }
//     builder.run().unwrap();
//     Ok(())
// }

async fn async_main() -> Result<(), tokio::task::JoinError> {
    tokio::task::spawn_blocking(|| {
        let repo = BuildChunkedOCIOpts::parse().run().unwrap();
        println!("repo: {:?}", repo);

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
    //BuildChunkedOCIOpts::parse().run()?;
    Ok(())
}
