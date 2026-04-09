use crate::config::validate_domain_name;
use crate::*;
use luahelper::impl_lua_conversion_dynamic;
use std::fmt::Display;
use std::str::FromStr;
use wezterm_dynamic::{FromDynamic, ToDynamic};

#[derive(Debug, Clone, Copy, FromDynamic, ToDynamic)]
pub enum SshBackend {
    Ssh2,
    LibSsh,
}

impl Default for SshBackend {
    fn default() -> Self {
        Self::LibSsh
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, FromDynamic, ToDynamic)]
pub enum SshMultiplexing {
    WezTerm,
    None,
    // TODO: Tmux-cc in the future?
}

impl Default for SshMultiplexing {
    fn default() -> Self {
        Self::WezTerm
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, FromDynamic, ToDynamic)]
pub enum Shell {
    /// Unknown command shell: no assumptions can be made
    Unknown,

    /// Posix shell compliant, such that `cd DIR ; exec CMD` behaves
    /// as it does in the bourne shell family of shells
    Posix,
    // TODO: Cmd, PowerShell in the future?
}

impl Default for Shell {
    fn default() -> Self {
        Self::Unknown
    }
}

#[derive(Default, Debug, Clone, FromDynamic, ToDynamic)]
pub struct SshDomain {
    /// The name of this specific domain.  Must be unique amongst
    /// all types of domain in the configuration file.
    #[dynamic(validate = "validate_domain_name")]
    pub name: String,

    /// identifies the host:port pair of the remote server.
    pub remote_address: String,

    /// Whether agent auth should be disabled
    #[dynamic(default)]
    pub no_agent_auth: bool,

    /// The username to use for authenticating with the remote host
    pub username: Option<String>,

    /// If true, connect to this domain automatically at startup
    #[dynamic(default)]
    pub connect_automatically: bool,

    #[dynamic(default = "default_read_timeout")]
    pub timeout: Duration,

    #[dynamic(default = "default_local_echo_threshold_ms")]
    pub local_echo_threshold_ms: Option<u64>,

    /// Show time since last response when waiting for a response.
    /// It is recommended to use
    /// <https://wezterm.org/config/lua/pane/get_metadata.html#since_last_response_ms>
    /// instead.
    #[dynamic(default)]
    pub overlay_lag_indicator: bool,

    /// The path to the wezterm binary on the remote host
    pub remote_wezterm_path: Option<String>,
    /// Override the entire `wezterm cli proxy` invocation that would otherwise
    /// be computed from remote_wezterm_path and other information.
    pub override_proxy_command: Option<String>,

    pub ssh_backend: Option<SshBackend>,

    /// If false, then don't use a multiplexer connection,
    /// just connect directly using ssh. This doesn't require
    /// that the remote host have wezterm installed, and is equivalent
    /// to using `wezterm ssh` to connect.
    #[dynamic(default)]
    pub multiplexing: SshMultiplexing,

    /// ssh_config option values
    #[dynamic(default)]
    pub ssh_option: HashMap<String, String>,

    pub default_prog: Option<Vec<String>>,

    #[dynamic(default)]
    pub assume_shell: Shell,

    // --- weezterm remote features ---
    /// Whether to set the $BROWSER environment variable on the remote host
    /// to a helper that opens URLs on the local/client browser via OSC 7457.
    /// Default: true
    #[dynamic(default = "default_true")]
    pub set_remote_browser: Option<bool>,

    /// Configuration for automatic port forwarding.
    #[dynamic(default)]
    pub port_forwarding: PortForwardConfig,

    /// Whether to automatically install weezterm binaries on the remote host
    /// when using multiplexing mode. If the remote doesn't have weezterm or
    /// has a different version, the client will download (if cross-arch) and
    /// SFTP the correct binaries.
    /// Default: true (only applies when multiplexing = "WezTerm").
    #[dynamic(default = "default_true_bool")]
    pub auto_install_mux: bool,

