pub mod cloud;
pub mod config;

use crate::cloud::CloudError;
use crate::config::{Config, ConfigError};
use std::env::args;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Error while interacting with cloud provider: {0}")]
    Cloud(#[from] CloudError),
    #[error("Error while loading configuration: {0}")]
    Config(#[from] ConfigError),
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let mut args = args();
    let bin = args.next().unwrap();

    let config = match args.next() {
        Some(file) => Config::from_file(file)?,
        None => {
            eprintln!("Usage {} <config.toml>", bin);
            return Ok(());
        }
    };
    let cloud = config.cloud()?;

    let created = dbg!(cloud.spawn().await?);
    let server = cloud.wait_for_ip(&created.id).await?;
    println!("IP: {}", server.ip);
    println!("Password: {}", created.password);
    dbg!(
        cloud
            .setup(&created.id, &created.password, &config.server)
            .await?
    );
    Ok(())
}
