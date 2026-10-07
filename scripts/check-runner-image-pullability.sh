#!/usr/bin/env bash
# Check whether every digest in the runner-image catalog is anonymously
# pullable. GitHub Actions container: images are fetched before job steps run,
# so private or nonexistent GHCR digests fail at "Initialize containers".
#
# The fork's replacement runner images live at
#   ghcr.io/marlucool/mesh-llm-ci-runner
# This script probes each reference without credentials:
#   1. Request an anonymous GHCR pull token for the image repository.
#   2. Request the exact manifest or index digest using that token.
# HTTP 200 means the manifest is public and exists; other statuses fail closed.
#
# Keep this check in CI after changing image references; do not infer pullability
# from a successful authenticated build/push.
#
# Usage:
#   scripts/check-runner-image-pullability.sh [--json PATH] [REF ...]
#     --json PATH   catalog to read (default: ci/runner-images.json)
#     REF ...       additional image references (name[:tag|@digest]) to probe
#
# Exit status: 0 if every reference is anonymously pullable, 1 otherwise.
# Requires: bash, curl, python3 (or python; stdlib only).

set -euo pipefail

json_path="ci/runner-images.json"
extra_refs=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --json) json_path="${2:-}"; shift 2 ;;
    -h|--help) sed -n '2,22p' "$0"; exit 0 ;;
    *) extra_refs+=("$1"); shift ;;
  esac
done

python_bin="${PYTHON:-python3}"
command -v "$python_bin" >/dev/null 2>&1 || python_bin=python

probe() {
  local reference="$1" base repository ref code
  if [[ "$reference" == *"@sha256:"* ]]; then
    base="${reference%%@sha256:*}"
    ref="sha256:${reference##*@sha256:}"
  else
    base="$reference"
    ref="${base##*:}"
    if [[ "$ref" == "$base" ]]; then
      ref="latest"                    # bare name means :latest
    else
      base="${base%:*}"                # strip the :tag suffix
    fi
  fi
  repository="${base#ghcr.io/}"        # tolerate the registry prefix

  local token
  # A private package does not issue an anonymous pull token at all (HTTP 401);
  # probe with an empty token so the manifest request still yields a verdict.
  token=$(curl -s "https://ghcr.io/token?scope=repository:${repository}:pull&service=ghcr.io" \
    | "$python_bin" -c 'import json,sys; print(json.load(sys.stdin).get("token",""))')
  code=$(curl -s -o /dev/null -w '%{http_code}' \
    -H "Authorization: Bearer ${token}" \
    -H 'Accept: application/vnd.oci.image.index.v1+json, application/vnd.oci.image.manifest.v1+json, application/vnd.docker.distribution.manifest.list.v2+json, application/vnd.docker.distribution.manifest.v2+json' \
    "https://ghcr.io/v2/${repository}/manifests/${ref}")
  printf '%s  %s\n' "$code" "$reference"
}

refs=()
while IFS= read -r reference; do
  reference="${reference%[$'\r\n']}"   # tolerate CRLF from Windows pythons
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
  line=$(probe "$reference")
  printf '%s\n' "$line"
  [[ "$line" == 200* ]] || failing=1
done

if [[ "$failing" -ne 0 ]]; then
  cat >&2 <<'EOF'

NOT anonymously pullable: at least one pinned runner image is unavailable to
this repository's CI. Container jobs pinning these references will fail at
"Initialize containers". Either the image must be published under this
account/organization with access inherited from this repository, or the jobs
must not depend on the upstream Mesh-LLM package.
EOF
fi
exit "$failing"
