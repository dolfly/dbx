#!/usr/bin/env bash
# Sync a GitHub release (tag + assets) to CNB.
#
# Usage:
#   ./scripts/sync-release-to-cnb.sh <tag> [options]
#
# Options:
#   --token <cnb_token>    CNB access token (required; also read from CNB_TOKEN env)
#   --draft                Create as draft release (default: false)
#   --prerelease           Mark as prerelease (default: false)
#   --dry-run              Show what would be done without doing it
#   --skip-tag-push        Skip pushing git tags to CNB
#   --asset <pattern>      Only sync assets matching glob (repeatable; default: all)
#   --concurrency <n>      Concurrent uploads (default: 4)
#
# Env vars:
#   CNB_TOKEN              CNB access token (overridden by --token)
#   GH_TOKEN               GitHub token (optional; for higher rate limits)
#   GITHUB_REPOSITORY      e.g. "t8y2/dbx" (default: t8y2/dbx)
#
# Example:
#   CNB_TOKEN=xxx ./scripts/sync-release-to-cnb.sh v0.5.33

set -euo pipefail

# ── Config ──────────────────────────────────────────────────
GITHUB_REPO="${GITHUB_REPOSITORY:-t8y2/dbx}"
CNB_REPO="dbxio.com/dbx"
CNB_API="https://api.cnb.cool"
CNB_GIT="https://cnb.cool/${CNB_REPO}.git"
GITHUB_API="https://api.github.com"

TAG=""
CNB_TOKEN="${CNB_TOKEN:-}"
DRAFT="false"
PRERELEASE="false"
DRY_RUN="false"
SKIP_TAG_PUSH="false"
CONCURRENCY=4
ASSET_FILTERS=()
TMPDIR="${TMPDIR:-/tmp}"

# ── Parse args ──────────────────────────────────────────────
while [[ $# -gt 0 ]]; do
  case "$1" in
    --token)        CNB_TOKEN="$2"; shift 2 ;;
    --draft)        DRAFT="true"; shift ;;
    --prerelease)   PRERELEASE="true"; shift ;;
    --dry-run)      DRY_RUN="true"; shift ;;
    --skip-tag-push) SKIP_TAG_PUSH="true"; shift ;;
    --asset)        ASSET_FILTERS+=("$2"); shift 2 ;;
    --concurrency)  CONCURRENCY="$2"; shift 2 ;;
    --help|-h)
      sed -n '2,/^$/p' "$0" | tail -n+2
      exit 0
      ;;
    -*) echo "Unknown option: $1" >&2; exit 1 ;;
    *)  TAG="$1"; shift ;;
  esac
done

if [[ -z "$TAG" ]]; then
  echo "Error: tag required. Usage: sync-release-to-cnb.sh <tag>" >&2
  exit 1
fi

if [[ -z "$CNB_TOKEN" ]]; then
  echo "Error: CNB_TOKEN is required (set env or pass --token)" >&2
  exit 1
fi

# ── Helpers ─────────────────────────────────────────────────
api_headers() {
  echo "-H" "Authorization: Bearer ${CNB_TOKEN}" "-H" "Accept: application/vnd.cnb.api+json" "-H" "Content-Type: application/json"
}

gh_api() {
  local url="$1"
  local hdr=(-s)
  if [[ -n "${GH_TOKEN:-}" ]]; then
    hdr+=(-H "Authorization: Bearer ${GH_TOKEN}")
  fi
  hdr+=(-H "Accept: application/vnd.github+json")
  curl "${hdr[@]}" "$url"
}

# ── Step 0: Check GitHub release ────────────────────────────
echo "==> Fetching GitHub release for ${TAG} …"
GH_RELEASE="$(gh_api "${GITHUB_API}/repos/${GITHUB_REPO}/releases/tags/${TAG}")"
GH_RELEASE_ID="$(echo "$GH_RELEASE" | python3 -c "import json,sys; print(json.load(sys.stdin)['id'])")"
GH_RELEASE_NAME="$(echo "$GH_RELEASE" | python3 -c "import json,sys; print(json.load(sys.stdin).get('name',''))")"
GH_RELEASE_BODY="$(echo "$GH_RELEASE" | python3 -c "import json,sys; print(json.load(sys.stdin).get('body',''))")"
GH_PRERELEASE="$(echo "$GH_RELEASE" | python3 -c "import json,sys; print(str(json.load(sys.stdin).get('prerelease', False)).lower())")"
GH_DRAFT="$(echo "$GH_RELEASE" | python3 -c "import json,sys; print(str(json.load(sys.stdin).get('draft', False)).lower())")"

if [[ -z "$GH_RELEASE_ID" || "$GH_RELEASE_ID" == "null" ]]; then
  echo "Error: GitHub release ${TAG} not found" >&2
  exit 1
