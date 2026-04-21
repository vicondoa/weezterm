# Remote Extensions

## Overview

WeezTerm extends WezTerm with **VS Code Remote SSH-style features** that make
working on remote machines feel native. When you connect to a remote host via
`wezterm ssh`, WeezTerm can:

1. **Open URLs from the remote host in your local browser** — no X11 forwarding
   or manual copy-paste required.
2. **Automatically detect and forward remote ports to localhost** — so you can
   access development servers, dashboards, and OAuth callbacks without manually
   setting up SSH tunnels. Works with both direct SSH and multiplexed domains.
3. **Auto-install weezterm on the remote host** for multiplexing mode — no
   manual setup required on the server.
4. **Validate remote URLs with security policies** — allow-list and
   per-domain policy (Allow / Confirm / Deny) with dangerous schemes always
   blocked.
5. **Auto-reconnect dropped SSH sessions** — exponential backoff retry after
   connection loss (e.g., laptop suspend/resume), with port forwarding restart.
6. **Browse and edit configuration from a TUI overlay** — `Ctrl+Shift+,` opens
   a Ratatui-based editor for ~80 settings, SSH domains, DevContainer domains,
   and per-monitor overrides.
7. **Persist window state across restarts** — position, size, and
   maximized/fullscreen state restored on the correct monitor.
8. **Run shells inside Docker devcontainers** — auto-discover containers,
   manage them from a TUI overlay (`Ctrl+Shift+D`), and connect via
   `docker exec` with optional SSH tunneling.

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
| `port_forwarding.detect_with_proc_net_tcp` | string | `"OnlyNew"` | Detection mode for `/proc/net/tcp` polling (Linux only). `"None"`: disabled. `"All"`: forward all listening ports. `"OnlyNew"`: only ports opened after connection (default). |
| `port_forwarding.detect_with_terminal_scrape` | boolean | `true` | Enable detection of ports from URLs printed in the terminal. |
| `port_forwarding.poll_interval_secs` | number | `2` | How often (in seconds) to poll `/proc/net/tcp` for new listeners. |
| `port_forwarding.exclude_ports` | list of numbers | `[22]` | Ports to never auto-forward. |
| `port_forwarding.include_ports` | list of numbers | `[]` | Ports to always forward when detected on connect. |
| `port_forwarding.port_conflict_handling` | string | `"Skip"` | What to do when the preferred local port is already in use. `"Skip"`: don't forward, re-check periodically. `"RandomPort"`: forward on a random available local port. |

Example configuration (inside an `ssh_domains` entry):

```lua
config.ssh_domains = {
  {
    name = "my-server",
    remote_address = "my.server.com",
    port_forwarding = {
      enabled = true,
      auto_forward = true,
      detect_with_proc_net_tcp = "OnlyNew",  -- "None", "All", or "OnlyNew"
      detect_with_terminal_scrape = true,
      poll_interval_secs = 3,
      exclude_ports = { 22, 3306, 5432 },
      include_ports = {},
      port_conflict_handling = "Skip",  -- "Skip" or "RandomPort"
    },
  },
}
```

### Multiplexed Domain Support

Port forwarding works with both direct SSH and multiplexed SSH domains
(`multiplexing = "WezTerm"`). When using multiplexing mode, the SSH session
used for port forwarding persists through the mux proxy, so:

- Port forwards survive SSH reconnection
- The port manager overlay shows ports from the underlying SSH session
- All detection methods work the same as in direct SSH mode

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

## Open-URL Security

### How It Works

When a remote program opens a URL via `$BROWSER` (OSC 7457), WeezTerm validates
the URL against a security policy before opening it in your local browser. This
prevents malicious or unexpected URLs from being opened automatically.

The validation flow:

```
Remote program emits OSC 7457 with URL
            │
            ▼
   Scheme check: http(s) only?
      ├── No  → DENY (always blocked)
      └── Yes ▼
   Domain allow_list prefix match?
      ├── Yes → ALLOW (open immediately)
      └── No  ▼
   Global allow_list prefix match?
      ├── Yes → ALLOW (open immediately)
      └── No  ▼
   Apply default_policy (Allow / Confirm / Deny)
```

### Blocked Schemes

The following URL schemes are **always blocked**, regardless of policy:

