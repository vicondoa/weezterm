//! Port detection for remote SSH hosts.
//!
//! Detects listening ports by:
//! 1. Parsing /proc/net/tcp{,6} output from the remote host
//! 2. Scraping terminal output for localhost URLs
//!
//! --- weezterm remote features ---

use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// A port detected on the remote host
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DetectedPort {
    pub port: u16,
    pub local_address: IpAddr,
    pub inode: u64,
    pub process_name: Option<String>,
}

/// Parses /proc/net/tcp or /proc/net/tcp6 content.
///
/// Each line (after header) has format:
///   sl  local_address rem_address   st tx_queue:rx_queue ...
///
/// local_address is hex_ip:hex_port.
/// State 0A = LISTEN.
pub fn parse_proc_net_tcp(content: &str) -> Vec<DetectedPort> {
    let mut ports = Vec::new();
    for line in content.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 4 {
            continue;
        }

        // Field 3 is the state; 0A = LISTEN
        let state = fields[3];
        if state != "0A" {
            continue;
        }

        if let Some((addr, port)) = parse_hex_address(fields[1]) {
            let inode = fields
                .get(9)
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            ports.push(DetectedPort {
                port,
                local_address: addr,
                inode,
                process_name: None,
            });
        }
    }
    ports
}

/// Parse a hex-encoded address from /proc/net/tcp.
///
/// IPv4 format: `HHHHHHHH:PPPP` (8 hex chars for IP, 4 for port)
/// IPv6 format: `HHHHHHHHHHHHHHHHHHHHHHHHHHHHHHHH:PPPP` (32 hex chars for IP)
///
/// IPv4 addresses in /proc/net/tcp are in host byte order (little-endian on x86).
fn parse_hex_address(hex_addr: &str) -> Option<(IpAddr, u16)> {
    let parts: Vec<&str> = hex_addr.split(':').collect();
    if parts.len() != 2 {
        return None;
    }

    let port = u16::from_str_radix(parts[1], 16).ok()?;
    let ip_hex = parts[0];

    let addr = if ip_hex.len() == 8 {
        // IPv4: stored in little-endian on x86
        let ip_u32 = u32::from_str_radix(ip_hex, 16).ok()?;
        IpAddr::V4(Ipv4Addr::from(ip_u32.to_be()))
    } else if ip_hex.len() == 32 {
        // IPv6: stored as 4 groups of 4 bytes, each group in little-endian
        let mut octets = [0u8; 16];
        for i in 0..4 {
            let group_hex = &ip_hex[i * 8..(i + 1) * 8];
            let group_u32 = u32::from_str_radix(group_hex, 16).ok()?;
            let bytes = group_u32.to_be_bytes();
            octets[i * 4] = bytes[0];
            octets[i * 4 + 1] = bytes[1];
            octets[i * 4 + 2] = bytes[2];
            octets[i * 4 + 3] = bytes[3];
        }
        IpAddr::V6(Ipv6Addr::from(octets))
    } else {
        return None;
    };

    Some((addr, port))
}

/// Scanner that tracks known ports and detects changes.
pub struct ProcNetTcpScanner {
    known_ports: HashSet<u16>,
    exclude_ports: HashSet<u16>,
}

impl ProcNetTcpScanner {
    pub fn new(exclude_ports: HashSet<u16>) -> Self {
        Self {
            known_ports: HashSet::new(),
            exclude_ports,
        }
    }

    /// Given fresh /proc/net/tcp content, return newly-detected listening ports.
    pub fn scan(&mut self, tcp_content: &str, tcp6_content: &str) -> Vec<DetectedPort> {
        let mut all_ports = parse_proc_net_tcp(tcp_content);
        all_ports.extend(parse_proc_net_tcp(tcp6_content));

        let mut new_ports = Vec::new();
        let current: HashSet<u16> = all_ports.iter().map(|p| p.port).collect();

        for port_info in &all_ports {
            if !self.known_ports.contains(&port_info.port)
                && !self.exclude_ports.contains(&port_info.port)
            {
                new_ports.push(port_info.clone());
            }
        }

        self.known_ports = current;
        new_ports
    }

    /// Return ports that disappeared since the last scan.
    pub fn removed_since_last(&self, tcp_content: &str, tcp6_content: &str) -> Vec<u16> {
        let mut all_ports = parse_proc_net_tcp(tcp_content);
        all_ports.extend(parse_proc_net_tcp(tcp6_content));
        let current: HashSet<u16> = all_ports.iter().map(|p| p.port).collect();
        self.known_ports.difference(&current).copied().collect()
    }

