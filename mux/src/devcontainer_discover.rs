// --- weezterm remote features ---
//! Docker container discovery for devcontainer domains.
//!
//! Discovers devcontainers by querying Docker via the CLI with label filters.
//! Uses `docker ps` and `docker inspect` — never the devcontainer CLI
//! (which is only used for creating new containers).

use anyhow::Context;
use serde::Deserialize;
use std::collections::HashMap;

/// Status of a Docker container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainerStatus {
    Running,
    Exited,
    Paused,
    Restarting,
    Created,
    Dead,
    Removing,
}

impl ContainerStatus {
    pub fn from_docker_state(state: &str) -> Self {
        match state.to_lowercase().as_str() {
            "running" => Self::Running,
            "exited" => Self::Exited,
            "paused" => Self::Paused,
            "restarting" => Self::Restarting,
            "created" => Self::Created,
            "dead" => Self::Dead,
            "removing" => Self::Removing,
            _ => Self::Exited,
        }
    }

    pub fn is_running(&self) -> bool {
        *self == Self::Running
    }
}

/// Information about a discovered devcontainer.
#[derive(Debug, Clone)]
pub struct DevContainerInfo {
    /// Docker container ID (full sha256)
    pub container_id: String,
    /// Docker container name (without leading /)
    pub container_name: String,
    /// Container status
    pub status: ContainerStatus,
    /// Docker image name
    pub image: String,
    /// From `devcontainer.local_folder` label — host workspace path
    pub local_folder: String,
    /// From `devcontainer.config_file` label
    pub config_file: Option<String>,
    /// Workspace folder inside the container (from metadata or inspect)
    pub workspace_folder: Option<String>,
    /// Container creation timestamp
    pub created_at: String,
}

/// Raw JSON output from `docker ps --format '{{json .}}'`
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct DockerPsEntry {
    #[serde(rename = "ID")]
    id: String,
    names: String,
    state: String,
    image: String,
    created_at: Option<String>,
    labels: Option<String>,
}

/// Labels from `docker inspect --format '{{json .Config.Labels}}'`
#[derive(Debug, Deserialize, Default)]
struct DockerLabels {
    #[serde(rename = "devcontainer.local_folder")]
    local_folder: Option<String>,
    #[serde(rename = "devcontainer.config_file")]
    config_file: Option<String>,
    #[serde(rename = "devcontainer.metadata")]
    metadata: Option<String>,
}

/// Parsed devcontainer metadata from the `devcontainer.metadata` label.
/// This is a JSON array; we merge all entries.
#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct DevContainerMetadataEntry {
    remote_user: Option<String>,
    container_user: Option<String>,
    remote_env: Option<HashMap<String, String>>,
}

/// Full inspect output (subset of fields we care about)
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct DockerInspectResult {
    id: String,
    name: String,
    created: Option<String>,
    config: DockerInspectConfig,
    state: DockerInspectState,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct DockerInspectConfig {
    image: Option<String>,
    labels: Option<HashMap<String, String>>,
    working_dir: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct DockerInspectState {
    status: String,
}

/// Parse the JSON output of `docker ps -a --filter "label=devcontainer.local_folder" --format '{{json .}}'`.
///
/// Each line is a separate JSON object.
pub fn parse_docker_ps_output(output: &str) -> Vec<DockerPsEntry> {
    output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            serde_json::from_str::<DockerPsEntry>(line)
                .map_err(|e| {
                    log::debug!("Failed to parse docker ps line: {}: {}", e, line);
                    e
                })
                .ok()
        })
        .collect()
}

/// Parse the JSON output of `docker inspect <ids...>`.
///
/// Output is a JSON array of inspect results.
pub fn parse_docker_inspect_output(output: &str) -> anyhow::Result<Vec<DockerInspectResult>> {
    serde_json::from_str(output).context("Failed to parse docker inspect output")
}

/// Convert a `DockerInspectResult` into a `DevContainerInfo`.
pub fn inspect_to_container_info(inspect: &DockerInspectResult) -> Option<DevContainerInfo> {
    let labels = inspect.config.labels.as_ref()?;
    let local_folder = labels.get("devcontainer.local_folder")?.clone();

    let config_file = labels.get("devcontainer.config_file").cloned();
    let metadata_str = labels.get("devcontainer.metadata");

    let workspace_folder = extract_workspace_folder(
        metadata_str.map(|s| s.as_str()),
        inspect.config.working_dir.as_deref(),
    );

    let name = inspect
        .name
        .strip_prefix('/')
        .unwrap_or(&inspect.name)
        .to_string();

    Some(DevContainerInfo {
        container_id: inspect.id.clone(),
        container_name: name,
        status: ContainerStatus::from_docker_state(&inspect.state.status),
        image: inspect
            .config
            .image
            .clone()
            .unwrap_or_else(|| "<unknown>".to_string()),
        local_folder,
        config_file,
        workspace_folder,
        created_at: inspect.created.clone().unwrap_or_default(),
    })
}

