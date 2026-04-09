// --- weezterm remote features ---
//! Auto-install weezterm binaries on a remote host for mux mode.
//!
//! When connecting via SSH multiplexing (`SSHMUX:`), the remote host needs
//! `weezterm` (CLI) and `weezterm-mux-server`. This module detects whether
//! they are present/up-to-date and installs them if needed.
//!
//! Strategy:
//! 1. Check remote version marker via SSH exec
//! 2. If missing → install. If mismatch → prompt user, then install.
//! 3. Same-arch: SFTP local binaries. Cross-arch: download from release URL.
//! 4. SFTP upload to `~/.weezterm/bin/`, set permissions, write version marker.
// --- end weezterm remote features ---

use anyhow::{anyhow, bail, Context};
use config::SshDomain;
use flate2::read::GzDecoder;
use mux::connui::ConnectionUI;
use std::convert::TryFrom;
use std::io::Read;
use std::path::{Path, PathBuf};
use wezterm_ssh::Session;

/// Top-level orchestrator: ensure weezterm is installed on the remote.
///
/// Returns `Some(path)` with the remote weezterm binary path if auto-install
/// is enabled, or `None` if disabled (caller should fall back to default).
pub fn ensure_remote_weezterm(
    sess: &Session,
    ssh_dom: &SshDomain,
    ui: &ConnectionUI,
) -> anyhow::Result<Option<String>> {
    if !ssh_dom.auto_install_mux {
        return Ok(None);
    }

    let raw_install_dir = &ssh_dom.remote_install_dir;
    let local_version = wezterm_version::wezterm_version();

    // Resolve ~ to the remote $HOME up front so all commands use an absolute path.
    // This avoids issues with ~ inside single quotes (which prevents shell expansion).
    let install_dir = &resolve_remote_dir(sess, raw_install_dir)?;

    // Step 1: Check remote version
    ui.output_str("Checking remote weezterm installation...\n");
    let remote_version = check_remote_version(sess, install_dir)?;

    match &remote_version {
        Some(rv) if rv == local_version => {
            log::info!("Remote weezterm version matches local: {}", local_version);
            return Ok(Some(format!("{}/weezterm", install_dir)));
        }
        Some(rv) => {
            // Version mismatch — prompt user before overwriting
            let prompt = format!(
                "\n\u{26a0}\u{fe0f}  Remote weezterm version ({}) differs from local ({}).\n\
                 \n\
                 Updating is recommended to avoid compatibility issues.\n\
                 \u{26a0}\u{fe0f}  WARNING: Updating will restart the remote mux server,\n\
                 which will terminate all existing sessions on this host.\n\
                 \n\
                 Declining (N) is at your own risk — protocol mismatches\n\
                 between client and server may cause crashes or hangs.\n\
                 \n\
                 Update remote installation? [Y/n]: ",
                rv, local_version
            );
            let response = ui.input(&prompt)?;
            if response.trim().eq_ignore_ascii_case("n") {
                log::info!("User declined remote weezterm update");
                return Ok(Some(format!("{}/weezterm", install_dir)));
            }
        }
        None => {
            ui.output_str("No weezterm found on remote host. Installing...\n");
        }
    }

    // Step 2: Detect remote platform
    ui.output_str("Detecting remote platform...\n");
    let (remote_os, remote_arch) = detect_remote_platform(sess)?;
    log::info!("Remote platform: {}-{}", remote_os, remote_arch);

    // Step 3: Obtain binaries
    let binaries_dir = if let Some(ref dir) = ssh_dom.remote_install_binaries_dir {
        // Explicit local directory with pre-built binaries for the remote platform
        ui.output_str(&format!("Using pre-built binaries from: {}\n", dir));
        let p = PathBuf::from(dir);
        if !p.exists() {
            bail!("remote_install_binaries_dir '{}' does not exist", dir);
        }
        p
    } else if is_same_arch(&remote_os, &remote_arch) {
        ui.output_str("Same architecture — using local binaries.\n");
        local_binaries_dir()?
    } else {
        if ssh_dom.remote_install_url.is_empty() {
            bail!(
                "Cross-architecture install required ({}-{}) but \
                 remote_install_url is not configured.\n\
                 Set `remote_install_url` or `remote_install_binaries_dir` \
                 in your SSH domain config, \
                 or install weezterm on the remote host manually.",
                remote_os,
                remote_arch
            );
        }

        let url = ssh_dom
            .remote_install_url
            .replace("{version}", local_version)
            .replace("{os}", &remote_os)
            .replace("{arch}", &remote_arch);

        ui.output_str(&format!(
            "Downloading weezterm for {}-{}...\n",
            remote_os, remote_arch
        ));

        let cache_dir = local_cache_dir(local_version, &remote_os, &remote_arch)?;
        download_and_extract(&url, &cache_dir)?
    };

    // Step 4: Upload via SFTP
    ui.output_str("Uploading weezterm binaries to remote host...\n");

    sftp_upload_binaries(sess, &binaries_dir, install_dir, local_version, ui)?;

    // Step 5: Stop the old mux server so the new binary is used on next connect.
    // The `wezterm cli --prefer-mux proxy` command (run after this function)
    // will auto-start a fresh mux server from the newly installed binary.
    ui.output_str("Stopping old mux server (if running)...\n");
    let kill_cmd = format!(
        "pkill -f '{}/weezterm-mux-server' 2>/dev/null; true",
        install_dir
    );
    let _ = exec_remote(sess, &kill_cmd);

    ui.output_str("Remote weezterm installation complete.\n");
    Ok(Some(format!("{}/weezterm", install_dir)))
}

