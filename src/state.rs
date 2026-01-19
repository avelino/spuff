//! Local state management for tracking active instances.
//!
//! This module provides SQLite-backed persistence for tracking which instances
//! are currently active. This allows the CLI to maintain state across invocations.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::config::AppConfig;
use crate::error::Result;
use crate::provider::ProviderInstance;

/// Instance information stored locally.
///
/// This is separate from `ProviderInstance` which represents the provider's view.
/// `LocalInstance` contains additional metadata needed for CLI operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalInstance {
    /// Provider-specific instance ID
    pub id: String,

    /// Human-readable instance name
    pub name: String,

    /// Public IP address (as string for storage)
    pub ip: String,

    /// Which cloud provider manages this instance
    pub provider: String,

    /// Region/datacenter where the instance runs
    pub region: String,

    /// Instance size/type
    pub size: String,

    /// When the instance was created
    pub created_at: DateTime<Utc>,
}

/// Legacy type alias for backward compatibility.
#[deprecated(since = "0.2.0", note = "Use LocalInstance instead")]
#[allow(dead_code)]
pub type Instance = LocalInstance;

impl LocalInstance {
    /// Create a LocalInstance from a ProviderInstance and additional metadata.
    pub fn from_provider(
        provider_instance: &ProviderInstance,
        name: String,
        provider: String,
        region: String,
        size: String,
    ) -> Self {
        Self {
            id: provider_instance.id.clone(),
            name,
            ip: provider_instance.ip.to_string(),
            provider,
            region,
            size,
            created_at: provider_instance.created_at,
        }
    }

    /// Create a new LocalInstance with the current timestamp.
    #[allow(dead_code)]
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        ip: impl Into<String>,
        provider: impl Into<String>,
        region: impl Into<String>,
        size: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            ip: ip.into(),
            provider: provider.into(),
            region: region.into(),
            size: size.into(),
            created_at: Utc::now(),
        }
    }
}

/// SQLite-backed state database.
pub struct StateDb {
    conn: Connection,
}

impl StateDb {
    /// Open or create the state database.
    pub fn open() -> Result<Self> {
        let path = Self::db_path()?;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(&path)?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS instances (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                ip TEXT NOT NULL,
                provider TEXT NOT NULL,
                region TEXT NOT NULL,
                size TEXT NOT NULL,
                created_at TEXT NOT NULL,
                active INTEGER DEFAULT 1
            )",
            [],
        )?;

