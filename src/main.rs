use crate::cloud::{Cloud, CloudError, Server};
use crate::config::{Config, ConfigError, ServerConfig};
use crate::dns::{DynDnsClient, DynDnsError};
use crate::rcon::Rcon;
use crate::ssh::SshError;
use chrono::Utc;
use cron::Schedule;
use ssh::SshSession;
use std::env::args;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::select;
use tokio::signal::ctrl_c;
use tokio::time::sleep;

mod cloud;
mod config;
mod dns;
mod rcon;
mod ssh;

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
    #[error("Error while updating dyndns: {0}")]
    DynDns(#[from] DynDnsError),
    #[error("Already running")]
    AlreadyRunning,
    #[error("{0}")]
    Schedule(#[from] cron::error::Error),
    #[error("{0}")]
    Rcon(#[from] ::rcon::Error),
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

    ssh.exec("dd if=/dev/zero of=/swapfile bs=1M count=1024")
        .await?;
    ssh.exec("chmod 600 /swapfile && mkswap /swapfile && swapon /swapfile")
        .await?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    pretty_env_logger::init();

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

    let start_schedule = Schedule::from_str(&config.schedule.start)?;
    let stop_schedule = Schedule::from_str(&config.schedule.stop)?;

    select! {
        _ = run_loop(cloud, config, start_schedule, stop_schedule) => {},
        _ = ctrl_c() => {},
    }

    Ok(())
}

async fn run_loop(
    cloud: Arc<dyn Cloud>,
    config: Config,
    start_schedule: Schedule,
    stop_schedule: Schedule,
) {
    let mut active_server = if config.server.manage_existing {
        cloud.list().await.into_iter().flatten().next()
    } else {
        None
    };

    loop {
        let next_start = start_schedule.upcoming(Utc).next().unwrap();
        let next_stop = stop_schedule.upcoming(Utc).next().unwrap();

        // we're between start time and stop time
        if active_server.is_none() && next_start > next_stop {
            println!("Starting server");
            match start(cloud.as_ref(), &config).await {
                Ok(server) => active_server = Some(server),
                Err(e) => eprintln!("{:#}", e),
            };
        }

        // we're between stop time and start time
        if active_server.is_some() && next_stop > next_start {
            let active_players_res = match Rcon::new(
                (active_server.as_ref().unwrap().ip, 27015),
                &config.server.rcon,
            )
            .await
            {
                Ok(mut rcon) => rcon.player_count().await,
                Err(e) => Err(e),
            };
            let stop = match active_players_res {
                Ok(0) => true,
                Ok(count) => {
                    println!(
                        "Want to stop server, but there are still {} active players",
                        count
                    );
                    false
                }
                Err(e) => {
                    eprintln!("{}", e);
                    true
                }
            };
            if stop {
                let id = &active_server.as_ref().unwrap().id;
                println!("Stopping server {}", id);
                match cloud.kill(&id).await {
                    Ok(_) => {
                        active_server = None;
                    }
                    Err(e) => eprintln!("{:#}", e),
                }
            }
        }

        sleep(Duration::from_secs(60)).await;
    }
}

async fn start(cloud: &dyn Cloud, config: &Config) -> Result<Server, Error> {
    let list = cloud.list().await?;
    if !list.is_empty() {
        eprintln!("Non empty server list while starting: {:?}", list);
        return Err(Error::AlreadyRunning);
    }

    let ssh_key = if let Some(key) = config.server.ssh_key.as_ref() {
        Some(cloud.get_ssh_key_id(key).await?)
    } else {
        None
    };

    let created = cloud.spawn(ssh_key.as_deref()).await?;
    let server = cloud.wait_for_ip(&created.id).await?;

    println!("Server is booting");
    println!("  IP: {}", server.ip);
    println!("  Root Password: {}", created.password);

    let connect_host = if let Some(dns_config) = config.dyndns.as_ref() {
        let dns = DynDnsClient::new(
            dns_config.update_url.to_string(),
            dns_config.username.to_string(),
            dns_config.password.to_string(),
        );
        println!(
            "Updating DynDNS entry for {} to {}",
            dns_config.hostname, server.ip
        );
        dns.update(&dns_config.hostname, server.ip).await?;
        dns_config.hostname.to_string()
    } else {
        format!("{}", server.ip)
    };

    let mut ssh = SshSession::open(server.ip, &created.password).await?;
    setup(&mut ssh, &config.server).await?;
    ssh.close().await?;

    println!("Server has been setup and is starting");
    println!("Connect using");
    println!(
        "  connect {}; password {}",
        connect_host, config.server.password
    );
    Ok(server)
}