    /// Remote directory to install weezterm binaries into.
    /// Default: "~/.weezterm/bin"
    #[dynamic(default = "default_remote_install_dir")]
    pub remote_install_dir: String,

    /// URL template for downloading cross-architecture release artifacts.
    /// Placeholders: {version}, {os}, {arch}
    /// When empty, cross-arch auto-install is disabled (same-arch SFTP only).
    #[dynamic(default = "default_remote_install_url")]
    pub remote_install_url: String,

    /// Local directory containing pre-built binaries for the remote platform.
    /// When set, auto-install uploads binaries from this directory instead of
    /// using the running executable's directory or downloading from a URL.
    /// Useful for cross-platform development (e.g., Windows host → Linux remote)
    /// where you build Linux binaries via WSL or cross-compilation.
    #[dynamic(default)]
    pub remote_install_binaries_dir: Option<String>,

    /// Open URL security policy for this domain.
    /// If not set, falls back to the global `open_url` config.
    #[dynamic(default)]
    pub open_url: Option<OpenUrlConfig>,
    // --- end weezterm remote features ---
}

fn default_true() -> Option<bool> {
    Some(true)
}

fn default_true_bool() -> bool {
    true
}

fn default_poll_interval() -> u64 {
    2
}

fn default_exclude_ports() -> Vec<u16> {
    vec![22]
}

fn default_remote_install_dir() -> String {
    "~/.weezterm/bin".to_string()
}

fn default_remote_install_url() -> String {
    // Placeholder — users should set this to their fork's release URL.
    // Example: "https://github.com/user/weezterm/releases/download/v{version}/weezterm-mux-{os}-{arch}.tar.gz"
    String::new()
}

// --- weezterm remote features ---
/// What to do when a detected remote port's preferred local port is already in use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, FromDynamic, ToDynamic)]
pub enum PortConflictHandling {
    /// Skip forwarding and show the port as inactive with a reason.
    /// The port will be re-checked periodically and auto-forwarded when freed.
    Skip,
    /// Forward on a random available local port instead.
    RandomPort,
}

impl Default for PortConflictHandling {
    fn default() -> Self {
        Self::Skip
    }
}
// --- end weezterm remote features ---

// --- weezterm remote features ---
/// Controls how /proc/net/tcp port detection behaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, FromDynamic, ToDynamic)]
pub enum PortDetectionMode {
    /// Disable /proc/net/tcp detection entirely.
    None,
    /// Detect and forward all listening ports, including those already open
    /// when weezterm connects.
    All,
    /// Only detect and forward ports that are opened AFTER weezterm connects.
    /// Ports that are closed and re-opened will also be forwarded.
    OnlyNew,
}

impl Default for PortDetectionMode {
    fn default() -> Self {
        Self::OnlyNew
    }
}
// --- end weezterm remote features ---

/// Configuration for automatic port forwarding on SSH domains.
#[derive(Debug, Clone, FromDynamic, ToDynamic)]
pub struct PortForwardConfig {
    /// Master switch to enable/disable port forwarding.
    /// Default: true
    #[dynamic(default = "default_true_bool")]
    pub enabled: bool,

    /// Whether to automatically forward newly detected ports.
    /// Default: true
    #[dynamic(default = "default_true_bool")]
    pub auto_forward: bool,

    /// Detection mode for /proc/net/tcp polling (Linux only).
    /// "None": disabled. "All": forward all ports. "OnlyNew": only ports
    /// opened after weezterm connects (closed+reopened ports also detected).
    /// Default: "OnlyNew"
    #[dynamic(default)]
    pub detect_with_proc_net_tcp: PortDetectionMode,

    /// Enable detection via terminal output URL scraping.
    /// Default: true
    #[dynamic(default = "default_true_bool")]
    pub detect_with_terminal_scrape: bool,

    /// Polling interval in seconds for /proc/net/tcp scanning.
    /// Default: 2
    #[dynamic(default = "default_poll_interval")]
    pub poll_interval_secs: u64,

