use crate::cloud::{Cloud, CloudError, Created, NetworkError, ResponseError, Result, Server};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use petname::petname;
use reqwest::Client;
use serde::{Deserialize, Serialize};
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

    async fn spawn(&self, ssh_key_id: Option<&str>) -> Result<Created> {
        let key_ids = if let Some(key) = ssh_key_id {
            vec![key]
        } else {
            vec![]
        };
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
                sshkey_id: &key_ids,
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
            .find(|key| key.ssh_key == ssh_key)
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
    sshkey_id: &'a [&'a str],
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
