# --- weezterm remote features ---
# WeezTerm shell integration wrapper.
# Sources the upstream wezterm.sh (which works with any terminal that
# understands the same OSC sequences) and adds WEEZTERM_* env var aliases.
# --- end weezterm remote features ---

# Source the upstream integration (it works for weezterm too)
_weezterm_dir="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)"
if [ -f "$_weezterm_dir/wezterm.sh" ]; then
  . "$_weezterm_dir/wezterm.sh"
fi
unset _weezterm_dir