// ─── Version Checking ───────────────────────────────────────────────

/// Read the version marker file on the remote host.
fn check_remote_version(sess: &Session, install_dir: &str) -> anyhow::Result<Option<String>> {
    validate_path(install_dir)?;
    let cmd = format!("cat '{}/.version' 2>/dev/null", install_dir);
    let output = exec_remote(sess, &cmd)?;
    let trimmed = output.trim().to_string();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(trimmed))
    }
}

// ─── Platform Detection ─────────────────────────────────────────────

/// Detect the remote host's OS and architecture via `uname`.
fn detect_remote_platform(sess: &Session) -> anyhow::Result<(String, String)> {
    let output = exec_remote(sess, "uname -s && uname -m")?;
    let lines: Vec<&str> = output.trim().lines().collect();
    if lines.len() < 2 {
        bail!(
            "Unexpected uname output (expected 2 lines, got {}): {:?}",
            lines.len(),
            output
        );
    }
    Ok((normalize_os(lines[0]), normalize_arch(lines[1])))
}

fn normalize_os(os: &str) -> String {
    match os.trim().to_lowercase().as_str() {
        "linux" => "linux",
        "darwin" => "darwin",
        _ => os.trim(),
    }
    .to_string()
}

fn normalize_arch(arch: &str) -> String {
    match arch.trim() {
        "x86_64" | "amd64" => "x86_64",
        "aarch64" | "arm64" => "aarch64",
        other => other,
    }
    .to_string()
}

/// Check if the local and remote architectures match.
fn is_same_arch(remote_os: &str, remote_arch: &str) -> bool {
    let local_triple = wezterm_version::wezterm_target_triple();
    let parts: Vec<&str> = local_triple.split('-').collect();
    if parts.is_empty() {
        return false;
    }
    let local_arch = normalize_arch(parts[0]);
    let local_os = if local_triple.contains("linux") {
        "linux"
    } else if local_triple.contains("darwin") || local_triple.contains("apple") {
        "darwin"
    } else if local_triple.contains("windows") {
        "windows"
    } else {
        "unknown"
    };
    local_arch == remote_arch && local_os == remote_os
}

// ─── Local Binaries ─────────────────────────────────────────────────

