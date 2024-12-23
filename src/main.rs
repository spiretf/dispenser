extern crate core;

use crate::cloud::{Cloud, CloudError, CreatedAuth, Server};
use crate::config::{Config, ConfigError, DynDnsConfig, ServerConfig};
use crate::dns::{DynDnsClient, DynDnsError};
use crate::rcon::Rcon;
use crate::ssh::SshError;
use chrono::Utc;
use clap::{Parser, Subcommand};
use cron::Schedule;
use main_error::MainResult;
use ssh::SshSession;
use std::net::IpAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::signal::ctrl_c;
use tokio::time::sleep;
use tokio::{select, spawn};
use tracing::{debug, error, info, instrument, warn};

mod cloud;
mod config;
mod dns;
mod rcon;
mod ssh;

/// Manage ephemeral tf2 servers
#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
struct Args {
    #[clap(subcommand)]
    command: Option<Commands>,
    config: String,
}

#[derive(Subcommand, Default)]
enum Commands {
    /// Start a new server if none is running
    Start,
    /// Start the server if one is running
    Stop,
    /// List running servers
    List,
    /// Run the management daemon
    #[default]
    Daemon,
}

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
    AlreadyRunning(Server),
    #[error("{0}")]
    Schedule(#[from] cron::error::Error),
    #[error("{0}")]
    Rcon(#[from] ::rcon::Error),
}

#[instrument(skip(config))]
async fn setup(
    ssh: &mut SshSession,
    config: &ServerConfig,
    hostname: Option<&str>,
) -> Result<(), Error> {
    sleep(Duration::from_secs(10)).await;

    let mut tries = 0;

    debug!(image = display(&config.image), "pulling image");
    loop {
        tries += 1;
        sleep(Duration::from_secs(2)).await;
        let result = ssh.exec(format!("docker pull {}", config.image)).await?;
        if result.success() {
            break;
        } else if tries > 5 {
            error!(
                tries = tries,
                output = display(result.output()),
                "Failed to pull docker image to many times, giving up"
            );
            return Err(Error::SetupError(result.output()));
        } else {
            error!(
                tries = tries,
                output = display(result.output()),
                "Failed to pull docker image, retrying"
            );
        }
    }

    info!("starting container");

    let cmnd = format!(
        "docker run --name spire -d \
            -e NAME={name} -e TV_NAME={tv_name} -e PASSWORD={password} -e RCON_PASSWORD={rcon} \
            -e DEMOSTF_APIKEY={demostf} -e LOGSTF_APIKEY={logstf} \
            -e CONFIG_LEAGUE={league} -e CONFIG_MODE={mode} -e 'EXTRA_CFG={extra_cfg}' \
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
        demostf = config.demostf_key.as_deref().unwrap_or_default(),
        logstf = config.logstf_key.as_deref().unwrap_or_default(),
        league = config.config_league,
        mode = config.config_mode,
        image = config.image,
        extra_cfg = config.extra_cfg,
    );

    debug!("running {cmnd}");

    let result = ssh.exec(cmnd).await?;

    if !result.success() {
        return Err(Error::SetupError(result.output()));
    }

    info!("setting up swap");
    ssh.exec("dd if=/dev/zero of=/swapfile bs=1M count=1024")
        .await?;
    ssh.exec("chmod 600 /swapfile && mkswap /swapfile && swapon /swapfile")
        .await?;

    info!("setting up prometheus");
    ssh.exec("wget https://github.com/icewind1991/palantir/raw/main/palantir.service -O /etc/systemd/system/palantir.service").await?;
    ssh.exec("wget https://github.com/icewind1991/palantir/releases/download/v1.1.0/palantir-x86_64-unknown-linux-musl -O /usr/local/bin/palantir").await?;
    ssh.exec("chmod +x /usr/local/bin/palantir").await?;
    ssh.exec(
        r#"sed -i -e "s|User=palantir|DynamicUser=true|" /etc/systemd/system/palantir.service"#,
    )
    .await?;
    ssh.exec("iptables -I INPUT -p tcp --dport 5665 -j ACCEPT")
        .await?;
    if let Some(hostname) = hostname {
        ssh.exec(&format!(
            "hostname {} && systemctl start palantir",
            hostname
        ))
        .await?;
    } else {
        ssh.exec("systemctl start palantir").await?;
    }

    Ok(())
}

#[tokio::main]
async fn main() -> MainResult {
    tracing_subscriber::fmt::init();

    let cli = Args::parse();

    let config = Config::from_file(&cli.config)?;
    let cloud = config.cloud()?;

    match cli.command.unwrap_or_default() {
        Commands::Daemon => {
            let start_schedule = Schedule::from_str(&config.schedule.start)?;
            let stop_schedule = Schedule::from_str(&config.schedule.stop)?;

            select! {
                _ = run_loop(cloud, config, start_schedule, stop_schedule) => {},
                _ = ctrl_c() => {},
            }
        }
        Commands::List => {
            let servers = cloud.list().await?;
            if servers.is_empty() {
                println!("No running server");
            } else {
                for server in servers {
                    let player_count =
                        match Rcon::new((server.ip, 27015), &config.server.rcon).await {
                            Ok(mut rcon) => rcon.player_count().await,
                            Err(e) => Err(e),
                        };

                    if let Ok(player_count) = player_count {
                        println!("{}: {} with {} players", server.id, server.ip, player_count);
                    } else {
                        println!("{}: {}", server.id, server.ip);
                    }
                }
            }
        }
        Commands::Start => {
            match start(cloud.as_ref(), &config).await {
                Ok(_) => {}
                Err(Error::AlreadyRunning(_)) => {
                    println!("Server already running");
                }
                Err(e) => eprintln!("{:#}", e),
            };
        }
        Commands::Stop => match cloud.list().await?.first() {
            Some(server) => match cloud.kill(&server.id).await {
                Ok(_) => {
                    println!("Server stopped");
                }
                Err(e) => eprintln!("{:#}", e),
            },
            None => {
                eprintln!("No server running");
            }
        },
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

    if let Some(server) = active_server.as_ref() {
        info!(
            server = debug(server),
            "Taking ownership of existing server"
        );
    }

    let mut start_of_stop_time = None;
    let stop_grace_time = Duration::from_secs(config.schedule.stop_grace_time);

    loop {
        let next_start = start_schedule.upcoming(Utc).next().unwrap();
        let next_stop = stop_schedule.upcoming(Utc).next().unwrap();

        // we're between start time and stop time
        if active_server.is_none() && next_start > next_stop {
            start_of_stop_time = None;
            println!("Starting server");
            match start(cloud.as_ref(), &config).await {
                Ok(server) => active_server = Some(server),
                Err(Error::AlreadyRunning(server)) if config.server.manage_existing => {
                    info!(
                        server = debug(&server),
                        "Taking ownership of existing server"
                    );
                    if let Some(dns_config) = config.dyndns.as_ref() {
                        spawn(set_dyndns(dns_config.clone(), server.ip));
                    }
                    active_server = Some(server);
                }
                Err(e) => eprintln!("{:#}", e),
            };
        }

        // we're between stop time and start time
        if active_server.is_some() && next_stop > next_start {
            let stop_elapsed = start_of_stop_time
                .get_or_insert_with(Instant::now)
                .elapsed();

            let stop = if stop_elapsed > stop_grace_time {
                warn!("Server took longer than the grace time of {} seconds to empty, shutting down with players left", stop_grace_time.as_secs());
                true
            } else {
                let active_players_res = match Rcon::new(
                    (active_server.as_ref().unwrap().ip, 27015),
                    &config.server.rcon,
                )
                .await
                {
                    Ok(mut rcon) => rcon.player_count().await,
                    Err(e) => Err(e),
                };
                match active_players_res {
                    Ok(0) => true,
                    Ok(count) => {
                        info!(
                            "Want to stop server, but there are still {} active players",
                            count
                        );
                        false
                    }
                    Err(e) => {
                        error!("Error while trying get player count: {}", e);
                        false
                    }
                }
            };
            if stop {
                let id = &active_server.as_ref().unwrap().id;
                println!("Stopping server {}", id);
                match cloud.kill(id).await {
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

#[instrument(skip(cloud, config))]
async fn start(cloud: &dyn Cloud, config: &Config) -> Result<Server, Error> {
    let list = cloud.list().await?;
    let count = list.len();
    let first = list.into_iter().next();
    if let Some(first) = first {
        warn!(
            "Non empty server list while starting: {:?}, and {} more",
            first,
            count - 1
        );
        return Err(Error::AlreadyRunning(first));
    }

    let created = cloud.spawn(&config.server.ssh_keys).await?;
    let server = cloud.wait_for_ip(&created.id).await?;

    println!("Server is booting");
    println!("  IP: {}", server.ip);
    println!("  Root Password: {}", created.auth);

    let connect_host = if let Some(dns_config) = config.dyndns.as_ref() {
        spawn(set_dyndns(dns_config.clone(), server.ip));
        dns_config.hostname.to_string()
    } else {
        format!("{}", server.ip)
    };

    let mut ssh = connect_ssh(server.ip, &created.auth).await?;
    setup(
        &mut ssh,
        &config.server,
        config.dyndns.as_ref().map(|dns| dns.hostname.as_str()),
    )
    .await?;
    ssh.close().await?;

    println!("Server has been setup and is starting");
    println!("Connect using");
    println!(
        "  connect {}; password {}",
        connect_host, config.server.password
    );
    Ok(server)
}

async fn set_dyndns(dns_config: DynDnsConfig, ip: IpAddr) {
    let dns = DynDnsClient::new(
        dns_config.update_url,
        dns_config.username,
        dns_config.password,
    );
    println!(
        "Updating DynDNS entry for {} to {}",
        dns_config.hostname, ip
    );
    if let Err(e) = dns.update(&dns_config.hostname, ip).await {
        eprintln!("Error while updating DynDNS: {}", e);
    }
}

async fn connect_ssh(ip: IpAddr, auth: &CreatedAuth) -> Result<SshSession, Error> {
    let mut tries = 0;

    loop {
        tries += 1;
        sleep(Duration::from_secs(5)).await;

        match SshSession::open(ip, auth).await {
            Ok(ssh) => {
                return Ok(ssh);
            }
            Err(e) if tries > 5 => {
                error!(
                    tries = tries,
                    error = %e,
                    "Failed to connect to ssh to many times, giving up"
                );
                return Err(e.into());
            }
            Err(e) => {
                warn!(tries = tries, error = %e, "Failed to connect to ssh");
            }
        }
    }
}
