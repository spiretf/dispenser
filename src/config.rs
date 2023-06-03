use crate::cloud::digitalocean::DigitalOcean;
use crate::cloud::vultr::Vultr;
use crate::cloud::Cloud;
use camino::Utf8PathBuf;
use serde::de::Error;
use serde::{Deserialize, Deserializer};
use std::fs::read_to_string;
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Failed to open \"{0}\"")]
    Open(Utf8PathBuf),
    #[error("Malformed toml: {0}")]
    Toml(#[from] TomlError),
    #[error("No cloud provider configured")]
    NoProvider,
    #[error("Multiple cloud providers configured")]
    MultipleProviders,
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
    pub digital_ocean: Option<DigitalOceanConfig>,
    pub server: ServerConfig,
    pub dyndns: Option<DynDnsConfig>,
    pub schedule: ScheduleConfig,
}

impl Config {
    pub fn from_file<P: AsRef<Path> + Into<Utf8PathBuf>>(path: P) -> Result<Self, ConfigError> {
        let content = read_to_string(path.as_ref()).map_err(|_| ConfigError::Open(path.into()))?;
        Ok(toml::from_str(&content).map_err(TomlError::from)?)
    }

    pub fn cloud(&self) -> Result<Arc<dyn Cloud>, ConfigError> {
        if self.vultr.is_some() && self.digital_ocean.is_some() {
            Err(ConfigError::NoProvider)
        } else if let Some(vultr) = &self.vultr {
            Ok(Arc::new(Vultr::new(
                vultr.api_key.clone(),
                vultr.region.clone(),
                vultr.plan.clone(),
            )))
        } else if let Some(digital_ocean) = &self.digital_ocean {
            Ok(Arc::new(DigitalOcean::new(
                digital_ocean.api_key.clone(),
                digital_ocean.region.clone(),
                digital_ocean.plan.clone(),
            )))
        } else {
            Err(ConfigError::NoProvider)
        }
    }
}

fn deserialize_opt_secret<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = <Option<String>>::deserialize(deserializer)?;
    raw.map(load_secret).transpose().map_err(D::Error::custom)
}

fn deserialize_secret<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    load_secret(raw).map_err(D::Error::custom)
}

fn load_secret(raw: String) -> Result<String, std::io::Error> {
    let path: &Path = raw.as_ref();
    if raw.starts_with('/') && path.exists() {
        let raw = read_to_string(raw)?;
        Ok(raw.trim().into())
    } else {
        Ok(raw)
    }
}

#[derive(Deserialize, Debug)]
pub struct ServerConfig {
    #[serde(deserialize_with = "deserialize_secret")]
    pub rcon: String,
    #[serde(deserialize_with = "deserialize_secret")]
    pub password: String,
    #[serde(default = "server_default_image")]
    pub image: String,
    #[serde(deserialize_with = "deserialize_opt_secret")]
    pub demostf_key: Option<String>,
    #[serde(deserialize_with = "deserialize_opt_secret")]
    pub logstf_key: Option<String>,
    #[serde(default = "server_default_league")]
    pub config_league: String,
    #[serde(default = "server_default_mode")]
    pub config_mode: String,
    #[serde(default = "server_default_name")]
    pub name: String,
    #[serde(default = "server_default_tv_name")]
    pub tv_name: String,
    #[serde(default)]
    pub ssh_keys: Vec<String>,
    #[serde(default)]
    pub manage_existing: bool,
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
    #[serde(deserialize_with = "deserialize_secret")]
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

#[derive(Deserialize, Debug)]
pub struct DigitalOceanConfig {
    #[serde(deserialize_with = "deserialize_secret")]
    pub api_key: String,
    /// See https://api.vultr.com/v2/regions for a list of plans
    pub region: String,
    /// See https://api.vultr.com/v2/plans for a list of plans
    #[serde(default = "digital_ocean_default_plan")]
    pub plan: String,
}

fn digital_ocean_default_plan() -> String {
    String::from("s-2vcpu-2gb")
}

#[derive(Deserialize, Debug, Clone)]
pub struct DynDnsConfig {
    pub update_url: String,
    pub hostname: String,
    pub username: String,
    #[serde(deserialize_with = "deserialize_secret")]
    pub password: String,
}

#[derive(Deserialize, Debug)]
pub struct ScheduleConfig {
    pub start: String,
    pub stop: String,
}