    /// Ports to never auto-forward (e.g., SSH port 22).
    /// Default: [22]
    #[dynamic(default = "default_exclude_ports")]
    pub exclude_ports: Vec<u16>,

    /// Ports to always forward when detected on connect.
    /// Default: []
    #[dynamic(default)]
    pub include_ports: Vec<u16>,

    // --- weezterm remote features ---
    /// What to do when the preferred local port is already in use.
    /// "Skip": don't forward, show as inactive (re-checked periodically).
    /// "RandomPort": forward on a random available local port.
    /// Default: "Skip"
    #[dynamic(default)]
    pub port_conflict_handling: PortConflictHandling,
    // --- end weezterm remote features ---
}

impl Default for PortForwardConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_forward: true,
            detect_with_proc_net_tcp: PortDetectionMode::OnlyNew,
            detect_with_terminal_scrape: true,
            poll_interval_secs: 2,
            exclude_ports: vec![22],
            include_ports: vec![],
            port_conflict_handling: PortConflictHandling::Skip,
        }
    }
}

impl_lua_conversion_dynamic!(PortForwardConfig);
impl_lua_conversion_dynamic!(SshDomain);
// --- weezterm remote features ---
impl_lua_conversion_dynamic!(OpenUrlConfig);

/// Policy for URLs not on the allow-list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, FromDynamic, ToDynamic)]
pub enum OpenUrlPolicy {
    /// Open immediately without prompting.
    Allow,
    /// Show a toast notification; user must click to open.
    Confirm,
    /// Silently block the URL (log a warning).
    Deny,
}

impl Default for OpenUrlPolicy {
    fn default() -> Self {
        Self::Confirm
    }
}

fn default_open_url_allow_list() -> Vec<String> {
    vec![
        "https://login.microsoftonline.com/".to_string(),
        "https://login.live.com/".to_string(),
    ]
}

fn default_confirm_timeout_secs() -> u64 {
    15
}

/// Security policy for the remote open-URL feature ($BROWSER / OSC 7457).
///
/// Controls which URLs from remote hosts are allowed to open in the local browser.
/// Non-http(s) schemes (file://, javascript:, data:, etc.) are always blocked.
#[derive(Debug, Clone, FromDynamic, ToDynamic)]
pub struct OpenUrlConfig {
    /// Policy for URLs NOT on the allow_list.
    /// Default: "Confirm" (show toast, user clicks to open)
    #[dynamic(default)]
    pub default_policy: OpenUrlPolicy,

    /// URL prefixes that are auto-approved (opened without confirmation).
    /// Uses prefix matching against the full URL.
    /// Default: ["https://login.microsoftonline.com/", "https://login.live.com/"]
    #[dynamic(default = "default_open_url_allow_list")]
    pub allow_list: Vec<String>,

    /// How long the confirmation toast stays visible (seconds).
    /// Default: 15
    #[dynamic(default = "default_confirm_timeout_secs")]
    pub confirm_timeout_secs: u64,
}

impl Default for OpenUrlConfig {
    fn default() -> Self {
        Self {
            default_policy: OpenUrlPolicy::Confirm,
            allow_list: default_open_url_allow_list(),
            confirm_timeout_secs: 15,
        }
    }
}

/// Check the open-URL policy for a given URL.
///
/// Returns the policy to apply:
/// - Non-http(s) schemes → always `Deny`
/// - URL matches an allow_list entry (prefix) → `Allow`
/// - Otherwise → the configured `default_policy`
///
/// If `domain_config` is provided, its allow_list and default_policy are checked
/// first. If the URL doesn't match the domain allow_list, the global config is
/// checked as a fallback.
pub fn check_open_url_policy(url: &str, domain_config: Option<&OpenUrlConfig>) -> OpenUrlPolicy {
    let global_cfg = crate::configuration().open_url.clone();
    check_open_url_policy_with(url, domain_config, &global_cfg)
}

