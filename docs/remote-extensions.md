# Remote Extensions

## Overview

Weezterm extends WezTerm with **VS Code Remote SSH-style features** that make
working on remote machines feel native. When you connect to a remote host via
`wezterm ssh`, Weezterm can:

1. **Open URLs from the remote host in your local browser** — no X11 forwarding
   or manual copy-paste required.
2. **Automatically detect and forward remote ports to localhost** — so you can
   access development servers, dashboards, and OAuth callbacks without manually
   setting up SSH tunnels.

These extensions are inspired by
[VS Code Remote - SSH](https://code.visualstudio.com/docs/remote/ssh) and aim
to bring the same quality-of-life improvements to a terminal-first workflow.

---

## Remote Browser Opening

### How It Works

When you connect via `wezterm ssh`, Weezterm sets the `$BROWSER` environment
variable on the remote host to a small helper that sends an
**OSC 7457 escape sequence** back through the terminal. The local Weezterm
instance intercepts this sequence and opens the URL in your default browser.

The flow looks like this:

```
Remote process ─▸ $BROWSER <url>
                     │
                     ▼
            Writes OSC 7457 escape sequence to stdout
                     │
                     ▼
            Terminal (Weezterm) intercepts the sequence
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
| `enable_remote_browser` | boolean | `true` | Set the `$BROWSER` env var on remote SSH sessions. |
| `remote_browser_open_osc` | number | `7457` | The OSC number used for the browser-open escape sequence. |

Example configuration:

```lua
return {
  enable_remote_browser = true,
}
```

---

## Automatic Port Forwarding

### How It Works

Weezterm monitors the remote host for listening TCP ports and automatically
creates SSH port forwards so those ports are accessible on `localhost`.

#### Detection Methods

1. **`/proc/net/tcp` polling** — On Linux remotes, Weezterm periodically reads
   `/proc/net/tcp` (and `/proc/net/tcp6`) to discover ports in the `LISTEN`
   state. This is lightweight and requires no extra tools.

2. **Terminal URL scraping** — Weezterm watches terminal output for patterns
   that look like local URLs (e.g., `http://localhost:3000`,
   `http://127.0.0.1:8080`). When a match is found, the referenced port is
   forwarded automatically.

Both methods run concurrently. A port discovered by either method is
auto-forwarded unless it conflicts with an existing forward or is excluded by
configuration.

### Auto-Forwarding Behavior

When a new listening port is detected:

1. Weezterm checks whether the port is already forwarded or excluded.
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

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `enable_auto_port_forward` | boolean | `true` | Enable automatic port forwarding for SSH sessions. |
| `port_forward_poll_interval_secs` | number | `2` | How often (in seconds) to poll `/proc/net/tcp` for new listeners. |
| `port_forward_exclude` | list of numbers | `[]` | Ports to never auto-forward (e.g., `[22, 3306]`). |
| `port_forward_include_range` | list of two numbers | `[1024, 65535]` | Only auto-forward ports within this range (inclusive). |
| `port_forward_url_scraping` | boolean | `true` | Enable detection of ports from URLs printed in the terminal. |
| `port_forward_notification` | boolean | `true` | Show a notification when a port is auto-forwarded. |

Example configuration:

```lua
return {
  enable_auto_port_forward = true,
  port_forward_poll_interval_secs = 3,
  port_forward_exclude = { 22, 3306, 5432 },
  port_forward_include_range = { 1024, 65535 },
  port_forward_url_scraping = true,
  port_forward_notification = true,
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

3. **What Weezterm does automatically:**

   - **Port forwarding:** The auto-port-forward feature detects port `8400`
     (via `/proc/net/tcp` or URL scraping) and forwards it to your local
     machine.
   - **Browser opening:** If `az login` invokes `$BROWSER`, Weezterm opens the
     login URL in your local browser — which can now reach
     `http://localhost:8400` thanks to the port forward.

4. **Complete the login flow in your local browser.** The OAuth callback hits
   `localhost:8400`, which is forwarded to the remote host, and `az login`
   completes successfully.

5. **Clean-up:** Once `az login` finishes and stops listening on port `8400`,
   Weezterm automatically removes the port forward.

No manual SSH tunnels. No copy-pasting URLs. It just works.

---

## Full Configuration Reference

Below is a consolidated reference of all remote extension configuration
options and their defaults:

```lua
return {
  -- Remote Browser Opening
  enable_remote_browser = true,
  remote_browser_open_osc = 7457,

  -- Automatic Port Forwarding
  enable_auto_port_forward = true,
  port_forward_poll_interval_secs = 2,
  port_forward_exclude = {},
  port_forward_include_range = { 1024, 65535 },
  port_forward_url_scraping = true,
  port_forward_notification = true,
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
port will fail. Weezterm will log a warning and skip the forward.

**Solutions:**

- Free the conflicting local port.
- Use the Port Manager Overlay to manually set up a forward with a different
  local port.

### Auto-forward security considerations

Automatic port forwarding exposes remote services on your local machine.
Keep the following in mind:

- **Restrict the port range** with `port_forward_include_range` to limit
  exposure to expected development ports.
- **Exclude sensitive ports** (e.g., databases) with `port_forward_exclude`.
- Forwarded ports bind to `localhost` only — they are not accessible from
  other machines on your network.
- If you are on a shared remote host, other users' services could be
  forwarded. Use `port_forward_exclude` or disable auto-forwarding entirely
  if this is a concern.
