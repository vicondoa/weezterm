// --- weezterm remote features ---
use crate::config::validate_domain_name;
use crate::ssh::SshDomain;
use wezterm_dynamic::{FromDynamic, ToDynamic};

#[derive(Default, Debug, Clone, FromDynamic, ToDynamic)]
pub struct DevContainerDomainConfig {
    /// The name of this domain. Must be unique amongst all domain types.
    #[dynamic(validate = "validate_domain_name")]
    pub name: String,

    /// SSH connection config — uses the exact same `SshDomain` type from
    /// `config/src/ssh.rs`. All SSH domain options are available: hostname,
    /// username, port, multiplexing, ssh_backend, identity files,
    /// no_agent_auth, connect_automatically, remote_wezterm_path, etc.
    /// If absent, uses local Docker (no SSH).
    pub ssh: Option<SshDomain>,

    /// Default local workspace folder on the remote host.
    /// When set, the domain auto-discovers the devcontainer whose
    /// `devcontainer.local_folder` label matches this path and makes
    /// it the primary container. If no match, offers to create one.
    pub default_workspace_folder: Option<String>,

    /// Default container name or ID to auto-connect to.
    /// Takes priority over workspace-folder matching.
    pub default_container: Option<String>,

    /// Docker executable path (default: "docker")
    #[dynamic(default = "default_docker_command")]
    pub docker_command: String,

    /// Devcontainer CLI path (default: "devcontainer")
    #[dynamic(default = "default_devcontainer_command")]
    pub devcontainer_command: String,

    /// Default shell to use inside containers (default: "/bin/bash")
    pub default_shell: Option<String>,

    /// Override the container's default user. If not set, docker exec
    /// uses the container's own default user (from Dockerfile USER or
    /// devcontainer.json remoteUser).
    pub override_user: Option<String>,

    /// Poll interval for container discovery in seconds (default: 10)
    #[dynamic(default = "default_poll_interval_secs")]
    pub poll_interval_secs: u64,

    /// Whether to auto-discover running devcontainers on attach (default: true)
    #[dynamic(default = "default_true_bool")]
    pub auto_discover: bool,
}

fn default_docker_command() -> String {
    "docker".to_string()
}

fn default_devcontainer_command() -> String {
    "devcontainer".to_string()
}

fn default_poll_interval_secs() -> u64 {
    10
}

fn default_true_bool() -> bool {
    true
}
// --- end weezterm remote features ---
