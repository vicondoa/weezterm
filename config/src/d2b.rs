use crate::config::validate_domain_name;
use d2b_toolkit_core::WorkloadTarget;
use std::convert::TryFrom;
use std::path::PathBuf;
use wezterm_dynamic::{FromDynamic, ToDynamic};

// --- weezterm remote features ---
#[derive(Debug, Clone, PartialEq, Eq, FromDynamic, ToDynamic)]
#[dynamic(try_from = "RawD2bDomainConfig", into = "SerializedD2bDomainConfig")]
pub struct D2bDomainConfig {
    /// Unique WeezTerm domain name for this d2b target.
    #[dynamic(validate = "validate_domain_name")]
    pub name: String,

    /// Canonical d2b workload target, or a legacy VM name during migration.
    pub target: String,

    /// Optional override for the d2b public daemon socket.
    #[dynamic(default)]
    pub socket_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, FromDynamic)]
struct RawD2bDomainConfig {
    #[dynamic(validate = "validate_domain_name")]
    name: String,
    target: Option<String>,
    vm: Option<String>,
    socket_path: Option<PathBuf>,
}

#[derive(ToDynamic)]
struct SerializedD2bDomainConfig {
    name: String,
    target: String,
    socket_path: Option<PathBuf>,
}

impl TryFrom<RawD2bDomainConfig> for D2bDomainConfig {
    type Error = String;

    fn try_from(raw: RawD2bDomainConfig) -> Result<Self, Self::Error> {
        let target = match (raw.target.as_deref(), raw.vm.as_deref()) {
            (Some(target), Some(vm)) => {
                let target = normalize_d2b_target(target)?;
                let vm = normalize_d2b_target(vm)?;
                if target != vm {
                    return Err("d2b domain `target` and compatibility alias `vm` differ; \
                         remove `vm` or make both values identical"
                        .to_string());
                }
                target
            }
            (Some(target), None) | (None, Some(target)) => normalize_d2b_target(target)?,
            (None, None) => {
                return Err("d2b domain requires `target` (or compatibility alias `vm`)".to_string())
            }
        };

        Ok(Self {
            name: raw.name,
            target,
            socket_path: raw.socket_path,
        })
    }
}

impl From<&D2bDomainConfig> for SerializedD2bDomainConfig {
    fn from(config: &D2bDomainConfig) -> Self {
        Self {
            name: config.name.clone(),
            target: config.target.clone(),
            socket_path: config.socket_path.clone(),
        }
    }
}

pub fn normalize_d2b_target(target: &str) -> Result<String, String> {
    if target.starts_with("d2b://") || target.contains('.') {
        return WorkloadTarget::parse(target)
            .map(|target| target.to_canonical())
            .map_err(|_| {
                "d2b targets must use `<workload>.<realm>.d2b` with lowercase labels".to_string()
            });
    }

    validate_d2b_vm_name(target)?;
    Ok(target.to_string())
}

pub(crate) fn validate_d2b_vm_name(name: &str) -> Result<(), String> {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return Err("legacy d2b VM names must start with [a-z]".to_string()),
    }

    if !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
        return Err("legacy d2b VM names may contain only [a-z0-9-]".to_string());
    }

    if name.starts_with("sys-") || name == "launcher" {
        return Err("legacy d2b VM name is reserved by the framework".to_string());
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use wezterm_dynamic::{FromDynamic, ToDynamic, Value};

    fn decode(value: serde_json::Value) -> Result<D2bDomainConfig, wezterm_dynamic::Error> {
        D2bDomainConfig::from_dynamic(&crate::json_to_dynamic(&value), Default::default())
    }

    #[test]
    fn canonical_target_deserializes_and_normalizes_scheme() {
        let config = decode(serde_json::json!({
            "name": "host-tools",
            "target": "d2b://tools.host.d2b"
        }))
        .unwrap();

        assert_eq!(config.target, "tools.host.d2b");
        assert_eq!(config.socket_path, None);
    }

    #[test]
    fn vm_alias_migrates_to_target_and_serializes_canonically() {
        let config = decode(serde_json::json!({
            "name": "work",
            "vm": "corp-vm"
        }))
        .unwrap();
        assert_eq!(config.target, "corp-vm");

        let Value::Object(encoded) = config.to_dynamic() else {
            panic!("d2b domain did not serialize as an object");
        };
        assert_eq!(
            encoded.get_by_str("target"),
            Some(&Value::String("corp-vm".to_string()))
        );
        assert!(encoded.get_by_str("vm").is_none());
    }

    #[test]
    fn matching_target_and_vm_alias_are_accepted() {
        let config = decode(serde_json::json!({
            "name": "work",
            "target": "corp.work.d2b",
            "vm": "d2b://corp.work.d2b"
        }))
        .unwrap();
        assert_eq!(config.target, "corp.work.d2b");
    }

    #[test]
    fn conflicting_target_and_vm_alias_fail_closed() {
        let err = decode(serde_json::json!({
            "name": "work",
            "target": "corp.work.d2b",
            "vm": "personal"
        }))
        .unwrap_err()
        .to_string();
        assert!(err.contains("target"));
        assert!(err.contains("compatibility alias"));
        assert!(err.contains("differ"));
        assert!(!err.contains("corp.work.d2b"));
    }

    #[test]
    fn missing_target_and_alias_is_actionable() {
        let err = decode(serde_json::json!({"name": "work"}))
            .unwrap_err()
            .to_string();
        assert!(err.contains("requires `target`"));
    }
}
// --- end weezterm remote features ---
