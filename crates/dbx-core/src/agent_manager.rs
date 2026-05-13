use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::db::agent_driver::AgentDriverClient;
use crate::models::connection::DatabaseType;

pub const DEFAULT_JRE_KEY: &str = "17";

fn default_jre_key() -> String {
    DEFAULT_JRE_KEY.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRegistry {
    #[serde(default)]
    pub jre: Option<JreInfo>,
    #[serde(default)]
    pub jres: std::collections::HashMap<String, JreInfo>,
    pub drivers: std::collections::HashMap<String, DriverInfo>,
}

impl AgentRegistry {
    pub fn resolve_jre(&self, key: &str) -> Option<&JreInfo> {
        if !self.jres.is_empty() {
            return self.jres.get(key);
        }
        if key == DEFAULT_JRE_KEY {
            self.jre.as_ref()
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JreInfo {
    pub version: String,
    pub platforms: std::collections::HashMap<String, ArtifactInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriverInfo {
    pub version: String,
    pub label: String,
    pub min_app_version: String,
    pub jar: ArtifactInfo,
    #[serde(default = "default_jre_key")]
    pub jre: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactInfo {
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentState {
    #[serde(default)]
    pub jre_version: Option<String>,
    #[serde(default)]
    pub jre_versions: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub installed_drivers: std::collections::HashMap<String, InstalledDriver>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledDriver {
    pub version: String,
    pub installed_at: String,
    #[serde(default = "default_jre_key")]
    pub jre: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDriverInfo {
    pub db_type: String,
    pub label: String,
    pub version: String,
    pub size: u64,
    pub installed: bool,
    pub installed_version: Option<String>,
    pub update_available: bool,
    pub jre: String,
    pub jre_installed: bool,
}

pub struct AgentManager {
    base_dir: PathBuf,
    daemons: Mutex<std::collections::HashMap<String, AgentDriverClient>>,
}

impl AgentManager {
    pub fn new() -> Self {
        let home =
            std::env::var(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).unwrap_or_else(|_| ".".to_string());
        let mgr = Self {
            base_dir: PathBuf::from(home).join(".dbx").join("agents"),
            daemons: Mutex::new(std::collections::HashMap::new()),
        };
        mgr.migrate_legacy_jre();
        mgr
    }

    fn migrate_legacy_jre(&self) {
        let legacy = self.base_dir.join("jre");
        let versioned = self.jre_dir(DEFAULT_JRE_KEY);
        if legacy.exists() && !versioned.exists() {
            let _ = std::fs::rename(&legacy, &versioned);
        }
    }

    pub fn base_dir(&self) -> &PathBuf {
        &self.base_dir
    }

    pub fn jre_dir(&self, jre_key: &str) -> PathBuf {
        self.base_dir.join(format!("jre-{jre_key}"))
    }

    pub fn jre_java_path(&self, jre_key: &str) -> PathBuf {
        let dir = self.jre_dir(jre_key);
        if cfg!(windows) {
            dir.join("bin").join("java.exe")
        } else {
            dir.join("bin").join("java")
        }
    }

    pub fn driver_jar_path(&self, db_type: &str) -> PathBuf {
        self.base_dir.join("drivers").join(db_type).join("agent.jar")
    }

    fn state_path(&self) -> PathBuf {
        self.base_dir.join("state.json")
    }

    pub fn load_state(&self) -> AgentState {
        std::fs::read_to_string(self.state_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(
            AgentState { jre_version: None, jre_versions: Default::default(), installed_drivers: Default::default() },
        )
    }

    pub fn save_state(&self, state: &AgentState) -> Result<(), String> {
        let dir = self.base_dir.clone();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let json = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
        std::fs::write(self.state_path(), json).map_err(|e| e.to_string())
    }

    pub fn is_jre_installed(&self, jre_key: &str) -> bool {
        self.jre_java_path(jre_key).exists()
    }

    pub fn is_driver_installed(&self, db_type: &str) -> bool {
        self.driver_jar_path(db_type).exists()
    }

    pub fn db_type_to_agent_key(db_type: &DatabaseType) -> Option<&'static str> {
        match db_type {
            DatabaseType::Dameng => Some("dameng"),
            DatabaseType::Kingbase => Some("kingbase"),
            DatabaseType::Vastbase => Some("vastbase"),
            DatabaseType::Goldendb => Some("goldendb"),
            DatabaseType::Oracle => Some("oracle"),
            DatabaseType::H2 => Some("h2"),
            DatabaseType::Snowflake => Some("snowflake"),
            DatabaseType::Trino => Some("trino"),
            DatabaseType::Hive => Some("hive"),
            DatabaseType::Db2 => Some("db2"),
            DatabaseType::Informix => Some("informix"),
            DatabaseType::Neo4j => Some("neo4j"),
            DatabaseType::Cassandra => Some("cassandra"),
            DatabaseType::Bigquery => Some("bigquery"),
            DatabaseType::Kylin => Some("kylin"),
            DatabaseType::Sundb => Some("sundb"),
            DatabaseType::Gaussdb => Some("gaussdb"),
            _ => None,
        }
    }

    pub fn is_agent_type(db_type: &DatabaseType) -> bool {
        Self::db_type_to_agent_key(db_type).is_some()
    }

    pub async fn spawn(&self, db_type: &DatabaseType) -> Result<AgentDriverClient, String> {
        let key = Self::db_type_to_agent_key(db_type)
            .ok_or_else(|| format!("{:?} is not an agent-driven database type", db_type))?;

        let state = self.load_state();
        let jre_key = state.installed_drivers.get(key).map(|d| d.jre.as_str()).unwrap_or(DEFAULT_JRE_KEY);

        if !self.is_jre_installed(jre_key) {
            return Err(format!("JRE {jre_key} runtime is not installed. Please install it from the Driver Manager."));
        }
        if !self.is_driver_installed(key) {
            return Err(format!("{key} driver is not installed. Please install it from the Driver Manager."));
        }

        let java = self.jre_java_path(jre_key).to_string_lossy().to_string();
        let jar = self.driver_jar_path(key).to_string_lossy().to_string();
        AgentDriverClient::spawn(&java, &jar).await
    }

    pub async fn call_daemon<T: serde::de::DeserializeOwned + Send + 'static>(
        &self,
        db_type: &DatabaseType,
        method: &str,
        params: serde_json::Value,
    ) -> Result<T, String> {
        let key = Self::db_type_to_agent_key(db_type)
            .ok_or_else(|| format!("{:?} is not an agent-driven database type", db_type))?
            .to_string();

        let mut daemons = self.daemons.lock().await;

        if !daemons.contains_key(&key) {
            let state = self.load_state();
            let jre_key = state.installed_drivers.get(&key).map(|d| d.jre.as_str()).unwrap_or(DEFAULT_JRE_KEY);

            if !self.is_jre_installed(jre_key) {
                return Err(format!(
                    "JRE {jre_key} runtime is not installed. Please install it from the Driver Manager."
                ));
            }
            if !self.is_driver_installed(&key) {
                return Err(format!("{key} driver is not installed. Please install it from the Driver Manager."));
            }
            let java = self.jre_java_path(jre_key).to_string_lossy().to_string();
            let jar = self.driver_jar_path(&key).to_string_lossy().to_string();
            let client = AgentDriverClient::spawn(&java, &jar).await?;
            daemons.insert(key.clone(), client);
        }

        let client = daemons.get_mut(&key).unwrap();
        match client.call::<T>(method, params.clone()).await {
            Ok(result) => Ok(result),
            Err(e) => {
                log::warn!("[agent] daemon call failed, respawning: {e}");
                daemons.remove(&key);
                let state = self.load_state();
                let jre_key = state.installed_drivers.get(&key).map(|d| d.jre.as_str()).unwrap_or(DEFAULT_JRE_KEY);
                let java = self.jre_java_path(jre_key).to_string_lossy().to_string();
                let jar = self.driver_jar_path(&key).to_string_lossy().to_string();
                let mut new_client = AgentDriverClient::spawn(&java, &jar).await?;
                let result = new_client.call::<T>(method, params).await?;
                daemons.insert(key, new_client);
                Ok(result)
            }
        }
    }

    pub async fn download_file(url: &str, dest: &Path) -> Result<(), String> {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let resp = reqwest::get(url).await.map_err(|e| format!("Download failed: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("Download failed with status: {}", resp.status()));
        }
        let bytes = resp.bytes().await.map_err(|e| format!("Download read failed: {e}"))?;
        std::fs::write(dest, &bytes).map_err(|e| format!("Failed to write file: {e}"))
    }

    pub fn current_platform() -> &'static str {
        if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
            "macos-aarch64"
        } else if cfg!(target_os = "macos") && cfg!(target_arch = "x86_64") {
            "macos-x64"
        } else if cfg!(target_os = "linux") && cfg!(target_arch = "aarch64") {
            "linux-aarch64"
        } else if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
            "linux-x64"
        } else if cfg!(target_os = "windows") && cfg!(target_arch = "aarch64") {
            "windows-aarch64"
        } else if cfg!(target_os = "windows") && cfg!(target_arch = "x86_64") {
            "windows-x64"
        } else {
            "unknown"
        }
    }
}
