use crate::cloud::ssh::{SshError, SshSession};
use crate::cloud::{Cloud, CloudError, Created, NetworkError, ResponseError, Result, Server};
use crate::config::ServerConfig;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use petname::petname;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::time::Duration;
use tokio::time::{sleep, timeout};

pub struct Vultr {
    region: String,
    plan: String,
    token: String,
    client: Client,
}

impl Vultr {
    pub fn new(token: String, region: String, plan: String) -> Self {
        Vultr {
            token,
            region,
            plan,
            client: Client::default(),
        }
    }
}

#[async_trait]
impl Cloud for Vultr {
    async fn list(&self) -> Result<Vec<Server>> {
        let response = self
            .client
            .get("https://api.vultr.com/v2/instances")
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(NetworkError::from)?;
        CloudError::from_status_code(response.status())?;

        let response: VultrListResponse = response.json().await.map_err(ResponseError::from)?;

        Ok(response
            .instances
            .into_iter()
            .filter(|instance| instance.tag == "spire")
            .map(Server::from)
            .collect())
    }

    async fn spawn(&self) -> Result<Created> {
        let response = self
            .client
            .post("https://api.vultr.com/v2/instances")
            .bearer_auth(&self.token)
            .json(&VultrCreateParams {
                region: self.region.as_str(),
                plan: self.plan.as_str(),
                tag: "spire",
                label: petname(2, "-"),
                app_id: self.get_app_id("docker").await?,
            })
            .send()
            .await
            .map_err(NetworkError::from)?;
        CloudError::from_status_code(response.status())?;

        if response.status().is_success() {
            let response: VultrCreateResponse =
                response.json().await.map_err(ResponseError::from)?;
            Ok(response.instance.into())
        } else {
            Err(ResponseError::Other(response.text().await.map_err(NetworkError::from)?).into())
        }
    }

    async fn kill(&self, _id: &str) -> Result<()> {
        todo!()
    }

    async fn wait_for_ip(&self, id: &str) -> Result<Server> {
        let instance = loop {
            let instance = self.get_instance(id).await?;
            if !instance.main_ip.is_unspecified() {
                break instance;
            } else {
                sleep(Duration::from_millis(500)).await;
            }
        };
        Ok(instance.into())
    }

    async fn setup(
        &self,
        id: &str,
        password: &str,
        config: &ServerConfig,
    ) -> Result<Server, CloudError> {
        let server = self.wait_for_ip(id).await?;
        let ip = server.ip;
        let mut ssh = timeout(Duration::from_secs(5 * 60), async move {
            loop {
                sleep(Duration::from_secs(1)).await;
                match SshSession::open(ip, password).await {
                    Ok(ssh) => return Ok(ssh),
                    Err(SshError::ConnectionTimeout) => {}
                    Err(e) => return Err(e),
                }
            }
        })
        .await
        .map_err(|_| CloudError::StartTimeout)??;
        println!("connected");

        ssh.exec("docker pull spiretf/docker-spire-server").await?;
        println!("pulled");
        ssh.exec(format!(
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

        Ok(server)
    }
}

impl Vultr {
    async fn get_app_id(&self, short_name: &str) -> Result<u16> {
        let response = self
            .client
            .get("https://api.vultr.com/v2/applications")
            .send()
            .await
            .map_err(NetworkError::from)?;
        let response: VultrApplicationsResponse =
            response.json().await.map_err(ResponseError::from)?;
        Ok(response
            .applications
            .into_iter()
            .find_map(|application| (application.short_name == short_name).then(|| application.id))
            .ok_or_else(|| {
                ResponseError::Other(format!("Application \"{}\" not found", short_name))
            })?)
    }

    async fn get_instance(&self, id: &str) -> Result<VultrInstanceResponse> {
        let response = self
            .client
            .get(format!("https://api.vultr.com/v2/instances/{}", id))
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(NetworkError::from)?;
        CloudError::from_status_code(response.status())?;

        let response: VultrGetResponse = response.json().await.map_err(ResponseError::from)?;
        Ok(response.instance)
    }
}

#[derive(Serialize)]
struct VultrCreateParams<'a> {
    region: &'a str,
    plan: &'a str,
    tag: &'a str,
    label: String,
    app_id: u16,
}

#[derive(Debug, Deserialize)]
struct VultrListResponse {
    instances: Vec<VultrInstanceResponse>,
}

#[derive(Debug, Deserialize)]
struct VultrGetResponse {
    instance: VultrInstanceResponse,
}

#[derive(Debug, Deserialize)]
struct VultrCreateResponse {
    instance: VultrCreatedInstanceResponse,
}

#[derive(Debug, Deserialize)]
struct VultrInstanceResponse {
    id: String,
    os: String,
    ram: u64,
    main_ip: IpAddr,
    region: String,
    vcpu_count: u16,
    date_created: DateTime<Utc>,
    tag: String,
}

#[derive(Debug, Deserialize)]
struct VultrCreatedInstanceResponse {
    id: String,
    default_password: String,
}

impl From<VultrInstanceResponse> for Server {
    fn from(instance: VultrInstanceResponse) -> Self {
        Server {
            id: instance.id,
            created: instance.date_created,
            ip: instance.main_ip,
        }
    }
}

impl From<VultrCreatedInstanceResponse> for Created {
    fn from(instance: VultrCreatedInstanceResponse) -> Self {
        Created {
            id: instance.id,
            password: instance.default_password,
        }
    }
}

#[derive(Debug, Deserialize)]
struct VultrApplicationsResponse {
    applications: Vec<VultrApplicationResponse>,
}

#[derive(Debug, Deserialize)]
struct VultrApplicationResponse {
    id: u16,
    short_name: String,
}