    /// Get the set of currently known listening ports.
    pub fn known_ports(&self) -> &HashSet<u16> {
        &self.known_ports
    }
}

/// Regex patterns for detecting localhost URLs in terminal output.
pub mod url_scraper {
    use regex::Regex;
    use std::sync::LazyLock;

    static LOCALHOST_URL_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"https?://(?:localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1?\]):(\d{1,5})").unwrap()
    });

    /// Extract port numbers from text that contains localhost URLs.
    ///
    /// Returns a vec of (port, full_url_match) tuples.
    pub fn extract_ports(text: &str) -> Vec<(u16, String)> {
        let mut results = Vec::new();
        for cap in LOCALHOST_URL_RE.captures_iter(text) {
            if let (Some(full_match), Some(port_match)) = (cap.get(0), cap.get(1)) {
                if let Ok(port) = port_match.as_str().parse::<u16>() {
                    if port > 0 {
                        results.push((port, full_match.as_str().to_string()));
                    }
                }
            }
        }
        results
    }
}

/// Adapter that scans text for localhost URLs and produces Alert-compatible events.
/// This is designed to be called with terminal output text from the mux layer.
pub struct TerminalOutputPortScanner {
    /// Ports already detected from terminal output (to avoid duplicate alerts)
    seen_ports: HashSet<u16>,
}

impl TerminalOutputPortScanner {
    pub fn new() -> Self {
        Self {
            seen_ports: HashSet::new(),
        }
    }

    /// Scan a chunk of terminal output text for localhost URLs.
    /// Returns a list of (port, url) pairs for newly-detected ports.
    pub fn scan_text(&mut self, text: &str) -> Vec<(u16, String)> {
        let mut results = Vec::new();
        for (port, url) in url_scraper::extract_ports(text) {
            if self.seen_ports.insert(port) {
                results.push((port, url));
            }
        }
        results
    }

    /// Reset the scanner state (e.g., on reconnection).
    pub fn reset(&mut self) {
        self.seen_ports.clear();
    }
}

#[cfg(test)]
mod test {
    use super::*;

    const SAMPLE_PROC_NET_TCP: &str = "\
  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 00000000:0BB8 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 12345 1 0000000000000000 100 0 0 10 0
   1: 0100007F:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 23456 1 0000000000000000 100 0 0 10 0
   2: 0100007F:C000 0100007F:1F90 01 00000000:00000000 00:00000000 00000000  1000        0 34567 1 0000000000000000 100 0 0 10 0";

    #[test]
    fn test_parse_proc_net_tcp_listen_ports() {
        let ports = parse_proc_net_tcp(SAMPLE_PROC_NET_TCP);
        assert_eq!(ports.len(), 2); // Only LISTEN (0A) entries
        assert_eq!(ports[0].port, 3000); // 0x0BB8
        assert_eq!(ports[1].port, 8080); // 0x1F90
    }

    #[test]
    fn test_parse_proc_net_tcp_ignores_established() {
        let ports = parse_proc_net_tcp(SAMPLE_PROC_NET_TCP);
        // The third line has state 01 (ESTABLISHED) — must be ignored
        assert!(!ports.iter().any(|p| p.port == 0xC000));
    }

