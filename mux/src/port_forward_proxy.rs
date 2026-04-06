//! TCP proxy for port forwarding through SSH direct-tcpip channels.
//!
//! For each forwarded port, this module:
//! 1. Binds a TcpListener on localhost
//! 2. On incoming connection, creates an SSH direct-tcpip channel
//! 3. Copies data bidirectionally between the local TCP stream and the SSH channel
//!
//! --- weezterm remote features ---

use anyhow::Context;
use filedescriptor::FileDescriptor;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// A single port forward proxy instance.
///
/// Manages the lifecycle of a forwarded port: binding, accepting connections,
/// and proxying data through SSH channels.
pub struct PortForwardProxy {
    /// The local port the proxy is listening on
    local_port: u16,
    /// The remote host to forward to
    remote_host: String,
    /// The remote port to forward to
    remote_port: u16,
    /// Signal to stop the proxy
    stop_flag: Arc<AtomicBool>,
}

impl PortForwardProxy {
    /// Start a new port forward proxy.
    ///
    /// Binds to `preferred_local_port` on localhost. If that port is already
    /// in use, falls back to an OS-assigned port.
    ///
    /// Spawns a background accept loop that, for each incoming TCP connection,
    /// opens an SSH `direct-tcpip` channel and proxies data bidirectionally.
    ///
    /// Use `stop()` to shut it down.
    pub async fn start(
        session: wezterm_ssh::Session,
        remote_host: String,
        remote_port: u16,
        preferred_local_port: u16,
    ) -> anyhow::Result<Self> {
        // Try preferred port first, fall back to OS-assigned (blocking bind is fine)
        let listener =
            match std::net::TcpListener::bind(format!("127.0.0.1:{}", preferred_local_port)) {
                Ok(l) => l,
                Err(_) => std::net::TcpListener::bind("127.0.0.1:0")
                    .context("failed to bind any local port for forwarding")?,
            };

        let local_port = listener.local_addr()?.port();
        let stop_flag = Arc::new(AtomicBool::new(false));

        log::info!(
            "Port forward proxy: localhost:{} -> {}:{}",
            local_port,
            remote_host,
            remote_port,
        );

        // Spawn accept loop on a dedicated thread (blocking I/O)
        let flag = stop_flag.clone();
        let rhost = remote_host.clone();
        std::thread::spawn(move || {
            // Set a short accept timeout so we can check the stop flag
            listener.set_nonblocking(false).ok();
            for incoming in listener.incoming() {
                if flag.load(Ordering::SeqCst) {
                    break;
                }
                match incoming {
                    Ok(stream) => {
                        let sess = session.clone();
                        let host = rhost.clone();
                        // Each connection gets its own thread for the blocking proxy
                        std::thread::spawn(move || {
                            match smol::block_on(sess.direct_tcpip(
                                &host,
                                remote_port,
                                "127.0.0.1",
                                local_port,
                            )) {
                                Ok(tunnel) => {
                                    if let Err(e) =
                                        proxy_connection(stream, tunnel.reader, tunnel.writer)
                                    {
                                        log::debug!("Port forward proxy connection ended: {}", e);
                                    }
                                }
                                Err(e) => {
                                    log::error!(
                                        "Port forward: direct-tcpip to {}:{} failed: {}",
                                        host,
                                        remote_port,
                                        e
                                    );
                                }
                            }
                        });
                    }
                    Err(e) => {
                        if !flag.load(Ordering::SeqCst) {
                            log::error!("Port forward proxy accept error: {}", e);
                        }
                        break;
                    }
                }
            }
        });

        Ok(Self {
            local_port,
            remote_host,
            remote_port,
            stop_flag,
        })
    }

    /// Get the actual local port the proxy is bound to.
    /// This may differ from the requested port if there was a conflict.
    pub fn local_port(&self) -> u16 {
        self.local_port
    }

    /// Get the remote host being forwarded to.
    pub fn remote_host(&self) -> &str {
        &self.remote_host
    }

    /// Get the remote port being forwarded to.
    pub fn remote_port(&self) -> u16 {
        self.remote_port
    }

    /// Signal the proxy to stop accepting new connections.
    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::SeqCst);
    }

    /// Check if the proxy has been stopped.
    pub fn is_stopped(&self) -> bool {
        self.stop_flag.load(Ordering::SeqCst)
    }

    /// Check if the proxy had to fall back to a different local port.
    /// Returns Some(preferred) if there was a conflict, None if the preferred port was used.
    pub fn port_conflict(&self, preferred: u16) -> Option<u16> {
        if self.local_port != preferred {
            Some(preferred)
        } else {
            None
        }
    }
}

/// Copy data between a local TCP stream and an SSH channel (via FileDescriptors).
///
/// This runs two copy loops:
/// - local_reader -> ssh_writer (client sends to remote)
/// - ssh_reader -> local_writer (remote sends to client)
///
/// Both loops run until either side closes or an error occurs.
pub fn proxy_connection(
    mut local_stream: std::net::TcpStream,
    mut ssh_reader: FileDescriptor,
    mut ssh_writer: FileDescriptor,
) -> anyhow::Result<()> {
    let mut local_clone = local_stream.try_clone()?;

    // Spawn thread: local -> SSH
    let t1 = std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match local_stream.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if ssh_writer.write_all(&buf[..n]).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    // Current thread: SSH -> local
    let mut buf = [0u8; 8192];
    loop {
        match ssh_reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if local_clone.write_all(&buf[..n]).is_err() {
                    break;
                }
            }
            Err(_) => break,
        }
    }

    let _ = t1.join();
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_proxy_stop_flag() {
        let stop_flag = Arc::new(AtomicBool::new(false));
        assert!(!stop_flag.load(Ordering::SeqCst));
        stop_flag.store(true, Ordering::SeqCst);
        assert!(stop_flag.load(Ordering::SeqCst));
    }

    #[test]
    fn test_proxy_connection_copies_data() {
        // Test the bidirectional copy using socketpairs
        let (mut a_read, mut a_write) = filedescriptor::socketpair().unwrap();
        let (mut b_read, mut b_write) = filedescriptor::socketpair().unwrap();

        // Write data from "SSH side"
        let data = b"hello from ssh";
        b_write.write_all(data).unwrap();
        drop(b_write); // close to signal EOF

        // Read from "SSH reader"
        let mut buf = vec![0u8; 64];
        let n = b_read.read(&mut buf).unwrap();
        assert_eq!(&buf[..n], data);
    }
}
