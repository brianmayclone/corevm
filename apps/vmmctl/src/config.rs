//! Client-side configuration — server contexts with token storage.
//!
//! Config file: ~/.vmmctl/config.toml
//! Tokens stored alongside in ~/.vmmctl/tokens/<context-name>

use serde::{Serialize, Deserialize};
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize)]
pub struct VmmctlConfig {
    #[serde(default = "default_context")]
    pub current_context: String,
    #[serde(default)]
    pub contexts: Vec<Context>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Context {
    pub name: String,
    pub server: String,
    #[serde(default)]
    pub insecure: bool,
}

fn default_context() -> String { "default".into() }

impl Default for VmmctlConfig {
    fn default() -> Self {
        Self { current_context: default_context(), contexts: Vec::new() }
    }
}

impl VmmctlConfig {
    /// Path to the config directory (~/.vmmctl/).
    pub fn config_dir() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".vmmctl")
    }

    /// Path to the config file.
    pub fn config_path() -> PathBuf {
        Self::config_dir().join("config.toml")
    }

    /// Path to token storage directory.
    fn tokens_dir() -> PathBuf {
        Self::config_dir().join("tokens")
    }

    /// Load config from disk (or return default).
    pub fn load() -> Self {
        let path = Self::config_path();
        if !path.exists() {
            return Self::default();
        }
        let content = std::fs::read_to_string(&path).unwrap_or_default();
        toml::from_str(&content).unwrap_or_default()
    }

    /// Save config to disk.
    pub fn save(&self) -> Result<(), String> {
        let dir = Self::config_dir();
        std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create config dir: {}", e))?;
        let content = toml::to_string_pretty(self)
            .map_err(|e| format!("Serialize error: {}", e))?;
        std::fs::write(Self::config_path(), content)
            .map_err(|e| format!("Write error: {}", e))
    }

    /// Get the current context.
    pub fn current(&self) -> Option<&Context> {
        self.contexts.iter().find(|c| c.name == self.current_context)
    }

    /// Get context by name.
    pub fn get_context(&self, name: &str) -> Option<&Context> {
        self.contexts.iter().find(|c| c.name == name)
    }

    /// Add or update a context.
    pub fn set_context(&mut self, name: &str, server: &str, insecure: bool) {
        if let Some(ctx) = self.contexts.iter_mut().find(|c| c.name == name) {
            ctx.server = server.to_string();
            ctx.insecure = insecure;
        } else {
            self.contexts.push(Context {
                name: name.to_string(),
                server: server.to_string(),
                insecure,
            });
        }
    }

    /// Remove a context.
    pub fn remove_context(&mut self, name: &str) -> bool {
        let len = self.contexts.len();
        self.contexts.retain(|c| c.name != name);
        // Also remove stored token
        let _ = Self::remove_token(name);
        self.contexts.len() < len
    }

    /// Store a JWT token for a context.
    pub fn store_token(context_name: &str, token: &str) -> Result<(), String> {
        let dir = Self::tokens_dir();
        std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create tokens dir: {}", e))?;
        let path = dir.join(context_name);
        // Set restrictive permissions before writing
        std::fs::write(&path, token).map_err(|e| format!("Token write error: {}", e))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }

    /// Load stored JWT token for a context.
    pub fn load_token(context_name: &str) -> Option<String> {
        // Check environment variable first
        if let Ok(token) = std::env::var("VMMCTL_TOKEN") {
            if !token.is_empty() {
                return Some(token);
            }
        }
        let path = Self::tokens_dir().join(context_name);
        std::fs::read_to_string(&path).ok().map(|s| s.trim().to_string())
    }

    /// Remove stored token for a context.
    pub fn remove_token(context_name: &str) -> Result<(), String> {
        let path = Self::tokens_dir().join(context_name);
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| format!("Cannot remove token: {}", e))?;
        }
        Ok(())
    }

    /// Resolve the server URL to use (CLI override > current context).
    pub fn resolve_server(&self, cli_server: Option<&str>) -> Result<String, String> {
        if let Some(s) = cli_server {
            return Ok(s.to_string());
        }
        self.current()
            .map(|c| c.server.clone())
            .ok_or_else(|| "No server configured. Run: vmmctl config set-server <url>".into())
    }

    /// Whether to skip TLS verification.
    pub fn resolve_insecure(&self, cli_insecure: bool) -> bool {
        if cli_insecure { return true; }
        self.current().map(|c| c.insecure).unwrap_or(false)
    }
}