    #[test]
    fn test_parse_hex_address_loopback() {
        let (addr, port) = parse_hex_address("0100007F:1F90").unwrap();
        assert_eq!(port, 8080);
        assert_eq!(addr, IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
    }

    #[test]
    fn test_parse_hex_address_any() {
        let (addr, port) = parse_hex_address("00000000:0BB8").unwrap();
        assert_eq!(port, 3000);
        assert_eq!(addr, IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)));
    }

    #[test]
    fn test_parse_hex_address_invalid() {
        assert!(parse_hex_address("garbage").is_none());
        assert!(parse_hex_address("").is_none());
        assert!(parse_hex_address(":").is_none());
        assert!(parse_hex_address("ZZZZZZZZ:0000").is_none());
    }

    #[test]
    fn test_scanner_detects_new_ports() {
        let mut scanner = ProcNetTcpScanner::new(HashSet::new());
        let new = scanner.scan(SAMPLE_PROC_NET_TCP, "");
        assert_eq!(new.len(), 2);

        // Second scan with same data → no new ports
        let new = scanner.scan(SAMPLE_PROC_NET_TCP, "");
        assert_eq!(new.len(), 0);
    }

    #[test]
    fn test_scanner_excludes_ports() {
        let exclude: HashSet<u16> = HashSet::from([22u16, 3000]);
        let mut scanner = ProcNetTcpScanner::new(exclude);
        let new = scanner.scan(SAMPLE_PROC_NET_TCP, "");
        assert_eq!(new.len(), 1);
        assert_eq!(new[0].port, 8080);
    }

    #[test]
    fn test_scanner_detects_removed_ports() {
        let mut scanner = ProcNetTcpScanner::new(HashSet::new());
        scanner.scan(SAMPLE_PROC_NET_TCP, "");

        let header_only = "  sl  local_address rem_address   st\n";
        let removed = scanner.removed_since_last(header_only, "");
        assert!(removed.contains(&3000));
        assert!(removed.contains(&8080));
    }

    #[test]
    fn test_scanner_with_tcp6() {
        let tcp6 = "\
  sl  local_address                         remote_address                        st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 00000000000000000000000000000000:1F90 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 45678 1 0000000000000000 100 0 0 10 0";
        let mut scanner = ProcNetTcpScanner::new(HashSet::new());
        let new = scanner.scan("", tcp6);
        assert_eq!(new.len(), 1);
        assert_eq!(new[0].port, 8080);
    }

    #[test]
    fn test_empty_proc_net_tcp() {
        let content = "  sl  local_address rem_address   st\n";
        let ports = parse_proc_net_tcp(content);
        assert_eq!(ports.len(), 0);
    }

    #[test]
    fn test_malformed_lines() {
        let content = "  sl  local_address rem_address   st\ngarbage line\n\n   ";
        let ports = parse_proc_net_tcp(content);
        assert_eq!(ports.len(), 0);
    }

    #[test]
    fn test_parse_inode() {
        let ports = parse_proc_net_tcp(SAMPLE_PROC_NET_TCP);
        assert_eq!(ports[0].inode, 12345);
        assert_eq!(ports[1].inode, 23456);
    }

    // --- URL scraper tests ---

    #[test]
    fn test_url_scraper_localhost() {
        let text = "Server running at http://localhost:3000\nAlso at https://127.0.0.1:8443/api";
        let ports = url_scraper::extract_ports(text);
        assert_eq!(ports.len(), 2);
        assert_eq!(ports[0].0, 3000);
        assert_eq!(ports[1].0, 8443);
    }

    #[test]
    fn test_url_scraper_ipv6() {
        let text = "Listening on http://[::1]:5000";
        let ports = url_scraper::extract_ports(text);
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].0, 5000);
    }

    #[test]
    fn test_url_scraper_zero_addr() {
        let text = "Bound to http://0.0.0.0:9090/health";
        let ports = url_scraper::extract_ports(text);
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].0, 9090);
    }

    #[test]
    fn test_url_scraper_no_match_external() {
        let text = "Visit https://example.com:443 for docs";
        let ports = url_scraper::extract_ports(text);
        assert_eq!(ports.len(), 0); // example.com is not localhost
    }

    #[test]
    fn test_url_scraper_az_login_output() {
        let text = "A web browser has been opened at https://login.microsoftonline.com/...\n\
                    Opening in existing browser session.\n\
                    Listening on http://localhost:8400";
        let ports = url_scraper::extract_ports(text);
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].0, 8400);
    }

    #[test]
    fn test_url_scraper_multiple_on_same_line() {
        let text = "http://localhost:3000 and http://127.0.0.1:3001";
        let ports = url_scraper::extract_ports(text);
        assert_eq!(ports.len(), 2);
        assert_eq!(ports[0].0, 3000);
        assert_eq!(ports[1].0, 3001);
    }

    #[test]
    fn test_url_scraper_no_port() {
        let text = "Visit http://localhost for info";
        let ports = url_scraper::extract_ports(text);
        assert_eq!(ports.len(), 0); // No port number
    }

    // --- TerminalOutputPortScanner tests ---

    #[test]
    fn test_terminal_output_scanner_dedup() {
        let mut scanner = TerminalOutputPortScanner::new();

        // First scan finds the port
        let results = scanner.scan_text("Server running at http://localhost:3000");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, 3000);

        // Second scan with same text produces nothing (already seen)
        let results = scanner.scan_text("Server running at http://localhost:3000");
        assert_eq!(results.len(), 0);

        // New port is detected
        let results = scanner.scan_text("Also at http://localhost:8080");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, 8080);
    }

    #[test]
    fn test_terminal_output_scanner_reset() {
        let mut scanner = TerminalOutputPortScanner::new();
        scanner.scan_text("http://localhost:3000");
        assert_eq!(scanner.scan_text("http://localhost:3000").len(), 0);

        scanner.reset();
        // After reset, the port should be detected again
        let results = scanner.scan_text("http://localhost:3000");
        assert_eq!(results.len(), 1);
    }
}
