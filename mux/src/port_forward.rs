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
    /// Skipped because the local port is already in use.
    /// Will be re-checked periodically and auto-forwarded when freed.
    Skipped {
        /// Human-readable reason (e.g., "Local port already in use")
        reason: String,
    },
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
    /// A port was skipped (local port in use)
    PortSkipped { remote_port: u16, reason: String },
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

    /// Mark a port as skipped (local port already in use).
    /// The port will be re-checked periodically by the orchestrator.
    pub fn mark_skipped(&mut self, remote_port: u16, reason: String) {
        if let Some(entry) = self.entries.get_mut(&remote_port) {
            entry.state = ForwardState::Skipped {
                reason: reason.clone(),
            };
            let _ = self.event_tx.try_send(PortForwardEvent::PortSkipped {
                remote_port,
                reason,
            });
        }
    }

    /// Get the list of remote ports currently in Skipped state.
    pub fn skipped_ports(&self) -> Vec<(u16, String)> {
        self.entries
            .values()
            .filter_map(|e| match &e.state {
                ForwardState::Skipped { .. } => Some((e.remote_port, e.remote_host.clone())),
                _ => None,
            })
            .collect()
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

    /// Clean up all state on session close or reconnection.
    /// Stops all active forwards and clears detected ports.
    pub fn cleanup(&mut self) {
        for (port, entry) in self.entries.drain() {
            if matches!(entry.state, ForwardState::Active { .. }) {
                let _ = self
                    .event_tx
                    .try_send(PortForwardEvent::PortStopped { remote_port: port });
            }
        }
        log::info!("Port forward manager: cleaned up all entries");
    }

    /// Re-detect ports after reconnection.
    /// Clears all entries but preserves exclusions and auto-forward setting.
    pub fn reset_for_reconnect(&mut self) {
        self.cleanup();
        // Exclusions and auto_forward settings are preserved
        log::info!("Port forward manager: reset for reconnection");
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
    mode: config::PortDetectionMode,
) {
    let mut scanner = ProcNetTcpScanner::new(manager.lock().unwrap().excluded_ports().clone());
    let mut is_first_scan = true;

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
                let sections: Vec<&str> = content.split("  sl  local_address").collect();

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

                let new_ports = if is_first_scan && mode == config::PortDetectionMode::OnlyNew {
                    // OnlyNew: seed the scanner with existing ports, don't report them
                    scanner.seed(&tcp_content, &tcp6_content);
                    is_first_scan = false;
                    Vec::new()
                } else {
                    is_first_scan = false;
                    scanner.scan(&tcp_content, &tcp6_content)
                };
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

/// Orchestrate port forwarding for a direct SSH session.
///
/// This is the main entry point that ties together detection, state management,
/// and proxy creation. It:
/// 1. Starts the `/proc/net/tcp` detection loop (if enabled)
/// 2. Listens for `PortDetected` events from the manager
/// 3. Auto-creates SSH tunnel + TCP proxy for each detected port (if enabled)
///
/// Designed to be spawned as an async task when an SSH session is authenticated.
pub async fn run_port_forward_orchestrator(
    session: wezterm_ssh::Session,
    manager: std::sync::Arc<std::sync::Mutex<PortForwardManager>>,
    config: config::PortForwardConfig,
    stop_rx: smol::channel::Receiver<()>,
) {
    use crate::port_forward_proxy::{is_local_port_available, PortForwardProxy};
    use config::PortConflictHandling;
    use std::collections::HashMap;

    log::info!("Port forwarding orchestrator started");

    let event_rx = manager.lock().unwrap().event_receiver();
    let poll_interval = Duration::from_secs(config.poll_interval_secs);

    // Start /proc/net/tcp detection loop if enabled
    if config.detect_with_proc_net_tcp != config::PortDetectionMode::None {
        let sess = session.clone();
        let mgr = manager.clone();
        let interval = poll_interval;
        let stop = stop_rx.clone();
        let mode = config.detect_with_proc_net_tcp;
        smol::spawn(async move {
            run_proc_net_tcp_detection(sess, mgr, interval, stop, mode).await;
        })
        .detach();
    }

    // Track active proxies so we can stop them
    let mut proxies: HashMap<u16, PortForwardProxy> = HashMap::new();

    /// Try to forward a port, handling conflict policy.
    /// Returns the proxy on success.
    async fn try_forward_port(
        session: &wezterm_ssh::Session,
        remote_host: &str,
        remote_port: u16,
        preferred_local_port: u16,
        conflict_handling: PortConflictHandling,
        manager: &std::sync::Arc<std::sync::Mutex<PortForwardManager>>,
    ) -> Option<PortForwardProxy> {
        // Pre-check: is the preferred local port available?
        if !is_local_port_available(preferred_local_port) {
            match conflict_handling {
                PortConflictHandling::Skip => {
                    let reason = "Local port already in use".to_string();
                    manager
                        .lock()
                        .unwrap()
                        .mark_skipped(remote_port, reason.clone());
                    log::info!("Port {} skipped: {}", remote_port, reason);
                    return None;
                }
                PortConflictHandling::RandomPort => {
                    // Allow fallback to random port
                }
            }
        }

        let allow_random = matches!(conflict_handling, PortConflictHandling::RandomPort);
        match PortForwardProxy::start(
            session.clone(),
            remote_host.to_string(),
            remote_port,
            preferred_local_port,
            allow_random,
        )
        .await
        {
            Ok(proxy) => {
                let local_port = proxy.local_port();
                manager
                    .lock()
                    .unwrap()
                    .mark_forwarded(remote_port, local_port);
                log::info!("Port {} forwarded to localhost:{}", remote_port, local_port);
                Some(proxy)
            }
            Err(e) => {
                let msg = format!("{:#}", e);
                manager.lock().unwrap().mark_error(remote_port, msg.clone());
                log::error!("Failed to forward port {}: {}", remote_port, msg);
                None
            }
        }
    }

    // Main event loop: react to detected ports, and periodically re-check
    // skipped ports.
    loop {
        // Race: next event OR periodic re-check timer OR stop signal
        enum Action {
            Event(PortForwardEvent),
            RecheckSkipped,
            Stop,
        }

        let action = smol::future::or(
            smol::future::or(
                async {
                    match event_rx.recv().await {
                        Ok(e) => Action::Event(e),
                        Err(_) => Action::Stop,
                    }
                },
                async {
                    smol::Timer::after(poll_interval).await;
                    Action::RecheckSkipped
                },
            ),
            async {
                stop_rx.recv().await.ok();
                Action::Stop
            },
        )
        .await;

        match action {
            Action::Stop => break,

            Action::RecheckSkipped => {
                // Re-check all skipped ports to see if they've become available
                let skipped = manager.lock().unwrap().skipped_ports();
                for (remote_port, remote_host) in skipped {
                    if is_local_port_available(remote_port) {
                        log::info!(
                            "Skipped port {} is now available, auto-forwarding",
                            remote_port
                        );
                        if let Some(proxy) = try_forward_port(
                            &session,
                            &remote_host,
                            remote_port,
                            remote_port,
                            config.port_conflict_handling,
                            &manager,
                        )
                        .await
                        {
                            proxies.insert(remote_port, proxy);
                        }
                    }
                }
            }

            Action::Event(event) => match event {
                PortForwardEvent::PortDetected(entry) => {
                    let auto = manager.lock().unwrap().is_auto_forward();
                    if !auto {
                        log::info!(
                            "Port {} detected but auto-forward is off",
                            entry.remote_port
                        );
                        continue;
                    }
                    log::info!(
                        "Auto-forwarding port {} ({}:{})",
                        entry.remote_port,
                        entry.remote_host,
                        entry.remote_port
                    );
                    if let Some(proxy) = try_forward_port(
                        &session,
                        &entry.remote_host,
                        entry.remote_port,
                        entry.remote_port,
                        config.port_conflict_handling,
                        &manager,
                    )
                    .await
                    {
                        proxies.insert(entry.remote_port, proxy);
                    }
                }
                PortForwardEvent::PortStopped { remote_port } => {
                    if let Some(proxy) = proxies.remove(&remote_port) {
                        proxy.stop();
                        log::info!("Stopped forward for port {}", remote_port);
                    }
                }
                PortForwardEvent::PortRemoved { remote_port } => {
                    if let Some(proxy) = proxies.remove(&remote_port) {
                        proxy.stop();
                        log::info!("Removed forward for port {}", remote_port);
                    }
                }
                _ => {}
            },
        }
    }

    // Clean up all proxies on shutdown
    for (port, proxy) in proxies.drain() {
        proxy.stop();
        log::info!("Shutting down proxy for port {}", port);
    }
    log::info!("Port forwarding orchestrator stopped");
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
    fn test_cleanup() {
        let mut mgr = PortForwardManager::new(true, HashSet::new());
        let rx = mgr.event_receiver();

        mgr.port_detected(3000, "127.0.0.1".into(), DetectionSource::ProcNetTcp);
        mgr.mark_forwarded(3000, 3000);
        mgr.port_detected(8080, "0.0.0.0".into(), DetectionSource::ProcNetTcp);

        // Drain existing events
        while rx.try_recv().is_ok() {}

        mgr.cleanup();
        assert_eq!(mgr.entries().len(), 0);

        // Should get a PortStopped event for the forwarded port
        let event = rx.try_recv().unwrap();
        assert!(matches!(
            event,
            PortForwardEvent::PortStopped { remote_port: 3000 }
        ));
    }

    #[test]
    fn test_reset_preserves_exclusions() {
        let mut mgr = PortForwardManager::new(true, HashSet::from([22u16]));
        mgr.port_detected(3000, "127.0.0.1".into(), DetectionSource::ProcNetTcp);

        mgr.reset_for_reconnect();
        assert_eq!(mgr.entries().len(), 0);
        assert!(mgr.excluded_ports().contains(&22));

        // Port 22 should still be excluded after reset
        assert!(mgr
            .port_detected(22, "0.0.0.0".into(), DetectionSource::ProcNetTcp)
            .is_none());
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
        let new = scan_terminal_output(&mut scanner, &mut mgr, "Server at http://localhost:3000");
        assert_eq!(new.len(), 0);
    }

    #[test]
    fn test_mark_skipped() {
        let mut mgr = PortForwardManager::new(true, HashSet::new());
        let rx = mgr.event_receiver();

        mgr.port_detected(3000, "127.0.0.1".into(), DetectionSource::ProcNetTcp);
        // Drain the PortDetected event
        let _ = rx.try_recv();

        mgr.mark_skipped(3000, "Local port already in use".into());

        let entries = mgr.entries();
        assert!(matches!(
            &entries[0].state,
            ForwardState::Skipped { reason } if reason == "Local port already in use"
        ));

        let event = rx.try_recv().unwrap();
        assert!(matches!(
            event,
            PortForwardEvent::PortSkipped {
                remote_port: 3000,
                ..
            }
        ));
    }

    #[test]
    fn test_skipped_ports() {
        let mut mgr = PortForwardManager::new(true, HashSet::new());

        mgr.port_detected(3000, "127.0.0.1".into(), DetectionSource::ProcNetTcp);
        mgr.port_detected(8080, "0.0.0.0".into(), DetectionSource::ProcNetTcp);
        mgr.port_detected(5432, "0.0.0.0".into(), DetectionSource::ProcNetTcp);

        mgr.mark_skipped(3000, "in use".into());
        mgr.mark_skipped(5432, "in use".into());
        mgr.mark_forwarded(8080, 8080);

        let skipped = mgr.skipped_ports();
        assert_eq!(skipped.len(), 2);
        let ports: Vec<u16> = skipped.iter().map(|(p, _)| *p).collect();
        assert!(ports.contains(&3000));
        assert!(ports.contains(&5432));
        assert!(!ports.contains(&8080));
    }

    #[test]
    fn test_skipped_then_forwarded() {
        let mut mgr = PortForwardManager::new(true, HashSet::new());

        mgr.port_detected(3000, "127.0.0.1".into(), DetectionSource::ProcNetTcp);
        mgr.mark_skipped(3000, "in use".into());
        assert_eq!(mgr.skipped_ports().len(), 1);

        // Port becomes available, mark as forwarded
        mgr.mark_forwarded(3000, 3000);
        assert_eq!(mgr.skipped_ports().len(), 0);
        assert!(matches!(
            mgr.entries()[0].state,
            ForwardState::Active { local_port: 3000 }
        ));
    }
}
