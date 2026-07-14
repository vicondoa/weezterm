#!/usr/bin/env bash
set -Eeuo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
REPO_ROOT="${WEEZTERM_TITLE_REPO_ROOT:-$(cd -- "$SCRIPT_DIR/../.." && pwd -P)}"
HARNESS_WORK_DIR="$REPO_ROOT/tests/wayland-title"
NIRI_CONFIG="${WEEZTERM_TITLE_NIRI_CONFIG:-$SCRIPT_DIR/niri.kdl}"

SMOKE=0
ARTIFACT_DIR=""
WEZTERM_BIN="$REPO_ROOT/target/debug/weezterm"
WEZTERM_GUI_BIN="$REPO_ROOT/target/debug/weezterm-gui"
TARGET=""
DOMAIN="title-test"
D2B_SOCKET="/run/d2b/public.sock"
SHELL_ONE=""
SHELL_TWO=""
SECOND_LOCAL=0
READY_TIMEOUT=20
TITLE_TIMEOUT=15

usage() {
  cat <<'EOF'
Usage:
  run.sh --smoke [options]
  run.sh --target TARGET [options]

Options:
  --artifact-dir DIR       Result directory (default: results/run-TIMESTAMP-PID)
  --weezterm-bin PATH      Source-built weezterm CLI
  --weezterm-gui-bin PATH  Source-built weezterm-gui
  --target TARGET          d2b target (required outside --smoke)
  --domain NAME            Generated d2b domain name (default: title-test)
  --d2b-socket PATH        d2b public socket (default: /run/d2b/public.sock)
  --shell-one NAME         Disposable first shell session name
  --shell-two NAME         Disposable second shell session name
  --second-local           Use a local second tab for single-attachment targets
  --ready-timeout SECONDS  Process/socket readiness timeout (default: 20)
  --title-timeout SECONDS  Per-title screenshot/OCR timeout (default: 15)
  --smoke                  Validate isolated Weston, Niri, and grim only
  -h, --help               Show this help
EOF
}

die() {
  printf 'wayland-title: error: %s\n' "$*" >&2
  if [[ -n "$ARTIFACT_DIR" ]]; then
    printf 'wayland-title: logs and partial artifacts: %s\n' "$ARTIFACT_DIR" >&2
  fi
  exit 1
}