/// Extract workspace folder from devcontainer metadata or fallback to
/// the container's working directory.
fn extract_workspace_folder(metadata_json: Option<&str>, working_dir: Option<&str>) -> Option<String> {
    if let Some(json) = metadata_json {
        // devcontainer.metadata is a JSON array of metadata entries.
        // The last entry with a remoteWorkspaceFolder wins.
        if let Ok(entries) = serde_json::from_str::<Vec<serde_json::Value>>(json) {
            for entry in entries.iter().rev() {
                if let Some(folder) = entry.get("remoteWorkspaceFolder").and_then(|v| v.as_str()) {
                    if !folder.is_empty() {
                        return Some(folder.to_string());
                    }
                }
            }
        }
        // Try as a single object too
        if let Ok(entry) = serde_json::from_str::<serde_json::Value>(json) {
            if let Some(folder) = entry.get("remoteWorkspaceFolder").and_then(|v| v.as_str()) {
                if !folder.is_empty() {
                    return Some(folder.to_string());
                }
            }
        }
    }

    // Fallback to container's WorkingDir
    working_dir.map(|s| s.to_string())
}

/// Build the `docker ps` command arguments for listing devcontainers.
pub fn docker_ps_args(docker_command: &str) -> Vec<String> {
    vec![
        docker_command.to_string(),
        "ps".to_string(),
        "-a".to_string(),
        "--filter".to_string(),
        "label=devcontainer.local_folder".to_string(),
        "--format".to_string(),
        "{{json .}}".to_string(),
    ]
}

/// Build the `docker inspect` command arguments.
pub fn docker_inspect_args(docker_command: &str, container_ids: &[String]) -> Vec<String> {
    let mut args = vec![
        docker_command.to_string(),
        "inspect".to_string(),
    ];
    args.extend(container_ids.iter().cloned());
    args
}

/// Build a `docker exec` command for attaching to a container.
pub fn docker_exec_args(
    docker_command: &str,
    container_id: &str,
    workspace_folder: Option<&str>,
    override_user: Option<&str>,
    shell: &str,
) -> Vec<String> {
    let mut args = vec![
        docker_command.to_string(),
        "exec".to_string(),
        "-it".to_string(),
    ];
    if let Some(user) = override_user {
        args.push("-u".to_string());
        args.push(user.to_string());
    }
    if let Some(workdir) = workspace_folder {
        args.push("-w".to_string());
        args.push(workdir.to_string());
    }
    args.push(container_id.to_string());
    args.push(shell.to_string());
    args
}

/// Build a `docker start` command.
pub fn docker_start_args(docker_command: &str, container_id: &str) -> Vec<String> {
    vec![
        docker_command.to_string(),
        "start".to_string(),
        container_id.to_string(),
    ]
}

/// Build a `docker stop` command.
pub fn docker_stop_args(docker_command: &str, container_id: &str) -> Vec<String> {
    vec![
        docker_command.to_string(),
        "stop".to_string(),
        container_id.to_string(),
    ]
}

/// Build a `docker rm` command.
pub fn docker_rm_args(docker_command: &str, container_id: &str) -> Vec<String> {
    vec![
        docker_command.to_string(),
        "rm".to_string(),
        container_id.to_string(),
    ]
}