- `file://` — local file access
- `javascript:` — script injection
- `data:` — inline content injection
- Any non-`http://` or `https://` scheme

### Confirmation Flow

When the policy is `Confirm` (the default), WeezTerm shows a toast notification
with the URL. You must click the notification to open the URL. The notification
auto-dismisses after `confirm_timeout_secs` (default: 15 seconds) if not acted upon.

### Configuration

The `open_url` block can be set per-domain (inside `ssh_domains`) or globally.
Per-domain settings take precedence, with the global allow-list used as fallback.

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `open_url.default_policy` | string | `"Confirm"` | Policy for URLs not on the allow-list. `"Allow"`: open immediately. `"Confirm"`: show toast, user clicks to open. `"Deny"`: silently block. |
| `open_url.allow_list` | list of strings | `["https://login.microsoftonline.com/", "https://login.live.com/"]` | URL prefixes that are auto-approved (opened without confirmation). Uses prefix matching. |
| `open_url.confirm_timeout_secs` | number | `15` | How long the confirmation toast stays visible (seconds). |

**Per-domain configuration** (inside an `ssh_domains` entry):

```lua
config.ssh_domains = {
  {
    name = "my-server",
    remote_address = "my.server.com",
    open_url = {
      default_policy = "Confirm",
      allow_list = {
        "https://login.microsoftonline.com/",
        "https://mycompany.okta.com/",
      },
      confirm_timeout_secs = 15,
    },
  },
}
```

**Global configuration** (applies to all domains without a per-domain override):

```lua
config.open_url = {
  default_policy = "Confirm",
  allow_list = {
    "https://login.microsoftonline.com/",
    "https://login.live.com/",
  },
  confirm_timeout_secs = 15,
}
```

---

## SSH Auto-Reconnect

### How It Works

When an SSH connection drops (e.g., laptop suspend/resume, network interruption),
WeezTerm automatically reconnects instead of closing your terminal windows. The
reconnection uses exponential backoff to avoid overwhelming the server.

On successful reconnect:
- Terminal sessions resume (when using multiplexing mode)
- Port forwarding restarts automatically
- No manual intervention is needed

### Configuration

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `auto_reconnect` | boolean | `true` | Whether to automatically reconnect when the SSH connection is lost. When `true`, the client retries with exponential backoff. When `false`, the connection is closed and terminal windows are destroyed. |

Example configuration (inside an `ssh_domains` entry):

```lua
config.ssh_domains = {
  {
    name = "my-server",
    remote_address = "my.server.com",
    auto_reconnect = true,  -- default: true
  },
}
```

---

## Config Overlay

### How It Works

Press **Ctrl+Shift+,** (comma) to open a built-in TUI overlay for browsing and
editing WeezTerm configuration. The overlay is built with Ratatui and provides a
visual interface for ~80 settings — no need to edit Lua config files.

### Sections

The overlay is organized into the following sections:

| Section | Description |
|---------|-------------|
| General | Window appearance, scrollback, bell, updates |
| Font & Text | Font family, size, line height, cell width, freetype settings |
| Tabs & Panes | Tab bar position, style, pane split defaults |
| Cursor & Animation | Cursor shape, blink rate, animation FPS |
| Terminal | TERM value, scroll-to-bottom behavior, unicode version |
| Input | Key handling, IME, dead keys, leader key |
| SSH & Domains | SSH domain management (add/edit/remove), DevContainer domains |
| Rendering | GPU frontend, WebGpu, color scheme selection |
| Monitors | Per-monitor overrides with layout diagram |

### Features

- **Enum picker popup** — Enum-valued settings show a popup with all variants
  and descriptions.
- **Color scheme picker** — Browse color schemes with live preview applied to
  the terminal.
- **SSH domain management** — Add new SSH domains, edit existing ones, or
  remove domains directly from the overlay.
- **DevContainer domain management** — Same add/edit/remove workflow for
  DevContainer domains.
- **Monitor overrides** — Expandable per-monitor groups with an ASCII layout
  diagram showing monitor arrangement. Override color scheme per monitor.
- **Field status badges** — Each field shows its source: `lua` (from Lua
  config), `editable` (from overlay), or `modified` (changed this session).
