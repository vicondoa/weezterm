//! Port forwarding state management for remote SSH sessions.
//!
//! Tracks detected ports, active forwards, and exclusions.
//! Emits events for UI notifications and auto-forwarding logic.
//!
//! --- weezterm remote features ---

use crate::port_detect::{ProcNetTcpScanner, TerminalOutputPortScanner};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

/// How a port was detected
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetectionSource {
    /// Detected via /proc/net/tcp polling
    ProcNetTcp,
    /// Detected via terminal output URL scraping
    TerminalOutput,
    /// Manually added by user
    Manual,
}

/// State of a port forward
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForwardState {
    /// Detected but not yet forwarded
    Detected,
    /// Actively forwarded
    Active {
        /// The local port the forward is bound to
        local_port: u16,
    },
    /// Forwarding stopped by user
    Stopped,
    /// Error during forwarding
    Error(String),
}

/// An entry in the port forwarding table
#[derive(Debug, Clone)]
pub struct PortForwardEntry {
    /// The port on the remote host
    pub remote_port: u16,
    /// The remote host address (e.g., "127.0.0.1", "0.0.0.0")
    pub remote_host: String,
    /// The local port to forward to (may differ from remote_port on conflict)
    pub local_port: u16,
    /// Optional label for display (e.g., process name)
    pub label: Option<String>,
    /// Current state of the forward
    pub state: ForwardState,
    /// How the port was detected
    pub source: DetectionSource,
}

/// Events emitted by the port forward manager
#[derive(Debug, Clone)]
pub enum PortForwardEvent {
    /// A new port was detected
    PortDetected(PortForwardEntry),
    /// A port forward is now active
    PortForwarded(PortForwardEntry),
    /// A port forward was stopped
    PortStopped { remote_port: u16 },
    /// A port was removed/excluded
    PortRemoved { remote_port: u16 },
    /// An error occurred with a port forward
    PortError { remote_port: u16, error: String },
}

/// Manages the state of port forwarding for a single SSH domain/session.
pub struct PortForwardManager {
    /// All known port entries, keyed by remote port
    entries: HashMap<u16, PortForwardEntry>,
    /// Ports explicitly excluded by the user
    excluded: HashSet<u16>,
    /// Whether to auto-forward newly detected ports
    auto_forward: bool,
    /// Channel for emitting events to listeners (UI, auto-forward logic)
    event_tx: smol::channel::Sender<PortForwardEvent>,
    event_rx: smol::channel::Receiver<PortForwardEvent>,
}

impl PortForwardManager {
    /// Create a new port forward manager.
    ///
    /// * `auto_forward` - Whether to auto-forward newly detected ports
    /// * `exclude_ports` - Ports to never auto-forward (e.g., 22, 80, 443)
    pub fn new(auto_forward: bool, exclude_ports: HashSet<u16>) -> Self {
        let (event_tx, event_rx) = smol::channel::unbounded();
        Self {
            entries: HashMap::new(),
            excluded: exclude_ports,
            auto_forward,
            event_tx,
            event_rx,
        }
    }

    /// Get the event receiver for listening to port forward events.
    /// Multiple receivers can be cloned from this.
    pub fn event_receiver(&self) -> smol::channel::Receiver<PortForwardEvent> {
        self.event_rx.clone()
    }

    /// Called when a port is detected (from scanner or terminal scraper).
    /// Returns the event if the port was newly detected, None if already known or excluded.
    pub fn port_detected(
        &mut self,
        port: u16,
        host: String,
        source: DetectionSource,
    ) -> Option<PortForwardEvent> {
        if self.excluded.contains(&port) {
            return None;
        }
        if self.entries.contains_key(&port) {
            return None;
        }

        let entry = PortForwardEntry {
            remote_port: port,
            remote_host: host,
            local_port: port, // Default: same as remote
            label: None,
            state: ForwardState::Detected,
            source,
        };
        self.entries.insert(port, entry.clone());
        let event = PortForwardEvent::PortDetected(entry);
        let _ = self.event_tx.try_send(event.clone());
        Some(event)
    }

