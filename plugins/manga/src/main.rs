use std::env;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tracing::{error, info, warn};

pub use mop_plugin_common::{archive, classify, error, paths};

mod cbz;
mod comicinfo;
mod config;
mod convert;
mod dispatch;
mod doctor;
mod image;
mod inspect;
mod layout;
mod rpc;
mod title;
mod watcher;
mod worker;

use mop_plugin_sdk::{RpcError, RpcRequest, RpcResponse};
use rpc::PluginState;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "mop_plugin_manga=info".into()),
        )
        .init();

    let plugin_id = env::var("MOP_PLUGIN_ID").unwrap_or_else(|_| "mop.manga".to_string());
    let socket_path = env::var("MOP_PLUGIN_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/run/mop/plugins/mop.manga.sock"));
    let host_socket = env::var("MOP_HOST_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/run/mop/host.sock"));

    info!("Starting {} on socket {}", plugin_id, socket_path.display());

    if socket_path.exists() {
        let _ = std::fs::remove_file(&socket_path);
    }
    if let Some(parent) = socket_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let listener = UnixListener::bind(&socket_path)?;
    let _ = std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o660));

    let state = PluginState::new(plugin_id, host_socket);

    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let state_clone = state.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream, state_clone).await {
                        error!("Connection error: {e}");
                    }
                });
            }
            Err(e) => {
                warn!("Accept error: {e}");
                break;
            }
        }
    }

    Ok(())
}

async fn handle_connection(
    stream: UnixStream,
    state: PluginState,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }

        let req: RpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let res = RpcResponse::error(None, RpcError::parse_error(e.to_string()));
                let bytes = serde_json::to_vec(&res)?;
                writer.write_all(&bytes).await?;
                writer.write_all(b"\n").await?;
                continue;
            }
        };

        let response = rpc::handle_rpc_request(req, &state).await;
        let bytes = serde_json::to_vec(&response)?;
        writer.write_all(&bytes).await?;
        writer.write_all(b"\n").await?;
    }

    Ok(())
}
