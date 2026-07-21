#!/usr/bin/env bash
# --- weezterm remote features ---
# Filters `cargo clippy --message-format=json` output down to diagnostics
# whose primary span touches one specific file. Used by the `cargo-clippy`
# flake check (nix/flake.nix) to hard-fail only on findings inside the
# code this seam owns (config/src/d2b.rs), while tolerating pre-existing
# lint debt in the vendored upstream wezterm/config tree it does not.
#
# Usage:
#   clippy-scope-filter.sh <clippy-json-file> <scoped-file-path>
#   clippy-scope-filter.sh --self-test
#
# Exit status:
#   0  no compiler-message diagnostic has a span in <scoped-file-path>
#   1  a diagnostic was found in <scoped-file-path>, OR the input could not
#      be parsed/filtered (malformed JSON, jq failure, etc.) -- a parse
#      failure must never be silently treated as "no findings".
#
# With jq 1.8.1 (pinned via nixpkgs; re-verify if that pin ever changes),
# `jq -e` on this exact `select(...) | ... | select(...)` pipeline exits:
#   0  a match was found (truthy last output)
#   4  the pipeline produced zero outputs (no match, or empty input)
#   5  the input could not be parsed as JSON at all
# Any status other than 0 or 4 (5, or anything else) is treated as a
# parsing/filtering failure, not "no findings".
set -euo pipefail

self_test() {
  local tmp status
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' RETURN

  # Case 1: a compiler-message with a span in the scoped file -> must fail.
  cat >"$tmp/with-finding.json" <<'JSON'
{"reason":"compiler-message","message":{"rendered":"warning: example\n","spans":[{"file_name":"config/src/d2b.rs"}]}}
JSON
  status=0
  bash "$0" "$tmp/with-finding.json" "config/src/d2b.rs" || status=$?
  if [ "$status" -eq 0 ]; then
    echo "self-test FAILED: expected a finding in the scoped file to fail" >&2
    return 1
  fi

  # Case 2: a compiler-message with a span elsewhere -> must pass.
  cat >"$tmp/without-finding.json" <<'JSON'
{"reason":"compiler-message","message":{"rendered":"warning: example\n","spans":[{"file_name":"config/src/lib.rs"}]}}
JSON
  status=0
  bash "$0" "$tmp/without-finding.json" "config/src/d2b.rs" || status=$?
  if [ "$status" -ne 0 ]; then
    echo "self-test FAILED: expected a finding outside the scoped file to pass" >&2
    return 1
  fi

  # Case 3: malformed JSON -> must fail, not be silently treated as clean.
  printf 'not json\n' >"$tmp/malformed.json"
  status=0
  bash "$0" "$tmp/malformed.json" "config/src/d2b.rs" || status=$?
  if [ "$status" -eq 0 ]; then
    echo "self-test FAILED: expected malformed input to fail" >&2
    return 1
  fi

  # Case 4: an empty (but well-formed, e.g. no diagnostics) input -> pass.
  : >"$tmp/empty.json"
  status=0
  bash "$0" "$tmp/empty.json" "config/src/d2b.rs" || status=$?
  if [ "$status" -ne 0 ]; then
    echo "self-test FAILED: expected empty input to pass" >&2
    return 1
  fi

  echo "clippy-scope-filter.sh self-test OK"
}

if [ "${1:-}" = "--self-test" ]; then
  self_test
  exit 0
fi

json_file="${1:?usage: clippy-scope-filter.sh <clippy-json-file> <scoped-file-path>}"
scoped_file="${2:?usage: clippy-scope-filter.sh <clippy-json-file> <scoped-file-path>}"

set +e
jq -e --arg file "$scoped_file" '
    select(.reason == "compiler-message")
    | .message.spans[]?
    | select(.file_name == $file)
  ' "$json_file" >/dev/null
jq_status=$?
set -e

case "$jq_status" in
0)
  echo "clippy findings in $scoped_file:" >&2
  jq -r --arg file "$scoped_file" '
      select(.reason == "compiler-message")
      | select(.message.spans[]?.file_name == $file)
      | .message.rendered
    ' "$json_file" >&2
  exit 1
  ;;
4)
  exit 0
  ;;
*)
  echo "failed to parse/filter $json_file with jq (exit $jq_status)" >&2
  exit 1
  ;;
esac
# --- end weezterm remote features ---
