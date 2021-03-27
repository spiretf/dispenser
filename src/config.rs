use crate::cloud::vultr::Vultr;
use crate::cloud::Cloud;
use camino::Utf8PathBuf;
use serde::Deserialize;
use std::fs::read_to_string;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Failed to open \"{0}\"")]
    Open(Utf8PathBuf),
    #[error("Malformed toml: {0}")]
    Toml(#[from] TomlError),
    #[error("No cloud provider configured")]
    NoProvider,
}

/// Intentionally opaque error
#[derive(Debug, Error)]
#[error("{0}")]
pub struct TomlError(toml::de::Error);

impl From<toml::de::Error> for TomlError {
    fn from(e: toml::de::Error) -> Self {
        TomlError(e)
    }
}

#[derive(Deserialize, Debug)]
pub struct Config {
    pub vultr: Option<VultrConfig>,
    pub server: ServerConfig,
}

impl Config {
    pub fn from_file<P: AsRef<Path> + Into<Utf8PathBuf>>(path: P) -> Result<Self, ConfigError> {
        let content = read_to_string(path.as_ref()).map_err(|_| ConfigError::Open(path.into()))?;
        Ok(toml::from_str(&content).map_err(TomlError::from)?)
    }

    pub fn cloud(&self) -> Result<Box<dyn Cloud>, ConfigError> {
        if let Some(vultr) = &self.vultr {
            Ok(Box::new(Vultr::new(
                vultr.api_key.clone(),
                vultr.region.clone(),
                vultr.plan.clone(),
            )))
        } else {
            Err(ConfigError::NoProvider)
        }
    }
}

#[derive(Deserialize, Debug)]
pub struct ServerConfig {
    pub rcon: String,
    pub password: String,
    #[serde(default = "server_default_image")]
    pub image: String,
    pub demostf_key: Option<String>,
    pub logstf_key: Option<String>,
    #[serde(default = "server_default_league")]
    pub config_league: String,
    #[serde(default = "server_default_mode")]
    pub config_mode: String,
    #[serde(default = "server_default_name")]
    pub name: String,
    #[serde(default = "server_default_tv_name")]
    pub tv_name: String,
}

fn server_default_image() -> String {
    String::from("spiretf/docker-spire-server")
}

fn server_default_name() -> String {
    String::from("Spire")
}

fn server_default_tv_name() -> String {
    String::from("SpireTV")
}

fn server_default_league() -> String {
    String::from("etf2l")
}

fn server_default_mode() -> String {
    String::from("6v6")
}

#[derive(Deserialize, Debug)]
pub struct VultrConfig {
    pub api_key: String,
    /// See https://api.vultr.com/v2/regions for a list of plans
    pub region: String,
    /// See https://api.vultr.com/v2/plans for a list of plans
    #[serde(default = "vultr_default_plan")]
    pub plan: String,
}

fn vultr_default_plan() -> String {
    String::from("vc2-1c-2gb")
}
