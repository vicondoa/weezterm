fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // --- weezterm remote features ---
    // The version comes from Cargo.toml (CARGO_PKG_VERSION).
    // For release builds, CI writes a .tag file with the exact version.
    // For dev builds, we append "-dev.YYYYMMDD.HASH" from git.
    let base_version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".to_string());

    let ci_tag = if let Ok(tag) = std::fs::read("../.tag") {
        // CI release build: .tag contains the version (e.g., "0.2.0")
        println!("cargo:rerun-if-changed=../.tag");
        String::from_utf8(tag)
            .map(|s| s.trim().to_string())
            .unwrap_or(base_version)
    } else {
        // Dev build: derive suffix from git
        let git_suffix = git_dev_suffix();
        if git_suffix.is_empty() {
            base_version
        } else {
            format!("{}-dev.{}", base_version, git_suffix)
        }
    };
    // --- end weezterm remote features ---

    let target = std::env::var("TARGET").unwrap_or_else(|_| "unknown".to_string());

    println!("cargo:rustc-env=WEZTERM_TARGET_TRIPLE={}", target);
    println!("cargo:rustc-env=WEZTERM_CI_TAG={}", ci_tag);
}

// --- weezterm remote features ---
/// Derive a dev build suffix from git: "YYYYMMDD.SHORTHASH"
fn git_dev_suffix() -> String {
    if let Ok(repo) = git2::Repository::discover(".") {
        // Set up cargo rerun-if-changed for HEAD
        if let Ok(ref_head) = repo.find_reference("HEAD") {
            let repo_path = repo.path().to_path_buf();
            if let Ok(resolved) = ref_head.resolve() {
                if let Some(name) = resolved.name() {
                    let path = repo_path.join(name);
                    if path.exists() {
                        println!(
                            "cargo:rerun-if-changed={}",
                            path.canonicalize().unwrap().display()
                        );
                    }
                }
            }
        }

        // Get date and short hash: "20240203.abc12345"
        if let Ok(output) = std::process::Command::new("git")
            .args(&[
                "-c",
                "core.abbrev=8",
                "show",
                "-s",
                "--format=%cd.%h",
                "--date=format:%Y%m%d",
            ])
            .output()
        {
            let info = String::from_utf8_lossy(&output.stdout);
            return info.trim().to_string();
        }
    }
    String::new()
}
// --- end weezterm remote features ---