    /// Mark a port as actively forwarded.
    pub fn mark_forwarded(&mut self, remote_port: u16, local_port: u16) {
        if let Some(entry) = self.entries.get_mut(&remote_port) {
            entry.state = ForwardState::Active { local_port };
            entry.local_port = local_port;
            let _ = self
                .event_tx
                .try_send(PortForwardEvent::PortForwarded(entry.clone()));
        }
    }

    /// Mark a port forward as errored.
    pub fn mark_error(&mut self, remote_port: u16, error: String) {
        if let Some(entry) = self.entries.get_mut(&remote_port) {
            entry.state = ForwardState::Error(error.clone());
            let _ = self
                .event_tx
                .try_send(PortForwardEvent::PortError { remote_port, error });
        }
    }

    /// Stop forwarding a port (user action).
    pub fn stop_forward(&mut self, remote_port: u16) {
        if let Some(entry) = self.entries.get_mut(&remote_port) {
            entry.state = ForwardState::Stopped;
            let _ = self
                .event_tx
                .try_send(PortForwardEvent::PortStopped { remote_port });
        }
    }

    /// Exclude a port permanently (user opted out). Removes it from entries.
    pub fn exclude_port(&mut self, port: u16) {
        self.excluded.insert(port);
        self.entries.remove(&port);
        let _ = self
            .event_tx
            .try_send(PortForwardEvent::PortRemoved { remote_port: port });
    }

    /// Remove a port that is no longer detected (e.g., service stopped).
    pub fn port_gone(&mut self, port: u16) {
        if self.entries.remove(&port).is_some() {
            let _ = self
                .event_tx
                .try_send(PortForwardEvent::PortRemoved { remote_port: port });
        }
    }

    /// Set label for a port (e.g., process name discovered from /proc).
    pub fn set_label(&mut self, port: u16, label: String) {
        if let Some(entry) = self.entries.get_mut(&port) {
            entry.label = Some(label);
        }
    }

    /// Get a snapshot of all entries for UI display.
    pub fn entries(&self) -> Vec<PortForwardEntry> {
        let mut entries: Vec<_> = self.entries.values().cloned().collect();
        entries.sort_by_key(|e| e.remote_port);
        entries
    }

    /// Get a specific entry by remote port.
    pub fn get(&self, remote_port: u16) -> Option<&PortForwardEntry> {
        self.entries.get(&remote_port)
    }

    /// Check if auto-forward is enabled.
    pub fn is_auto_forward(&self) -> bool {
        self.auto_forward
    }

    /// Toggle auto-forward mode.
    pub fn set_auto_forward(&mut self, auto: bool) {
        self.auto_forward = auto;
    }

    /// Get the set of excluded ports.
    pub fn excluded_ports(&self) -> &HashSet<u16> {
        &self.excluded
    }

    /// Get the number of active forwards.
    pub fn active_forward_count(&self) -> usize {
        self.entries
            .values()
            .filter(|e| matches!(e.state, ForwardState::Active { .. }))
            .count()
    }
}

