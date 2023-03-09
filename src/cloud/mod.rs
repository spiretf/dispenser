use std::fmt::{Display, Formatter};
use std::net::IpAddr;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::StatusCode;
use thiserror::Error;
use thrussh_keys::key::KeyPair;

pub mod digitalocean;
pub mod vultr;

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
    Json(reqwest::Error),
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
        ResponseError::Json(e)
    }
}

pub type Result<T, E = CloudError> = std::result::Result<T, E>;

#[async_trait]
pub trait Cloud: Send + Sync + 'static {
    /// List all running servers on this cloud
    async fn list(&self) -> Result<Vec<Server>>;
    /// Create a new server with the given parameter
    async fn spawn(&self, ssh_keys: &[String]) -> Result<Created>;
    /// Destroy a given server
    async fn kill(&self, id: &str) -> Result<()>;
    /// Wait until the server has an ip
    async fn wait_for_ip(&self, id: &str) -> Result<Server>;
}

#[derive(Debug)]
pub struct Server {
    pub id: String,
    pub created: DateTime<Utc>,
    pub ip: IpAddr,
    pub ip_v6: Option<IpAddr>,
}

#[derive(Debug)]
pub struct Created {
    pub id: String,
    pub auth: CreatedAuth,
}

#[derive(Debug)]
pub enum CreatedAuth {
    Password(String),
    Ssh(Arc<KeyPair>),
}

impl Display for CreatedAuth {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            CreatedAuth::Password(s) => s.fmt(f),
            CreatedAuth::Ssh(_) => write!(f, "public key only"),
        }
    }
}

fn key_cmp(a: &str, b: &str) -> bool {
    let mut a_parts = a.split(' ');
    let mut b_parts = b.split(' ');

    // compare the first 2 space-seperated parts
    a_parts.next() == b_parts.next() && a_parts.next() == b_parts.next()
}