/// Inner implementation that takes the global config explicitly (for testing).
pub fn check_open_url_policy_with(
    url: &str,
    domain_config: Option<&OpenUrlConfig>,
    global_config: &OpenUrlConfig,
) -> OpenUrlPolicy {
    // Step 1: Reject non-http(s) schemes
    let lower = url.to_ascii_lowercase();
    if !lower.starts_with("http://") && !lower.starts_with("https://") {
        log::warn!(
            "Blocked URL with disallowed scheme: {}",
            url.chars().take(80).collect::<String>()
        );
        return OpenUrlPolicy::Deny;
    }

    // Step 2: Check domain-level allow_list (if present)
    if let Some(domain_cfg) = domain_config {
        if domain_cfg
            .allow_list
            .iter()
            .any(|prefix| url.starts_with(prefix.as_str()))
        {
            return OpenUrlPolicy::Allow;
        }
    }

    // Step 3: Check global allow_list
    if global_config
        .allow_list
        .iter()
        .any(|prefix| url.starts_with(prefix.as_str()))
    {
        return OpenUrlPolicy::Allow;
    }

    // Step 4: Return the most specific default_policy
    if let Some(domain_cfg) = domain_config {
        domain_cfg.default_policy
    } else {
        global_config.default_policy
    }
}
// --- end weezterm remote features ---
// --- end weezterm remote features ---

impl SshDomain {
    pub fn default_domains() -> Vec<Self> {
        let mut config = wezterm_ssh::Config::new();
        config.add_default_config_files();

        let mut plain_ssh = vec![];
        let mut mux_ssh = vec![];
        for host in config.enumerate_hosts() {
            plain_ssh.push(Self {
                name: format!("SSH:{host}"),
                remote_address: host.to_string(),
                multiplexing: SshMultiplexing::None,
                local_echo_threshold_ms: default_local_echo_threshold_ms(),
                ..SshDomain::default()
            });

            mux_ssh.push(Self {
                name: format!("SSHMUX:{host}"),
                remote_address: host.to_string(),
                multiplexing: SshMultiplexing::WezTerm,
                local_echo_threshold_ms: default_local_echo_threshold_ms(),
                ..SshDomain::default()
            });
        }

        plain_ssh.append(&mut mux_ssh);
        plain_ssh
    }
}

#[derive(Clone, Debug)]
pub struct SshParameters {
    pub username: Option<String>,
    pub host_and_port: String,
}

impl Display for SshParameters {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(user) = &self.username {
            write!(f, "{}@{}", user, self.host_and_port)
        } else {
            write!(f, "{}", self.host_and_port)
        }
    }
}

pub fn username_from_env() -> anyhow::Result<String> {
    #[cfg(unix)]
    const USER: &str = "USER";
    #[cfg(windows)]
    const USER: &str = "USERNAME";

    std::env::var(USER).with_context(|| format!("while resolving {} env var", USER))
}

impl FromStr for SshParameters {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split('@').collect();

        if parts.len() == 2 {
            Ok(Self {
                username: Some(parts[0].to_string()),
                host_and_port: parts[1].to_string(),
            })
        } else if parts.len() == 1 {
            Ok(Self {
                username: None,
                host_and_port: parts[0].to_string(),
            })
        } else {
            bail!("failed to parse ssh parameters from `{}`", s);
        }
    }
}

// --- weezterm remote features ---
#[cfg(test)]
mod test {
    use super::*;

    fn global_config() -> OpenUrlConfig {
        OpenUrlConfig {
            default_policy: OpenUrlPolicy::Confirm,
            allow_list: vec![
                "https://login.microsoftonline.com/".to_string(),
                "https://login.live.com/".to_string(),
            ],
            ..Default::default()
        }
    }

    #[test]
    fn test_allow_listed_url_is_allowed() {
        let global = global_config();
        let policy = check_open_url_policy_with(
            "https://login.microsoftonline.com/oauth2/authorize?client_id=abc",
            None,
            &global,
        );
        assert_eq!(policy, OpenUrlPolicy::Allow);
    }

