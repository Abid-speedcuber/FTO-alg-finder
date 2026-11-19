#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
if ! command -v emcc >/dev/null; then
  echo 'Emscripten 4.0.20 required. Run source /path/to/emsdk/emsdk_env.sh first.' >&2
  exit 1
fi
if [[ $(emcc --version | head -1) != *4.0.20* ]]; then
  echo 'This build was tested with Emscripten 4.0.20; use that SDK version.' >&2
  exit 1
fi
# Rebuild std as well: its pthread implementation needs the same atomic ABI.
export RUSTFLAGS='-C target-feature=+atomics,+bulk-memory,+mutable-globals -C panic=abort -C link-arg=-pthread -C link-arg=-sMODULARIZE=1 -C link-arg=-sEXPORT_ES6=1 -C link-arg=-sEXPORT_NAME=createFtoModule -C link-arg=-sENVIRONMENT=worker,node -C link-arg=-sALLOW_MEMORY_GROWTH=1 -C link-arg=-sMAXIMUM_MEMORY=4294967296 -C link-arg=-sINITIAL_MEMORY=67108864 -C link-arg=-sSTACK_SIZE=2097152 -C link-arg=-sPTHREAD_POOL_SIZE=4 -C link-arg=-sPTHREAD_POOL_SIZE_STRICT=0 -C link-arg=-sEXIT_RUNTIME=0 -C link-arg=-sFORCE_FILESYSTEM=1 -C link-arg=-sEXPORTED_FUNCTIONS=["_main","_fto_dispatch","_fto_events"] -C link-arg=-sEXPORTED_RUNTIME_METHODS=["ccall","FS"]'
cargo +nightly build -Z build-std=std,panic_abort --target wasm32-unknown-emscripten --release -p fto-web
cp target/wasm32-unknown-emscripten/release/fto-web.js public/wasm/fto-web.js
cp target/wasm32-unknown-emscripten/release/fto_web.wasm public/wasm/fto_web.wasm
# Older SDKs emit a separate worker bootstrap; preserve it when present.
for file in target/wasm32-unknown-emscripten/release/fto-web.*worker*; do
  [[ ! -f "$file" ]] || cp "$file" public/wasm/
done
