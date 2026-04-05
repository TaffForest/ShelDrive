use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::Mutex;

const MAX_RESTART_ATTEMPTS: u8 = 3;

// ---------------------------------------------------------------------------
// JSON-RPC types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: &'static str,
    id: u64,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    #[allow(dead_code)]
    jsonrpc: String,
    id: u64,
    result: Option<serde_json::Value>,
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i32,
    message: String,
    #[allow(dead_code)]
    data: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Shelby operation types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PinResult {
    pub cid: String,
    pub size_bytes: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrieveResult {
    pub content: String,
    pub size_bytes: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnpinResult {
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListResult {
    pub cids: Vec<String>,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShelbyStatus {
    pub connected: bool,
    pub network: String,
    pub node_url: Option<String>,
}

// ---------------------------------------------------------------------------
// Bridge config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ShelbyConfig {
    pub network: String,
    pub api_key: Option<String>,
    pub rpc_url: Option<String>,
    pub private_key: Option<String>,
}

impl ShelbyConfig {
    pub fn load() -> Self {
        let config_path = dirs::home_dir()
            .expect("Could not determine home directory")
            .join(".sheldrive")
            .join("config.toml");

        if let Ok(content) = std::fs::read_to_string(&config_path) {
            Self::parse_toml(&content)
        } else {
            info!("No config file found at {:?} — using defaults", config_path);
            Self::default()
        }
    }

    fn parse_toml(content: &str) -> Self {
        let mut config = Self::default();
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('#') || line.starts_with('[') || line.is_empty() {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim();
                let value = value.trim().trim_matches('"');
                match key {
                    "network" => config.network = value.to_string(),
                    "api_key" => config.api_key = Some(value.to_string()),
                    "rpc_url" => config.rpc_url = Some(value.to_string()),
                    "private_key" => config.private_key = Some(value.to_string()),
                    _ => {}
                }
            }
        }
        config
    }

    fn default() -> Self {
        Self {
            network: "SHELBYNET".to_string(),
            api_key: None,
            rpc_url: None,
            private_key: None,
        }
    }
}

// ---------------------------------------------------------------------------
// ShelbyBridge
// ---------------------------------------------------------------------------

pub struct ShelbyBridge {
    child: Mutex<Option<Child>>,
    next_id: AtomicU64,
    restart_count: AtomicU8,
    config: ShelbyConfig,
    sidecar_path: String,
}

impl ShelbyBridge {
    pub fn new(sidecar_path: &str) -> Self {
        let config = ShelbyConfig::load();
        info!("Shelby config loaded: network={}", config.network);

        Self {
            child: Mutex::new(None),
            next_id: AtomicU64::new(1),
            restart_count: AtomicU8::new(0),
            config,
            sidecar_path: sidecar_path.to_string(),
        }
    }

    pub fn start(&self) -> Result<(), String> {
        self.start_inner()
    }

    fn start_inner(&self) -> Result<(), String> {
        let mut child_guard = self.child.lock().map_err(|e| e.to_string())?;

        // Check if child is still alive
        if let Some(ref mut child) = *child_guard {
            match child.try_wait() {
                Ok(None) => return Ok(()), // Still running
                Ok(Some(status)) => {
                    warn!("Sidecar exited with status: {}", status);
                    *child_guard = None;
                }
                Err(e) => {
                    warn!("Failed to check sidecar status: {}", e);
                    *child_guard = None;
                }
            }
        }

        info!("Starting sidecar: {}", self.sidecar_path);

        let mut cmd = Command::new("node");
        cmd.arg(&self.sidecar_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        cmd.env("SHELBY_NETWORK", &self.config.network);
        if let Some(ref key) = self.config.api_key {
            cmd.env("SHELBY_API_KEY", key);
        }
        if let Some(ref url) = self.config.rpc_url {
            cmd.env("SHELBY_RPC_URL", url);
        }
        if let Some(ref pk) = self.config.private_key {
            cmd.env("SHELBY_PRIVATE_KEY", pk);
        }

        let child = cmd
            .spawn()
            .map_err(|e| format!("Failed to spawn sidecar: {}", e))?;
        info!("Sidecar started (pid={})", child.id());

        *child_guard = Some(child);
        self.restart_count.store(0, Ordering::Relaxed);
        Ok(())
    }

    pub fn stop(&self) -> Result<(), String> {
        let mut child_guard = self.child.lock().map_err(|e| e.to_string())?;
        if let Some(mut child) = child_guard.take() {
            info!("Stopping sidecar (pid={})", child.id());
            let _ = child.kill();
            let _ = child.wait();
        }
        Ok(())
    }

    /// Check if sidecar is alive, restart if crashed.
    fn ensure_alive(&self) -> Result<(), String> {
        let needs_restart = {
            let mut guard = self.child.lock().map_err(|e| e.to_string())?;
            match guard.as_mut() {
                None => true,
                Some(child) => match child.try_wait() {
                    Ok(None) => false, // Still running
                    Ok(Some(status)) => {
                        warn!("Sidecar exited unexpectedly: {}", status);
                        *guard = None;
                        true
                    }
                    Err(_) => {
                        *guard = None;
                        true
                    }
                },
            }
        };

        if needs_restart {
            let count = self.restart_count.fetch_add(1, Ordering::Relaxed);
            if count >= MAX_RESTART_ATTEMPTS {
                return Err(format!(
                    "Sidecar crashed {} times — not restarting. Restart the app.",
                    count
                ));
            }
            warn!(
                "Auto-restarting sidecar (attempt {}/{})",
                count + 1,
                MAX_RESTART_ATTEMPTS
            );
            self.start_inner()?;
        }

        Ok(())
    }

    /// Send a JSON-RPC request and wait for the response.
    /// Auto-restarts sidecar if it has crashed.
    fn call(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, String> {
        self.ensure_alive()?;

        let mut child_guard = self.child.lock().map_err(|e| e.to_string())?;
        let child = child_guard
            .as_mut()
            .ok_or_else(|| "Sidecar not running".to_string())?;

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);

        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            id,
            method: method.to_string(),
            params,
        };

        let request_line =
            serde_json::to_string(&request).map_err(|e| format!("Serialize error: {}", e))?;
        debug!("→ sidecar: {}", request_line);

        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| "Sidecar stdin not available".to_string())?;
        if let Err(e) = writeln!(stdin, "{}", request_line) {
            // Pipe broken — sidecar probably crashed
            error!("Write to sidecar failed: {} — will restart on next call", e);
            *child_guard = None;
            return Err(format!("Sidecar pipe broken: {}", e));
        }
        if let Err(e) = stdin.flush() {
            error!("Flush to sidecar failed: {}", e);
            *child_guard = None;
            return Err(format!("Sidecar flush failed: {}", e));
        }

        let stdout = child
            .stdout
            .as_mut()
            .ok_or_else(|| "Sidecar stdout not available".to_string())?;

        let mut reader = BufReader::new(stdout);
        let mut response_line = String::new();
        match reader.read_line(&mut response_line) {
            Ok(0) => {
                error!("Sidecar stdout closed (EOF)");
                *child_guard = None;
                return Err("Sidecar closed unexpectedly".to_string());
            }
            Ok(_) => {}
            Err(e) => {
                error!("Read from sidecar failed: {}", e);
                *child_guard = None;
                return Err(format!("Sidecar read failed: {}", e));
            }
        }

        debug!("← sidecar: {}", response_line.trim());

        let response: JsonRpcResponse = serde_json::from_str(&response_line).map_err(|e| {
            format!(
                "Parse sidecar response failed: {} (raw: {})",
                e,
                response_line.trim()
            )
        })?;

        if response.id != id {
            warn!(
                "Response ID mismatch: expected {}, got {}",
                id, response.id
            );
        }

        if let Some(err) = response.error {
            return Err(format!("Sidecar error [{}]: {}", err.code, err.message));
        }

        response
            .result
            .ok_or_else(|| "Sidecar returned null result".to_string())
    }

    // ---------------------------------------------------------------------------
    // Public API
    // ---------------------------------------------------------------------------

    pub fn ping(&self) -> Result<bool, String> {
        let result = self.call("shelby.ping", None)?;
        Ok(result
            .get("pong")
            .and_then(|v| v.as_bool())
            .unwrap_or(false))
    }

    pub fn status(&self) -> Result<ShelbyStatus, String> {
        let result = self.call("shelby.status", None)?;
        serde_json::from_value(result).map_err(|e| format!("Parse status failed: {}", e))
    }

    pub fn pin(&self, content_base64: &str, filename: Option<&str>) -> Result<PinResult, String> {
        let mut params = serde_json::json!({ "content": content_base64 });
        if let Some(name) = filename {
            params["filename"] = serde_json::Value::String(name.to_string());
        }
        let result = self.call("shelby.pin", Some(params))?;
        serde_json::from_value(result).map_err(|e| format!("Parse pin result failed: {}", e))
    }

    pub fn retrieve(&self, cid: &str) -> Result<RetrieveResult, String> {
        let params = serde_json::json!({ "cid": cid });
        let result = self.call("shelby.retrieve", Some(params))?;
        serde_json::from_value(result)
            .map_err(|e| format!("Parse retrieve result failed: {}", e))
    }

    pub fn unpin(&self, cid: &str) -> Result<UnpinResult, String> {
        let params = serde_json::json!({ "cid": cid });
        let result = self.call("shelby.unpin", Some(params))?;
        serde_json::from_value(result).map_err(|e| format!("Parse unpin result failed: {}", e))
    }

    pub fn list(&self) -> Result<ListResult, String> {
        let result = self.call("shelby.list", None)?;
        serde_json::from_value(result).map_err(|e| format!("Parse list result failed: {}", e))
    }

    /// Check if the sidecar process is alive.
    pub fn is_alive(&self) -> bool {
        if let Ok(mut guard) = self.child.lock() {
            if let Some(ref mut child) = *guard {
                return matches!(child.try_wait(), Ok(None));
            }
        }
        false
    }

    /// Reset restart counter (call after successful operation).
    pub fn reset_restart_count(&self) {
        self.restart_count.store(0, Ordering::Relaxed);
    }
}

impl Drop for ShelbyBridge {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
