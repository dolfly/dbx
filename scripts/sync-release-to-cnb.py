#!/usr/bin/env python3
"""Sync a GitHub release (tag + assets) to CNB.

Usage:
  python3 scripts/sync-release-to-cnb.py <tag> [options]

Options:
  --token <cnb_token>     CNB access token (also read from CNB_TOKEN env)
  --draft                 Create as draft release
  --prerelease            Mark as prerelease
  --dry-run               Show what would be done without doing it
  --skip-tag-push         Skip pushing git tags to CNB
  --asset <pattern>       Only sync assets matching glob (repeatable)
  --concurrency <n>       Concurrent uploads (default: 4)

Env vars:
  CNB_TOKEN               CNB access token
  GH_TOKEN                 GitHub token (for higher rate limits)
  GITHUB_REPOSITORY        e.g. "t8y2/dbx" (default: t8y2/dbx)

Requirements: requests, urllib3 (pip install requests)
"""

import argparse
import fnmatch
import json
import os
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor, as_completed

try:
    import requests
except ImportError:
    print("Error: 'requests' library required. Install: pip install requests", file=sys.stderr)
    sys.exit(1)

GITHUB_REPO = os.environ.get("GITHUB_REPOSITORY", "t8y2/dbx")
CNB_REPO = "dbxio.com/dbx"
CNB_API = "https://api.cnb.cool"
CNB_GIT = f"https://cnb.cool/{CNB_REPO}.git"
GITHUB_API = "https://api.github.com"


def cnb_headers(token: str) -> dict:
    return {
        "Authorization": f"Bearer {token}",
        "Accept": "application/vnd.cnb.api+json",
        "Content-Type": "application/json",
    }


def cnb_get(token: str, path: str) -> dict:
    r = requests.get(f"{CNB_API}{path}", headers=cnb_headers(token))
    return r.json() if r.status_code == 200 else {}


def cnb_post(token: str, path: str, data: dict = None) -> requests.Response:
    return requests.post(f"{CNB_API}{path}", headers=cnb_headers(token), json=data or {})


def gh_api(path: str) -> dict:
    h = {"Accept": "application/vnd.github+json"}
    if gh_token := os.environ.get("GH_TOKEN"):
        h["Authorization"] = f"Bearer {gh_token}"
    r = requests.get(f"{GITHUB_API}{path}", headers=h)
    r.raise_for_status()
    return r.json()


def push_tag(tag: str, token: str) -> bool:
    """Push a single git tag to CNB. Returns True if successful or already exists."""
    # Check if tag already exists on CNB
    existing = cnb_get(token, f"/{CNB_REPO}/-/git/tags/{tag}")
    if existing.get("name") == tag:
        print(f"   Tag {tag} already exists on CNB, skipping push")
        return True

    url = f"https://cnb:{token}@cnb.cool/{CNB_REPO}.git"
    result = subprocess.run(
        ["git", "push", url, tag],
        capture_output=True, text=True,
    )
    if result.returncode != 0:
        print(f"   Warning: git push failed: {result.stderr.strip()}", file=sys.stderr)
        return False
    return True


def find_existing_release(tag: str, token: str) -> str | None:
    """Find an existing release by tag name. Returns release_id or None."""
    r = cnb_get(token, f"/{CNB_REPO}/-/releases/tags/{tag}")
    return r.get("id") if r else None


def create_release(tag: str, name: str, body: str, draft: bool, prerelease: bool, token: str) -> str:
    """Create a release on CNB. Returns release_id. If release exists, returns existing ID."""
    existing_id = find_existing_release(tag, token)
    if existing_id:
        print(f"   Release for {tag} already exists (ID: {existing_id}), reusing")
        return existing_id

    data = {
        "tag_name": tag,
        "name": name,
        "body": body or "",
        "draft": draft,
        "prerelease": prerelease,
        "make_latest": "true",
    }
    r = cnb_post(token, f"/{CNB_REPO}/-/releases", data)
    if r.status_code != 201:
        print(f"Error creating release (HTTP {r.status_code}): {r.text}", file=sys.stderr)
        sys.exit(1)
    release = r.json()
    return release["id"]


