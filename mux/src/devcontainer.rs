// --- weezterm remote features ---
//! DevContainer domain implementation.
//!
//! Provides a Domain that spawns shells inside Docker devcontainers.
//! Supports both local Docker and remote Docker via SSH (with optional
//! mux server for session persistence).

use crate::devcontainer_discover::DevContainerInfo;
use crate::domain::{
    alloc_domain_id, Domain, DomainId, DomainState, FailedProcessSpawn, FailedSpawnPty,
    WriterWrapper,
};
use crate::localpane::LocalPane;
use crate::pane::{alloc_pane_id, Pane};
use crate::window::WindowId;
use crate::Mux;
use anyhow::bail;
use async_trait::async_trait;
use config::devcontainer::DevContainerDomainConfig;
use parking_lot::Mutex;
use portable_pty::{native_pty_system, CommandBuilder, PtySystem};
use std::io::Write;
use std::sync::Arc;
use wezterm_term::TerminalSize;

/// State of a managed devcontainer.
#[derive(Debug, Clone)]
struct ContainerManagerState {
    /// All known containers on this host.
    containers: Vec<DevContainerInfo>,
    /// The container selected as primary (new tabs spawn here).
    primary_container_id: Option<String>,
}

impl ContainerManagerState {
    fn new() -> Self {
        Self {
            containers: Vec::new(),
            primary_container_id: None,
        }
    }

    fn primary_container(&self) -> Option<&DevContainerInfo> {
        let id = self.primary_container_id.as_ref()?;
        self.containers.iter().find(|c| &c.container_id == id)
    }

    fn running_containers(&self) -> Vec<&DevContainerInfo> {
        self.containers
            .iter()
            .filter(|c| c.status.is_running())
            .collect()
    }

    /// Find the container matching a workspace folder.
    fn find_by_workspace(&self, workspace: &str) -> Option<&DevContainerInfo> {
        self.containers
            .iter()
            .find(|c| c.local_folder == workspace && c.status.is_running())
    }

    /// Set the primary container. Returns the container info if found.
    fn set_primary(&mut self, container_id: &str) -> Option<&DevContainerInfo> {
        if self
            .containers
            .iter()
            .any(|c| c.container_id == container_id)
        {
            self.primary_container_id = Some(container_id.to_string());
            self.primary_container()
        } else {
            None
        }
    }

    /// Auto-select a primary container based on config and available containers.
    fn auto_select_primary(&mut self, config: &DevContainerDomainConfig) {
        if self.primary_container().is_some() {
            return;
        }

        // Try default_container first
        if let Some(ref default) = config.default_container {
            if let Some(c) = self.containers.iter().find(|c| {
                c.status.is_running()
                    && (c.container_id.starts_with(default) || c.container_name == *default)
            }) {
                self.primary_container_id = Some(c.container_id.clone());
                return;
            }
        }

        // Try workspace folder match
        if let Some(ref workspace) = config.default_workspace_folder {
            if let Some(c) = self.find_by_workspace(workspace) {
                self.primary_container_id = Some(c.container_id.clone());
                return;
            }
        }

        // Auto-select if exactly one running container
        let running = self.running_containers();
        if running.len() == 1 {
            self.primary_container_id = Some(running[0].container_id.clone());
        }
    }
}

/// A domain that spawns shells inside Docker devcontainers.
///
/// For local Docker, this directly runs `docker exec` via a native PTY.
/// For remote Docker (with SSH), this delegates to the SSH mux infrastructure
/// or wraps SSH PTY channels, depending on the `multiplexing` setting.
pub struct DevContainerDomain {
    id: DomainId,
    name: String,
    config: DevContainerDomainConfig,
    pty_system: Mutex<Box<dyn PtySystem + Send>>,
    state: Mutex<ContainerManagerState>,
}

impl DevContainerDomain {
    /// Create a new local DevContainerDomain (Docker on localhost).
    pub fn new(config: DevContainerDomainConfig) -> Self {
        let id = alloc_domain_id();
        Self {
            id,
            name: config.name.clone(),
            config,
            pty_system: Mutex::new(native_pty_system()),
            state: Mutex::new(ContainerManagerState::new()),
        }
    }

    /// Build the `docker exec` CommandBuilder for a container.
    fn build_docker_exec_command(&self, container: &DevContainerInfo) -> CommandBuilder {
        let mut cmd = CommandBuilder::new(&self.config.docker_command);
        cmd.arg("exec");
        cmd.arg("-it");
        if let Some(ref user) = self.config.override_user {
            cmd.arg("-u");
            cmd.arg(user);
        }
        if let Some(ref workdir) = container.workspace_folder {
            cmd.arg("-w");
            cmd.arg(workdir);
        }
        cmd.arg(&container.container_id);
        let shell = self.config.default_shell.as_deref().unwrap_or("/bin/bash");
        cmd.arg(shell);
        cmd
    }

