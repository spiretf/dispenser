use crate::cloud::{Cloud, CloudError};
use crate::config::{Config, ConfigError, ServerConfig};
use crate::dns::{DynDnsClient, DynDnsError};
use crate::ssh::SshError;
use ssh::SshSession;
use std::env::args;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use thiserror::Error;
use tokio::task::{spawn, JoinError};
use tokio::time::sleep;
use tokio_cron_scheduler::{Job, JobScheduler};

mod cloud;
mod config;
mod dns;
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
    Schedule(ScheduleError),
}

#[derive(Debug, Error)]
#[error("{0}")]
pub struct ScheduleError(ScheduleErrorImpl);

#[derive(Debug, Error)]
enum ScheduleErrorImpl {
    #[error("Error setting up schedule")]
    Schedule(String),
    #[error("Error running schedule")]
    Join(JoinError),
}

impl From<ScheduleErrorImpl> for Error {
    fn from(e: ScheduleErrorImpl) -> Self {
        Error::Schedule(ScheduleError(e))
    }
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

    let mut sched = JobScheduler::new();

    let server_id: Arc<Mutex<Option<String>>> = Arc::default();

    sched
        .add(stop_job(cloud.clone(), &config, server_id.clone()))
        .map_err(|e| ScheduleErrorImpl::Schedule(format!("{:#}", e)))?;
    sched
        .add(start_job(cloud, config, server_id))
        .map_err(|e| ScheduleErrorImpl::Schedule(format!("{:#}", e)))?;

    sched.start().await.map_err(ScheduleErrorImpl::Join)?;

    Ok(())
}

fn stop_job(cloud: Arc<dyn Cloud>, config: &Config, server_id: Arc<Mutex<Option<String>>>) -> Job {
    Job::new(&config.schedule.stop, move |_uuid, _l| {
        let server_id = server_id.clone();
        let cloud = cloud.clone();
        spawn(async move {
            let id = server_id.lock().unwrap().take();
            if let Some(id) = id {
                println!("Stopping server {}", id);
                match cloud.kill(&id).await {
                    Ok(_) => {}
                    Err(e) => eprintln!("{:#}", e),
                };
            } else {
                println!("No server to stop")
            }
        });
    })
    .unwrap()
}

fn start_job(cloud: Arc<dyn Cloud>, config: Config, server_id: Arc<Mutex<Option<String>>>) -> Job {
    let schedule = config.schedule.start.clone();
    let config = Arc::new(config);
    Job::new(&schedule, move |_uuid, _l| {
        let cloud = cloud.clone();
        let config = config.clone();
        let server_id = server_id.clone();
        spawn(async move {
            let cloud = cloud.as_ref();
            let already_started = { server_id.lock().unwrap().is_some() };
            if !already_started {
                println!("Starting server");
                match start(cloud, &config).await {
                    Ok(id) => *server_id.lock().unwrap() = Some(id),
                    Err(e) => eprintln!("{:#}", e),
                };
            }
        });
    })
    .unwrap()
}

async fn start(cloud: &dyn Cloud, config: &Config) -> Result<String, Error> {
    let list = cloud.list().await?;
    if !list.is_empty() {
        return Err(Error::AlreadyRunning);
    }
    let created = cloud.spawn().await?;
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
    Ok(server.id)
}