/// Find the directory containing the local weezterm binaries.
fn local_binaries_dir() -> anyhow::Result<PathBuf> {
    let exe = std::env::current_exe().context("Failed to determine current executable path")?;
    let dir = exe
        .parent()
        .ok_or_else(|| anyhow!("Current executable has no parent directory"))?;

    // Verify the expected binaries exist
    let cli = dir.join("weezterm");
    let mux = dir.join("weezterm-mux-server");
    if !cli.exists() && !dir.join("wezterm").exists() {
        bail!("Cannot find weezterm CLI binary in {}", dir.display());
    }
    if !mux.exists() && !dir.join("wezterm-mux-server").exists() {
        bail!(
            "Cannot find weezterm-mux-server binary in {}",
            dir.display()
        );
    }
    Ok(dir.to_path_buf())
}

// ─── Cross-Arch Download ────────────────────────────────────────────

/// Determine the local cache directory for downloaded release artifacts.
fn local_cache_dir(version: &str, os: &str, arch: &str) -> anyhow::Result<PathBuf> {
    let home = dirs_next::home_dir().ok_or_else(|| anyhow!("Cannot determine home directory"))?;
    Ok(home
        .join(".weezterm")
        .join("cache")
        .join(version)
        .join(format!("{}-{}", os, arch)))
}

/// Download a release tarball and extract the mux binaries.
///
/// Returns the path to the directory containing the extracted binaries.
/// Results are cached in `cache_dir` to avoid re-downloading.
fn download_and_extract(url: &str, cache_dir: &Path) -> anyhow::Result<PathBuf> {
    // Check cache first
    if has_cached_binaries(cache_dir) {
        log::info!("Using cached binaries from {}", cache_dir.display());
        return Ok(cache_dir.to_path_buf());
    }

    std::fs::create_dir_all(cache_dir)
        .with_context(|| format!("Failed to create cache dir: {}", cache_dir.display()))?;

    // Download the tarball
    log::info!("Downloading release from {}", url);
    let uri = http_req::uri::Uri::try_from(url)
        .map_err(|e| anyhow!("Invalid download URL '{}': {}", url, e))?;

    let mut body = Vec::new();
    let response = http_req::request::Request::new(&uri)
        .header("User-Agent", "weezterm-auto-install")
        .send(&mut body)
        .map_err(|e| anyhow!("Failed to download {}: {}", url, e))?;

    if !response.status_code().is_success() {
        bail!(
            "Download failed: HTTP {} for {}",
            response.status_code(),
            url
        );
    }

    // Extract the tarball
    log::info!(
        "Downloaded {} bytes, extracting to {}",
        body.len(),
        cache_dir.display()
    );
    let decoder = GzDecoder::new(body.as_slice());
    let mut archive = tar::Archive::new(decoder);

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();

        // Only extract the binaries we need
        if file_name == "weezterm"
            || file_name == "weezterm-mux-server"
            || file_name == "wezterm"
            || file_name == "wezterm-mux-server"
        {
            let dest_name = if file_name.starts_with("wezterm") && !file_name.starts_with("weez") {
                // Rename wezterm → weezterm for consistency
                file_name.replacen("wezterm", "weezterm", 1)
            } else {
                file_name.to_string()
            };
            let dest = cache_dir.join(&dest_name);
            entry.unpack(&dest)?;

            // Make executable
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755))?;
            }
        }
    }

    if !has_cached_binaries(cache_dir) {
        bail!(
            "Downloaded archive did not contain expected binaries \
             (weezterm, weezterm-mux-server)"
        );
    }

    Ok(cache_dir.to_path_buf())
}

/// Check if the cache directory contains the expected binaries.
fn has_cached_binaries(dir: &Path) -> bool {
    dir.join("weezterm").exists() && dir.join("weezterm-mux-server").exists()
}

// ─── SFTP Upload ────────────────────────────────────────────────────

