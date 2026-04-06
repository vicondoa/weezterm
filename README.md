# Weezterm

<p align="center">
  <img height="256" alt="WeezTerm Mascot" src="assets/icon/weezterm/mascot.png">
</p>

<p align="center">
  <em>A fork of <a href="https://github.com/wezterm/wezterm">WezTerm</a> — the GPU-accelerated cross-platform terminal emulator and multiplexer written by <a href="https://github.com/wez">@wez</a> in <a href="https://www.rust-lang.org/">Rust</a> — with integrated remote SSH extensions.</em>
</p>

Weezterm extends WezTerm with VS Code Remote SSH-style features:
- **Remote browser opening** — Programs on the remote host can open URLs in your local browser (e.g., `az login` interactive auth)
- **Automatic port forwarding** — Ports opened on the remote host are detected and forwarded to localhost, enabling OAuth callback flows and dev server access

## Credits

Weezterm is built on top of **WezTerm** by [@wez](https://github.com/wez) (Wez Furlong).
All credit for the terminal emulator, multiplexer, GPU rendering, and the vast majority
of the codebase goes to the WezTerm project and its contributors.

- **Upstream**: [github.com/wezterm/wezterm](https://github.com/wezterm/wezterm)
- **Upstream docs**: [wezterm.org](https://wezterm.org/)
- **License**: Same as WezTerm (see [LICENSE.md](LICENSE.md))

The remote extensions added by this fork are inspired by
[VS Code Remote SSH](https://code.visualstudio.com/docs/remote/ssh).

## Remote Extensions

### Remote Browser Opening (`$BROWSER`)

When connected to a remote host via SSH, Weezterm sets the `$BROWSER` environment
variable to a helper that opens URLs on your **local** machine. This enables
interactive browser-based authentication flows (like `az login`, `gcloud auth login`,
etc.) to work seamlessly over SSH.

**How it works:**
1. Weezterm injects `$BROWSER` when spawning remote shells
2. When a program calls `$BROWSER <url>`, the helper sends an escape sequence through the terminal
3. Weezterm detects the sequence and opens the URL in your local browser

**Configuration:**
```lua
config.ssh_domains = {
  {
    name = "my-server",
    remote_address = "my.server.com",
    set_remote_browser = true,  -- default: true
  },
}
```

### Automatic Port Forwarding

Weezterm detects ports opened on the remote host and automatically forwards them to
localhost. This is essential for OAuth callback flows where the auth server redirects
to `http://localhost:PORT`.

**Detection methods:**
- Polling `/proc/net/tcp` on the remote host (Linux)
- Scanning terminal output for `localhost:PORT` URLs

**Port management:**
- Press `Ctrl+Shift+G` to open the port forwarding overlay
- Auto-forwarded ports show a toast notification
- Exclude ports or disable auto-forwarding in configuration

**Configuration:**
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
      poll_interval_secs = 2,
      exclude_ports = { 22, 80, 443 },
    },
  },
}
```

## Installation

Same as WezTerm: see [wezterm.org/installation](https://wezterm.org/installation).
Build from this fork's source for the remote extensions.

## Getting help

- [WezTerm documentation](https://wezterm.org/) — for all core terminal features
- [GitHub Issues](../../issues) — for Weezterm-specific remote extension issues
