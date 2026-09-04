use mop_core::error::AppError;
pub use mop_plugin_sdk::*;
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

pub const DEFAULT_RPC_TIMEOUT: Duration = Duration::from_secs(10);

/// Unix socket client for JSON-RPC 2.0 communication (Host -> Plugin)
#[derive(Clone)]
pub struct UnixRpcClient {
    socket_path: PathBuf,
    timeout: Duration,
}

impl UnixRpcClient {
    pub fn new(socket_path: impl Into<PathBuf>) -> Self {
        Self {
            socket_path: socket_path.into(),
            timeout: DEFAULT_RPC_TIMEOUT,
        }
    }

    pub fn with_timeout(socket_path: impl Into<PathBuf>, timeout: Duration) -> Self {
        Self {
            socket_path: socket_path.into(),
            timeout,
        }
    }

    /// Send a JSON-RPC request and wait for the response with a strict 10s timeout
    pub async fn call(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, AppError> {
        let req_id = serde_json::Value::Number(1.into());
        let request = RpcRequest::new(req_id.clone(), method, params);
        let req_bytes = serde_json::to_vec(&request)
            .map_err(|e| AppError::Plugin(format!("Failed to serialize RPC request: {e}")))?;

        let fut = async {
            let stream = UnixStream::connect(&self.socket_path).await.map_err(|e| {
                AppError::Plugin(format!(
                    "Failed to connect to plugin socket {}: {e}",
                    self.socket_path.display()
                ))
            })?;

            let (reader, mut writer) = stream.into_split();
            writer
                .write_all(&req_bytes)
                .await
                .map_err(|e| AppError::Plugin(format!("Failed to write to plugin socket: {e}")))?;
            writer.write_all(b"\n").await.map_err(|e| {
                AppError::Plugin(format!("Failed to write newline to plugin socket: {e}"))
            })?;
            writer
                .flush()
                .await
                .map_err(|e| AppError::Plugin(format!("Failed to flush plugin socket: {e}")))?;

            let mut lines = BufReader::new(reader).lines();
            if let Some(line) = lines.next_line().await.map_err(|e| {
                AppError::Plugin(format!("Failed to read response from plugin socket: {e}"))
            })? {
                let res: RpcResponse = serde_json::from_str(&line).map_err(|e| {
                    AppError::Plugin(format!("Failed to parse JSON-RPC response '{line}': {e}"))
                })?;

                if let Some(err) = res.error {
                    return Err(AppError::Plugin(format!(
                        "RPC error [{}] {}: {:?}",
                        err.code, err.message, err.data
                    )));
                }

                Ok(res.result.unwrap_or(serde_json::Value::Null))
            } else {
                Err(AppError::Plugin(
                    "Connection closed by plugin without response".to_string(),
                ))
            }
        };

        match tokio::time::timeout(self.timeout, fut).await {
            Ok(res) => res,
            Err(_) => Err(AppError::Plugin(format!(
                "RPC call to method '{}' timed out after {:?}",
                method, self.timeout
            ))),
        }
    }

    /// Send a raw JSON-RPC request and return the RpcResponse
    pub async fn call_raw(&self, request: RpcRequest) -> Result<RpcResponse, AppError> {
        let req_bytes = serde_json::to_vec(&request)
            .map_err(|e| AppError::Plugin(format!("Failed to serialize RPC request: {e}")))?;

        let method = request.method.clone();
        let fut = async {
            let stream = UnixStream::connect(&self.socket_path).await.map_err(|e| {
                AppError::Plugin(format!(
                    "Failed to connect to plugin socket {}: {e}",
                    self.socket_path.display()
                ))
            })?;

            let (reader, mut writer) = stream.into_split();
            writer
                .write_all(&req_bytes)
                .await
                .map_err(|e| AppError::Plugin(format!("Failed to write to plugin socket: {e}")))?;
            writer.write_all(b"\n").await.map_err(|e| {
                AppError::Plugin(format!("Failed to write newline to plugin socket: {e}"))
            })?;
            writer
                .flush()
                .await
                .map_err(|e| AppError::Plugin(format!("Failed to flush plugin socket: {e}")))?;

            let mut lines = BufReader::new(reader).lines();
            if let Some(line) = lines.next_line().await.map_err(|e| {
                AppError::Plugin(format!("Failed to read response from plugin socket: {e}"))
            })? {
                let res: RpcResponse = serde_json::from_str(&line).map_err(|e| {
                    AppError::Plugin(format!("Failed to parse JSON-RPC response '{line}': {e}"))
                })?;
                Ok(res)
            } else {
                Err(AppError::Plugin(
                    "Connection closed by plugin without response".to_string(),
                ))
            }
        };

        match tokio::time::timeout(self.timeout, fut).await {
            Ok(res) => res,
            Err(_) => Err(AppError::Plugin(format!(
                "RPC call to method '{}' timed out after {:?}",
                method, self.timeout
            ))),
        }
    }

    /// Send a JSON-RPC notification (no response expected)
    pub async fn notify(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<(), AppError> {
        let notification = RpcNotification::new(method, params);
        let notif_bytes = serde_json::to_vec(&notification)
            .map_err(|e| AppError::Plugin(format!("Failed to serialize notification: {e}")))?;

        let fut = async {
            let mut stream = UnixStream::connect(&self.socket_path).await.map_err(|e| {
                AppError::Plugin(format!(
                    "Failed to connect to socket {}: {e}",
                    self.socket_path.display()
                ))
            })?;

            stream
                .write_all(&notif_bytes)
                .await
                .map_err(|e| AppError::Plugin(format!("Failed to write notification: {e}")))?;
            stream
                .write_all(b"\n")
                .await
                .map_err(|e| AppError::Plugin(format!("Failed to write newline: {e}")))?;
            stream
                .flush()
                .await
                .map_err(|e| AppError::Plugin(format!("Failed to flush: {e}")))?;

            Ok(())
        };

        match tokio::time::timeout(self.timeout, fut).await {
            Ok(res) => res,
            Err(_) => Err(AppError::Plugin(format!(
                "RPC notification '{}' timed out after {:?}",
                method, self.timeout
            ))),
        }
    }
}