/// Upload binaries to the remote host via SFTP and write a version marker.
fn sftp_upload_binaries(
    sess: &Session,
    local_dir: &Path,
    remote_dir: &str,
    version: &str,
    ui: &ConnectionUI,
) -> anyhow::Result<()> {
    // Create remote directory via exec (SFTP create_dir doesn't do mkdir -p)
    ensure_remote_dir(sess, remote_dir)?;

    let sftp = sess.sftp();

    // Upload each binary
    for binary_name in &["weezterm", "weezterm-mux-server"] {
        let local_path = find_local_binary(local_dir, binary_name)?;
        let remote_path = format!("{}/{}", remote_dir, binary_name);
        // Upload to a temp name first, then atomically rename.
        // On Linux, you can't overwrite a running binary (ETXTBSY),
        // but rename replaces the directory entry while the running
        // process keeps its file descriptor to the old inode.
        let tmp_path = format!("{}.new", remote_path);
        let data = std::fs::read(&local_path)
            .with_context(|| format!("Failed to read {}", local_path.display()))?;

        let size_mb = data.len() as f64 / (1024.0 * 1024.0);
        ui.output_str(&format!(
            "  Uploading {} ({:.1} MB)...\n",
            binary_name, size_mb
        ));

        sftp_upload_file(&sftp, &data, &tmp_path, 0o755, ui)
            .with_context(|| format!("Failed to upload {}", remote_path))?;

        // Atomic rename: works even if the target binary is running
        exec_remote(sess, &format!("mv -f '{}' '{}'", tmp_path, remote_path))
            .with_context(|| format!("Failed to rename {} into place", remote_path))?;
    }

    // Write version marker
    let version_path = format!("{}/.version", remote_dir);
    let version_tmp = format!("{}/.version.new", remote_dir);
    sftp_upload_file(&sftp, version.as_bytes(), &version_tmp, 0o644, ui)
        .with_context(|| "Failed to write version marker")?;
    exec_remote(sess, &format!("mv -f '{}' '{}'", version_tmp, version_path))
        .with_context(|| "Failed to rename version marker into place")?;

    Ok(())
}

/// Upload data to a remote file via SFTP, writing in chunks.
///
/// SFTP writes go through the SSH session's event loop which properly
/// interleaves keepalives, avoiding the timeout/reset issues that
/// plague exec+stdin based uploads.
fn sftp_upload_file(
    sftp: &wezterm_ssh::Sftp,
    data: &[u8],
    remote_path: &str,
    mode: i32,
    ui: &ConnectionUI,
) -> anyhow::Result<()> {
    use smol::io::AsyncWriteExt;
    use wezterm_ssh::{OpenFileType, OpenOptions, WriteMode};

    let mut file = smol::block_on(sftp.open_with_mode(
        remote_path,
        OpenOptions {
            read: false,
            write: Some(WriteMode::Write),
            mode,
            ty: OpenFileType::File,
        },
    ))
    .with_context(|| format!("SFTP open {} for writing", remote_path))?;

    // Write in 256KB chunks — each chunk goes through the SSH session's
    // event loop as a separate request, keeping the connection alive.
    let chunk_size = 256 * 1024;
    let total = data.len();
    let mut written = 0usize;
    let mut last_pct = 0u8;

    for chunk in data.chunks(chunk_size) {
        smol::block_on(AsyncWriteExt::write_all(&mut file, chunk))
            .with_context(|| format!("SFTP write to {}", remote_path))?;
        written += chunk.len();

        // Show progress every ~10%
        let pct = ((written as f64 / total.max(1) as f64) * 100.0) as u8;
        if pct / 10 > last_pct / 10 && total > chunk_size {
            ui.output_str(&format!("    {}%...\n", pct));
            last_pct = pct;
        }
    }

    smol::block_on(AsyncWriteExt::flush(&mut file))
        .with_context(|| format!("SFTP flush {}", remote_path))?;
    // Use async close to properly wait for the server-side close to complete.
    // Then forget the File to prevent Drop from sending a second Close request
    // which would corrupt the SFTP subsystem state for subsequent operations.
    smol::block_on(AsyncWriteExt::close(&mut file))
        .with_context(|| format!("SFTP close {}", remote_path))?;
    std::mem::forget(file);

    Ok(())
}

