//! Persistence for config overlay proposals.
//!
//! Saves and loads proposed config values to/from a JSON file in the config
//! directory so they survive process restarts.
//!
//! --- weezterm remote features ---

use super::data::SshDomainConfig;
use std::collections::HashMap;
use wezterm_dynamic::Value;

const OVERLAY_FILE_NAME: &str = "config-overlay.json";

/// Combined overlay data: proposals + user-managed SSH domains.
pub struct OverlayData {
    pub proposals: HashMap<String, Value>,
    pub ssh_domains: Vec<SshDomainConfig>,
}

impl Default for OverlayData {
    fn default() -> Self {
        Self {
            proposals: HashMap::new(),
            ssh_domains: vec![],
        }
    }
}

/// Returns the path to the overlay proposals file.
fn overlay_file_path() -> Option<std::path::PathBuf> {
    config::CONFIG_DIRS
        .first()
        .map(|dir| dir.join(OVERLAY_FILE_NAME))
}

/// Load saved overlay data from disk.
///
/// Supports both old format (flat object of proposals) and new format
/// with `proposals` and `ssh_domains` keys.
pub fn load_overlay_data() -> anyhow::Result<OverlayData> {
    let path = match overlay_file_path() {
        Some(p) => p,
        None => {
            return Ok(OverlayData {
                proposals: HashMap::new(),
                ssh_domains: vec![],
            })
        }
    };

    if !path.exists() {
        return Ok(OverlayData {
            proposals: HashMap::new(),
            ssh_domains: vec![],
        });
    }

    let content = std::fs::read_to_string(&path)?;
    let json: serde_json::Value = serde_json::from_str(&content)?;

    match &json {
        serde_json::Value::Object(obj) if obj.contains_key("proposals") => {
            // New format: { "proposals": {...}, "ssh_domains": [...] }
            let proposals = if let Some(p) = obj.get("proposals") {
                parse_proposals(p)
            } else {
                HashMap::new()
            };
            let ssh_domains = if let Some(d) = obj.get("ssh_domains") {
                parse_ssh_domains(d)
            } else {
                vec![]
            };
            Ok(OverlayData {
                proposals,
                ssh_domains,
            })
        }
        serde_json::Value::Object(_) => {
            // Old format: flat object of proposals (backward compat)
            Ok(OverlayData {
                proposals: parse_proposals(&json),
                ssh_domains: vec![],
            })
        }
        _ => Ok(OverlayData {
            proposals: HashMap::new(),
            ssh_domains: vec![],
        }),
    }
}

/// Load saved proposals from disk (backward-compatible wrapper).
pub fn load_proposals() -> anyhow::Result<HashMap<String, Value>> {
    Ok(load_overlay_data()?.proposals)
}

/// Save overlay data (proposals + domains) to disk as JSON.
pub fn save_overlay_data(
    proposals: &HashMap<String, Value>,
    ssh_domains: &[SshDomainConfig],
) -> anyhow::Result<()> {
    let path = match overlay_file_path() {
        Some(p) => p,
        None => anyhow::bail!("No config directory found"),
    };

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut root = serde_json::Map::new();

    // Serialize proposals
    let mut proposals_obj = serde_json::Map::new();
    for (key, val) in proposals {
        proposals_obj.insert(key.clone(), dynamic_to_json(val));
    }
    root.insert(
        "proposals".to_string(),
        serde_json::Value::Object(proposals_obj),
    );

    // Serialize SSH domains
    let domains_arr: Vec<serde_json::Value> = ssh_domains.iter().map(domain_to_json).collect();
    root.insert(
        "ssh_domains".to_string(),
        serde_json::Value::Array(domains_arr),
    );

    let json_str = serde_json::to_string_pretty(&serde_json::Value::Object(root))?;
    std::fs::write(&path, json_str)?;

    log::info!("Config overlay data saved to {}", path.display());
    Ok(())
}

/// Save proposals to disk as JSON (backward-compatible wrapper).
pub fn save_proposals(proposals: &HashMap<String, Value>) -> anyhow::Result<()> {
    // Load existing data to preserve domains
    let existing = load_overlay_data().unwrap_or(OverlayData {
        proposals: HashMap::new(),
        ssh_domains: vec![],
    });
    save_overlay_data(proposals, &existing.ssh_domains)
}

