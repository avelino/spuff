pub mod digitalocean;

use std::net::IpAddr;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::config::AppConfig;
use crate::error::{Result, SpuffError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceConfig {
    pub name: String,
    pub region: String,
    pub size: String,
    pub image: String,
    pub ssh_keys: Vec<String>,
    pub user_data: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Instance {
    pub id: String,
    pub ip: IpAddr,
    pub status: InstanceStatus,
    #[allow(dead_code)]
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InstanceStatus {
    New,
    Active,
    Off,
    Archive,
    Unknown(String),
}

impl std::fmt::Display for InstanceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstanceStatus::New => write!(f, "new"),
            InstanceStatus::Active => write!(f, "active"),
            InstanceStatus::Off => write!(f, "off"),
            InstanceStatus::Archive => write!(f, "archive"),
            InstanceStatus::Unknown(s) => write!(f, "{}", s),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub id: String,
    pub name: String,
    #[allow(dead_code)]
    pub created_at: Option<DateTime<Utc>>,
    #[allow(dead_code)]
    pub size_gb: Option<f64>,
}

#[async_trait]
pub trait Provider: Send + Sync {
    async fn create_instance(&self, config: &InstanceConfig) -> Result<Instance>;
    async fn destroy_instance(&self, id: &str) -> Result<()>;
    async fn get_instance(&self, id: &str) -> Result<Option<Instance>>;
    #[allow(dead_code)]
    async fn list_instances(&self) -> Result<Vec<Instance>>;
    async fn wait_ready(&self, id: &str) -> Result<Instance>;
    async fn create_snapshot(&self, instance_id: &str, name: &str) -> Result<Snapshot>;
    async fn list_snapshots(&self) -> Result<Vec<Snapshot>>;
    async fn delete_snapshot(&self, id: &str) -> Result<()>;
}

pub fn create_provider(config: &AppConfig) -> Result<Box<dyn Provider>> {
    match config.provider.as_str() {
        "digitalocean" => Ok(Box::new(digitalocean::DigitalOceanProvider::new(
            &config.api_token,
        )?)),
        "hetzner" => Err(SpuffError::Provider(
            "Hetzner provider not yet implemented".to_string(),
        )),
        "aws" => Err(SpuffError::Provider(
            "AWS provider not yet implemented".to_string(),
        )),
        _ => Err(SpuffError::Provider(format!(
            "Unknown provider: {}",
            config.provider
        ))),
    }
}