/// Run the /proc/net/tcp port detection loop on a remote host.
///
/// This periodically executes `cat /proc/net/tcp /proc/net/tcp6` on the remote
/// via SSH exec, parses the output, and feeds new ports to the manager.
///
/// Designed to be spawned as an async task.
pub async fn run_proc_net_tcp_detection(
    session: wezterm_ssh::Session,
    manager: std::sync::Arc<std::sync::Mutex<PortForwardManager>>,
    poll_interval: Duration,
    stop_rx: smol::channel::Receiver<()>,
) {
    let mut scanner = ProcNetTcpScanner::new(
        manager.lock().unwrap().excluded_ports().clone(),
    );

    loop {
        // Check for stop signal
        if stop_rx.try_recv().is_ok() {
            log::debug!("Port detection loop: stop signal received");
            break;
        }

        // Execute remote command to read /proc/net/tcp
        match session
            .exec("cat /proc/net/tcp /proc/net/tcp6 2>/dev/null", None)
            .await
        {
            Ok(exec_result) => {
                let mut buf = Vec::new();
                let mut reader = exec_result.stdout;
                let mut tmp = [0u8; 4096];
                loop {
                    match std::io::Read::read(&mut reader, &mut tmp) {
                        Ok(0) => break,
                        Ok(n) => buf.extend_from_slice(&tmp[..n]),
                        Err(_) => break,
                    }
                }

                let content = String::from_utf8_lossy(&buf);

                // Split output into tcp and tcp6 sections.
                // Both sections start with a header containing "sl  local_address".
                // We split on this header to separate the two concatenated outputs.
                let sections: Vec<&str> =
                    content.split("  sl  local_address").collect();

                let tcp_content = if sections.len() > 1 {
                    format!(
                        "  sl  local_address{}",
                        sections[1]
                            .split("  sl  local_address")
                            .next()
                            .unwrap_or("")
                    )
                } else {
                    String::new()
                };

                let tcp6_content = if sections.len() > 2 {
                    format!("  sl  local_address{}", sections[2])
                } else {
                    String::new()
                };

                let new_ports = scanner.scan(&tcp_content, &tcp6_content);
                if !new_ports.is_empty() {
                    let mut mgr = manager.lock().unwrap();
                    for port in &new_ports {
                        let host = port.local_address.to_string();
                        mgr.port_detected(port.port, host, DetectionSource::ProcNetTcp);
                    }
                    log::info!(
                        "Port detection: found {} new port(s): {:?}",
                        new_ports.len(),
                        new_ports.iter().map(|p| p.port).collect::<Vec<_>>()
                    );
                }
            }
            Err(err) => {
                log::debug!("Port detection: exec failed: {}", err);
            }
        }

        // Wait for the poll interval or stop signal
        smol::Timer::after(poll_interval).await;
    }
}

