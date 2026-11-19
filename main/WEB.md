# Browser build

This is an Emscripten/pthread port of the existing Rust backend, with the same
React UI. It runs locally in the browser; no solve requests leave the device.
It is a browser preview, not full desktop resource/GPU parity.

## Build and run

Install Node/npm and Rust. Install and activate Emscripten **4.0.20**, then install
the nightly Rust toolchain and its source component:

```sh
rustup toolchain install nightly --component rust-src --profile minimal
# In your emsdk checkout:
./emsdk install 4.0.20
./emsdk activate 4.0.20
source ./emsdk_env.sh
# In this repository:
cd main
npm ci
cd ..
./build.sh --web
```

The complete static app is written to `public/web/`. The landing page links to
`./web/` below the desktop downloads. Generated WASM and web output are ignored
by Git; build them before deploying the landing site's `public/` directory.
`--all` retains its existing desktop-only behavior; combine it with `--web` if
both kinds of output are wanted.

For development, run `npm run build:wasm` followed by `npm run dev` in `main/`.
`npm run build:web` builds the standalone browser app in `main/dist/`.
The initial WASM build recompiles Rust's standard library with atomics, so it is
considerably slower than subsequent builds. The scripts keep Tauri's build and
native GPU feature defaults separate from the browser target.

## Hosting

Use HTTPS (localhost also works). Prefer these response headers for `/web/` and
all its assets:

```text
Cross-Origin-Opener-Policy: same-origin
Cross-Origin-Embedder-Policy: require-corp
```

Serve `.wasm` as `application/wasm`. Do not embed the app in an iframe without
explicitly granting cross-origin isolation. `public/_headers` provides the rules
for hosts supporting that format. Vite dev/preview sends the headers itself.
A vendored MIT-licensed [coi-serviceworker 0.1.7](https://github.com/gzuidhof/coi-serviceworker)
bootstrap is included in web builds for static hosts such as GitHub Pages where
headers cannot be configured. It registers only under the app's own path and
reloads once to establish isolation; it does not cache application assets.
Desktop builds do not load this bootstrap. HTTPS is still required.

Deploy the JS/WASM files together and avoid long immutable caching for the
runtime files. A mismatched Emscripten loader and WASM binary cannot run.

## What carries over

The browser uses the desktop command implementations, not a separate simplified
solver. The port includes facelet validation; full, last-layer and partial
search; move restrictions and center targets; selected/all depths and solutions;
CPU pruning and Bloom generation; cancellation; RAM unload; table discovery and
delete/reset; and batch scheduling, pause/resume and portable checkpoints.
The viewer, algorithm combiner, settings, instances and existing IndexedDB batch
projects remain in the React frontend. Batch downloads already use browser Blob
downloads instead of the desktop Downloads-directory command.

`platform.ts` chooses Tauri or the browser worker at runtime. The worker loads
`wasm/fto-web.js`, restores `/fto` file metadata from IndexedDB, and dispatches
JSON commands. Computation runs on pthreads sharing one WASM memory; the host
worker remains available for stop/pause requests and event polling. Four workers
are prewarmed; more may be created on demand. This is not a four-thread search
cap. There is no fixed 1,024-thread application cap; actual parallelism still
depends on available branches and browser/OS resources.
Only one tab per origin can own the solver cache at a time, enforced using Web
Locks. This prevents cache snapshots from overwriting another tab's data.

## Storage behavior

Eligible restricted pattern tables store their compact byte array and coordinate
mapping directly. Existing dense files convert on their first successful compact
load without regenerating distances; subsequent loads avoid dense reads and table
compaction. Coordinate indexing is still prepared for the current move set.

Completed/cancelled operations sync their files to IndexedDB only after Rust has
stopped changing them. Deletes and resets also sync. The output reports when
saving starts and completes. Keep the tab open until saving completes; closing
it during an operation discards unfinished work and unflushed cache writes.
Quota/write errors are reported, and saving retries every 30 seconds without
pretending the files were saved. Batch projects use their existing independent
IndexedDB autosave; explicitly pause a batch before closing the page to preserve
its resumable DFS stacks.

Settings shows the origin's storage usage/quota and offers a persistent-storage
request. The browser decides whether to grant it. Clearing site data deletes
settings, projects and tables. Desktop cache files are not automatically imported.

## Substantial work still needed for desktop parity

1. **GPU:** The browser build deliberately reports GPU unavailable. The production
   wgpu backend uses blocking adapter/device initialization, polling, buffer map
   completion and readback. A WebGPU port needs asynchronous pipeline and dispatch
   orchestration at bounded batch boundaries, plus browser-device limits, adapter
   policy and new parity/performance tests. A dedicated async GPU worker with a
   bridge to Rust is one possible implementation; the search core need not become
   asynchronous. Simply enabling `wgpu`'s web feature
   cannot make that blocking backend work. The existing native GPU path accelerates
   table generation; GPU search itself is not implemented in the desktop app either.
2. **Memory above 4 GiB:** This target is wasm32. Its configured linear-memory
   maximum is 4 GiB shared by code data, stacks, tables and all search workers;
   individual Rust allocations are also bounded by `isize::MAX` (about 2 GiB).
   More host RAM does not remove those limits. A memory64 target or sharded
   tables/search across independent WASM memories requires changes to table
   ownership, indexing and search kernels, plus browser/toolchain validation.
3. **Large active tables:** Saved files stay in IndexedDB. A dedicated storage
   worker reads 16 MiB chunks on demand while the Emscripten host services Rust's
   synchronous file reads. Startup loads metadata, not entire cache files; newly
   saved file buffers are evicted after the manifest commits. RAM unload releases
   Rust table allocations while preserving saved files. The grown WASM heap can
   remain reserved for reuse; unloading does not shrink the memory object. Reloading
   recreates that heap. Loading a table for search
   still requires that table to fit into the solver's wasm32 address space.
   A bounded 16 MiB shared read buffer remains available for later reads. Replacing
   a saved file temporarily needs disk space for both versions. Searching tables
   larger than the solver address space needs paged or sharded search data.
4. **Unlimited threads:** Browsers and the OS enforce worker/resource limits.
   Additional threads also consume the same finite WASM address space. The native
   solver already has algorithmic parallelism limits; lifting these needs new
   work splitting, not just a larger UI input. This build preserves the existing
   scheduler and accepts its existing thread settings.
5. **Resume after closure:** Continuous execution after tab closure is not a
   requirement. Batch Pause saves resumable DFS checkpoints through the existing
   IndexedDB project autosave. Wait until pausing and saving finish before closing.
   Closing an active search abruptly preserves previously saved results/checkpoints,
   but can lose work since the last checkpoint. Single solves and unfinished table
   generation do not currently have resumable checkpoints.

The relevant platform constraints are documented by
[Rust's Emscripten target](https://doc.rust-lang.org/rustc/platform-support/wasm32-unknown-emscripten.html),
[Emscripten pthreads](https://emscripten.org/docs/porting/pthreads.html), and
[Emscripten memory settings](https://emscripten.org/docs/tools_reference/settings_reference.html).

## Verification

```sh
cd main
cargo test -p fto-web --lib
cargo check -p optimal-fto-solver-app
# After ./build.sh --web; use an installed Chrome/Chromium binary:
CHROME_PATH=/usr/bin/google-chrome npm run test:web
```

The browser integration test generates a real transition/Bloom cache, saves it,
restarts the worker, verifies restoration and deletion, searches a partial goal,
runs eight batch cases with eight threads, and checks cancellation. A separate
static-server test checks the service-worker isolation fallback without headers.
The first test can take several minutes because it builds the transition table.
