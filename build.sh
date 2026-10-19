#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MAIN_DIR="$SCRIPT_DIR/main"
OUTPUT_DIR="$SCRIPT_DIR/builds"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

log_info() { echo -e "${BLUE}[INFO]${NC} $1"; }
log_success() { echo -e "${GREEN}[SUCCESS]${NC} $1"; }
log_warning() { echo -e "${YELLOW}[WARNING]${NC} $1"; }
log_error() { echo -e "${RED}[ERROR]${NC} $1"; }

check_command() { command -v "$1" &>/dev/null; }

prepare_makensis_wrapper() {
    local real_makensis
    real_makensis="$(command -v makensis || true)"
    if [ -z "$real_makensis" ]; then
        return 0
    fi
    local wrapper_dir="$OUTPUT_DIR/tools"
    mkdir -p "$wrapper_dir"
    cat > "$wrapper_dir/makensis" << EOF
#!/usr/bin/env bash
args=()
script_path=""
for arg in "\$@"; do
  if [[ "\$arg" = /* && "\$arg" == *.nsi ]]; then
    script_path="\$arg"
    patched_script="\${arg%.nsi}-wine.nsi"
    python3 - "\$arg" "\$patched_script" << 'PY'
import os
import re
import subprocess
import sys

src, dst = sys.argv[1], sys.argv[2]
with open(src, "r", encoding="utf-8") as f:
    text = f.read()

paths = sorted(set(re.findall(r"/[^\"\\s]+", text)), key=len, reverse=True)
for path in paths:
    if os.path.exists(path):
        wine_path = subprocess.check_output(["winepath", "-w", path], text=True).strip()
        text = text.replace(path, wine_path)

with open(dst, "w", encoding="utf-8") as f:
    f.write(text)
PY
    if command -v winepath >/dev/null 2>&1; then
      args+=("\$(winepath -w "\$patched_script")")
    else
      args+=(-- "\$arg")
    fi
  else
    args+=("\$arg")
  fi
done
exec "$real_makensis" "\${args[@]}"
EOF
    chmod +x "$wrapper_dir/makensis"
    export PATH="$wrapper_dir:$PATH"
}

show_help() {
    cat << 'HELP'
Optimal FTO Solver - Build Script
Usage: ./build.sh [OPTIONS]
Options:
  --windows       Build Windows installer (.exe)
  --linux         Build Linux packages
  --deb           Build .deb
  --rpm           Build .rpm
  --pkg           Build .pkg.tar.zst (Arch)
  --appimage      Build AppImage
  --all           Build all
  --install-deps  Install deps
  --help          Show help
HELP
}

build_windows() {
    log_info "Building Windows installer..."
    mkdir -p "$OUTPUT_DIR/windows"
    cd "$MAIN_DIR"
    prepare_makensis_wrapper
    npx @tauri-apps/cli build --target x86_64-pc-windows-gnu
    local nsis_dir="$MAIN_DIR/target/x86_64-pc-windows-gnu/release/nsis/x64"
    if [ -f "$nsis_dir/nsis-output.exe" ]; then
        cp "$nsis_dir/nsis-output.exe" "$OUTPUT_DIR/windows/FTO-Alg-Finder-windows-x64.exe"
        log_success "Windows installer: $OUTPUT_DIR/windows/FTO-Alg-Finder-windows-x64.exe"
        return 0
    fi
    local bundle_installer
    bundle_installer="$(find "$MAIN_DIR/target/x86_64-pc-windows-gnu/release/bundle/nsis" -maxdepth 1 -name "*setup.exe" -type f 2>/dev/null | head -n 1)"
    if [ -n "$bundle_installer" ]; then
        cp "$bundle_installer" "$OUTPUT_DIR/windows/FTO-Alg-Finder-windows-x64.exe"
        log_success "Windows installer: $OUTPUT_DIR/windows/FTO-Alg-Finder-windows-x64.exe"
        return 0
    fi
    log_error "NSIS output not found"
    return 1
}

build_linux_all() {
    log_info "Building Linux packages..."
    mkdir -p "$OUTPUT_DIR/linux"
    cd "$MAIN_DIR"
    npx @tauri-apps/cli build --target x86_64-unknown-linux-gnu
    local bundle="$MAIN_DIR/target/x86_64-unknown-linux-gnu/release/bundle"
    local fmt extension asset source
    for fmt in deb rpm appimage pacman; do
        case "$fmt" in
            deb) extension="deb"; asset="FTO-Alg-Finder-linux-amd64.deb" ;;
            rpm) extension="rpm"; asset="FTO-Alg-Finder-linux-x86_64.rpm" ;;
            appimage) extension="AppImage"; asset="FTO-Alg-Finder-linux-x86_64.AppImage" ;;
            pacman) extension="pkg.tar.zst"; asset="FTO-Alg-Finder-linux-x86_64.pkg.tar.zst" ;;
        esac
        [ -d "$bundle/$fmt" ] || continue
        source="$(find "$bundle/$fmt" -maxdepth 1 -type f -name "*.$extension" -print -quit)"
        [ -n "$source" ] && cp "$source" "$OUTPUT_DIR/linux/$asset"
    done
    find "$OUTPUT_DIR/linux" -name "*.AppImage" -exec chmod +x {} \; 2>/dev/null || true
    log_success "Linux packages in $OUTPUT_DIR/linux"
}

prepare_release_assets() {
    # Stable asset names keep releases/latest/download links valid across versions.
    python3 - "$OUTPUT_DIR" << 'PYRELEASE'
from pathlib import Path
import hashlib
import shutil
import sys

root = Path(sys.argv[1])
release = root / "release"
release.mkdir(exist_ok=True)
assets = []
names = {
    "windows": ("FTO-Alg-Finder-windows-x64.exe",),
    "linux": (
        "FTO-Alg-Finder-linux-amd64.deb",
        "FTO-Alg-Finder-linux-x86_64.rpm",
        "FTO-Alg-Finder-linux-x86_64.AppImage",
        "FTO-Alg-Finder-linux-x86_64.pkg.tar.zst",
    ),
}
for folder, filenames in names.items():
    for name in filenames:
        source = root / folder / name
        if not source.is_file():
            continue
        destination = release / name
        shutil.copy2(source, destination)
        assets.append(destination)
checksums = []
for path in sorted(assets):
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    checksums.append(f"{digest}  {path.name}\n")
(release / "SHA256SUMS").write_text("".join(checksums))
PYRELEASE
    log_success "Release assets and SHA256SUMS: $OUTPUT_DIR/release"
}

main() {
    local bw=false bl=false bdeb=false brpm=false bpkg=false bapp=false ideps=false
    while [[ $# -gt 0 ]]; do
        case $1 in
            --windows) bw=true ;;
            --linux) bl=true ;;
            --deb) bdeb=true ;;
            --rpm) brpm=true ;;
            --pkg) bpkg=true ;;
            --appimage) bapp=true ;;
            --all) bw=true bl=true ;;
            --install-deps) ideps=true ;;
            --help|-h) show_help; exit 0 ;;
            *) log_error "Unknown $1"; show_help; exit 1 ;;
        esac
        shift
    done
    if ! $bw && ! $bl && ! $bdeb && ! $brpm && ! $bpkg && ! $bapp && ! $ideps; then
        show_help; exit 0
    fi
    mkdir -p "$OUTPUT_DIR"
    if $bw; then build_windows; fi
    if $bl || $bdeb || $brpm || $bpkg || $bapp; then build_linux_all; fi
    if $bw || $bl || $bdeb || $brpm || $bpkg || $bapp; then prepare_release_assets; fi
    log_success "Done"
}

main "$@"