/// Find a local binary by name, trying both `weezterm-*` and `wezterm-*` names.
fn find_local_binary(dir: &Path, name: &str) -> anyhow::Result<PathBuf> {
    // Try the weezterm name first
    let path = dir.join(name);
    if path.exists() {
        return Ok(path);
    }
    // Try the upstream wezterm name (compat symlinks created by deploy.sh)
    let compat_name = name.replacen("weezterm", "wezterm", 1);
    let compat_path = dir.join(&compat_name);
    if compat_path.exists() {
        return Ok(compat_path);
    }
    bail!(
        "Binary '{}' not found in {} (tried both '{}' and '{}')",
        name,
        dir.display(),
        name,
        compat_name
    );
}

// ─── Remote Helpers ─────────────────────────────────────────────────

/// Execute a command on the remote host and return its stdout.
fn exec_remote(sess: &Session, cmd: &str) -> anyhow::Result<String> {
    let exec = smol::block_on(sess.exec(cmd, None))
        .with_context(|| format!("Failed to execute remote command: {}", cmd))?;
    let mut stdout = exec.stdout;
    let mut output = String::new();
    stdout
        .read_to_string(&mut output)
        .with_context(|| format!("Failed to read output of: {}", cmd))?;
    Ok(output)
}

/// Resolve a remote directory path, expanding `~` to the remote $HOME.
fn resolve_remote_dir(sess: &Session, dir: &str) -> anyhow::Result<String> {
    validate_path(dir)?;
    if dir.starts_with("~/") || dir == "~" {
        let home = exec_remote(sess, "echo $HOME")?;
        let home = home.trim();
        if home.is_empty() {
            bail!("Could not determine remote $HOME");
        }
        Ok(dir.replacen('~', home, 1))
    } else {
        Ok(dir.to_string())
    }
}

/// Ensure a remote directory exists, creating it and parents if necessary.
/// Uses SSH exec with mkdir -p and waits for the child process to fully
/// exit before returning, so the SFTP subsystem sees the directory.
fn ensure_remote_dir(sess: &Session, dir: &str) -> anyhow::Result<()> {
    validate_path(dir)?;
    // Single command: create dir, verify it exists, echo a sentinel.
    // We read stdout for the sentinel to be sure the command completed.
    let cmd = format!("mkdir -p '{}' && test -d '{}' && echo MKDIR_OK", dir, dir);
    let exec = smol::block_on(sess.exec(&cmd, None))
        .with_context(|| format!("Failed to exec mkdir for {}", dir))?;

    let mut stdout = exec.stdout;
    let mut output = String::new();
    stdout
        .read_to_string(&mut output)
        .with_context(|| format!("Failed to read mkdir output for {}", dir))?;

    // Wait for the child process to fully exit
    let mut child = exec.child;
    let _ = smol::block_on(child.async_wait());

    if !output.trim().contains("MKDIR_OK") {
        bail!(
            "Failed to create remote directory '{}': output was '{}'",
            dir,
            output.trim()
        );
    }
    log::info!("Remote directory ensured: {}", dir);
    Ok(())
}