while (($#)); do
  case "$1" in
    --artifact-dir)
      (($# >= 2)) || die "--artifact-dir requires a value"
      ARTIFACT_DIR=$2
      shift 2
      ;;
    --weezterm-bin|--wezterm-bin)
      (($# >= 2)) || die "--weezterm-bin requires a value"
      WEZTERM_BIN=$2
      shift 2
      ;;
    --weezterm-gui-bin|--wezterm-gui-bin)
      (($# >= 2)) || die "--weezterm-gui-bin requires a value"
      WEZTERM_GUI_BIN=$2
      shift 2
      ;;
    --target)
      (($# >= 2)) || die "--target requires a value"
      TARGET=$2
      shift 2
      ;;
    --domain)
      (($# >= 2)) || die "--domain requires a value"
      DOMAIN=$2
      shift 2
      ;;
    --d2b-socket)
      (($# >= 2)) || die "--d2b-socket requires a value"
      D2B_SOCKET=$2
      shift 2
      ;;
    --shell-one)
      (($# >= 2)) || die "--shell-one requires a value"
      SHELL_ONE=$2
      shift 2
      ;;
    --shell-two)
      (($# >= 2)) || die "--shell-two requires a value"
      SHELL_TWO=$2
      shift 2
      ;;
    --second-local)
      SECOND_LOCAL=1
      shift
      ;;
    --ready-timeout)
      (($# >= 2)) || die "--ready-timeout requires a value"
      READY_TIMEOUT=$2
      shift 2
      ;;
    --title-timeout)
      (($# >= 2)) || die "--title-timeout requires a value"
      TITLE_TIMEOUT=$2
      shift 2
      ;;
    --smoke)
      SMOKE=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      die "unknown option: $1"
      ;;
  esac
done

[[ "$READY_TIMEOUT" =~ ^[1-9][0-9]*$ ]] || die "--ready-timeout must be a positive integer"
[[ "$TITLE_TIMEOUT" =~ ^[1-9][0-9]*$ ]] || die "--title-timeout must be a positive integer"

if [[ -z "$ARTIFACT_DIR" ]]; then
  ARTIFACT_DIR="$HARNESS_WORK_DIR/results/run-$(date +%Y%m%d-%H%M%S)-$$"
fi
mkdir -p -- "$ARTIFACT_DIR"
ARTIFACT_DIR="$(cd -- "$ARTIFACT_DIR" && pwd -P)"
umask 077

require_command() {
  command -v "$1" >/dev/null 2>&1 || die "missing required command '$1'; use nix run .#niri-title-test"
}

for command in weston niri grim magick identify find stat; do
  require_command "$command"
done
if ((SMOKE == 0)); then
  for command in jq tesseract; do
    require_command "$command"
  done
fi

[[ -f "$NIRI_CONFIG" ]] || die "missing Niri fixture: $NIRI_CONFIG"
niri validate -c "$NIRI_CONFIG" >"$ARTIFACT_DIR/niri-validate.log" 2>&1 ||
  die "Niri rejected $NIRI_CONFIG; see niri-validate.log"

validate_shell_name() {
  local name=$1
  ((${#name} >= 1 && ${#name} <= 64)) &&
    [[ "$name" =~ ^[A-Za-z0-9_][A-Za-z0-9._-]*$ ]]
}

if ((SMOKE == 0)); then
  [[ -n "$TARGET" ]] || die "--target is required outside --smoke"
  [[ "$TARGET" =~ ^[a-z][a-z0-9.-]*$ ]] || die "target must use lowercase [a-z0-9.-] labels"
  [[ "$TARGET" != *..* ]] || die "target must not contain consecutive dots"
  [[ "$DOMAIN" =~ ^[A-Za-z0-9_.-]+$ ]] || die "domain must use only [A-Za-z0-9_.-]"
  [[ -S "$D2B_SOCKET" ]] || die "d2b public socket is not live: $D2B_SOCKET"
  [[ -x "$WEZTERM_BIN" ]] ||
    die "source-built weezterm CLI is not executable: $WEZTERM_BIN ($(stat -c '%A %U:%G' "$WEZTERM_BIN" 2>/dev/null || printf 'missing'))"
  [[ -x "$WEZTERM_GUI_BIN" ]] ||
    die "source-built weezterm-gui is not executable: $WEZTERM_GUI_BIN ($(stat -c '%A %U:%G' "$WEZTERM_GUI_BIN" 2>/dev/null || printf 'missing'))"

  SHELL_ONE=${SHELL_ONE:-"title-a-$$"}
  SHELL_TWO=${SHELL_TWO:-"title-b-$$"}
  validate_shell_name "$SHELL_ONE" || die "invalid --shell-one; use 1-64 safe ASCII shell-name characters"
  validate_shell_name "$SHELL_TWO" || die "invalid --shell-two; use 1-64 safe ASCII shell-name characters"
  [[ "$SHELL_ONE" != "$SHELL_TWO" ]] || die "shell names must differ"
fi

INHERITED_RUNTIME=${XDG_RUNTIME_DIR-}
INHERITED_WAYLAND=${WAYLAND_DISPLAY-}
INHERITED_WAYLAND_PATH=""
if [[ -n "$INHERITED_RUNTIME" && -n "$INHERITED_WAYLAND" && "$INHERITED_WAYLAND" != /* ]]; then
  INHERITED_WAYLAND_PATH="$INHERITED_RUNTIME/$INHERITED_WAYLAND"
elif [[ "$INHERITED_WAYLAND" == /* ]]; then
  INHERITED_WAYLAND_PATH=$INHERITED_WAYLAND
fi

for variable in WAYLAND_DISPLAY WAYLAND_SOCKET NIRI_SOCKET DISPLAY \
  DBUS_SESSION_BUS_ADDRESS DBUS_STARTER_ADDRESS DBUS_STARTER_BUS_TYPE \
  DESKTOP_SESSION XDG_CURRENT_DESKTOP XDG_SESSION_DESKTOP; do
  unset "$variable"
done
while IFS='=' read -r variable _; do
  [[ "$variable" == DBUS_* ]] && unset "$variable"
done < <(env)

[[ -d "$HARNESS_WORK_DIR" ]] || die "harness work directory does not exist: $HARNESS_WORK_DIR"
RUNTIME_PARENT="${WEEZTERM_TITLE_RUNTIME_PARENT:-${INHERITED_RUNTIME:-/run/user/$UID}}"
[[ -d "$RUNTIME_PARENT" ]] || die "temporary directory does not exist: $RUNTIME_PARENT"
RUNTIME_DIR=$(mktemp -d "$RUNTIME_PARENT/wt-title.XXXXXXXX")
chmod 0700 -- "$RUNTIME_DIR"
export XDG_RUNTIME_DIR="$RUNTIME_DIR"
export LIBGL_ALWAYS_SOFTWARE=1
[[ "$(stat -c '%a' "$RUNTIME_DIR")" == 700 ]] || die "private runtime is not mode 0700"
[[ "$RUNTIME_DIR" != "$INHERITED_RUNTIME" ]] || die "private runtime equals inherited XDG_RUNTIME_DIR"

WESTON_PID=""
NIRI_PID=""
WEZTERM_PID=""

terminate_pid() {
  local pid=$1
  local label=$2
  [[ -n "$pid" ]] || return 0
  if kill -0 "$pid" 2>/dev/null; then
    kill -TERM "$pid" 2>/dev/null || true
    local deadline=$((SECONDS + 3))
    while kill -0 "$pid" 2>/dev/null && ((SECONDS < deadline)); do
      sleep 0.05
    done
    if kill -0 "$pid" 2>/dev/null; then
      printf 'wayland-title: %s PID %s did not exit after TERM; sending KILL\n' "$label" "$pid" >&2
      kill -KILL "$pid" 2>/dev/null || true
    fi
  fi
  wait "$pid" 2>/dev/null || true
}

cleanup() {
  local status=$?
  trap - EXIT INT TERM
  terminate_pid "$WEZTERM_PID" "WezTerm"
  terminate_pid "$NIRI_PID" "Niri"
  terminate_pid "$WESTON_PID" "Weston"
  if [[ -n "${RUNTIME_DIR-}" && "$RUNTIME_DIR" == "$RUNTIME_PARENT"/wt-title.* ]]; then
    rm -rf -- "$RUNTIME_DIR"
  fi
  exit "$status"
}
trap cleanup EXIT INT TERM

process_alive() {
  kill -0 "$1" 2>/dev/null
}

wait_for_socket() {
  local socket=$1
  local pid=$2
  local label=$3
  local deadline=$((SECONDS + READY_TIMEOUT))
  while [[ ! -S "$socket" ]]; do
    process_alive "$pid" || die "$label exited before creating $socket"
    ((SECONDS < deadline)) || die "timed out waiting for $label socket: $socket"
    sleep 0.05
  done
}

find_one_socket() {
  local pattern=$1
  local excluded=${2-}
  local -a sockets=()
  while IFS= read -r -d '' socket; do
    [[ -n "$excluded" && "$socket" == "$excluded" ]] || sockets+=("$socket")
  done < <(find "$RUNTIME_DIR" -maxdepth 2 -type s -name "$pattern" -print0)
  ((${#sockets[@]} == 1)) || return 1
  printf '%s\n' "${sockets[0]}"
}

wait_for_discovered_socket() {
  local pattern=$1
  local excluded=$2
  local pid=$3
  local label=$4
  local deadline=$((SECONDS + READY_TIMEOUT))
  local socket=""
  while ! socket=$(find_one_socket "$pattern" "$excluded"); do
    process_alive "$pid" || die "$label exited before its socket was ready"
    ((SECONDS < deadline)) || die "timed out waiting for exactly one $label socket matching $pattern"
    sleep 0.05
  done
  printf '%s\n' "$socket"
}

assert_private_socket() {
  local socket=$1
  local label=$2
  [[ -S "$socket" ]] || die "$label is not a live socket: $socket"
  case "$(realpath -m -- "$socket")" in
    "$RUNTIME_DIR"/*) ;;
    *) die "$label escaped private runtime: $socket" ;;
  esac
  if [[ -n "$INHERITED_WAYLAND_PATH" ]]; then
    [[ "$(realpath -m -- "$socket")" != "$(realpath -m -- "$INHERITED_WAYLAND_PATH")" ]] ||
      die "$label resolves to the inherited desktop socket"
  fi
}

WESTON_SOCKET_NAME="weston-title-test"
WESTON_SOCKET="$RUNTIME_DIR/$WESTON_SOCKET_NAME"
weston \
  --backend=headless-backend.so \
  --shell=kiosk-shell.so \
  --renderer=gl \
  --width=1400 \
  --height=900 \
  --idle-time=0 \
  --socket="$WESTON_SOCKET_NAME" \
  --log="$ARTIFACT_DIR/weston.log" \
  >>"$ARTIFACT_DIR/weston.log" 2>&1 &
WESTON_PID=$!
wait_for_socket "$WESTON_SOCKET" "$WESTON_PID" "Weston"
assert_private_socket "$WESTON_SOCKET" "Weston socket"

WAYLAND_DISPLAY="$WESTON_SOCKET_NAME" niri -c "$NIRI_CONFIG" \
  >"$ARTIFACT_DIR/niri.log" 2>&1 &
NIRI_PID=$!

NIRI_WAYLAND_SOCKET=$(wait_for_discovered_socket "wayland-*" "$WESTON_SOCKET" "$NIRI_PID" "Niri Wayland")
NIRI_IPC_SOCKET=$(wait_for_discovered_socket "niri.*.sock" "" "$NIRI_PID" "Niri IPC")
assert_private_socket "$NIRI_WAYLAND_SOCKET" "Niri Wayland socket"
assert_private_socket "$NIRI_IPC_SOCKET" "Niri IPC socket"

export WAYLAND_DISPLAY="${NIRI_WAYLAND_SOCKET##*/}"
export NIRI_SOCKET="$NIRI_IPC_SOCKET"
[[ "$WAYLAND_DISPLAY" != "$INHERITED_WAYLAND" || "$RUNTIME_DIR" != "$INHERITED_RUNTIME" ]] ||
  die "nested Niri display was not isolated from the inherited desktop"

capture_plain() {
  local name=$1
  local raw="$RUNTIME_DIR/$name-raw.png"
  local output="$ARTIFACT_DIR/$name.png"
  WAYLAND_DISPLAY="$WAYLAND_DISPLAY" grim "$raw"
  magick "$raw" -strip -define png:compression-level=9 "$output"
  local bytes
  bytes=$(stat -c '%s' "$output")
  ((bytes < 5 * 1024 * 1024)) || die "$output is $bytes bytes; expected less than 5 MB"
}

capture_plain "00-smoke"
{
  printf 'private_runtime=%s\n' "$RUNTIME_DIR"
  printf 'weston_socket=%s\n' "$WESTON_SOCKET"
  printf 'niri_wayland_socket=%s\n' "$NIRI_WAYLAND_SOCKET"
  printf 'niri_ipc_socket=%s\n' "$NIRI_IPC_SOCKET"
} >"$ARTIFACT_DIR/isolation.txt"

if ((SMOKE == 1)); then
  printf 'wayland-title: smoke passed; artifacts: %s\n' "$ARTIFACT_DIR"
  exit 0
fi

WEZTERM_CONFIG="$RUNTIME_DIR/wezterm.lua"
cat >"$WEZTERM_CONFIG" <<'EOF'
local wezterm = require 'wezterm'
local mux = wezterm.mux
local config = wezterm.config_builder()

local domain = assert(os.getenv('WEEZTERM_TITLE_TEST_DOMAIN'))
local target = assert(os.getenv('WEEZTERM_TITLE_TEST_TARGET'))
local socket_path = assert(os.getenv('WEEZTERM_TITLE_TEST_D2B_SOCKET'))
local shell_one = assert(os.getenv('WEEZTERM_TITLE_TEST_SHELL_ONE'))
local shell_two = assert(os.getenv('WEEZTERM_TITLE_TEST_SHELL_TWO'))
local second_local = os.getenv('WEEZTERM_TITLE_TEST_SECOND_LOCAL') == '1'

config.enable_wayland = true
config.front_end = 'Software'
config.window_decorations = 'TITLE | RESIZE'
config.enable_tab_bar = false
config.initial_cols = 110
config.initial_rows = 32
config.font_size = 14.0
config.automatically_reload_config = false
config.check_for_updates = false
config.window_frame = {
  font_size = 17.0,
  active_titlebar_bg = '#101010',
  active_titlebar_fg = '#ffffff',
  inactive_titlebar_bg = '#101010',
  inactive_titlebar_fg = '#ffffff',
}
config.d2b_domains = {
  {
    name = domain,
    target = target,
    socket_path = socket_path,
  },
}
config.default_domain = domain

wezterm.on('gui-startup', function()
  local first_tab, _, window = mux.spawn_window {
    domain = { DomainName = domain },
    set_environment_variables = {
      WEEZTERM_D2B_SHELL_NAME = shell_one,
    },
  }
  if second_local then
    window:spawn_tab {
      domain = { DomainName = 'local' },
    }
  else
    window:spawn_tab {
      domain = { DomainName = domain },
      set_environment_variables = {
        WEEZTERM_D2B_SHELL_NAME = shell_two,
      },
    }
  end
  first_tab:activate()
end)

return config
EOF
cp -- "$WEZTERM_CONFIG" "$ARTIFACT_DIR/wezterm.lua"

export WEEZTERM_TITLE_TEST_DOMAIN="$DOMAIN"
export WEEZTERM_TITLE_TEST_TARGET="$TARGET"
export WEEZTERM_TITLE_TEST_D2B_SOCKET="$D2B_SOCKET"
export WEEZTERM_TITLE_TEST_SHELL_ONE="$SHELL_ONE"
export WEEZTERM_TITLE_TEST_SHELL_TWO="$SHELL_TWO"
export WEEZTERM_TITLE_TEST_SECOND_LOCAL="$SECOND_LOCAL"
export WEEZTERM_D2B_BOUND_TARGET="$TARGET"
export WEEZTERM_D2B_BOUND_VM="$TARGET"
unset WEZTERM_UNIX_SOCKET WEEZTERM_UNIX_SOCKET

"$WEZTERM_GUI_BIN" --config-file "$WEZTERM_CONFIG" start \
  --always-new-process --no-auto-connect \
  >"$ARTIFACT_DIR/wezterm.log" 2>&1 &
WEZTERM_PID=$!

MUX_SOCKET=$(wait_for_discovered_socket "gui-sock-d2b-*" "" "$WEZTERM_PID" "WezTerm d2b mux")
assert_private_socket "$MUX_SOCKET" "WezTerm mux socket"
export WEEZTERM_UNIX_SOCKET="$MUX_SOCKET"
unset WEZTERM_UNIX_SOCKET

wezterm_cli() {
  WEEZTERM_UNIX_SOCKET="$MUX_SOCKET" \
    "$WEZTERM_BIN" --config-file "$WEZTERM_CONFIG" cli --no-auto-start "$@"
}

PANES_JSON="$ARTIFACT_DIR/panes.json"
deadline=$((SECONDS + READY_TIMEOUT))
while :; do
  process_alive "$WEZTERM_PID" || die "WezTerm exited before two d2b tabs were ready"
  if wezterm_cli list --format json >"$PANES_JSON.next" 2>>"$ARTIFACT_DIR/wezterm-cli.log" &&
    jq -e '
      length == 2
      and (map(.window_id) | unique | length == 1)
      and (map(.tab_id) | unique | length == 2)
    ' "$PANES_JSON.next" >/dev/null; then
    mv -- "$PANES_JSON.next" "$PANES_JSON"
    break
  fi
  ((SECONDS < deadline)) || die "timed out waiting for two tabs on the private WezTerm mux"
  sleep 0.1
done

mapfile -t IDS < <(
  jq -r 'sort_by(.tab_id)[] | [.tab_id, .pane_id] | @tsv' "$PANES_JSON"
)
((${#IDS[@]} == 2)) || die "unable to identify exactly two tabs from panes.json"
IFS=$'\t' read -r TAB_ONE PANE_ONE <<<"${IDS[0]}"
IFS=$'\t' read -r TAB_TWO PANE_TWO <<<"${IDS[1]}"
D2B_IDENTITY=$(
  jq -r --argjson pane "$PANE_ONE" '
    .[]
    | select(.pane_id == $pane)
    | .title
    | capture("\\[(?<identity>[^][]+:[^][]+)\\]$").identity
  ' "$PANES_JSON"
)
[[ -n "$D2B_IDENTITY" ]] ||
  die "first pane did not expose a trusted [target:shell] title in panes.json"

send_osc_code() {
  local pane=$1
  local code=$2
  local title=$3
  local payload
  payload="printf '\\033]${code};${title}\\007'"$'\r'
  wezterm_cli send-text --pane-id "$pane" --no-paste "$payload" \
    >>"$ARTIFACT_DIR/wezterm-cli.log" 2>&1
}

send_osc_title() {
  send_osc_code "$1" 0 "$2"
}

wait_for_pane_title() {
  local pane=$1
  local title=$2
  local deadline=$((SECONDS + TITLE_TIMEOUT))
  local json="$RUNTIME_DIR/panes-current.json"
  while :; do
    process_alive "$WEZTERM_PID" || die "WezTerm exited while waiting for pane $pane title '$title'"
    if wezterm_cli list --format json >"$json" 2>>"$ARTIFACT_DIR/wezterm-cli.log" &&
      jq -e --argjson pane "$pane" --arg title "$title" '
        any(.[]; .pane_id == $pane and (.title | contains($title)))
      ' "$json" >/dev/null; then
      return 0
    fi
    ((SECONDS < deadline)) || die "timed out waiting for pane $pane to report OSC title '$title'"
    sleep 0.1
  done
}

normalize_text() {
  tr '[:upper:]' '[:lower:]' |
    sed -E 's/[^a-z0-9]+/ /g; s/^ +//; s/ +$//; s/ +/ /g; s/(^| )d(zb|2h)( |$)/\1d2b\3/g; s/(^| )asc2( |$)/\1osc2\2/g; s/(^| )tocls( |$)/\1tools\2/g'
}

capture_until_title() {
  local name=$1
  local expected=$2
  local expected_normalized
  expected_normalized=$(printf '%s' "$expected" | normalize_text)
  local deadline=$((SECONDS + TITLE_TIMEOUT))
  local raw="$RUNTIME_DIR/$name-raw.png"
  local full="$ARTIFACT_DIR/$name.png"
  local crop="$ARTIFACT_DIR/$name-title.png"
  local ocr="$ARTIFACT_DIR/$name-ocr.txt"
  local width bytes actual_normalized

  printf '%s\n' "$expected" >"$ARTIFACT_DIR/$name-expected.txt"
  while :; do
    process_alive "$WEZTERM_PID" || die "WezTerm exited while capturing '$expected'"
    WAYLAND_DISPLAY="$WAYLAND_DISPLAY" grim "$raw"
    magick "$raw" -strip -define png:compression-level=9 "$full"
    width=$(identify -format '%w' "$full")
    magick "$full" -gravity North -crop "${width}x24+0+0" +repage \
      -colorspace Gray -threshold 65% -filter Lanczos -resize 800% \
      -strip -define png:compression-level=9 "$crop"
    tesseract "$crop" stdout --psm 7 2>>"$ARTIFACT_DIR/tesseract.log" >"$ocr" || true
    actual_normalized=$(normalize_text <"$ocr")
    printf '%s\n' "$actual_normalized" >"$ARTIFACT_DIR/$name-ocr-normalized.txt"

    if [[ " $actual_normalized " == *" $expected_normalized "* ]]; then
      for png in "$full" "$crop"; do
        bytes=$(stat -c '%s' "$png")
        ((bytes < 5 * 1024 * 1024)) ||
          die "$png is $bytes bytes; expected less than 5 MB"
      done
      return 0
    fi
    ((SECONDS < deadline)) ||
      die "OCR did not find '$expected' in $name-title.png; normalized OCR was '$actual_normalized'"
    sleep 0.15
  done
}

TITLE_ONE="first-latest"
TITLE_TWO_INITIAL="second-initial"
TITLE_TWO_UPDATED="second-updated"
TITLE_LONG="fake-admin-padding-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"
EXPECTED_TITLE_ONE="$TITLE_ONE [$D2B_IDENTITY]"
EXPECTED_TITLE_TWO="$TITLE_TWO_UPDATED [$TARGET:$SHELL_TWO]"
if ((SECOND_LOCAL == 1)); then
  EXPECTED_TITLE_TWO="[2/2] $TITLE_TWO_UPDATED"
fi

send_osc_title "$PANE_ONE" "$TITLE_ONE"
wait_for_pane_title "$PANE_ONE" "$TITLE_ONE"
send_osc_title "$PANE_TWO" "$TITLE_TWO_INITIAL"
wait_for_pane_title "$PANE_TWO" "$TITLE_TWO_INITIAL"
wezterm_cli activate-tab --tab-id "$TAB_ONE" >>"$ARTIFACT_DIR/wezterm-cli.log" 2>&1
capture_until_title "01-first-active" "$EXPECTED_TITLE_ONE"

send_osc_title "$PANE_TWO" "$TITLE_TWO_UPDATED"
wait_for_pane_title "$PANE_TWO" "$TITLE_TWO_UPDATED"
wezterm_cli activate-tab --tab-id "$TAB_TWO" >>"$ARTIFACT_DIR/wezterm-cli.log" 2>&1
capture_until_title "02-background-updated-active" "$EXPECTED_TITLE_TWO"

wezterm_cli activate-tab --tab-id "$TAB_ONE" >>"$ARTIFACT_DIR/wezterm-cli.log" 2>&1
capture_until_title "03-switched-back" "$EXPECTED_TITLE_ONE"

wezterm_cli activate-tab --tab-id "$TAB_TWO" >>"$ARTIFACT_DIR/wezterm-cli.log" 2>&1
capture_until_title "04-switched-again" "$EXPECTED_TITLE_TWO"

send_osc_title "$PANE_ONE" "$TITLE_LONG"
wait_for_pane_title "$PANE_ONE" "fake-admin-padding"
wezterm_cli activate-tab --tab-id "$TAB_ONE" >>"$ARTIFACT_DIR/wezterm-cli.log" 2>&1
capture_until_title "05-long-title-keeps-identity" "[$D2B_IDENTITY]"

send_osc_code "$PANE_ONE" 1 "shell-icon-title"
wait_for_pane_title "$PANE_ONE" "shell-icon-title"
send_osc_code "$PANE_ONE" 2 "copilot-osc2-title"
wait_for_pane_title "$PANE_ONE" "copilot-osc2-title"
capture_until_title "06-osc2-overrides-icon-title" "copilot-osc2-title [$D2B_IDENTITY]"

{
  printf 'target=%s\n' "$TARGET"
  printf 'domain=%s\n' "$DOMAIN"
  printf 'shell_one=%s\n' "$SHELL_ONE"
  printf 'shell_two=%s\n' "$SHELL_TWO"
  printf 'd2b_identity=%s\n' "$D2B_IDENTITY"
  printf 'second_local=%s\n' "$SECOND_LOCAL"
  printf 'mux_socket=%s\n' "$MUX_SOCKET"
} >"$ARTIFACT_DIR/run.txt"

printf 'wayland-title: passed; artifacts: %s\n' "$ARTIFACT_DIR"
