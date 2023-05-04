use crate::cloud::{
    key_cmp, Cloud, CloudError, Created, CreatedAuth, NetworkError, ResponseError, Result, Server,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures_util::stream::FuturesUnordered;
use futures_util::TryStreamExt;
use petname::petname;
use reqwest::Client;
use serde::{Deserialize, Deserializer, Serialize};
use std::net::IpAddr;
use std::time::Duration;
use tokio::time::sleep;

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

    async fn spawn(&self, ssh_keys: &[String]) -> Result<Created> {
        let key_ids = ssh_keys
            .iter()
            .map(|key| self.get_ssh_key_id(key))
            .collect::<FuturesUnordered<_>>()
            .try_collect::<Vec<String>>()
            .await?;

        let response = self
            .client
            .post("https://api.vultr.com/v2/instances")
            .bearer_auth(&self.token)
            .json(&VultrCreateParams {
                region: self.region.as_str(),
                plan: self.plan.as_str(),
                tag: "spire",
                label: petname(2, "-"),
                image_id: self.get_app_image_id("docker").await?,
                sshkey_id: key_ids,
                enable_ipv6: true,
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

    async fn kill(&self, id: &str) -> Result<()> {
        let response = self
            .client
            .delete(format!("https://api.vultr.com/v2/instances/{}", id))
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(NetworkError::from)?;
        CloudError::from_status_code(response.status())
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
}

impl Vultr {
    async fn get_app_image_id(&self, short_name: &str) -> Result<String> {
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
            .find_map(|application| {
                (application.short_name == short_name).then(|| application.image_id)
            })
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

    async fn get_ssh_key_id(&self, ssh_key: &str) -> Result<String> {
        let response = self
            .client
            .get("https://api.vultr.com/v2/ssh-keys")
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

        let response: VultrSshListResponse = response.json().await.map_err(ResponseError::from)?;
        if let Some(key) = response
            .ssh_keys
            .into_iter()
            .find(|key| key_cmp(&key.ssh_key, ssh_key))
        {
            Ok(key.id)
        } else {
            let response = self
                .client
                .post("https://api.vultr.com/v2/ssh-keys")
                .bearer_auth(&self.token)
                .json(&VultrCreateSshKeyParams {
                    name: "Dispenser Key",
                    ssh_key,
                })
                .send()
                .await
                .map_err(NetworkError::from)?;
            CloudError::from_status_code(response.status())?;
            let response: VultrSshCreateResponse =
                response.json().await.map_err(ResponseError::from)?;

            Ok(response.ssh_key.id)
        }
    }
}

#[derive(Serialize)]
struct VultrCreateParams<'a> {
    region: &'a str,
    plan: &'a str,
    tag: &'a str,
    label: String,
    image_id: String,
    sshkey_id: Vec<String>,
    enable_ipv6: bool,
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

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct VultrInstanceResponse {
    id: String,
    os: String,
    ram: u64,
    main_ip: IpAddr,
    #[serde(deserialize_with = "ok_or_default")]
    v6_main_ip: Option<IpAddr>,
    region: String,
    vcpu_count: u16,
    date_created: DateTime<Utc>,
    tag: String,
}

fn ok_or_default<'a, T, D>(deserializer: D) -> Result<T, D::Error>
where
    T: Deserialize<'a> + Default,
    D: Deserializer<'a>,
{
    let v: T = Deserialize::deserialize(deserializer).unwrap_or_default();
    Ok(v)
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
            ip_v6: instance.v6_main_ip,
        }
    }
}

impl From<VultrCreatedInstanceResponse> for Created {
    fn from(instance: VultrCreatedInstanceResponse) -> Self {
        Created {
            id: instance.id,
            auth: CreatedAuth::Password(instance.default_password),
        }
    }
}

#[derive(Debug, Deserialize)]
struct VultrApplicationsResponse {
    applications: Vec<VultrApplicationResponse>,
}

#[derive(Debug, Deserialize)]
struct VultrApplicationResponse {
    image_id: String,
    short_name: String,
}

#[derive(Debug, Deserialize)]
struct VultrSshListResponse {
    ssh_keys: Vec<VultrSshKeyResponse>,
}

#[derive(Debug, Deserialize)]
struct VultrSshCreateResponse {
    ssh_key: VultrSshKeyResponse,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct VultrSshKeyResponse {
    id: String,
    name: String,
    ssh_key: String,
}

#[derive(Serialize)]
struct VultrCreateSshKeyParams<'a> {
    name: &'a str,
    ssh_key: &'a str,
}
