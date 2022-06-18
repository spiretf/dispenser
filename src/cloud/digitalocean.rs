use crate::cloud::{Cloud, CloudError, Created, NetworkError, ResponseError, Result, Server};
use crate::CreatedAuth;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures_util::stream::FuturesUnordered;
use futures_util::TryStreamExt;
use petname::petname;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use std::time::Duration;
use thrussh_keys::key::KeyPair;
use thrussh_keys::PublicKeyBase64;
use tokio::time::sleep;

pub struct DigitalOcean {
    region: String,
    plan: String,
    token: String,
    client: Client,
}

impl DigitalOcean {
    pub fn new(token: String, region: String, plan: String) -> Self {
        DigitalOcean {
            token,
            region,
            plan,
            client: Client::default(),
        }
    }
}

#[async_trait]
impl Cloud for DigitalOcean {
    async fn list(&self) -> Result<Vec<Server>> {
        let response = self
            .client
            .get("https://api.digitalocean.com/v2/droplets")
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(NetworkError::from)?;
        CloudError::from_status_code(response.status())?;

        let response: DigitalOceanListResponse =
            response.json().await.map_err(ResponseError::from)?;

        Ok(response
            .droplets
            .into_iter()
            .filter(|instance| instance.tags.iter().any(|tag| tag == "spire"))
            .map(Server::from)
            .collect())
    }

    async fn spawn(&self, ssh_keys: &[String]) -> Result<Created> {
        let startup_key = Arc::new(KeyPair::generate_ed25519().unwrap());
        let startup_key_id = self
            .create_key(
                "Dispenser Deploy Key",
                &format!(
                    "{} {} {}",
                    startup_key.name(),
                    startup_key.public_key_base64(),
                    "dispenser-deploy"
                ),
            )
            .await?;

        let mut key_ids = ssh_keys
            .iter()
            .map(|key| self.get_ssh_key_id(key))
            .collect::<FuturesUnordered<_>>()
            .try_collect::<Vec<_>>()
            .await?;
        key_ids.push(startup_key_id);

        let response_res = self
            .client
            .post("https://api.digitalocean.com/v2/droplets")
            .bearer_auth(&self.token)
            .json(&DigitalOceanCreateParams {
                region: self.region.as_str(),
                size: self.plan.as_str(),
                tags: &["spire"],
                name: petname(2, "-"),
                image: "docker-20-04",
                ssh_keys: key_ids,
                ipv6: true,
            })
            .send()
            .await
            .map_err(NetworkError::from);

        self.remove_key(startup_key_id).await?;

        // remove the deploy key, even if the spawn request failed
        let response = response_res?;

        CloudError::from_status_code(response.status())?;

        if response.status().is_success() {
            let response: DigitalOceanCreateResponse =
                response.json().await.map_err(ResponseError::from)?;
            Ok((response.droplet, startup_key).into())
        } else {
            Err(ResponseError::Other(response.text().await.map_err(NetworkError::from)?).into())
        }
    }

    async fn kill(&self, id: &str) -> Result<()> {
        let response = self
            .client
            .delete(format!("https://api.digitalocean.com/v2/droplets/{}", id))
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(NetworkError::from)?;
        CloudError::from_status_code(response.status())
    }

    async fn wait_for_ip(&self, id: &str) -> Result<Server> {
        let instance = loop {
            let instance = self.get_instance(id).await?;
            let ip = instance.networks.v4().next();
            if ip.is_some() {
                break instance;
            } else {
                sleep(Duration::from_millis(500)).await;
            }
        };
        Ok(instance.into())
    }
}

impl DigitalOcean {
    async fn get_instance(&self, id: &str) -> Result<DigitalOceanInstanceResponse> {
        let response = self
            .client
            .get(format!("https://api.digitalocean.com/v2/droplets/{}", id))
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(NetworkError::from)?;
        CloudError::from_status_code(response.status())?;

        let response: DigitalOceanGetResponse =
            response.json().await.map_err(ResponseError::from)?;
        Ok(response.droplet)
    }

    async fn get_ssh_key_id(&self, ssh_key: &str) -> Result<u32> {
        let response = self
            .client
            .get("https://api.digitalocean.com/v2/account/keys/")
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(NetworkError::from)?;
        CloudError::from_status_code(response.status())?;

        if !response.status().is_success() {
            return Err(
                ResponseError::Other(response.text().await.map_err(NetworkError::from)?).into(),
            );
        }

        let response: DigitalOceanSshListResponse =
            response.json().await.map_err(ResponseError::from)?;
        if let Some(key) = response
            .ssh_keys
            .into_iter()
            .find(|key| key.public_key == ssh_key)
        {
            Ok(key.id)
        } else {
            self.create_key("Dispenser Key", ssh_key).await
        }
    }

