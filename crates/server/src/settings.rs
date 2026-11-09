//! Server Settings: the Server Key and the Server Address, stored in Postgres and edited by Admins.

use sqlx::PgPool;
use tokio::sync::RwLock;

use crate::{pki::Ca, server_key};

/// Current Server Settings, kept in memory and backed by Postgres.
pub struct Settings {
    key: RwLock<String>,
    address: RwLock<String>,
}

impl Settings {
    /// Loads the settings, seeding the address from `default_address` (`RCLI_AGENT_HOSTNAMES`) on first start.
    pub async fn load(db: &PgPool, ca: &Ca, default_address: &str) -> anyhow::Result<Self> {
        let key = server_key::load_or_create(db, ca).await?;
        let stored: String = sqlx::query_scalar("SELECT server_address FROM server_ca").fetch_one(db).await?;
        let address = match stored.is_empty() {
            true => {
                sqlx::query("UPDATE server_ca SET server_address = $1").bind(default_address).execute(db).await?;
                default_address.to_owned()
            }
            false => stored,
        };
        Ok(Self { key: RwLock::new(key), address: RwLock::new(address) })
    }

    pub async fn key(&self) -> String {
        self.key.read().await.clone()
    }

    pub async fn address(&self) -> String {
        self.address.read().await.clone()
    }

    /// A new secret for the same CA: registered Devices keep their certificates.
    pub async fn rotate_key(&self, db: &PgPool, ca: &Ca) -> anyhow::Result<String> {
        let key = server_key::rotate(db, ca).await?;
        *self.key.write().await = key.clone();
        Ok(key)
    }

    pub async fn set_address(&self, db: &PgPool, address: &str) -> anyhow::Result<()> {
        sqlx::query("UPDATE server_ca SET server_address = $1").bind(address).execute(db).await?;
        *self.address.write().await = address.to_owned();
        Ok(())
    }
}

/// The names the Server's TLS certificate must carry: address entries without their ports.
pub fn hostnames(address: &str) -> Vec<String> {
    address
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| match s.strip_prefix('[') {
            Some(rest) => rest.split(']').next().unwrap_or(rest).to_owned(),
            None => s
                .rsplit_once(':')
                .filter(|(_, port)| port.chars().all(|c| c.is_ascii_digit()))
                .map_or(s, |(host, _)| host)
                .to_owned(),
        })
        .collect()
}
