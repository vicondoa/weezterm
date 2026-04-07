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

    /// Enable detection via /proc/net/tcp polling (Linux only).
    /// Default: true
    #[dynamic(default = "default_true_bool")]
    pub detect_with_proc_net_tcp: bool,

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
}

impl Default for PortForwardConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_forward: true,
            detect_with_proc_net_tcp: true,
            detect_with_terminal_scrape: true,
            poll_interval_secs: 2,
            exclude_ports: vec![22],
            include_ports: vec![],
        }
    }
}

impl_lua_conversion_dynamic!(PortForwardConfig);
impl_lua_conversion_dynamic!(SshDomain);

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
