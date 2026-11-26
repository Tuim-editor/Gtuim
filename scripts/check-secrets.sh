#!/usr/bin/env bash
# Fail closed before pushing: scan all reachable Git history, with redacted output.
set -euo pipefail
cd "$(dirname "$0")/.."
scanner="${GITLEAKS_BIN:-gitleaks}"
if ! command -v "$scanner" >/dev/null 2>&1; then
  echo 'Secret scan blocked: install gitleaks or set GITLEAKS_BIN to its executable.' >&2
  exit 1
fi
"$scanner" git --redact --no-banner --no-color --log-opts='--all' .