    /// Get the primary container info, if one is set.
    pub fn primary_container(&self) -> Option<DevContainerInfo> {
        self.state.lock().primary_container().cloned()
    }

    /// Get all known containers.
    pub fn containers(&self) -> Vec<DevContainerInfo> {
        self.state.lock().containers.clone()
    }

    /// Set the primary container by ID.
    pub fn set_primary_container(&self, container_id: &str) -> bool {
        self.state.lock().set_primary(container_id).is_some()
    }

    /// Update the container list from discovery results.
    pub fn update_containers(&self, containers: Vec<DevContainerInfo>) {
        let mut state = self.state.lock();
        state.containers = containers;
        state.auto_select_primary(&self.config);
    }

    /// Get the domain config.
    pub fn config(&self) -> &DevContainerDomainConfig {
        &self.config
    }
}

#[async_trait(?Send)]
impl Domain for DevContainerDomain {
    async fn spawn_pane(
        &self,
        size: TerminalSize,
        command: Option<CommandBuilder>,
        _command_dir: Option<String>,
    ) -> anyhow::Result<Arc<dyn Pane>> {
        // If an explicit command was provided (e.g. from SpawnV2 in mux mode),
        // use it directly. Otherwise, build a docker exec command for the
        // primary container.
        let cmd = if let Some(cmd) = command {
            cmd
        } else {
            let container = self.primary_container().ok_or_else(|| {
                anyhow::anyhow!(
                    "No primary devcontainer selected for domain '{}'. \
                         Use the DevContainer Manager (Ctrl+Shift+D) to select one.",
                    self.name
                )
            })?;

            if !container.status.is_running() {
                bail!(
                    "Primary devcontainer '{}' is not running (status: {:?}). \
                     Start it first or select a different container.",
                    container.container_name,
                    container.status,
                );
            }

            self.build_docker_exec_command(&container)
        };

        let pane_id = alloc_pane_id();
        let pair = self
            .pty_system
            .lock()
            .openpty(crate::terminal_size_to_pty_size(size)?)?;

        let command_line = cmd
            .as_unix_command_line()
            .unwrap_or_else(|err| format!("error rendering command line: {:?}", err));
        let command_description = format!(
            "\"{}\" in devcontainer domain \"{}\"",
            if command_line.is_empty() {
                cmd.get_shell()
            } else {
                command_line
            },
            self.name
        );

        let child_result = pair.slave.spawn_command(cmd);
        let mut writer = WriterWrapper::new(pair.master.take_writer()?);

        let terminal = wezterm_term::Terminal::new(
            size,
            std::sync::Arc::new(config::TermConfig::new()),
            config::branding::APP_NAME_DISPLAY,
            config::wezterm_version(),
            Box::new(writer.clone()),
        );

        let pane: Arc<dyn Pane> = match child_result {
            Ok(child) => Arc::new(LocalPane::new(
                pane_id,
                terminal,
                child,
                pair.master,
                Box::new(writer),
                self.id,
                command_description,
            )),
            Err(err) => {
                write!(writer, "{err:#}").ok();
                Arc::new(LocalPane::new(
                    pane_id,
                    terminal,
                    Box::new(FailedProcessSpawn {}),
                    Box::new(FailedSpawnPty {
                        inner: Mutex::new(pair.master),
                    }),
                    Box::new(writer),
                    self.id,
                    command_description,
                ))
            }
        };

        let mux = Mux::get();
        mux.add_pane(&pane)?;

        Ok(pane)
    }

    fn domain_id(&self) -> DomainId {
        self.id
    }

    fn domain_name(&self) -> &str {
        &self.name
    }

    async fn domain_label(&self) -> String {
        let container_info = if let Some(primary) = self.primary_container() {
            format!("{} ({})", primary.container_name, primary.image)
        } else if let Some(ref workspace) = self.config.default_workspace_folder {
            workspace.clone()
        } else {
            "no container".to_string()
        };
        format!("\u{1F4E6} {} \u{2014} {}", self.name, container_info)
    }

    async fn attach(&self, _window_id: Option<WindowId>) -> anyhow::Result<()> {
        Ok(())
    }

    fn detachable(&self) -> bool {
        false
    }

    fn detach(&self) -> anyhow::Result<()> {
        bail!("detach not implemented for DevContainerDomain");
    }

    fn state(&self) -> DomainState {
        DomainState::Attached
    }
}
// --- end weezterm remote features ---
