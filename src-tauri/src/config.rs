use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

pub const DEFAULT_SERVER_ID: &str = "default";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Server {
    pub id: String,
    pub name: String,
    pub url: String,
    /// "none" | "basic" | "token"
    #[serde(default = "default_auth")]
    pub auth_type: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Subscription {
    pub id: String,
    pub server_id: String,
    pub topic: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub servers: Vec<Server>,
    pub subscriptions: Vec<Subscription>,
}

fn default_auth() -> String {
    "none".into()
}

fn default_true() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Config {
            servers: vec![Server {
                id: DEFAULT_SERVER_ID.into(),
                name: "ntfy.sh".into(),
                url: "https://ntfy.sh".into(),
                auth_type: default_auth(),
                username: String::new(),
                password: String::new(),
                token: String::new(),
            }],
            subscriptions: vec![],
        }
    }
}

impl Config {
    /// Ensures the default server always exists.
    fn normalize(mut self) -> Self {
        if !self.servers.iter().any(|s| s.id == DEFAULT_SERVER_ID) {
            self.servers.insert(0, Config::default().servers.remove(0));
        }
        self
    }
}

fn path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    Ok(dir.join("config.json"))
}

pub fn load(app: &AppHandle) -> Config {
    path(app)
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str::<Config>(&s).ok())
        .unwrap_or_default()
        .normalize()
}

pub fn save(app: &AppHandle, config: &Config) -> Result<Config, String> {
    let config = config.clone().normalize();
    let p = path(app)?;
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    std::fs::write(&p, json).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        // Config may contain credentials; keep it private to the user.
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
    }
    Ok(config)
}
