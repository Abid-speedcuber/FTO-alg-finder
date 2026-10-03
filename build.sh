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
        cp "$nsis_dir/nsis-output.exe" "$OUTPUT_DIR/windows/FTO-Alg-Finder-0.1.0-x64.exe"
        log_success "Windows installer: $OUTPUT_DIR/windows/FTO-Alg-Finder-0.1.0-x64.exe"
        return 0
    fi
    local bundle_installer
    bundle_installer="$(find "$MAIN_DIR/target/x86_64-pc-windows-gnu/release/bundle/nsis" -maxdepth 1 -name "*setup.exe" -type f 2>/dev/null | head -n 1)"
    if [ -n "$bundle_installer" ]; then
        cp "$bundle_installer" "$OUTPUT_DIR/windows/FTO-Alg-Finder-0.1.0-x64.exe"
        log_success "Windows installer: $OUTPUT_DIR/windows/FTO-Alg-Finder-0.1.0-x64.exe"
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
    for fmt in deb rpm appimage pacman; do
        [ -d "$bundle/$fmt" ] && cp "$bundle/$fmt"/* "$OUTPUT_DIR/linux/" 2>/dev/null || true
    done
    find "$OUTPUT_DIR/linux" -name "*.AppImage" -exec chmod +x {} \; 2>/dev/null || true
    log_success "Linux packages in $OUTPUT_DIR/linux"
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
    log_success "Done"
}

main "$@"
