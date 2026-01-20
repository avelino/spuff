//! SSH port forwarding (tunneling).
//!
//! Provides local port forwarding functionality.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use russh::client::Handle;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use crate::error::{Result, SpuffError};
use crate::ssh::client::ClientHandler;

/// A port forward handle that can be used to stop the tunnel.
pub struct PortForward {
    running: Arc<AtomicBool>,
    task: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl PortForward {
    /// Stop the tunnel.
    pub async fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);

        let mut task_guard = self.task.lock().await;
        if let Some(task) = task_guard.take() {
            task.abort();
        }
    }
}

/// Create a local port forward.
///
/// This binds to `local_port` on localhost and forwards all connections
/// to `remote_port` on the remote host via SSH.
pub async fn create_local_forward(
    session: Arc<Mutex<Handle<ClientHandler>>>,
    _remote_host: &str,
    local_port: u16,
    remote_port: u16,
) -> Result<PortForward> {
    // Bind local port
    let listener = TcpListener::bind(format!("127.0.0.1:{}", local_port))
        .await
        .map_err(|e| SpuffError::Ssh(format!("Failed to bind local port {}: {}", local_port, e)))?;

    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();

    // Spawn the forwarding task
    let task = tokio::spawn(async move {
        while running_clone.load(Ordering::SeqCst) {
            // Accept connections with timeout for shutdown check
            let accept_result = tokio::time::timeout(
                std::time::Duration::from_secs(1),
                listener.accept(),
            )
            .await;

            let (mut local_stream, peer_addr) = match accept_result {
                Ok(Ok(conn)) => conn,
                Ok(Err(e)) => {
                    tracing::debug!("Accept error: {}", e);
                    continue;
                }
                Err(_) => continue, // timeout, check running flag
            };

            tracing::info!("Tunnel: accepted connection from {}", peer_addr);

            // Open channel to remote - use 127.0.0.1 instead of localhost
            let channel_result = {
                let session_guard = session.lock().await;
                session_guard
                    .channel_open_direct_tcpip(
                        "127.0.0.1",
                        remote_port as u32,
                        "127.0.0.1",
                        local_port as u32,
                    )
                    .await
            };

            let channel = match channel_result {
                Ok(ch) => {
                    tracing::info!("Tunnel: opened channel to 127.0.0.1:{}", remote_port);
                    ch
                }
                Err(e) => {
                    tracing::error!("Tunnel: failed to open channel: {}", e);
                    // Close local connection since we can't forward
                    drop(local_stream);
                    continue;
                }
            };

            // Get the channel stream
            let mut channel_stream = channel.into_stream();

            // Spawn bidirectional copy
            tokio::spawn(async move {
                match tokio::io::copy_bidirectional(&mut local_stream, &mut channel_stream).await {
                    Ok((to_remote, from_remote)) => {
                        tracing::debug!(
                            "Tunnel: connection closed. Sent {} bytes, received {} bytes",
                            to_remote, from_remote
                        );
                    }
                    Err(e) => {
                        tracing::debug!("Tunnel: copy error: {}", e);
                    }
                }
            });
        }
    });

    Ok(PortForward {
        running,
        task: Arc::new(Mutex::new(Some(task))),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    #[tokio::test]
    async fn test_port_forward_stop() {
        let running = Arc::new(AtomicBool::new(true));
        let forward = PortForward {
            running: running.clone(),
            task: Arc::new(Mutex::new(None)),
        };

        assert!(running.load(Ordering::SeqCst));
        forward.stop().await;
        assert!(!running.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_port_forward_stop_aborts_task() {
        let running = Arc::new(AtomicBool::new(true));
        let task = tokio::spawn(async {
            // Simulate long-running task
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        });

        let forward = PortForward {
            running: running.clone(),
            task: Arc::new(Mutex::new(Some(task))),
        };

        forward.stop().await;

        // Task should be aborted and removed
        let task_guard = forward.task.lock().await;
        assert!(task_guard.is_none());
    }

    #[tokio::test]
    async fn test_tcp_listener_binds_to_port() {
        // Test that we can bind to a random port
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(port > 0);
    }

    #[tokio::test]
    async fn test_tcp_bidirectional_copy() {
        // Create a simple echo server
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        // Spawn echo server
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            loop {
                match socket.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        socket.write_all(&buf[..n]).await.unwrap();
                    }
                    Err(_) => break,
                }
            }
        });

        // Connect and test echo
        let mut client = TcpStream::connect(format!("127.0.0.1:{}", port))
            .await
            .unwrap();

        let test_data = b"hello tunnel";
        client.write_all(test_data).await.unwrap();

        let mut response = vec![0u8; test_data.len()];
        client.read_exact(&mut response).await.unwrap();

        assert_eq!(&response, test_data);
    }
}