/// Build a `devcontainer up` command.
pub fn devcontainer_up_args(devcontainer_command: &str, workspace_folder: &str) -> Vec<String> {
    vec![
        devcontainer_command.to_string(),
        "up".to_string(),
        "--workspace-folder".to_string(),
        workspace_folder.to_string(),
    ]
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_parse_docker_ps_single_line() {
        let input = r#"{"ID":"abc123","Names":"my-container","State":"running","Image":"python:3.11","CreatedAt":"2025-01-15 10:00:00","Labels":"devcontainer.local_folder=/home/user/proj"}"#;
        let entries = parse_docker_ps_output(input);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "abc123");
        assert_eq!(entries[0].names, "my-container");
        assert_eq!(entries[0].state, "running");
        assert_eq!(entries[0].image, "python:3.11");
    }

    #[test]
    fn test_parse_docker_ps_multiple_lines() {
        let input = concat!(
            r#"{"ID":"abc","Names":"c1","State":"running","Image":"img1"}"#,
            "\n",
            r#"{"ID":"def","Names":"c2","State":"exited","Image":"img2"}"#,
            "\n",
        );
        let entries = parse_docker_ps_output(input);
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_parse_docker_ps_empty() {
        let entries = parse_docker_ps_output("");
        assert_eq!(entries.len(), 0);
    }

    #[test]
    fn test_parse_docker_ps_invalid_line_skipped() {
        let input = concat!(
            r#"{"ID":"abc","Names":"c1","State":"running","Image":"img1"}"#,
            "\n",
            "this is not json\n",
            r#"{"ID":"def","Names":"c2","State":"exited","Image":"img2"}"#,
        );
        let entries = parse_docker_ps_output(input);
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_container_status_from_docker_state() {
        assert_eq!(
            ContainerStatus::from_docker_state("running"),
            ContainerStatus::Running
        );
        assert_eq!(
            ContainerStatus::from_docker_state("Running"),
            ContainerStatus::Running
        );
        assert_eq!(
            ContainerStatus::from_docker_state("exited"),
            ContainerStatus::Exited
        );
        assert_eq!(
            ContainerStatus::from_docker_state("paused"),
            ContainerStatus::Paused
        );
        assert_eq!(
            ContainerStatus::from_docker_state("unknown"),
            ContainerStatus::Exited
        );
    }

    #[test]
    fn test_parse_docker_inspect() {
        let input = r#"[{
            "Id": "abc123def456",
            "Name": "/my-container",
            "Created": "2025-01-15T10:00:00Z",
            "Config": {
                "Image": "python:3.11",
                "Labels": {
                    "devcontainer.local_folder": "/home/user/proj",
                    "devcontainer.config_file": "/home/user/proj/.devcontainer/devcontainer.json"
                },
                "WorkingDir": "/workspaces/proj"
            },
            "State": {
                "Status": "running"
            }
        }]"#;
        let results = parse_docker_inspect_output(input).unwrap();
        assert_eq!(results.len(), 1);
        let info = inspect_to_container_info(&results[0]).unwrap();
        assert_eq!(info.container_id, "abc123def456");
        assert_eq!(info.container_name, "my-container");
        assert_eq!(info.status, ContainerStatus::Running);
        assert_eq!(info.image, "python:3.11");
        assert_eq!(info.local_folder, "/home/user/proj");
        assert_eq!(
            info.config_file.as_deref(),
            Some("/home/user/proj/.devcontainer/devcontainer.json")
        );
        assert_eq!(info.workspace_folder.as_deref(), Some("/workspaces/proj"));
    }

    #[test]
    fn test_inspect_without_devcontainer_label_returns_none() {
        let input = r#"[{
            "Id": "abc",
            "Name": "/regular-container",
            "Config": {
                "Labels": {},
                "WorkingDir": "/app"
            },
            "State": { "Status": "running" }
        }]"#;
        let results = parse_docker_inspect_output(input).unwrap();
        assert!(inspect_to_container_info(&results[0]).is_none());
    }

    #[test]
    fn test_extract_workspace_folder_from_metadata() {
        let metadata = r#"[{"remoteWorkspaceFolder": "/workspaces/my-project"}]"#;
        let result = extract_workspace_folder(Some(metadata), Some("/app"));
        assert_eq!(result.as_deref(), Some("/workspaces/my-project"));
    }

    #[test]
    fn test_extract_workspace_folder_fallback_to_workdir() {
        let result = extract_workspace_folder(None, Some("/app"));
        assert_eq!(result.as_deref(), Some("/app"));
    }

    #[test]
    fn test_extract_workspace_folder_single_object_metadata() {
        let metadata = r#"{"remoteWorkspaceFolder": "/workspaces/proj"}"#;
        let result = extract_workspace_folder(Some(metadata), None);
        assert_eq!(result.as_deref(), Some("/workspaces/proj"));
    }

    #[test]
    fn test_docker_exec_args_minimal() {
        let args = docker_exec_args("docker", "abc123", None, None, "/bin/bash");
        assert_eq!(
            args,
            vec!["docker", "exec", "-it", "abc123", "/bin/bash"]
        );
    }

    #[test]
    fn test_docker_exec_args_with_user_and_workdir() {
        let args = docker_exec_args(
            "docker",
            "abc123",
            Some("/workspaces/proj"),
            Some("vscode"),
            "/bin/zsh",
        );
        assert_eq!(
            args,
            vec![
                "docker",
                "exec",
                "-it",
                "-u",
                "vscode",
                "-w",
                "/workspaces/proj",
                "abc123",
                "/bin/zsh"
            ]
        );
    }

    #[test]
    fn test_docker_exec_args_workdir_only() {
        let args =
            docker_exec_args("docker", "abc123", Some("/workspaces/proj"), None, "/bin/bash");
        assert_eq!(
            args,
            vec!["docker", "exec", "-it", "-w", "/workspaces/proj", "abc123", "/bin/bash"]
        );
    }
}
// --- end weezterm remote features ---
