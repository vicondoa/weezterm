use crate::config::validate_domain_name;
use std::path::PathBuf;
use wezterm_dynamic::{FromDynamic, ToDynamic};

// --- weezterm remote features ---
#[derive(Debug, Clone, PartialEq, Eq, FromDynamic, ToDynamic)]
pub struct D2bDomainConfig {
    /// Unique WeezTerm domain name for this d2b VM.
    #[dynamic(validate = "validate_domain_name")]
    pub name: String,

    /// d2b VM name served by the native provider.
    #[dynamic(validate = "validate_d2b_vm_name")]
    pub vm: String,

    /// Optional override for the d2b public daemon socket.
    #[dynamic(default)]
    pub socket_path: Option<PathBuf>,
}

pub(crate) fn validate_d2b_vm_name(name: &str) -> Result<(), String> {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return Err("d2b VM names must start with [a-z]".to_string()),
    }

    if !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
        return Err("d2b VM names may contain only [a-z0-9-]".to_string());
    }

    if name.starts_with("sys-") || name == "launcher" {
        return Err("d2b VM name is reserved by the framework".to_string());
    }

    Ok(())
}
// --- end weezterm remote features ---