/// Reject paths that contain shell-unsafe characters.
fn validate_path(path: &str) -> anyhow::Result<()> {
    if path.contains('\'') || path.contains('\0') || path.contains('`') || path.contains('$') {
        bail!(
            "Path '{}' contains unsafe characters (single quotes, \
             null bytes, backticks, or dollar signs are not allowed)",
            path
        );
    }
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use std::fs;

    #[test]
    fn test_normalize_os() {
        assert_eq!(normalize_os("Linux"), "linux");
        assert_eq!(normalize_os("linux"), "linux");
        assert_eq!(normalize_os("Darwin"), "darwin");
        assert_eq!(normalize_os("DARWIN"), "darwin");
        assert_eq!(normalize_os("  Linux  "), "linux");
        // Unknown OS passed through as-is (trimmed)
        assert_eq!(normalize_os("FreeBSD"), "FreeBSD");
    }

    #[test]
    fn test_normalize_arch() {
        assert_eq!(normalize_arch("x86_64"), "x86_64");
        assert_eq!(normalize_arch("amd64"), "x86_64");
        assert_eq!(normalize_arch("aarch64"), "aarch64");
        assert_eq!(normalize_arch("arm64"), "aarch64");
        assert_eq!(normalize_arch("  x86_64  "), "x86_64");
        // Unknown arch passed through as-is
        assert_eq!(normalize_arch("riscv64"), "riscv64");
    }

    #[test]
    fn test_validate_path_safe() {
        assert!(validate_path("/home/user/.weezterm/bin").is_ok());
        assert!(validate_path("/tmp/test-dir").is_ok());
        assert!(validate_path("relative/path").is_ok());
    }

    #[test]
    fn test_validate_path_rejects_single_quotes() {
        assert!(validate_path("/home/user'; rm -rf /").is_err());
    }

    #[test]
    fn test_validate_path_rejects_backticks() {
        assert!(validate_path("/home/`whoami`/bin").is_err());
    }

    #[test]
    fn test_validate_path_rejects_dollar() {
        assert!(validate_path("/home/$USER/bin").is_err());
    }

    #[test]
    fn test_validate_path_rejects_null() {
        assert!(validate_path("/home/user\0/bin").is_err());
    }

    #[test]
    fn test_find_local_binary_weezterm_name() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("weezterm"), b"fake").unwrap();
        let result = find_local_binary(dir.path(), "weezterm");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), dir.path().join("weezterm"));
    }

    #[test]
    fn test_find_local_binary_compat_name() {
        let dir = tempfile::tempdir().unwrap();
        // Only the upstream name exists
        fs::write(dir.path().join("wezterm"), b"fake").unwrap();
        let result = find_local_binary(dir.path(), "weezterm");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), dir.path().join("wezterm"));
    }

    #[test]
    fn test_find_local_binary_mux_server() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("weezterm-mux-server"), b"fake").unwrap();
        let result = find_local_binary(dir.path(), "weezterm-mux-server");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), dir.path().join("weezterm-mux-server"));
    }

    #[test]
    fn test_find_local_binary_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let result = find_local_binary(dir.path(), "weezterm");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("not found"), "error: {}", msg);
    }

    #[test]
    fn test_has_cached_binaries_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!has_cached_binaries(dir.path()));
    }

    #[test]
    fn test_has_cached_binaries_partial() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("weezterm"), b"fake").unwrap();
        assert!(!has_cached_binaries(dir.path()));
    }

    #[test]
    fn test_has_cached_binaries_complete() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("weezterm"), b"fake").unwrap();
        fs::write(dir.path().join("weezterm-mux-server"), b"fake").unwrap();
        assert!(has_cached_binaries(dir.path()));
    }

    #[test]
    fn test_is_same_arch_windows_vs_linux() {
        // On any platform, windows != linux
        let local_triple = wezterm_version::wezterm_target_triple();
        if local_triple.contains("windows") {
            assert!(!is_same_arch("linux", "x86_64"));
        } else if local_triple.contains("linux") {
            assert!(!is_same_arch("windows", "x86_64"));
        }
    }

    #[test]
    fn test_is_same_arch_matching() {
        let local_triple = wezterm_version::wezterm_target_triple();
        let parts: Vec<&str> = local_triple.split('-').collect();
        let local_arch = normalize_arch(parts[0]);
        let local_os = if local_triple.contains("linux") {
            "linux"
        } else if local_triple.contains("darwin") || local_triple.contains("apple") {
            "darwin"
        } else if local_triple.contains("windows") {
            "windows"
        } else {
            "unknown"
        };
        assert!(is_same_arch(local_os, &local_arch));
    }
}
