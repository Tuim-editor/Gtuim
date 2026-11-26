#!/usr/bin/env bash
# Fail closed before pushing: scan all reachable Git history, with redacted output.
set -euo pipefail
cd "$(dirname "$0")/.."
scanner="${GITLEAKS_BIN:-gitleaks}"
if ! command -v "$scanner" >/dev/null 2>&1; then
  echo 'Secret scan blocked: install gitleaks or set GITLEAKS_BIN to its executable.' >&2
  exit 1
fi
# Explicit outgoing OIDs also cover pushes of commits not reachable from a ref.
log_opts='--all'
for oid in "$@"; do
  if [[ ! "$oid" =~ ^[0-9a-fA-F]{40}$ && ! "$oid" =~ ^[0-9a-fA-F]{64}$ ]]; then
    echo 'Secret scan blocked: invalid outgoing object ID.' >&2
    exit 1
  fi
  log_opts+=" $oid"
done
"$scanner" git --redact --ignore-gitleaks-allow --no-banner --no-color --log-opts="$log_opts" .