        Ok(Self { conn })
    }

    fn db_path() -> Result<PathBuf> {
        Ok(AppConfig::config_dir()?.join("state.db"))
    }

    /// Save an instance, marking it as the only active one.
    pub fn save_instance(&self, instance: &LocalInstance) -> Result<()> {
        // First, mark all existing instances as inactive
        self.conn.execute("UPDATE instances SET active = 0", [])?;

        // Insert or replace the new instance as active
        self.conn.execute(
            "INSERT OR REPLACE INTO instances (id, name, ip, provider, region, size, created_at, active)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1)",
            params![
                instance.id,
                instance.name,
                instance.ip,
                instance.provider,
                instance.region,
                instance.size,
                instance.created_at.to_rfc3339(),
            ],
        )?;

        Ok(())
    }

    /// Get the currently active instance, if any.
    pub fn get_active_instance(&self) -> Result<Option<LocalInstance>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, ip, provider, region, size, created_at
             FROM instances
             WHERE active = 1
             LIMIT 1",
        )?;

        let mut rows = stmt.query([])?;

        if let Some(row) = rows.next()? {
            let created_at_str: String = row.get(6)?;
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map_or_else(|_| Utc::now(), |dt| dt.with_timezone(&Utc));

            Ok(Some(LocalInstance {
                id: row.get(0)?,
                name: row.get(1)?,
                ip: row.get(2)?,
                provider: row.get(3)?,
                region: row.get(4)?,
                size: row.get(5)?,
                created_at,
            }))
        } else {
            Ok(None)
        }
    }

    /// Remove an instance from the database.
    pub fn remove_instance(&self, id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM instances WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// List all instances (including inactive ones).
    #[allow(dead_code)]
    pub fn list_instances(&self) -> Result<Vec<LocalInstance>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, ip, provider, region, size, created_at FROM instances ORDER BY created_at DESC",
        )?;

        let instances = stmt
            .query_map([], |row| {
                let created_at_str: String = row.get(6)?;
                let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                    .map_or_else(|_| Utc::now(), |dt| dt.with_timezone(&Utc));

                Ok(LocalInstance {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    ip: row.get(2)?,
                    provider: row.get(3)?,
                    region: row.get(4)?,
                    size: row.get(5)?,
                    created_at,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(instances)
    }

    /// Update the IP address of an instance.
    #[allow(dead_code)]
    pub fn update_instance_ip(&self, id: &str, ip: &str) -> Result<()> {
        self.conn
            .execute("UPDATE instances SET ip = ?1 WHERE id = ?2", params![ip, id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn create_test_db() -> StateDb {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute(
            "CREATE TABLE IF NOT EXISTS instances (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                ip TEXT NOT NULL,
                provider TEXT NOT NULL,
                region TEXT NOT NULL,
                size TEXT NOT NULL,
                created_at TEXT NOT NULL,
                active INTEGER DEFAULT 1
            )",
            [],
        )
        .unwrap();
        StateDb { conn }
    }

    fn create_test_instance(id: &str, name: &str) -> LocalInstance {
        LocalInstance {
            id: id.to_string(),
            name: name.to_string(),
            ip: "10.0.0.1".to_string(),
            provider: "digitalocean".to_string(),
            region: "nyc1".to_string(),
            size: "s-2vcpu-4gb".to_string(),
            created_at: Utc::now(),
        }
    }

    #[test]
    fn test_save_and_get_instance() {
        let db = create_test_db();
        let instance = create_test_instance("123", "spuff-test");

        db.save_instance(&instance).unwrap();

        let retrieved = db.get_active_instance().unwrap();
        assert!(retrieved.is_some());

        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.id, "123");
        assert_eq!(retrieved.name, "spuff-test");
        assert_eq!(retrieved.ip, "10.0.0.1");
        assert_eq!(retrieved.provider, "digitalocean");
        assert_eq!(retrieved.region, "nyc1");
        assert_eq!(retrieved.size, "s-2vcpu-4gb");
    }

    #[test]
    fn test_only_one_active_instance() {
        let db = create_test_db();

        let instance1 = create_test_instance("111", "spuff-first");
        let instance2 = create_test_instance("222", "spuff-second");

        db.save_instance(&instance1).unwrap();
        db.save_instance(&instance2).unwrap();

        let active = db.get_active_instance().unwrap().unwrap();
        assert_eq!(active.id, "222");
        assert_eq!(active.name, "spuff-second");

        // First instance should now be inactive
        let all = db.list_instances().unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_remove_instance() {
        let db = create_test_db();
        let instance = create_test_instance("456", "spuff-remove");

        db.save_instance(&instance).unwrap();
        assert!(db.get_active_instance().unwrap().is_some());

        db.remove_instance("456").unwrap();
        assert!(db.get_active_instance().unwrap().is_none());
    }

    #[test]
    fn test_get_active_instance_none() {
        let db = create_test_db();
        let result = db.get_active_instance().unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_list_instances() {
        let db = create_test_db();

        let instance1 = create_test_instance("aaa", "spuff-a");
        let instance2 = create_test_instance("bbb", "spuff-b");
        let instance3 = create_test_instance("ccc", "spuff-c");

        db.save_instance(&instance1).unwrap();
        db.save_instance(&instance2).unwrap();
        db.save_instance(&instance3).unwrap();

        let all = db.list_instances().unwrap();
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn test_update_instance_ip() {
        let db = create_test_db();
        let instance = create_test_instance("789", "spuff-ip-test");

        db.save_instance(&instance).unwrap();
        db.update_instance_ip("789", "192.168.1.100").unwrap();

        let retrieved = db.get_active_instance().unwrap().unwrap();
        assert_eq!(retrieved.ip, "192.168.1.100");
    }

    #[test]
    fn test_instance_replace_on_same_id() {
        let db = create_test_db();

        let instance1 = LocalInstance {
            id: "same-id".to_string(),
            name: "first-name".to_string(),
            ip: "1.1.1.1".to_string(),
            provider: "digitalocean".to_string(),
            region: "nyc1".to_string(),
            size: "small".to_string(),
            created_at: Utc::now(),
        };

        let instance2 = LocalInstance {
            id: "same-id".to_string(),
            name: "second-name".to_string(),
            ip: "2.2.2.2".to_string(),
            provider: "hetzner".to_string(),
            region: "fsn1".to_string(),
            size: "large".to_string(),
            created_at: Utc::now(),
        };

        db.save_instance(&instance1).unwrap();
        db.save_instance(&instance2).unwrap();

        let all = db.list_instances().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "second-name");
        assert_eq!(all[0].ip, "2.2.2.2");
    }

    #[test]
    fn test_instance_serialization() {
        let instance = create_test_instance("ser-123", "spuff-serial");
        let json = serde_json::to_string(&instance).unwrap();

        assert!(json.contains("ser-123"));
        assert!(json.contains("spuff-serial"));
        assert!(json.contains("digitalocean"));

        let deserialized: LocalInstance = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, instance.id);
        assert_eq!(deserialized.name, instance.name);
    }

    #[test]
    fn test_local_instance_new() {
        let instance = LocalInstance::new("id-123", "test-name", "1.2.3.4", "do", "nyc1", "small");
        assert_eq!(instance.id, "id-123");
        assert_eq!(instance.name, "test-name");
        assert_eq!(instance.ip, "1.2.3.4");
        assert_eq!(instance.provider, "do");
    }
}
