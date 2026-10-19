#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

usage() {
    cat <<'HELP'
Usage:
  ./release --current       Show the latest published release tag
  ./release TAG             Publish builds/release/ under TAG and mark it latest
  ./release --help          Show this help

Examples:
  ./release --current
  ./build.sh --all
  ./release v1.0.0.209

Requires GitHub CLI (gh auth login), git, and python3.
Commit and push the source used for the builds before publishing.
release.sh accepts the same arguments as release.
HELP
}

fail() { printf 'error: %s\n' "$*" >&2; exit 1; }

if [[ $# -ne 1 ]]; then
    usage >&2
    exit 2
fi
case "$1" in
    --help|-h) usage; exit 0 ;;
    --current) ;;
    -*) fail "Unknown option: $1 (use --help)" ;;
    *) git check-ref-format "refs/tags/$1" >/dev/null || fail "Invalid tag: $1" ;;
esac

command -v gh >/dev/null 2>&1 || fail "Install GitHub CLI, then run gh auth login."

if [[ "$1" == "--current" ]]; then
    gh release view --json tagName --jq '.tagName'
    exit 0
fi

tag="$1"
command -v python3 >/dev/null 2>&1 || fail "python3 is required to verify release checksums."
release_dir="$SCRIPT_DIR/builds/release"
assets=(
    FTO-Alg-Finder-windows-x64.exe
    FTO-Alg-Finder-linux-x86_64.AppImage
    FTO-Alg-Finder-linux-amd64.deb
    FTO-Alg-Finder-linux-x86_64.rpm
)
if [[ -f "$release_dir/FTO-Alg-Finder-linux-x86_64.pkg.tar.zst" ]]; then
    assets+=(FTO-Alg-Finder-linux-x86_64.pkg.tar.zst)
fi

# Verify every uploaded file and require a matching manifest before contacting
# GitHub. No builds, source changes, commits, or pushes are performed here.
python3 - "$release_dir" "${assets[@]}" <<'PY'
from pathlib import Path
import hashlib
import sys

root = Path(sys.argv[1])
expected = set(sys.argv[2:])
try:
    manifest = root / "SHA256SUMS"
    checksums = {}
    for line in manifest.read_text().splitlines():
        if not line.strip():
            continue
        digest, name = line.split("  ", 1)
        if name in checksums:
            raise ValueError(f"duplicate checksum for {name}")
        checksums[name] = digest
    if set(checksums) != expected:
        raise ValueError("SHA256SUMS must list exactly the staged installers")
    for name in sorted(expected):
        digest = hashlib.sha256()
        with (root / name).open("rb") as artifact:
            for chunk in iter(lambda: artifact.read(1024 * 1024), b""):
                digest.update(chunk)
        if digest.hexdigest() != checksums[name]:
            raise ValueError(f"checksum mismatch: {name}")
except (OSError, ValueError) as error:
    print(f"error: {error}\nRun ./build.sh --all to prepare builds/release/.", file=sys.stderr)
    sys.exit(1)
PY

repo="$(gh repo view --json nameWithOwner --jq '.nameWithOwner')"
commit="$(git rev-parse HEAD)"
existing_tag="$(git ls-remote --tags origin "refs/tags/$tag")"
[[ -z "$existing_tag" ]] || fail "Tag $tag already exists on origin. Choose a new tag; existing releases are never overwritten."

files=()
for asset in "${assets[@]}" SHA256SUMS; do
    files+=("$release_dir/$asset")
done
printf 'Publishing %s from %s to %s (%s assets)...\n' "$tag" "$commit" "$repo" "${#files[@]}"
# Keep incomplete uploads unpublished if a transfer fails.
gh release create "$tag" "${files[@]}" \
    --repo "$repo" --target "$commit" --title "FTO Alg Finder $tag" \
    --generate-notes --draft >/dev/null
gh release edit "$tag" --repo "$repo" --draft=false --latest
gh release view "$tag" --repo "$repo" --json url --jq '.url'
