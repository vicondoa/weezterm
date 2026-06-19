#!/usr/bin/env bash
# Validate CHANGELOG.md format (Keep a Changelog + semver).
# Exit 0 if valid, non-zero with diagnostics otherwise.
set -euo pipefail

CHANGELOG="${1:-CHANGELOG.md}"

if [ ! -f "$CHANGELOG" ]; then
  echo "ERROR: $CHANGELOG not found"
  exit 1
fi

errors=0

# Check [Unreleased] section exists
if ! grep -q '^\## \[Unreleased\]' "$CHANGELOG"; then
  echo "ERROR: Missing '## [Unreleased]' section"
  errors=$((errors + 1))
fi

# Extract version headers (excluding Unreleased)
versions=$(grep -oP '(?<=^## \[)[0-9]+\.[0-9]+\.[0-9]+(?=\] - )' "$CHANGELOG" || true)

if [ -z "$versions" ]; then
  echo "WARN: No released versions found in changelog (only [Unreleased])"
  exit $errors
fi

# Check format of each version line
while IFS= read -r line; do
  if echo "$line" | grep -qP '^\## \['; then
    # Skip [Unreleased]
    if echo "$line" | grep -q '^\## \[Unreleased\]'; then
      continue
    fi
    # Must match ## [X.Y.Z] - YYYY-MM-DD
    if ! echo "$line" | grep -qP '^\## \[\d+\.\d+\.\d+\] - \d{4}-\d{2}-\d{2}$'; then
      echo "ERROR: Malformed version header: $line"
      echo "  Expected format: ## [X.Y.Z] - YYYY-MM-DD"
      errors=$((errors + 1))
    fi
  fi
done < "$CHANGELOG"

# Check for duplicate versions
dupes=$(echo "$versions" | sort | uniq -d)
if [ -n "$dupes" ]; then
  echo "ERROR: Duplicate version(s): $dupes"
  errors=$((errors + 1))
fi

# Check descending order (latest first)
sorted=$(echo "$versions" | sort -rV)
if [ "$versions" != "$sorted" ]; then
  echo "ERROR: Versions are not in descending order"
  echo "  Found:    $(echo "$versions" | tr '\n' ' ')"
  echo "  Expected: $(echo "$sorted" | tr '\n' ' ')"
  errors=$((errors + 1))
fi

# Validate dates are real ISO 8601 dates
while IFS= read -r line; do
  if echo "$line" | grep -qP '^\## \[\d'; then
    date_str=$(echo "$line" | grep -oP '\d{4}-\d{2}-\d{2}' || true)
    if [ -n "$date_str" ]; then
      if ! date -d "$date_str" >/dev/null 2>&1; then
        echo "ERROR: Invalid date '$date_str' in: $line"
        errors=$((errors + 1))
      fi
    fi
  fi
done < "$CHANGELOG"

if [ $errors -gt 0 ]; then
  echo "FAILED: $errors error(s) found in $CHANGELOG"
  exit 1
fi

echo "OK: $CHANGELOG format is valid ($(echo "$versions" | wc -l) released version(s))"
