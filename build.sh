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
    npx @tauri-apps/cli build --target x86_64-pc-windows-gnu
    local nsis_dir="$MAIN_DIR/target/x86_64-pc-windows-gnu/release/nsis/x64"
    if [ -f "$nsis_dir/nsis-output.exe" ]; then
        cp "$nsis_dir/nsis-output.exe" "$OUTPUT_DIR/windows/FTO-Alg-Finder-0.1.0-x64.exe"
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