    async fn create_key(&self, name: &str, ssh_key: &str) -> Result<u32> {
        let response = self
            .client
            .post("https://api.digitalocean.com/v2/account/keys/")
            .bearer_auth(&self.token)
            .json(&DigitalOceanCreateSshKeyParams {
                name,
                public_key: ssh_key,
            })
            .send()
            .await
            .map_err(NetworkError::from)?;
        CloudError::from_status_code(response.status())?;
        let response = response.error_for_status().map_err(NetworkError)?;
        let response: DigitalOceanSshCreateResponse =
            response.json().await.map_err(ResponseError::from)?;

        Ok(response.ssh_key.id)
    }

    async fn remove_key(&self, key_id: u32) -> Result<()> {
        let response = self
            .client
            .delete(format!(
                "https://api.digitalocean.com/v2/account/keys/{}",
                key_id
            ))
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(NetworkError::from)?;
        CloudError::from_status_code(response.status())?;

        Ok(())
    }
}

#[derive(Serialize)]
struct DigitalOceanCreateParams<'a> {
    name: String,
    region: &'a str,
    size: &'a str,
    tags: &'a [&'a str],
    image: &'a str,
    ssh_keys: Vec<u32>,
    ipv6: bool,
}

#[derive(Debug, Deserialize)]
struct DigitalOceanListResponse {
    droplets: Vec<DigitalOceanInstanceResponse>,
}

#[derive(Debug, Deserialize)]
struct DigitalOceanGetResponse {
    droplet: DigitalOceanInstanceResponse,
}

#[derive(Debug, Deserialize)]
struct DigitalOceanCreateResponse {
    droplet: DigitalOceanCreatedInstanceResponse,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct DigitalOceanInstanceResponse {
    id: u32,
    memory: u64,
    networks: DigitalOceanNetworks,
    vcpus: u16,
    created_at: DateTime<Utc>,
    tags: Vec<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct DigitalOceanNetworks {
    v4: Vec<DigitalOceanNetwork>,
    v6: Vec<DigitalOceanNetwork>,
}

impl DigitalOceanNetworks {
    fn v4(&self) -> impl Iterator<Item = IpAddr> + '_ {
        self.v4
            .iter()
            .filter(|net| net.ty == DigitalOceanNetworkType::Public)
            .map(|net| net.ip_address)
    }

    fn v6(&self) -> impl Iterator<Item = IpAddr> + '_ {
        self.v6
            .iter()
            .filter(|net| net.ty == DigitalOceanNetworkType::Public)
            .map(|net| net.ip_address)
    }
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct DigitalOceanNetwork {
    ip_address: IpAddr,
    gateway: String,
    #[serde(rename = "type")]
    ty: DigitalOceanNetworkType,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
enum DigitalOceanNetworkType {
    Private,
    Public,
}

#[derive(Debug, Deserialize)]
struct DigitalOceanCreatedInstanceResponse {
    id: u32,
}

impl From<DigitalOceanInstanceResponse> for Server {
    fn from(instance: DigitalOceanInstanceResponse) -> Self {
        Server {
            id: instance.id.to_string(),
            created: instance.created_at,
            ip: instance
                .networks
                .v4()
                .next()
                .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED)),
            ip_v6: instance.networks.v6().next(),
        }
    }
}

impl From<(DigitalOceanCreatedInstanceResponse, Arc<KeyPair>)> for Created {
    fn from((instance, key): (DigitalOceanCreatedInstanceResponse, Arc<KeyPair>)) -> Self {
        Created {
            id: instance.id.to_string(),
            auth: CreatedAuth::Ssh(key),
        }
    }
}

#[allow(dead_code)]
#[derive(Serialize)]
struct DigitalOceanCreateSshKeyParams<'a> {
    name: &'a str,
    public_key: &'a str,
}

#[derive(Debug, Deserialize)]
struct DigitalOceanSshCreateResponse {
    ssh_key: DigitalOceanSshCreateKey,
}

#[derive(Debug, Deserialize)]
struct DigitalOceanSshCreateKey {
    id: u32,
}

#[derive(Debug, Deserialize)]
struct DigitalOceanSshListResponse {
    ssh_keys: Vec<DigitalOceanSshKey>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct DigitalOceanSshKey {
    id: u32,
    fingerprint: String,
    public_key: String,
    name: String,
}
