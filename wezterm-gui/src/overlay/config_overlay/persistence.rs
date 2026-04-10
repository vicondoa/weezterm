//! Persistence for config overlay proposals.
//!
//! Saves and loads proposed config values to/from a JSON file in the config
//! directory so they survive process restarts.
//!
//! --- weezterm remote features ---

use std::collections::HashMap;
use wezterm_dynamic::Value;

const OVERLAY_FILE_NAME: &str = "config-overlay.json";

/// Returns the path to the overlay proposals file.
fn overlay_file_path() -> Option<std::path::PathBuf> {
    config::CONFIG_DIRS
        .first()
        .map(|dir| dir.join(OVERLAY_FILE_NAME))
}

/// Load saved proposals from disk.
///
/// Returns an empty map if the file doesn't exist or can't be parsed.
pub fn load_proposals() -> anyhow::Result<HashMap<String, Value>> {
    let path = match overlay_file_path() {
        Some(p) => p,
        None => return Ok(HashMap::new()),
    };

    if !path.exists() {
        return Ok(HashMap::new());
    }

    let content = std::fs::read_to_string(&path)?;
    let json: serde_json::Value = serde_json::from_str(&content)?;

    let mut proposals = HashMap::new();
    if let serde_json::Value::Object(obj) = json {
        for (key, val) in obj {
            proposals.insert(key, json_to_dynamic(&val));
        }
    }

    Ok(proposals)
}

/// Save proposals to disk as JSON.
pub fn save_proposals(proposals: &HashMap<String, Value>) -> anyhow::Result<()> {
    let path = match overlay_file_path() {
        Some(p) => p,
        None => anyhow::bail!("No config directory found"),
    };

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut json_obj = serde_json::Map::new();
    for (key, val) in proposals {
        json_obj.insert(key.clone(), dynamic_to_json(val));
    }

    let json_str = serde_json::to_string_pretty(&serde_json::Value::Object(json_obj))?;
    std::fs::write(&path, json_str)?;

    log::info!("Config overlay proposals saved to {}", path.display());
    Ok(())
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
