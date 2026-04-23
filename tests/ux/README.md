# WeezTerm UX Tests

Automated black-box UX tests for WeezTerm window management on Windows.
Tests launch the real `weezterm-gui.exe` binary, manipulate windows via
Win32 API, capture screenshots, and assert on behavior.

## Process Isolation

Tests are fully isolated from any running WeezTerm instances:

- **`--config-file <temp>`** prevents the test instance from connecting to
  or publishing mux sockets for other instances
- **`XDG_CONFIG_HOME=<temp>`** isolates config dirs and `window-state.json`
- **`XDG_RUNTIME_DIR=<temp>`** isolates sockets, pid files, and logs
- All `WEEZTERM_*`/`WEZTERM_*` env vars are stripped from the test process
- Temp dirs are auto-cleaned via pytest fixtures, even on test failure

## Prerequisites

```bash
pip install -r requirements.txt
```

You also need a built `weezterm-gui.exe`. Either:
- Build: `cargo build -p wezterm-gui`
- Or set `WEEZTERM_BINARY` env var to the path of the binary

## Running

```bash
cd tests/ux

# Run all tests
python -m pytest -v -s

# Run specific test file
python -m pytest test_resize.py -v -s

# Run specific test
python -m pytest test_maximize.py::TestMaximize::test_unmaximize_restores_original_size -v -s

# Run by marker
python -m pytest -m startup -v -s
python -m pytest -m resize -v -s
python -m pytest -m maximize -v -s
python -m pytest -m dimensions -v -s
```

## Test Suites

| Suite | File | Tests | What it checks |
|-------|------|-------|----------------|
| **Startup** | `test_startup.py` | 4 | Startup time, initial rendering |
| **Resize** | `test_resize.py` | 6 | Shrink/grow, rapid resize, extreme sizes |
| **Maximize** | `test_maximize.py` | 6 | Maximize/restore cycles, size preservation |
| **Dimensions** | `test_dimensions.py` | 5 | Position/size/state persistence across restarts |

## Screenshot Artifacts

Failed tests save screenshots to `test-results/`. These are useful for
debugging rendering artifacts and are uploaded as CI artifacts on failure.

## Adding New Tests

1. Use the `running_app` fixture for tests that need a running WeezTerm window
2. Use the `app` fixture for tests that need to control startup/shutdown
3. Use `detect_rendering_artifacts()` from `helpers/screenshot.py` for artifact detection
4. Always call `settle()` after window operations to allow redraws
