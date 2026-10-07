#!/usr/bin/env bash
# Check that every pinned runner image can be pulled by this repository's CI.
# Set GHCR_USERNAME and GHCR_TOKEN to test authenticated access to the fork-owned
# private package. Without both variables, this probes anonymous/public access.
#
# The credentials are sent only to GHCR's HTTPS token endpoint. They are never
# printed or written to the step summary.
#
# Usage:
#   scripts/check-runner-image-pullability.sh [--json PATH] [REF ...]
#     --json PATH   catalog to read (default: ci/runner-images.json)
#     REF ...       additional image references (name[:tag|@digest]) to probe
#
# Exit status: 0 if every reference is pullable, 1 if an image is unavailable,
# and 2 for invalid arguments/configuration.
# Requires: bash, curl, python3 (or python; stdlib only).

set -euo pipefail

json_path="ci/runner-images.json"
extra_refs=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --json)
      [[ $# -ge 2 && -n "$2" ]] || { echo "--json requires a path" >&2; exit 2; }
      json_path="$2"
      shift 2
      ;;
    -h|--help) sed -n '2,22p' "$0"; exit 0 ;;
    *) extra_refs+=("$1"); shift ;;
  esac
done

if [[ -n "${GHCR_TOKEN:-}" && -z "${GHCR_USERNAME:-}" ]]; then
  echo "GHCR_USERNAME is required when GHCR_TOKEN is set" >&2
  exit 2
fi
if [[ -n "${GHCR_USERNAME:-}" && -z "${GHCR_TOKEN:-}" ]]; then
  echo "GHCR_TOKEN is required when GHCR_USERNAME is set" >&2
  exit 2
fi

python_bin="${PYTHON:-python3}"
command -v "$python_bin" >/dev/null 2>&1 || python_bin=python

probe() {
  local reference="$1" base repository ref token_url token_response token code
  if [[ "$reference" == *"@sha256:"* ]]; then
    base="${reference%%@sha256:*}"
    ref="sha256:${reference##*@sha256:}"
  else
    base="$reference"
    ref="${base##*:}"
    if [[ "$ref" == "$base" ]]; then
      ref="latest"
    else
      base="${base%:*}"
    fi
  fi
  repository="${base#ghcr.io/}"
  token_url="https://ghcr.io/token?scope=repository:${repository}:pull&service=ghcr.io"

  if [[ -n "${GHCR_TOKEN:-}" ]]; then
    token_response="$(curl -sS --user "${GHCR_USERNAME}:${GHCR_TOKEN}" "$token_url" || true)"
  else
    token_response="$(curl -sS "$token_url" || true)"
  fi
  token="$(printf '%s' "$token_response" | "$python_bin" -c '
import json, sys
try:
    payload = json.load(sys.stdin)
    print(payload.get("token") or payload.get("access_token") or "")
except (ValueError, AttributeError):
    print("")
')"
  code="$(curl -sS -o /dev/null -w '%{http_code}' \
    -H "Authorization: Bearer ${token}" \
    -H 'Accept: application/vnd.oci.image.index.v1+json, application/vnd.oci.image.manifest.v1+json, application/vnd.docker.distribution.manifest.list.v2+json, application/vnd.docker.distribution.manifest.v2+json' \
    "https://ghcr.io/v2/${repository}/manifests/${ref}" || true)"
  printf '%s  %s\n' "$code" "$reference"
}

refs=()
while IFS= read -r reference; do
  reference="${reference%[$'\r\n']}"
  [[ -n "$reference" ]] && refs+=("$reference")
done < <("$python_bin" - "$json_path" <<'PY'
import json, sys
with open(sys.argv[1], encoding="utf-8") as handle:
    for image in json.load(handle)["images"].values():
        print(image["reference"].strip())
PY
)
refs+=("${extra_refs[@]-}")

if [[ ${#refs[@]} -eq 0 ]]; then
  echo "no image references found in ${json_path}" >&2
  exit 1
fi

failing=0
for reference in "${refs[@]}"; do
  line="$(probe "$reference")"
  printf '%s\n' "$line"
  [[ "$line" == 200* ]] || failing=1
done

if [[ "$failing" -ne 0 ]]; then
  cat >&2 <<'EOF'

NOT PULLABLE: at least one pinned runner image could not be pulled with the
configured access. Container jobs using these references will fail at
"Initialize containers". Verify the package is linked to this repository, the
consumer grants packages: read, and GHCR_USERNAME / GHCR_TOKEN are available.
EOF
fi
exit "$failing"