def upload_one_asset(
    release_id: str,
    asset: dict,  # {name, size, url (API URL for download)}
    token: str,
    idx: int,
) -> bool:
    """Stream an asset directly from GitHub to CNB without writing to disk."""
    name = asset["name"]
    size = asset["size"]
    download_url = asset["url"]  # GitHub API URL (supports redirect)

    try:
        # Step 1: Get CNB upload URL first (fast API call, ~1s)
        print(f"   [{idx}] {name}: requesting CNB upload URL …")
        ul_r = cnb_post(
            token,
            f"/{CNB_REPO}/-/releases/{release_id}/asset-upload-url",
            {"asset_name": name, "size": size, "overwrite": True},
        )
        ul_r.raise_for_status()
        ul_data = ul_r.json()
        upload_url = ul_data.get("upload_url", "")
        verify_url = ul_data.get("verify_url", "")

        if not upload_url:
            print(f"   [{idx}] ERROR: no upload_url for {name}", file=sys.stderr)
            return False

        # Step 2: Stream GitHub → CNB directly, no disk intermediate
        print(f"   [{idx}] {name}: streaming {size:,} bytes GitHub → CNB …")
        gh_headers = {"Accept": "application/octet-stream"}
        if gh_token := os.environ.get("GH_TOKEN"):
            gh_headers["Authorization"] = f"Bearer {gh_token}"

        with requests.get(download_url, headers=gh_headers, stream=True) as gh_r:
            gh_r.raise_for_status()
            put_r = requests.put(upload_url, data=gh_r)
            if put_r.status_code not in (200, 201):
                print(f"   [{idx}] ERROR: upload returned HTTP {put_r.status_code}", file=sys.stderr)
                return False

        # Step 3: Confirm upload
        if verify_url:
            conf_r = cnb_post(token, verify_url.replace(CNB_API, ""))
            if conf_r.status_code != 200:
                print(f"   [{idx}] Warning: confirmation for {name} returned HTTP {conf_r.status_code}", file=sys.stderr)

        print(f"   [{idx}] Done: {name}")
        return True

    except Exception as e:
        print(f"   [{idx}] ERROR: {e}", file=sys.stderr)
        return False


def main():
    parser = argparse.ArgumentParser(description="Sync GitHub release to CNB")
    parser.add_argument("tag", help="Release tag (e.g. v0.5.33)")
    parser.add_argument("--token", help="CNB access token (or set CNB_TOKEN env)")
    parser.add_argument("--draft", action="store_true", help="Create as draft")
    parser.add_argument("--prerelease", action="store_true", help="Mark as prerelease")
    parser.add_argument("--dry-run", action="store_true", help="Show plan without executing")
    parser.add_argument("--skip-tag-push", action="store_true", help="Skip git tag push")
    parser.add_argument("--asset", action="append", dest="assets", help="Glob pattern to filter assets")
    parser.add_argument("--concurrency", type=int, default=4, help="Concurrent uploads (default: 4)")
    args = parser.parse_args()

    token = args.token or os.environ.get("CNB_TOKEN", "")
    if not token:
        print("Error: CNB_TOKEN required (set env or pass --token)", file=sys.stderr)
        sys.exit(1)

    # ── Step 0: Fetch GitHub release ────────────────────────────
    print(f"==> Fetching GitHub release for {args.tag} …")
    gh_release = gh_api(f"/repos/{GITHUB_REPO}/releases/tags/{args.tag}")
    gh_release_id = gh_release["id"]
    gh_release_name = gh_release.get("name", "")
    gh_release_body = gh_release.get("body", "") or ""
    gh_prerelease = gh_release.get("prerelease", False)
    gh_draft = gh_release.get("draft", False)

    # Build asset list
    all_assets = [
        {
            "name": a["name"],
            "size": a["size"],
            "url": a["url"],
        }
        for a in gh_release.get("assets", [])
    ]

    # Apply asset filters
    assets = all_assets
    if args.assets:
        assets = [
            a for a in all_assets
            if any(fnmatch.fnmatch(a["name"], p) for p in args.assets)
        ]

    print(f"   GitHub release ID: {gh_release_id}")
    print(f"   Name: {gh_release_name}")
    print(f"   Prerelease: {gh_prerelease}")
    print(f"   Assets to sync: {len(assets)}")

    if args.dry_run:
        for a in assets:
            print(f"     - {a['name']} ({a['size']:,} bytes)")
        print("\n[dry-run] No changes made.")
        return

    # ── Step 1: Push git tag ────────────────────────────────────
    if not args.skip_tag_push:
        print(f"==> Pushing tag {args.tag} to CNB …")
        push_tag(args.tag, token)
    else:
        print(f"==> Skipping tag push (--skip-tag-push)")

    # ── Step 2: Create CNB release ──────────────────────────────
    print(f"==> Creating CNB release for {args.tag} …")
    draft = args.draft or gh_draft
    prerelease = args.prerelease or gh_prerelease
    release_id = create_release(args.tag, gh_release_name, gh_release_body, draft, prerelease, token)
    print(f"   Created release ID: {release_id}")

    # ── Step 3: Upload assets ───────────────────────────────────
    print(f"==> Uploading {len(assets)} assets (concurrency={args.concurrency}) …")

    ok, fail = 0, 0
    with ThreadPoolExecutor(max_workers=args.concurrency) as ex:
        futures = {}
        for i, a in enumerate(assets):
            f = ex.submit(upload_one_asset, release_id, a, token, i + 1)
            futures[f] = a

        for f in as_completed(futures):
            a = futures[f]
            if f.result():
                ok += 1
            else:
                fail += 1
                print(f"FAILED: {a['name']}", file=sys.stderr)

    print(f"   Upload complete: {ok} succeeded, {fail} failed")

    if fail > 0:
        print(f"Some uploads failed ({fail})", file=sys.stderr)
        sys.exit(1)

    print(f"==> Done! Release synced to https://cnb.cool/{CNB_REPO}/-/releases/tags/{args.tag}")


if __name__ == "__main__":
    main()