fi

# Build asset list
ASSET_JSON="$(echo "$GH_RELEASE" | python3 -c "
import json, sys
rel = json.load(sys.stdin)
assets = []
for a in rel.get('assets', []):
    assets.append({'name': a['name'], 'size': a['size'], 'url': a['url'], 'browser_download_url': a['browser_download_url']})
print(json.dumps(assets))
")"

# Filter assets if filters specified
if [[ ${#ASSET_FILTERS[@]} -gt 0 ]]; then
  FILTER_PATTERN="$(printf '%s\n' "${ASSET_FILTERS[@]}" | python3 -c "
import sys
patterns = [line.strip() for line in sys.stdin if line.strip()]
print(json.dumps(patterns))
")"
  ASSET_JSON="$(echo "$ASSET_JSON" | python3 -c "
import json, sys, fnmatch
assets = json.load(sys.stdin)
patterns = json.loads('$FILTER_PATTERN')
filtered = [a for a in assets if any(fnmatch.fnmatch(a['name'], p) for p in patterns)]
print(json.dumps(filtered))
")"
fi

ASSET_COUNT="$(echo "$ASSET_JSON" | python3 -c "import json,sys; print(len(json.load(sys.stdin)))")"
echo "   GitHub release ID: ${GH_RELEASE_ID}"
echo "   Name: ${GH_RELEASE_NAME}"
echo "   Prerelease: ${GH_PRERELEASE}"
echo "   Assets to sync: ${ASSET_COUNT}"

if [[ "$DRY_RUN" == "true" ]]; then
  echo "$ASSET_JSON" | python3 -c "
import json,sys
for a in json.load(sys.stdin):
    print(f'     - {a[\"name\"]} ({a[\"size\"]:,} bytes)')
"
fi

# ── Step 1: Push git tag to CNB ─────────────────────────────
if [[ "$SKIP_TAG_PUSH" != "true" ]]; then
  echo "==> Pushing tag ${TAG} to CNB …"
  if [[ "$DRY_RUN" == "true" ]]; then
    echo "   [dry-run] git push ${CNB_GIT} ${TAG}"
  else
    # Check if tag already exists on CNB
    EXISTING="$(curl -s $(api_headers) "${CNB_API}/${CNB_REPO}/-/git/tags/${TAG}" | python3 -c "import json,sys; d=json.load(sys.stdin); print(d.get('name',''))" 2>/dev/null || true)"
    if [[ "$EXISTING" == "$TAG" ]]; then
      echo "   Tag ${TAG} already exists on CNB, skipping push"
    else
      git push "${CNB_GIT}" "${TAG}" 2>&1 || {
        echo "   Warning: git push failed (tag may already exist or repo not configured as remote)"
      }
    fi
  fi
else
  echo "==> Skipping tag push (--skip-tag-push)"
fi

# ── Step 2: Create CNB release ──────────────────────────────
echo "==> Creating CNB release for ${TAG} …"

CREATE_BODY="$(python3 -c "
import json
body = {
    'tag_name': '${TAG}',
    'name': '${GH_RELEASE_NAME}',
    'body': '''${GH_RELEASE_BODY}''',
    'draft': ${DRAFT},
    'prerelease': ${PRERELEASE},
    'make_latest': 'true',
}
print(json.dumps(body))
")"

if [[ "$DRY_RUN" == "true" ]]; then
  echo "   [dry-run] POST ${CNB_API}/${CNB_REPO}/-/releases"
  CNB_RELEASE_ID="dry-run-id"
else
  RESP="$(curl -s -w "\n%{http_code}" -X POST $(api_headers) \
    -d "$CREATE_BODY" \
    "${CNB_API}/${CNB_REPO}/-/releases")"
  HTTP_CODE="$(echo "$RESP" | tail -1)"
  RESP_BODY="$(echo "$RESP" | sed '$d')"

  if [[ "$HTTP_CODE" != "201" ]]; then
    echo "Error creating release (HTTP ${HTTP_CODE}): ${RESP_BODY}" >&2
    exit 1
  fi

  CNB_RELEASE_ID="$(echo "$RESP_BODY" | python3 -c "import json,sys; print(json.load(sys.stdin)['id'])")"
  echo "   Created release ID: ${CNB_RELEASE_ID}"
fi

# ── Step 3: Upload assets ───────────────────────────────────
echo "==> Uploading ${ASSET_COUNT} assets (concurrency=${CONCURRENCY}) …"

WORKDIR="${TMPDIR}/cnb-sync-${TAG}-$$"
mkdir -p "$WORKDIR"

upload_asset() {
  local name="$1"
  local size="$2"
  local download_url="$3"
  local idx="$4"

  local fpath="${WORKDIR}/${name}"

  # Download from GitHub
  echo "   [${idx}] Downloading ${name} …"
  if ! curl -sL -o "$fpath" $( [[ -n "${GH_TOKEN:-}" ]] && echo "-H" && echo "Authorization: Bearer ${GH_TOKEN}" ) "$download_url"; then
    echo "   [${idx}] ERROR: failed to download ${name}" >&2
    return 1
  fi

  # Get upload URL from CNB
  echo "   [${idx}] Requesting upload URL for ${name} …"
  local ul_resp
  ul_resp="$(curl -s -X POST $(api_headers) \
    -d "{\"asset_name\":\"${name}\",\"size\":${size},\"overwrite\":true}" \
    "${CNB_API}/${CNB_REPO}/-/releases/${CNB_RELEASE_ID}/asset-upload-url")"

  local upload_url verify_url
  upload_url="$(echo "$ul_resp" | python3 -c "import json,sys; print(json.load(sys.stdin).get('upload_url',''))")"
  verify_url="$(echo "$ul_resp" | python3 -c "import json,sys; print(json.load(sys.stdin).get('verify_url',''))")"

  if [[ -z "$upload_url" ]]; then
    echo "   [${idx}] ERROR: failed to get upload URL: ${ul_resp}" >&2
    rm -f "$fpath"
    return 1
  fi

  # Upload file
  echo "   [${idx}] Uploading ${name} (${size} bytes) …"
  local put_code
  put_code="$(curl -s -o /dev/null -w "%{http_code}" -X PUT --data-binary "@${fpath}" "${upload_url}")"
  rm -f "$fpath"

  if [[ "$put_code" != "200" && "$put_code" != "201" ]]; then
    echo "   [${idx}] ERROR: upload returned HTTP ${put_code}" >&2
    return 1
  fi

  # Confirm upload (verify_url is already a full URL like https://api.cnb.cool/...)
  if [[ -n "$verify_url" ]]; then
    local confirm_code
    confirm_code="$(curl -s -o /dev/null -w "%{http_code}" -X POST $(api_headers) "${verify_url}")"
    if [[ "$confirm_code" != "200" ]]; then
      echo "   [${idx}] Warning: upload confirmation returned HTTP ${confirm_code}" >&2
    fi
  fi

  echo "   [${idx}] Done: ${name}"
}

export -f upload_asset
export WORKDIR CNB_API CNB_REPO CNB_RELEASE_ID CNB_TOKEN GH_TOKEN

# Process in batches with concurrency
echo "$ASSET_JSON" | python3 -c "
import json, sys, subprocess, os
from concurrent.futures import ThreadPoolExecutor, as_completed

assets = json.load(sys.stdin)
concurrency = int(os.environ.get('CONCURRENCY_BATCH', '$CONCURRENCY'))

def upload(a, idx):
    cmd = [
        'bash', '-c',
        f'source <(declare -f upload_asset); '
        f'export WORKDIR=\"{os.environ[\"WORKDIR\"]}\" CNB_API=\"{os.environ[\"CNB_API\"]}\" CNB_REPO=\"{os.environ[\"CNB_REPO\"]}\" '
        f'CNB_RELEASE_ID=\"{os.environ[\"CNB_RELEASE_ID\"]}\" CNB_TOKEN=\"{os.environ[\"CNB_TOKEN\"]}\" GH_TOKEN=\"{os.environ.get(\"GH_TOKEN\", \"\")}\"; '
        f'upload_asset \"{a[\"name\"]}\" {a[\"size\"]} \"{a[\"url\"]}\" {idx}'
    ]
    result = subprocess.run(cmd, capture_output=True, text=True, shell=True)
    if result.stdout:
        print(result.stdout, end='')
    if result.stderr:
        print(result.stderr, end='', file=sys.stderr)
    return result.returncode == 0

print(f'Starting {len(assets)} uploads with concurrency={concurrency}...', file=sys.stderr)
with ThreadPoolExecutor(max_workers=concurrency) as ex:
    futures = {ex.submit(upload, a, i+1): a for i, a in enumerate(assets)}
    ok = 0
    fail = 0
    for f in as_completed(futures):
        a = futures[f]
        if f.result():
            ok += 1
        else:
            fail += 1
            print(f'FAILED: {a[\"name\"]}', file=sys.stderr)

print(f'Upload complete: {ok} succeeded, {fail} failed', file=sys.stderr)
if fail > 0:
    sys.exit(1)
"

UPLOAD_RC=$?

# ── Cleanup ──────────────────────────────────────────────────
rm -rf "$WORKDIR"

if [[ "$UPLOAD_RC" -ne 0 ]]; then
  echo "Some uploads failed (exit code ${UPLOAD_RC})" >&2
  exit 1
fi

echo "==> Done! Release synced to https://cnb.cool/${CNB_REPO}/-/releases/tags/${TAG}"
