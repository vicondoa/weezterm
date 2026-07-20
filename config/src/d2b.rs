use crate::config::validate_domain_name;
use d2b_client_toolkit::contracts::v2_identity::{RealmId, WorkloadId};
use d2b_client_toolkit::{ServiceKind, ServiceOwner, TargetInput};
use std::convert::TryFrom;
use wezterm_dynamic::{FromDynamic, ToDynamic};

#[derive(Debug, Clone, PartialEq, Eq, FromDynamic, ToDynamic)]
#[dynamic(try_from = "RawD2bDomainConfig", into = "SerializedD2bDomainConfig")]
pub struct D2bDomainConfig {
    /// Unique WeezTerm domain name reserved for this d2b workload.
    #[dynamic(validate = "validate_domain_name")]
    pub name: String,

    target: TargetInput,
}

impl D2bDomainConfig {
    pub fn target(&self) -> &TargetInput {
        &self.target
    }

    pub fn workload_ids(&self) -> (&RealmId, &WorkloadId) {
        match &self.target {
            TargetInput::Workload { realm, workload } => (realm, workload),
            _ => unreachable!("D2bDomainConfig accepts only workload targets"),
        }
    }

    pub fn shell_service_target(&self) -> TargetInput {
        let (realm, workload) = self.workload_ids();
        TargetInput::Service {
            owner: ServiceOwner::Workload {
                realm: realm.clone(),
                workload: workload.clone(),
            },
            service: ServiceKind::Shell,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, FromDynamic, ToDynamic)]
struct WorkloadTargetConfig {
    realm_id: String,
    workload_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, FromDynamic)]
struct RawD2bDomainConfig {
    #[dynamic(validate = "validate_domain_name")]
    name: String,
    target: WorkloadTargetConfig,
}

#[derive(ToDynamic)]
struct SerializedD2bDomainConfig {
    name: String,
    target: WorkloadTargetConfig,
}

impl TryFrom<RawD2bDomainConfig> for D2bDomainConfig {
    type Error = String;

    fn try_from(raw: RawD2bDomainConfig) -> Result<Self, Self::Error> {
        let realm = RealmId::parse(raw.target.realm_id)
            .map_err(|_| "d2b target has an invalid canonical realm id".to_string())?;
        let workload = WorkloadId::parse(raw.target.workload_id)
            .map_err(|_| "d2b target has an invalid canonical workload id".to_string())?;
        Ok(Self {
            name: raw.name,
            target: TargetInput::Workload { realm, workload },
        })
    }
}

impl From<&D2bDomainConfig> for SerializedD2bDomainConfig {
    fn from(config: &D2bDomainConfig) -> Self {
        let (realm, workload) = config.workload_ids();
        Self {
            name: config.name.clone(),
            target: WorkloadTargetConfig {
                realm_id: realm.as_str().to_string(),
                workload_id: workload.as_str().to_string(),
            },
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use wezterm_dynamic::{FromDynamic, ToDynamic, Value};

    const REALM_ID: &str = "aaaaaaaaaaaaaaaaaaaa";
    const WORKLOAD_ID: &str = "bbbbbbbbbbbbbbbbbbbq";

    fn decode(value: serde_json::Value) -> Result<D2bDomainConfig, wezterm_dynamic::Error> {
        D2bDomainConfig::from_dynamic(&crate::json_to_dynamic(&value), Default::default())
    }

    #[test]
    fn canonical_workload_target_deserializes() {
        let config = decode(serde_json::json!({
            "name": "work",
            "target": {
                "realm_id": REALM_ID,
                "workload_id": WORKLOAD_ID
            }
        }))
        .unwrap();

        let (realm, workload) = config.workload_ids();
        assert_eq!(realm.as_str(), REALM_ID);
        assert_eq!(workload.as_str(), WORKLOAD_ID);
        assert!(matches!(config.target(), TargetInput::Workload { .. }));
        assert!(matches!(
            config.shell_service_target(),
            TargetInput::Service {
                owner: ServiceOwner::Workload { .. },
                service: ServiceKind::Shell,
            }
        ));
    }

    #[test]
    fn canonical_workload_target_round_trips() {
        let config = decode(serde_json::json!({
            "name": "work",
            "target": {
                "realm_id": REALM_ID,
                "workload_id": WORKLOAD_ID
            }
        }))
        .unwrap();

        let Value::Object(encoded) = config.to_dynamic() else {
            panic!("d2b domain did not serialize as an object");
        };
        let Some(Value::Object(target)) = encoded.get_by_str("target") else {
            panic!("d2b target did not serialize as an object");
        };
        assert_eq!(
            target.get_by_str("realm_id"),
            Some(&Value::String(REALM_ID.to_string()))
        );
        assert_eq!(
            target.get_by_str("workload_id"),
            Some(&Value::String(WORKLOAD_ID.to_string()))
        );
    }

    #[test]
    fn legacy_string_target_is_rejected() {
        let err = decode(serde_json::json!({
            "name": "work",
            "target": "tools.host.d2b"
        }))
        .unwrap_err()
        .to_string();
        assert!(err.contains("target"));
    }

    #[test]
    fn invalid_canonical_ids_are_rejected_without_echoing_values() {
        let err = decode(serde_json::json!({
            "name": "work",
            "target": {
                "realm_id": "work",
                "workload_id": WORKLOAD_ID
            }
        }))
        .unwrap_err()
        .to_string();
        assert!(err.contains("canonical realm id"));
        assert!(!err.contains(WORKLOAD_ID));
    }
}
