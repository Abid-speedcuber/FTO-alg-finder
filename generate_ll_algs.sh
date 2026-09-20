#!/usr/bin/env bash
set -euo pipefail

depth="${1:-12}"
out="${2:-ll_algs_${depth}_or_under.txt}"
if [[ $# -gt 0 ]]; then
  shift
fi
if [[ $# -gt 0 ]]; then
  shift
fi

cargo run --release --manifest-path tools/ll-alg-generator/Cargo.toml -- \
  --max-depth "$depth" \
  --out "$out" \
  "$@"
