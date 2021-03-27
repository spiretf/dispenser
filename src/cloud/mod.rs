mod ssh;
pub mod vultr;

use crate::cloud::ssh::SshError;
use crate::config::ServerConfig;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::StatusCode;
use std::net::IpAddr;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CloudError {
    #[error("Invalid credentials")]
    Unauthorized,
    #[error("Specified server not found")]
    ServerNotFound,
    #[error("Network error: {0}")]
    Network(#[from] NetworkError),
    #[error("Network response from server: {0}")]
    InvalidResponse(#[from] ResponseError),
    #[error("Server boot timed out")]
    StartTimeout,
    #[error("Error while trying to connect trough ssh: {0}")]
    Ssh(#[from] SshError),
}

/// Intentionally opaque error
#[derive(Debug, Error)]
#[error("{0}")]
pub struct NetworkError(reqwest::Error);

impl CloudError {
    fn from_status_code(status: StatusCode) -> Result<()> {
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(CloudError::Unauthorized);
        }
        Ok(())
    }
}

/// Intentionally opaque error
#[derive(Debug, Error)]
pub enum ResponseError {
    #[error("{0}")]
    JSON(reqwest::Error),
    #[error("Unexpected response {0}")]
    Other(String),
}

impl From<reqwest::Error> for NetworkError {
    fn from(e: reqwest::Error) -> Self {
        NetworkError(e)
    }
}

impl From<reqwest::Error> for ResponseError {
    fn from(e: reqwest::Error) -> Self {
        ResponseError::JSON(e)
    }
}

pub type Result<T, E = CloudError> = std::result::Result<T, E>;

#[async_trait]
pub trait Cloud {
    /// List all running servers on this cloud
    async fn list(&self) -> Result<Vec<Server>>;
    /// Create a new server with the given parameter
    async fn spawn(&self) -> Result<Created>;
    /// Destroy a given server
    async fn kill(&self, id: &str) -> Result<()>;
    /// Wait until the server has an ip
    async fn wait_for_ip(&self, id: &str) -> Result<Server>;
    /// Setup the tf2 server on the instance
    async fn setup(&self, id: &str, password: &str, config: &ServerConfig) -> Result<Server>;
}

#[derive(Debug)]
pub struct Server {
    pub id: String,
    pub created: DateTime<Utc>,
    pub ip: IpAddr,
}

#[derive(Debug)]
pub struct Created {
    pub id: String,
    pub password: String,
}