/// Convert proposals map into a `wezterm_dynamic::Value::Object` suitable
/// for `TermWindowNotif::SetConfigOverrides`.
pub fn proposals_to_overrides(proposals: HashMap<String, Value>) -> Value {
    Value::Object(
        proposals
            .into_iter()
            .map(|(k, v)| (Value::String(k), v))
            .collect(),
    )
}

fn parse_proposals(val: &serde_json::Value) -> HashMap<String, Value> {
    let mut proposals = HashMap::new();
    if let serde_json::Value::Object(obj) = val {
        for (key, v) in obj {
            proposals.insert(key.clone(), json_to_dynamic(v));
        }
    }
    proposals
}

fn parse_ssh_domains(val: &serde_json::Value) -> Vec<SshDomainConfig> {
    let mut domains = vec![];
    if let serde_json::Value::Array(arr) = val {
        for item in arr {
            if let serde_json::Value::Object(obj) = item {
                domains.push(SshDomainConfig {
                    name: obj
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    remote_address: obj
                        .get("remote_address")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    username: obj
                        .get("username")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    multiplexing: obj
                        .get("multiplexing")
                        .and_then(|v| v.as_str())
                        .unwrap_or("None")
                        .to_string(),
                    ssh_backend: obj
                        .get("ssh_backend")
                        .and_then(|v| v.as_str())
                        .unwrap_or("LibSsh")
                        .to_string(),
                    no_agent_auth: obj
                        .get("no_agent_auth")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                    connect_automatically: obj
                        .get("connect_automatically")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                });
            }
        }
    }
    domains
}

fn domain_to_json(dom: &SshDomainConfig) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    obj.insert(
        "name".to_string(),
        serde_json::Value::String(dom.name.clone()),
    );
    obj.insert(
        "remote_address".to_string(),
        serde_json::Value::String(dom.remote_address.clone()),
    );
    obj.insert(
        "username".to_string(),
        serde_json::Value::String(dom.username.clone()),
    );
    obj.insert(
        "multiplexing".to_string(),
        serde_json::Value::String(dom.multiplexing.clone()),
    );
    obj.insert(
        "ssh_backend".to_string(),
        serde_json::Value::String(dom.ssh_backend.clone()),
    );
    obj.insert(
        "no_agent_auth".to_string(),
        serde_json::Value::Bool(dom.no_agent_auth),
    );
    obj.insert(
        "connect_automatically".to_string(),
        serde_json::Value::Bool(dom.connect_automatically),
    );
    serde_json::Value::Object(obj)
}

/// Convert a `serde_json::Value` to `wezterm_dynamic::Value`.
fn json_to_dynamic(val: &serde_json::Value) -> Value {
    match val {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::I64(i)
            } else if let Some(f) = n.as_f64() {
                Value::F64(f.into())
            } else {
                Value::Null
            }
        }
        serde_json::Value::String(s) => Value::String(s.clone()),
        serde_json::Value::Array(arr) => Value::Array(arr.iter().map(json_to_dynamic).collect()),
        serde_json::Value::Object(obj) => Value::Object(
            obj.iter()
                .map(|(k, v)| (Value::String(k.clone()), json_to_dynamic(v)))
                .collect(),
        ),
    }
}

/// Convert a `wezterm_dynamic::Value` to `serde_json::Value`.
fn dynamic_to_json(val: &Value) -> serde_json::Value {
    match val {
        Value::Null => serde_json::Value::Null,
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::I64(i) => serde_json::Value::Number((*i).into()),
        Value::U64(u) => serde_json::Value::Number((*u).into()),
        Value::F64(f) => {
            let f = f64::from(*f);
            serde_json::Number::from_f64(f)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null)
        }
        Value::String(s) => serde_json::Value::String(s.clone()),
        Value::Array(arr) => serde_json::Value::Array(arr.iter().map(dynamic_to_json).collect()),
        Value::Object(obj) => {
            let map: serde_json::Map<String, serde_json::Value> = obj
                .iter()
                .filter_map(|(k, v)| {
                    if let Value::String(key) = k {
                        Some((key.clone(), dynamic_to_json(v)))
                    } else {
                        None
                    }
                })
                .collect();
            serde_json::Value::Object(map)
        }
    }
}