- **Mouse support** — Click to select items, scroll through lists.
- **Theme-aware styling** — The overlay adapts to the current terminal
  color scheme.

### Persistence

Changes made through the config overlay are saved to `config-overlay.json` in
the WeezTerm configuration directory (typically `~/.config/wezterm/` on Linux/macOS
or `%APPDATA%\wezterm\` on Windows). These settings are applied as window-level
overrides and do not modify your Lua configuration files.

The file stores:
- `proposals` — Individual setting overrides
- `ssh_domains` — SSH domain configurations added/edited via the overlay
- `devcontainer_domains` — DevContainer domain configurations
- `monitor_overrides` — Per-monitor color scheme overrides

---

## Window State Persistence

### How It Works

WeezTerm automatically saves window state (position, size, maximized/fullscreen
mode, and monitor) when a window is closed or the application exits. On the next
launch, windows are restored to their previous positions.

### Saved Properties

| Property | Description |
|----------|-------------|
| `x`, `y` | Window position in screen coordinates |
| `width`, `height` | Window dimensions in pixels |
| `maximized` | Whether the window was maximized |
| `fullscreen` | Whether the window was in fullscreen mode |
| `monitor` | The name of the monitor the window was on |

### Multi-Monitor Support

Windows are restored to the same monitor they were on when saved. If a monitor
is no longer connected, the window falls back to the primary monitor. State is
tracked per workspace, so different workspaces can restore to different positions.

### Persistence

Window state is stored in `window-state.json` in the WeezTerm configuration
directory. States with zero dimensions or extreme coordinates are ignored to
prevent restoring to invalid positions.

---

## DevContainer Domain Support

### How It Works

WeezTerm supports Docker devcontainers as a first-class domain type. You can
spawn terminal tabs inside running devcontainers using `docker exec` — no
weezterm installation inside the container is needed.

The flow:

```
WeezTerm ──▸ docker ps (discover devcontainers)
                │
                ▼
         Filter by devcontainer.local_folder label
                │
                ▼
         Select primary container (by config or auto-match)
                │
                ▼
         docker exec -it <container> <shell>
                │
                ▼
         Terminal tab connected to container
```

### Container Discovery

WeezTerm discovers devcontainers by running `docker ps` with JSON format and
filtering for containers that have `devcontainer.*` labels (set by the
VS Code Dev Containers extension and the `devcontainer` CLI). The following
labels are used:

| Label | Description |
|-------|-------------|
| `devcontainer.local_folder` | The local workspace folder the container was created from |
| `devcontainer.config_file` | Path to the `devcontainer.json` that defined the container |

Discovery runs on initial attach and then periodically (configurable via
`poll_interval_secs`).

### Primary Container Selection

When multiple devcontainers are running, WeezTerm selects a primary container
using this priority:

1. **`default_container`** — If set, matches by container name or ID
2. **`default_workspace_folder`** — Matches the `devcontainer.local_folder`
   label against this path
3. **Auto-select** — If exactly one running container matches, it is selected
   automatically

### SSH Mode

DevContainer domains support two modes:

| Mode | `ssh` field | Description |
|------|-------------|-------------|
| Local Docker | `nil` (not set) | Connects to Docker running on the local machine |
| Remote Docker via SSH | `SshDomain` config | Connects to Docker running on a remote host via SSH. Supports both direct SSH and multiplexing modes. All `SshDomain` options are available. |

### DevContainer Manager Overlay

Press **Ctrl+Shift+D** to open the DevContainer Manager overlay. It provides:

- **List** all discovered devcontainers with status (Running, Exited, Paused)
- **Connect** to a container (opens a new tab with a shell inside the container)
- **Set as primary** — make a container the default for new tabs
- **Start / Stop** containers
- **Delete** containers
- **Create** new devcontainers from a workspace folder

### Launcher Integration

DevContainer domains automatically appear in the tab bar chevron menu and the
launcher, so you can connect to containers from the same UI used for SSH domains
and local shells.

### Configuration

DevContainer domain options live under the `devcontainer_domains` top-level
config key:

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `name` | string | (required) | Unique domain name. |
| `ssh` | `SshDomain` | `nil` | SSH connection config for remote Docker. If absent, uses local Docker. All `SshDomain` options are available. |
| `default_workspace_folder` | string | `nil` | Local workspace folder. Auto-discovers the devcontainer whose `devcontainer.local_folder` label matches this path. |
| `default_container` | string | `nil` | Container name or ID to auto-connect to. Takes priority over workspace-folder matching. |
| `docker_command` | string | `"docker"` | Path to the Docker executable. |
| `devcontainer_command` | string | `"devcontainer"` | Path to the `devcontainer` CLI executable. |
| `default_shell` | string | `nil` | Shell to run inside the container (e.g., `"/bin/bash"`). If not set, uses the container's default. |
| `override_user` | string | `nil` | Override the container's default user for `docker exec`. |
| `poll_interval_secs` | number | `10` | How often (in seconds) to poll for container status changes. |
| `auto_discover` | boolean | `true` | Whether to auto-discover running devcontainers on attach. |

**Local Docker example:**

```lua
config.devcontainer_domains = {
  {
    name = "my-project",
    default_workspace_folder = "/home/user/my-project",
    docker_command = "docker",
    auto_discover = true,
    poll_interval_secs = 10,
  },
}
```

**Remote Docker via SSH example:**

```lua
config.devcontainer_domains = {
  {
    name = "remote-devcontainer",
    ssh = {
      name = "dev-host",
      remote_address = "dev.example.com",
      username = "developer",
      multiplexing = "WezTerm",
    },
    default_workspace_folder = "/home/developer/project",
    auto_discover = true,
  },
}
```

---

## Full Configuration Reference

Below is a consolidated reference of all remote extension configuration
options and their defaults.

### SSH Domain Options (`ssh_domains`)

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
      detect_with_proc_net_tcp = "OnlyNew",  -- "None", "All", or "OnlyNew"
      detect_with_terminal_scrape = true,
      poll_interval_secs = 2,
      exclude_ports = { 22 },
      include_ports = {},
      port_conflict_handling = "Skip",  -- "Skip" or "RandomPort"
    },

    -- Open-URL Security
    open_url = {
      default_policy = "Confirm",  -- "Allow", "Confirm", or "Deny"
      allow_list = {
        "https://login.microsoftonline.com/",
        "https://login.live.com/",
      },
      confirm_timeout_secs = 15,
    },

    -- SSH Auto-Reconnect
    auto_reconnect = true,

    -- Auto-Install for Multiplexing
    auto_install_mux = true,
    remote_install_dir = "~/.weezterm/bin",
    remote_install_url = "",  -- set to release URL for cross-arch installs
    remote_install_binaries_dir = nil,  -- or path to pre-built binaries for remote platform
  },
}
```

### Global Open-URL Security (`open_url`)

```lua
-- Applies to all SSH domains without a per-domain open_url override
config.open_url = {
  default_policy = "Confirm",
  allow_list = {
    "https://login.microsoftonline.com/",
    "https://login.live.com/",
  },
  confirm_timeout_secs = 15,
}
```

### DevContainer Domain Options (`devcontainer_domains`)

```lua
config.devcontainer_domains = {
  {
    name = "my-devcontainer",

    -- SSH connection (nil for local Docker)
    ssh = nil,  -- or a full SshDomain table for remote Docker

    -- Container selection
    default_workspace_folder = nil,  -- match by devcontainer.local_folder label
    default_container = nil,         -- match by container name or ID

    -- Docker settings
    docker_command = "docker",
    devcontainer_command = "devcontainer",
    default_shell = nil,      -- e.g., "/bin/bash"
    override_user = nil,      -- override container's default user

    -- Discovery
    auto_discover = true,
    poll_interval_secs = 10,
  },
}
```

### Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Ctrl+Shift+G` | Open Port Manager overlay |
| `Ctrl+Shift+,` | Open Config Overlay |
| `Ctrl+Shift+D` | Open DevContainer Manager overlay |

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
port will fail. The behavior depends on the `port_conflict_handling` setting:

- **`Skip`** (default) — The forward is skipped and the port is shown as
  inactive. WeezTerm re-checks periodically and auto-forwards when the local
  port is freed.
- **`RandomPort`** — The forward is created on a random available local port
  instead. The Port Manager Overlay shows the actual local port.

**Other solutions:**

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
