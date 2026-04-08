# Remote Extensions

## Overview

WeezTerm extends WezTerm with **VS Code Remote SSH-style features** that make
working on remote machines feel native. When you connect to a remote host via
`wezterm ssh`, WeezTerm can:

1. **Open URLs from the remote host in your local browser** — no X11 forwarding
   or manual copy-paste required.
2. **Automatically detect and forward remote ports to localhost** — so you can
   access development servers, dashboards, and OAuth callbacks without manually
   setting up SSH tunnels.
3. **Auto-install weezterm on the remote host** for multiplexing mode — no
   manual setup required on the server.

These extensions are inspired by
[VS Code Remote - SSH](https://code.visualstudio.com/docs/remote/ssh) and aim
to bring the same quality-of-life improvements to a terminal-first workflow.

---

## Remote Browser Opening

### How It Works

When you connect via `wezterm ssh`, WeezTerm sets the `$BROWSER` environment
variable on the remote host to a small helper that sends an
**OSC 7457 escape sequence** back through the terminal. The local WeezTerm
instance intercepts this sequence and opens the URL in your default browser.

The flow looks like this:

```
Remote process ─▸ $BROWSER <url>
                     │
                     ▼
            Writes OSC 7457 escape sequence to stdout
                     │
                     ▼
            Terminal (WeezTerm) intercepts the sequence
                     │
                     ▼
            Opens <url> in local default browser
```

### The OSC 7457 Escape Sequence

The escape sequence format is:

```
ESC ] 7457 ; <url> ST
```

where `ESC ]` is the Operating System Command introducer and `ST` (`ESC \` or
`BEL`) is the String Terminator.

Any application running inside the terminal can emit this sequence directly to
request that the local machine open a URL. The `$BROWSER` helper is simply a
convenience wrapper.

### Shell Compatibility

The `$BROWSER` helper is a **POSIX-portable shell script** that works with:

- **bash**
- **zsh**
- **fish**
- **dash**
- Any other POSIX-compliant shell

Because it relies only on `printf` and standard file descriptors, it works on
virtually every Unix-like system without additional dependencies.

### Configuration

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `set_remote_browser` | boolean | `true` | Set the `$BROWSER` env var on remote SSH sessions to open URLs locally via OSC 7457. |

Example configuration (inside an `ssh_domains` entry):

```lua
config.ssh_domains = {
  {
    name = "my-server",
    remote_address = "my.server.com",
    set_remote_browser = true,  -- default: true
  },
}
```

---

## Automatic Port Forwarding

### How It Works

WeezTerm monitors the remote host for listening TCP ports and automatically
creates SSH port forwards so those ports are accessible on `localhost`.

#### Detection Methods

1. **`/proc/net/tcp` polling** — On Linux remotes, WeezTerm periodically reads
   `/proc/net/tcp` (and `/proc/net/tcp6`) to discover ports in the `LISTEN`
   state. This is lightweight and requires no extra tools.

2. **Terminal URL scraping** — WeezTerm watches terminal output for patterns
   that look like local URLs (e.g., `http://localhost:3000`,
   `http://127.0.0.1:8080`). When a match is found, the referenced port is
   forwarded automatically.

Both methods run concurrently. A port discovered by either method is
auto-forwarded unless it conflicts with an existing forward or is excluded by
configuration.

### Auto-Forwarding Behavior

When a new listening port is detected:

1. WeezTerm checks whether the port is already forwarded or excluded.
2. If eligible, an SSH port forward (`localhost:<port>` → `remote:<port>`) is
   created.
3. A transient notification informs you that the port has been forwarded.
4. When the remote process stops listening, the forward is removed.

### Port Manager Overlay

Press **Ctrl+Shift+G** to open the **Port Manager Overlay**, which shows:

- All currently forwarded ports
- The detection source (proc, URL scrape, or manual)
- Options to manually add or remove forwards
- Options to open a forwarded port in your browser

### Configuration

All port-forwarding options live under the `port_forwarding` table inside an
`ssh_domains` entry:

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `port_forwarding.enabled` | boolean | `true` | Master switch to enable/disable port forwarding. |
| `port_forwarding.auto_forward` | boolean | `true` | Whether to automatically forward newly detected ports. |
| `port_forwarding.detect_with_proc_net_tcp` | boolean | `true` | Enable detection via `/proc/net/tcp` polling (Linux only). |
| `port_forwarding.detect_with_terminal_scrape` | boolean | `true` | Enable detection of ports from URLs printed in the terminal. |
| `port_forwarding.poll_interval_secs` | number | `2` | How often (in seconds) to poll `/proc/net/tcp` for new listeners. |
| `port_forwarding.exclude_ports` | list of numbers | `[22]` | Ports to never auto-forward. |
| `port_forwarding.include_ports` | list of numbers | `[]` | Ports to always forward when detected on connect. |

Example configuration (inside an `ssh_domains` entry):

```lua
config.ssh_domains = {
  {
    name = "my-server",
    remote_address = "my.server.com",
    port_forwarding = {
      enabled = true,
      auto_forward = true,
      detect_with_proc_net_tcp = true,
      detect_with_terminal_scrape = true,
      poll_interval_secs = 3,
      exclude_ports = { 22, 3306, 5432 },
      include_ports = {},
    },
  },
}
```

---

## Use Case Walkthrough: `az login`

The Azure CLI's `az login` command is a great example of how remote browser
opening and automatic port forwarding work together.

### Step by Step

1. **Connect to your remote dev machine:**

   ```console
   $ wezterm ssh dev@my-azure-vm
   ```

2. **Run `az login` on the remote host:**

   ```console
   dev@my-azure-vm:~$ az login
   ```

   `az login` starts a temporary local HTTP server (e.g., on port `8400`) and
   prints a URL like:

   ```
   To sign in, use a web browser to open the page
   https://microsoft.com/devicelogin and enter the code XXXXXXXXX
   ```

   Or, in the default (interactive) mode, it attempts to open a browser
   pointing at `http://localhost:8400/...`.

3. **What WeezTerm does automatically:**

   - **Port forwarding:** The auto-port-forward feature detects port `8400`
     (via `/proc/net/tcp` or URL scraping) and forwards it to your local
     machine.
   - **Browser opening:** If `az login` invokes `$BROWSER`, WeezTerm opens the
     login URL in your local browser — which can now reach
     `http://localhost:8400` thanks to the port forward.

4. **Complete the login flow in your local browser.** The OAuth callback hits
   `localhost:8400`, which is forwarded to the remote host, and `az login`
   completes successfully.

5. **Clean-up:** Once `az login` finishes and stops listening on port `8400`,
   WeezTerm automatically removes the port forward.

No manual SSH tunnels. No copy-pasting URLs. It just works.

---

## Auto-Install for Multiplexing

### How It Works

When you connect to a remote host using SSH multiplexing mode (`SSHMUX:`),
Weezterm needs its `weezterm` CLI and `weezterm-mux-server` binaries on the
remote host. The auto-install feature handles this automatically:

1. **Version check** — On each connection, Weezterm reads a version marker
   file (`~/.weezterm/bin/.version`) on the remote host. This is fast and
   avoids running a binary.
2. **Same version** — If the remote version matches the local client, no
   action is needed (fast path).
3. **Version mismatch** — If versions differ, Weezterm prompts you before
   updating. This is important because the mux protocol is version-sensitive,
   and replacing binaries could affect other active sessions.
4. **Not installed** — If no version marker is found, Weezterm installs
   automatically without prompting.

### Installation Strategy

- **Same architecture** (e.g., Linux x86_64 → Linux x86_64): Weezterm
  copies its own local binaries to the remote host via SFTP. No internet
  access required on either side.
- **Cross-architecture** (e.g., macOS arm64 → Linux x86_64): Weezterm
  downloads the correct-architecture release tarball from a configured URL
  on the local machine, then uploads it to the remote via SFTP. The download
  is cached locally in `~/.weezterm/cache/` to avoid re-downloading.

Binaries are installed to `~/.weezterm/bin/` on the remote host (configurable
via `remote_install_dir`). No root access is required.

### Configuration

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `auto_install_mux` | boolean | `true` | Automatically install/update weezterm on the remote host when using multiplexing mode. |
| `remote_install_dir` | string | `~/.weezterm/bin` | Directory on the remote host to install weezterm binaries. |
| `remote_install_url` | string | `""` | URL template for downloading cross-arch release artifacts. Placeholders: `{version}`, `{os}`, `{arch}`. Required for cross-architecture installs (unless `remote_install_binaries_dir` is set). |
| `remote_install_binaries_dir` | string | `nil` | Local directory containing pre-built binaries for the remote platform. Overrides both same-arch detection and URL download. Useful for cross-platform dev (e.g., Windows host → Linux remote via WSL build). |

Example configuration (inside an `ssh_domains` entry):

```lua
config.ssh_domains = {
  {
    name = "my-server",
    remote_address = "my.server.com",
    auto_install_mux = true,  -- default: true
    remote_install_dir = "~/.weezterm/bin",  -- default
    remote_install_url = "https://github.com/user/weezterm/releases/download/v{version}/weezterm-mux-{os}-{arch}.tar.gz",
    -- Or point to locally-built Linux binaries for cross-platform dev:
    -- remote_install_binaries_dir = "//wsl$/Ubuntu/home/user/weezterm/target/release",
  },
}
```

### Troubleshooting

#### Cross-arch install fails with empty `remote_install_url`

If your local machine and remote host have different architectures (common:
macOS → Linux, Windows → Linux), you must either:
- Set `remote_install_binaries_dir` to a local directory containing pre-built
  binaries for the remote platform (e.g., built via WSL or cross-compilation), or
- Set `remote_install_url` pointing to your release artifacts. The URL template
  supports `{version}`, `{os}`, and `{arch}` placeholders.

#### Version mismatch prompts on every connection

This happens when the local and remote versions don't match. Either:
- Accept the update to sync versions, or
- Set `auto_install_mux = false` and manage remote installation manually.

#### SFTP upload fails

Some locked-down SSH servers disable SFTP. In this case, install weezterm
on the remote host manually and set `remote_wezterm_path` to point to it.

---

## Full Configuration Reference

Below is a consolidated reference of all remote extension configuration
options and their defaults (inside an `ssh_domains` entry):

```lua
config.ssh_domains = {
  {
    name = "my-server",
    remote_address = "my.server.com",

    -- Remote Browser Opening
    set_remote_browser = true,

    -- Automatic Port Forwarding
    port_forwarding = {
      enabled = true,
      auto_forward = true,
      detect_with_proc_net_tcp = true,
      detect_with_terminal_scrape = true,
      poll_interval_secs = 2,
      exclude_ports = { 22 },
      include_ports = {},
    },

    -- Auto-Install for Multiplexing
    auto_install_mux = true,
    remote_install_dir = "~/.weezterm/bin",
    remote_install_url = "",  -- set to release URL for cross-arch installs
    remote_install_binaries_dir = nil,  -- or path to pre-built binaries for remote platform
  },
}
```

---

## Troubleshooting

### Shell does not respect `$BROWSER`

Some programs (or custom shell configurations) ignore the `$BROWSER`
environment variable and try to launch a hard-coded browser binary instead.

**Workarounds:**

- Check whether the program has its own browser configuration option
  (e.g., `GIT_BROWSER`, `AZURE_CLI_BROWSER`).
- Create a symlink or wrapper script named after the expected binary
  (e.g., `xdg-open`) that emits the OSC 7457 sequence.

### `/proc/net/tcp` not available (non-Linux remotes)

The `/proc/net/tcp` detection method is Linux-specific. On macOS, BSD, or
other systems, this file does not exist.

**What still works:**

- Terminal URL scraping continues to detect ports from printed output.
- You can manually forward ports via the Port Manager Overlay
  (**Ctrl+Shift+G**).

**Possible workaround:**

- Use `ss -tlnp` or `netstat -tlnp` output piped through a helper that emits
  the appropriate escape sequences (advanced; community scripts may be
  available).

### Port conflicts

If a port is already in use on your local machine, the auto-forward for that
port will fail. WeezTerm will log a warning and skip the forward.

**Solutions:**

- Free the conflicting local port.
- Use the Port Manager Overlay to manually set up a forward with a different
  local port.

### Auto-forward security considerations

Automatic port forwarding exposes remote services on your local machine.
Keep the following in mind:

- **Exclude sensitive ports** (e.g., databases) with `exclude_ports`.
- Forwarded ports bind to `localhost` only — they are not accessible from
  other machines on your network.
- If you are on a shared remote host, other users' services could be
  forwarded. Use `exclude_ports` or set `enabled = false` in the
  `port_forwarding` config to disable auto-forwarding entirely if this is a
  concern.