/// Process terminal output text for port detection.
///
/// Call this with chunks of terminal output to detect localhost URLs.
/// Returns a list of newly detected (port, url) pairs.
pub fn scan_terminal_output(
    scanner: &mut TerminalOutputPortScanner,
    manager: &mut PortForwardManager,
    text: &str,
) -> Vec<(u16, String)> {
    let new_ports = scanner.scan_text(text);
    for (port, ref _url) in &new_ports {
        manager.port_detected(*port, "127.0.0.1".into(), DetectionSource::TerminalOutput);
    }
    new_ports
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_port_detected() {
        let mut mgr = PortForwardManager::new(true, HashSet::new());
        let event = mgr.port_detected(3000, "127.0.0.1".into(), DetectionSource::ProcNetTcp);
        assert!(event.is_some());
        assert_eq!(mgr.entries().len(), 1);
        assert_eq!(mgr.entries()[0].remote_port, 3000);
    }

    #[test]
    fn test_excluded_port_ignored() {
        let mut mgr = PortForwardManager::new(true, HashSet::from([22u16, 80]));
        assert!(mgr
            .port_detected(22, "0.0.0.0".into(), DetectionSource::ProcNetTcp)
            .is_none());
        assert!(mgr
            .port_detected(80, "0.0.0.0".into(), DetectionSource::ProcNetTcp)
            .is_none());
        assert_eq!(mgr.entries().len(), 0);
    }

    #[test]
    fn test_duplicate_detection_ignored() {
        let mut mgr = PortForwardManager::new(true, HashSet::new());
        mgr.port_detected(3000, "127.0.0.1".into(), DetectionSource::ProcNetTcp);
        let event = mgr.port_detected(3000, "127.0.0.1".into(), DetectionSource::TerminalOutput);
        assert!(event.is_none()); // Already known
        assert_eq!(mgr.entries().len(), 1);
    }

    #[test]
    fn test_forward_and_stop_lifecycle() {
        let mut mgr = PortForwardManager::new(true, HashSet::new());
        mgr.port_detected(8080, "0.0.0.0".into(), DetectionSource::ProcNetTcp);

        mgr.mark_forwarded(8080, 8080);
        let entries = mgr.entries();
        assert!(matches!(
            entries[0].state,
            ForwardState::Active { local_port: 8080 }
        ));
        assert_eq!(mgr.active_forward_count(), 1);

        mgr.stop_forward(8080);
        let entries = mgr.entries();
        assert!(matches!(entries[0].state, ForwardState::Stopped));
        assert_eq!(mgr.active_forward_count(), 0);
    }

    #[test]
    fn test_exclude_removes_entry() {
        let mut mgr = PortForwardManager::new(true, HashSet::new());
        mgr.port_detected(3000, "127.0.0.1".into(), DetectionSource::ProcNetTcp);
        assert_eq!(mgr.entries().len(), 1);

        mgr.exclude_port(3000);
        assert_eq!(mgr.entries().len(), 0);

        // Re-detection should be ignored since port is now excluded
        assert!(mgr
            .port_detected(3000, "127.0.0.1".into(), DetectionSource::ProcNetTcp)
            .is_none());
    }

    #[test]
    fn test_event_channel() {
        let mut mgr = PortForwardManager::new(true, HashSet::new());
        let rx = mgr.event_receiver();

        mgr.port_detected(3000, "127.0.0.1".into(), DetectionSource::ProcNetTcp);
        let event = rx.try_recv().unwrap();
        assert!(matches!(event, PortForwardEvent::PortDetected(_)));

        mgr.mark_forwarded(3000, 3000);
        let event = rx.try_recv().unwrap();
        assert!(matches!(event, PortForwardEvent::PortForwarded(_)));

        mgr.stop_forward(3000);
        let event = rx.try_recv().unwrap();
        assert!(matches!(
            event,
            PortForwardEvent::PortStopped { remote_port: 3000 }
        ));
    }

    #[test]
    fn test_port_gone() {
        let mut mgr = PortForwardManager::new(true, HashSet::new());
        mgr.port_detected(3000, "127.0.0.1".into(), DetectionSource::ProcNetTcp);
        assert_eq!(mgr.entries().len(), 1);

        mgr.port_gone(3000);
        assert_eq!(mgr.entries().len(), 0);

        // Can be re-detected after gone
        assert!(mgr
            .port_detected(3000, "127.0.0.1".into(), DetectionSource::ProcNetTcp)
            .is_some());
    }

    #[test]
    fn test_mark_error() {
        let mut mgr = PortForwardManager::new(true, HashSet::new());
        mgr.port_detected(3000, "127.0.0.1".into(), DetectionSource::ProcNetTcp);
        mgr.mark_error(3000, "address already in use".into());

        let entries = mgr.entries();
        assert!(
            matches!(&entries[0].state, ForwardState::Error(e) if e == "address already in use")
        );
    }

    #[test]
    fn test_entries_sorted_by_port() {
        let mut mgr = PortForwardManager::new(true, HashSet::new());
        mgr.port_detected(8080, "0.0.0.0".into(), DetectionSource::ProcNetTcp);
        mgr.port_detected(3000, "127.0.0.1".into(), DetectionSource::ProcNetTcp);
        mgr.port_detected(5432, "0.0.0.0".into(), DetectionSource::ProcNetTcp);

        let entries = mgr.entries();
        assert_eq!(entries[0].remote_port, 3000);
        assert_eq!(entries[1].remote_port, 5432);
        assert_eq!(entries[2].remote_port, 8080);
    }

    #[test]
    fn test_scan_terminal_output_integration() {
        use crate::port_detect::TerminalOutputPortScanner;

        let mut scanner = TerminalOutputPortScanner::new();
        let mut mgr = PortForwardManager::new(true, HashSet::new());

        let new = scan_terminal_output(
            &mut scanner,
            &mut mgr,
            "Server at http://localhost:3000\nAlso http://127.0.0.1:8080",
        );
        assert_eq!(new.len(), 2);
        assert_eq!(mgr.entries().len(), 2);

        // Duplicate scan produces nothing new
        let new = scan_terminal_output(
            &mut scanner,
            &mut mgr,
            "Server at http://localhost:3000",
        );
        assert_eq!(new.len(), 0);
    }
}
