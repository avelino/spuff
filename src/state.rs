//! Local state management for tracking active instances.
//!
//! This module provides ChronDB-backed persistence for tracking which instances
//! are currently active. This allows the CLI to maintain state across invocations.

use std::path::PathBuf;

use chrondb::ChronDB;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;

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
    #[cfg(test)]
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

/// ChronDB-backed state database.
pub struct StateDb {
    db: ChronDB,
}

impl StateDb {
    /// Open or create the state database.
    pub fn open() -> Result<Self> {
        let base = Self::db_base_path()?;
        std::fs::create_dir_all(&base)?;

        let data_path = base.join("data");
        let index_path = base.join("index");

        let db = ChronDB::open(
            data_path.to_str().unwrap_or_default(),
            index_path.to_str().unwrap_or_default(),
        )?;

        Ok(Self { db })
    }

    fn db_base_path() -> Result<PathBuf> {
        Ok(AppConfig::config_dir()?.join("chrondb"))
    }

    /// ChronDB overwrites the `id` field with the storage key (e.g. "instance:123").
    /// This restores the original instance ID by stripping the prefix.
    fn fix_instance_id(instance: &mut LocalInstance) {
        if let Some(stripped) = instance.id.strip_prefix("instance:") {
            instance.id = stripped.to_string();
        }
    }

    /// Save an instance, marking it as the only active one.
    pub fn save_instance(&self, instance: &LocalInstance) -> Result<()> {
        let doc = serde_json::to_value(instance)?;
        let key = format!("instance:{}", instance.id);

        self.db.put(&key, &doc, None)?;
        self.db
            .put("meta:active", &json!({"instance_id": instance.id}), None)?;

        Ok(())
    }

    /// Get the currently active instance, if any.
    pub fn get_active_instance(&self) -> Result<Option<LocalInstance>> {
        let meta = match self.db.get("meta:active", None) {
            Ok(val) => val,
            Err(chrondb::ChronDBError::NotFound) => return Ok(None),
            Err(e) => return Err(e.into()),
        };

        let instance_id = match meta.get("instance_id").and_then(|v| v.as_str()) {
            Some(id) => id.to_string(),
            None => return Ok(None),
        };

        let key = format!("instance:{}", instance_id);
        let doc = match self.db.get(&key, None) {
            Ok(val) => val,
            Err(chrondb::ChronDBError::NotFound) => return Ok(None),
            Err(e) => return Err(e.into()),
        };

        let mut instance: LocalInstance = serde_json::from_value(doc)?;
        Self::fix_instance_id(&mut instance);
        Ok(Some(instance))
    }

    /// Remove an instance from the database.
    pub fn remove_instance(&self, id: &str) -> Result<()> {
        // Check if this is the active instance
        let is_active = match self.db.get("meta:active", None) {
            Ok(meta) => meta
                .get("instance_id")
                .and_then(|v| v.as_str())
                .map(|active_id| active_id == id)
                .unwrap_or(false),
            Err(chrondb::ChronDBError::NotFound) => false,
            Err(e) => return Err(e.into()),
        };

        let key = format!("instance:{}", id);
        self.db.delete(&key, None)?;

        if is_active {
            // Ignore NotFound errors when cleaning up meta:active
            match self.db.delete("meta:active", None) {
                Ok(()) | Err(chrondb::ChronDBError::NotFound) => {}
                Err(e) => return Err(e.into()),
            }
        }

        Ok(())
    }

    /// List all instances.
    #[cfg(test)]
    pub fn list_instances(&self) -> Result<Vec<LocalInstance>> {
        let docs = match self.db.list_by_table("instance", None) {
            Ok(val) => val,
            Err(chrondb::ChronDBError::NotFound) => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };

        let mut instances: Vec<LocalInstance> = match docs.as_array() {
            Some(arr) => arr
                .iter()
                .filter_map(|v| serde_json::from_value(v.clone()).ok())
                .map(|mut i: LocalInstance| {
                    Self::fix_instance_id(&mut i);
                    i
                })
                .collect(),
            None => Vec::new(),
        };

        instances.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(instances)
    }

    /// Update the IP address of an instance.
    #[cfg(test)]
    pub fn update_instance_ip(&self, id: &str, ip: &str) -> Result<()> {
        let key = format!("instance:{}", id);
        let doc = self.db.get(&key, None)?;

        let mut instance: LocalInstance = serde_json::from_value(doc)?;
        instance.ip = ip.to_string();

        let updated = serde_json::to_value(&instance)?;
        self.db.put(&key, &updated, None)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::MutexGuard;
    use tempfile::TempDir;

    // GraalVM cannot handle multiple isolates concurrently in the same process.
    static DB_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn create_test_db() -> (StateDb, TempDir, MutexGuard<'static, ()>) {
        let guard = DB_LOCK.lock().unwrap();
        let dir = TempDir::new().unwrap();
        let data_path = dir.path().join("data");
        let index_path = dir.path().join("index");

        let db = ChronDB::open(data_path.to_str().unwrap(), index_path.to_str().unwrap()).unwrap();

        (StateDb { db }, dir, guard)
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
        let (db, _dir, _lock) = create_test_db();
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
        let (db, _dir, _lock) = create_test_db();

        let instance1 = create_test_instance("111", "spuff-first");
        let instance2 = create_test_instance("222", "spuff-second");

        db.save_instance(&instance1).unwrap();
        db.save_instance(&instance2).unwrap();

        let active = db.get_active_instance().unwrap().unwrap();
        assert_eq!(active.id, "222");
        assert_eq!(active.name, "spuff-second");

        let all = db.list_instances().unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_remove_instance() {
        let (db, _dir, _lock) = create_test_db();
        let instance = create_test_instance("456", "spuff-remove");

        db.save_instance(&instance).unwrap();
        assert!(db.get_active_instance().unwrap().is_some());

        db.remove_instance("456").unwrap();
        assert!(db.get_active_instance().unwrap().is_none());
    }

    #[test]
    fn test_get_active_instance_none() {
        let (db, _dir, _lock) = create_test_db();
        let result = db.get_active_instance().unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_list_instances() {
        let (db, _dir, _lock) = create_test_db();

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
        let (db, _dir, _lock) = create_test_db();
        let instance = create_test_instance("789", "spuff-ip-test");

        db.save_instance(&instance).unwrap();
        db.update_instance_ip("789", "192.168.1.100").unwrap();

        let retrieved = db.get_active_instance().unwrap().unwrap();
        assert_eq!(retrieved.ip, "192.168.1.100");
    }

    #[test]
    fn test_instance_replace_on_same_id() {
        let (db, _dir, _lock) = create_test_db();

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
