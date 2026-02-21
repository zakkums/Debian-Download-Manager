//! Control socket: server (during `ddm run`) and client (for `ddm pause`).
//! Protocol: one line per command: "pause <id>" or "cancel <id>".

use anyhow::Result;
use ddm_core::control::JobControl;
use std::path::Path;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::UnixListener;

/// Spawns a task that listens on `path` and calls `job_control.request_abort(id)`
/// for each "pause <id>" or "cancel <id>" line. Ignores malformed lines.
pub fn spawn_control_listener(
    job_control: Arc<JobControl>,
    path: impl AsRef<Path>,
) -> Result<tokio::task::JoinHandle<()>> {
    let path = path.as_ref().to_path_buf();
    let handle = tokio::spawn(async move {
        let _ = std::fs::remove_file(&path);
        let listener = match UnixListener::bind(&path) {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!(path = %path.display(), "control socket bind: {}", e);
                return;
            }
        };
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let control = Arc::clone(&job_control);
                    tokio::spawn(async move {
                        let mut reader = BufReader::new(stream).lines();
                        while let Ok(Some(line)) = reader.next_line().await {
                            let line = line.trim();
                            if line.starts_with("pause ") {
                                if let Ok(id) = line[6..].trim().parse::<i64>() {
                                    control.request_abort(id);
                                }
                            } else if line.starts_with("cancel ") {
                                if let Ok(id) = line[7..].trim().parse::<i64>() {
                                    control.request_abort(id);
                                }
                            }
                        }
                    });
                }
                Err(e) => tracing::debug!("control socket accept: {}", e),
            }
        }
    });
    Ok(handle)
}

/// Sends "pause <job_id>\n" to the control socket. No-op if the path does not exist.
pub async fn send_pause(socket_path: &Path, job_id: i64) -> Result<()> {
    if !socket_path.exists() {
        return Ok(());
    }
    let mut stream = tokio::net::UnixStream::connect(socket_path).await?;
    let msg = format!("pause {}\n", job_id);
    tokio::io::AsyncWriteExt::write_all(&mut stream, msg.as_bytes()).await?;
    Ok(())
}
