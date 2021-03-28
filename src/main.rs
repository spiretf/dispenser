use std::env::args;

use thiserror::Error;

use ssh::SshSession;

use crate::cloud::CloudError;
use crate::config::{Config, ConfigError, ServerConfig};
use crate::ssh::SshError;
use std::time::Duration;
use tokio::time::sleep;

pub mod cloud;
pub mod config;
pub mod ssh;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Error while interacting with cloud provider: {0}")]
    Cloud(#[from] CloudError),
    #[error("Error while loading configuration: {0}")]
    Config(#[from] ConfigError),
    #[error("Error while trying to connect trough ssh: {0}")]
    Ssh(#[from] SshError),
    #[error("Setup command returned an error: {0}")]
    SetupError(String),
}

async fn setup(ssh: &mut SshSession, config: &ServerConfig) -> Result<(), Error> {
    let mut tries = 0;
    loop {
        tries += 1;
        sleep(Duration::from_secs(1)).await;
        let result = ssh.exec("docker pull spiretf/docker-spire-server").await?;
        if result.success() {
            break;
        } else if tries > 5 {
            return Err(Error::SetupError(result.output()));
        }
    }
    let result = ssh
        .exec(format!(
            "docker run --name spire -d \
            -e NAME={name} -e TV_NAME={tv_name} -e PASSWORD={password} -e RCON_PASSWORD={rcon} \
            -e DEMOSTF_APIKEY={demostf} -e LOGSTF_APIKEY={logstf} \
            -e CONFIG_LEAGUE={league} -e CONFIG_MODE={mode} \
            -p 27015:27015 -p 27021:27021 -p 27015:27015/udp -p 27020:27020/udp -p 27025:27025 \
            -p 28015:27015 -p 28015:27015/udp -p 27115:27015 -p 27115:27015/udp -p 27215:27015 \
            -p 27215:27015/udp -p 27315:27015 -p 27315:27015/udp -p 27415:27015 -p 27415:27015/udp \
            -p 27515:27015 -p 27515:27015/udp -p 27615:27015 -p 27615:27015/udp -p 27715:27015 \
            -p 27715:27015/udp -p 27815:27015 -p 27815:27015/udp -p 27915:27015 -p 27915:27015/udp \
            {image}
            ",
            name = config.name,
            tv_name = config.tv_name,
            password = config.password,
            rcon = config.rcon,
            demostf = config
                .demostf_key
                .as_ref()
                .map(String::as_str)
                .unwrap_or_default(),
            logstf = config
                .logstf_key
                .as_ref()
                .map(String::as_str)
                .unwrap_or_default(),
            league = config.config_league,
            mode = config.config_mode,
            image = config.image
        ))
        .await?;

    if !result.success() {
        return Err(Error::SetupError(result.output()));
    }
    Ok(())
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

    let created = cloud.spawn().await?;
    let server = cloud.wait_for_ip(&created.id).await?;

    println!("Server is booting");
    println!("  IP: {}", server.ip);
    println!("  Password: {}", created.password);

    let mut ssh = SshSession::open(server.ip, &created.password).await?;
    setup(&mut ssh, &config.server).await?;
    ssh.close().await?;

    println!("Server has been setup and is starting");
    println!("Connect using");
    println!(
        "  connect {}; password {}",
        server.ip, config.server.password
    );

    Ok(())
}
