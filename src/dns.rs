use reqwest::{Client, StatusCode};
use serde::Serialize;
use std::net::IpAddr;
use thiserror::Error;

pub type Result<T, E = DynDnsError> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum DynDnsError {
    #[error("Invalid credentials")]
    Unauthorized,
    #[error("Network error: {0}")]
    Network(#[from] NetworkError),
    #[error("Network response from server: {0}")]
    InvalidResponse(String),
    #[error("Domain belongs to another user")]
    NotYourDomain,
    #[error("Invalid hostname")]
    InvalidHostname,
    #[error("Rate limited")]
    Abuse,
}

impl DynDnsError {
    fn from_status_code(status: StatusCode) -> Result<()> {
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(DynDnsError::Unauthorized);
        }
        Ok(())
    }
}

/// Intentionally opaque error
#[derive(Debug, Error)]
#[error("{0}")]
pub struct NetworkError(reqwest::Error);

pub struct DynDnsClient {
    client: Client,
    update_url: String,
    username: String,
    password: String,
}

impl DynDnsClient {
    pub fn new(update_url: String, username: String, password: String) -> Self {
        DynDnsClient {
            client: Client::new(),
            update_url,
            username,
            password,
        }
    }

    pub async fn update(&self, hostname: &str, ip: IpAddr) -> Result<()> {
        let response = self
            .client
            .get(&self.update_url)
            .basic_auth(&self.username, Some(&self.password))
            .query(&DynDnsParams { hostname, ip })
            .send()
            .await
            .map_err(NetworkError)?;

        let status = response.status();
        DynDnsError::from_status_code(status)?;

        let text = response.text().await.map_err(NetworkError)?;
        match text.as_str() {
            "badauth" => Err(DynDnsError::Unauthorized),
            "!yours" => Err(DynDnsError::NotYourDomain),
            "nochg" | "good" => Ok(()),
            "notfqdn" | "nohost" | "numhost" => Err(DynDnsError::InvalidHostname),
            "abuse" => Err(DynDnsError::Abuse),
            _ => Err(DynDnsError::InvalidResponse(text)),
        }
    }
}

#[derive(Serialize)]
struct DynDnsParams<'a> {
    hostname: &'a str,
    #[serde(rename = "myip")]
    ip: IpAddr,
}
