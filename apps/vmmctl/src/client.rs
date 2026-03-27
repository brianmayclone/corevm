//! HTTP client for communicating with vmm-server / vmm-cluster REST API.

use reqwest::{Client, Response, StatusCode};
use serde::de::DeserializeOwned;
use crate::config::VmmctlConfig;

pub struct ApiClient {
    http: Client,
    base_url: String,
    token: Option<String>,
}

impl ApiClient {
    /// Create a new API client from CLI args and config.
    pub fn from_cli(cli: &crate::Cli) -> Result<Self, String> {
        let cfg = VmmctlConfig::load();
        let base_url = cfg.resolve_server(cli.server.as_deref())?;
        let insecure = cfg.resolve_insecure(cli.insecure);

        let token = cfg.current()
            .and_then(|ctx| VmmctlConfig::load_token(&ctx.name));

        let http = Client::builder()
            .danger_accept_invalid_certs(insecure)
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| format!("HTTP client error: {}", e))?;

        Ok(Self { http, base_url: base_url.trim_end_matches('/').to_string(), token })
    }

    /// Create a client with a specific server URL and no auth (for login).
    pub fn unauthenticated(server: &str, insecure: bool) -> Result<Self, String> {
        let http = Client::builder()
            .danger_accept_invalid_certs(insecure)
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| format!("HTTP client error: {}", e))?;

        Ok(Self { http, base_url: server.trim_end_matches('/').to_string(), token: None })
    }

    /// Set auth token.
    pub fn set_token(&mut self, token: String) {
        self.token = Some(token);
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    fn auth_header(&self) -> Option<String> {
        self.token.as_ref().map(|t| format!("Bearer {}", t))
    }

    /// GET request returning parsed JSON.
    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, String> {
        let mut req = self.http.get(self.url(path));
        if let Some(auth) = self.auth_header() {
            req = req.header("Authorization", auth);
        }
        let resp = req.send().await.map_err(|e| connection_error(&e))?;
        parse_response(resp).await
    }

    /// GET request returning raw bytes (for screenshots etc).
    pub async fn get_bytes(&self, path: &str) -> Result<Vec<u8>, String> {
        let mut req = self.http.get(self.url(path));
        if let Some(auth) = self.auth_header() {
            req = req.header("Authorization", auth);
        }
        let resp = req.send().await.map_err(|e| connection_error(&e))?;
        check_status(&resp)?;
        resp.bytes().await.map(|b| b.to_vec()).map_err(|e| format!("Read error: {}", e))
    }

    /// POST request with JSON body.
    pub async fn post<B: serde::Serialize, T: DeserializeOwned>(&self, path: &str, body: &B) -> Result<T, String> {
        let mut req = self.http.post(self.url(path)).json(body);
        if let Some(auth) = self.auth_header() {
            req = req.header("Authorization", auth);
        }
        let resp = req.send().await.map_err(|e| connection_error(&e))?;
        parse_response(resp).await
    }

    /// POST request with no body.
    pub async fn post_empty<T: DeserializeOwned>(&self, path: &str) -> Result<T, String> {
        let mut req = self.http.post(self.url(path));
        if let Some(auth) = self.auth_header() {
            req = req.header("Authorization", auth);
        }
        let resp = req.send().await.map_err(|e| connection_error(&e))?;
        parse_response(resp).await
    }

    /// PUT request with JSON body.
    pub async fn put<B: serde::Serialize, T: DeserializeOwned>(&self, path: &str, body: &B) -> Result<T, String> {
        let mut req = self.http.put(self.url(path)).json(body);
        if let Some(auth) = self.auth_header() {
            req = req.header("Authorization", auth);
        }
        let resp = req.send().await.map_err(|e| connection_error(&e))?;
        parse_response(resp).await
    }

    /// DELETE request.
    pub async fn delete<T: DeserializeOwned>(&self, path: &str) -> Result<T, String> {
        let mut req = self.http.delete(self.url(path));
        if let Some(auth) = self.auth_header() {
            req = req.header("Authorization", auth);
        }
        let resp = req.send().await.map_err(|e| connection_error(&e))?;
        parse_response(resp).await
    }

    /// Upload a file via multipart POST.
    pub async fn upload_file<T: DeserializeOwned>(&self, path: &str, file_path: &str) -> Result<T, String> {
        let file_name = std::path::Path::new(file_path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "upload".into());

        let data = tokio::fs::read(file_path).await
            .map_err(|e| format!("Cannot read file '{}': {}", file_path, e))?;

        let part = reqwest::multipart::Part::bytes(data)
            .file_name(file_name)
            .mime_str("application/octet-stream")
            .map_err(|e| format!("MIME error: {}", e))?;

        let form = reqwest::multipart::Form::new().part("file", part);

        let mut req = self.http.post(self.url(path)).multipart(form);
        if let Some(auth) = self.auth_header() {
            req = req.header("Authorization", auth);
        }
        let resp = req.send().await.map_err(|e| connection_error(&e))?;
        parse_response(resp).await
    }
}

fn connection_error(e: &reqwest::Error) -> String {
    if e.is_connect() {
        format!("Connection failed: {} — is the server running?", e)
    } else if e.is_timeout() {
        "Request timed out".into()
    } else {
        format!("Request failed: {}", e)
    }
}

fn check_status(resp: &Response) -> Result<(), String> {
    if resp.status().is_success() {
        return Ok(());
    }
    match resp.status() {
        StatusCode::UNAUTHORIZED => Err("Unauthorized — run 'vmmctl login' first".into()),
        StatusCode::FORBIDDEN => Err("Access denied — insufficient permissions".into()),
        status => Err(format!("Server returned {}", status)),
    }
}

async fn parse_response<T: DeserializeOwned>(resp: Response) -> Result<T, String> {
    let status = resp.status();
    if status.is_success() {
        return resp.json().await.map_err(|e| format!("Invalid response: {}", e));
    }

    // Try to extract error message from JSON response
    let body = resp.text().await.unwrap_or_default();
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
        if let Some(err) = json.get("error").and_then(|e| e.as_str()) {
            return match status {
                StatusCode::UNAUTHORIZED => Err(format!("Unauthorized: {} — run 'vmmctl login'", err)),
                StatusCode::FORBIDDEN => Err(format!("Access denied: {}", err)),
                _ => Err(format!("Error ({}): {}", status.as_u16(), err)),
            };
        }
    }

    Err(format!("Server returned {} {}", status.as_u16(), body))
}