    #[test]
    fn test_non_listed_https_url_gets_default_policy() {
        let global = global_config();
        let policy = check_open_url_policy_with("https://example.com/page", None, &global);
        assert_eq!(policy, OpenUrlPolicy::Confirm);
    }

    #[test]
    fn test_file_scheme_always_denied() {
        let global = global_config();
        let policy = check_open_url_policy_with("file:///etc/passwd", None, &global);
        assert_eq!(policy, OpenUrlPolicy::Deny);
    }

    #[test]
    fn test_javascript_scheme_denied() {
        let global = global_config();
        let policy = check_open_url_policy_with("javascript:alert(1)", None, &global);
        assert_eq!(policy, OpenUrlPolicy::Deny);
    }

    #[test]
    fn test_data_scheme_denied() {
        let global = global_config();
        let policy =
            check_open_url_policy_with("data:text/html,<script>alert(1)</script>", None, &global);
        assert_eq!(policy, OpenUrlPolicy::Deny);
    }

    #[test]
    fn test_empty_string_denied() {
        let global = global_config();
        let policy = check_open_url_policy_with("", None, &global);
        assert_eq!(policy, OpenUrlPolicy::Deny);
    }

    #[test]
    fn test_domain_allow_list_takes_precedence() {
        let global = global_config();
        let domain = OpenUrlConfig {
            default_policy: OpenUrlPolicy::Deny,
            allow_list: vec!["https://internal.corp.com/".to_string()],
            ..Default::default()
        };
        let policy = check_open_url_policy_with(
            "https://internal.corp.com/dashboard",
            Some(&domain),
            &global,
        );
        assert_eq!(policy, OpenUrlPolicy::Allow);
    }

    #[test]
    fn test_domain_default_policy_used_when_not_listed() {
        let global = global_config();
        let domain = OpenUrlConfig {
            default_policy: OpenUrlPolicy::Allow,
            allow_list: vec![],
            ..Default::default()
        };
        // URL not in domain or global allow_list, but domain says Allow
        let policy =
            check_open_url_policy_with("https://some-random-site.com", Some(&domain), &global);
        assert_eq!(policy, OpenUrlPolicy::Allow);
    }

    #[test]
    fn test_global_allow_list_used_when_domain_has_no_match() {
        let global = global_config();
        let domain = OpenUrlConfig {
            default_policy: OpenUrlPolicy::Deny,
            allow_list: vec!["https://other.com/".to_string()],
            ..Default::default()
        };
        // URL matches global allow_list but not domain's
        let policy = check_open_url_policy_with(
            "https://login.microsoftonline.com/oauth2",
            Some(&domain),
            &global,
        );
        assert_eq!(policy, OpenUrlPolicy::Allow);
    }

    #[test]
    fn test_case_insensitive_scheme_check() {
        let global = global_config();
        // Uppercase scheme should be ok (scheme check is case-insensitive)
        let policy = check_open_url_policy_with("HTTPS://example.com", None, &global);
        assert_eq!(policy, OpenUrlPolicy::Confirm); // Not denied, just not allow-listed

        let policy = check_open_url_policy_with("FILE:///etc/passwd", None, &global);
        assert_eq!(policy, OpenUrlPolicy::Deny);
    }

    #[test]
    fn test_http_url_allowed_through() {
        let global = global_config();
        // http:// (not https) should pass scheme check (gets default policy)
        let policy = check_open_url_policy_with("http://localhost:3000", None, &global);
        assert_eq!(policy, OpenUrlPolicy::Confirm);
    }

    #[test]
    fn test_deny_default_policy() {
        let global = OpenUrlConfig {
            default_policy: OpenUrlPolicy::Deny,
            allow_list: vec![],
            ..Default::default()
        };
        let policy = check_open_url_policy_with("https://example.com", None, &global);
        assert_eq!(policy, OpenUrlPolicy::Deny);
    }
}
// --- end weezterm remote features ---
