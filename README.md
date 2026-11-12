# Optimal FTO Solver

FTO Alg Finder is a desktop app and Rust solver workspace for the Face-Turning Octahedron.

Created by **Abid Ibn Ashraf**. Many later contributions were made by Matt.

## License

This project is licensed under the **GNU General Public License v3.0**. See [`LICENSE`](LICENSE) for the full license text.

Plain-English summary:

- You may use, study, modify, and redistribute this code.
- You may use it in commercial or non-commercial projects.
- If you distribute this code, a modified version of it, or a project based on it, that distributed project must also provide source code under GPL-compatible terms. In other words: do not put this solver into a closed-source distributed app.
- Keep the copyright, license, and creator attribution notices. Credit **Abid Ibn Ashraf** as the creator of the project.
- Modified versions should clearly say what changed.
- The software is provided without warranty.

This summary is only an explanation. The GPL v3 license text is the legal source of truth.

## Core Algorithm

The heart of the solver is an **iterative-deepening depth-first search (IDDFS)** over FTO coordinate states, accelerated by generated transition tables and pruning tables.

The solver stores the puzzle in cubie/coordinate form, precomputes move transitions, and then searches depth by depth until it finds a solution at the requested depth. For deeper exact-depth CLI searches, there is also a bidirectional search path that builds backward paths from solved state and matches them from the start side.

Important implementation pieces:

- `main/crates/fto-core/src/search.rs`: search logic, threaded IDDFS, last-layer search, bidirectional search, solution formatting.
- `main/crates/fto-core/src/pruning.rs`: pruning-table generation and loading.
- `main/crates/fto-core/src/tables.rs`: transition-table generation and loading.
- `main/crates/fto-core/src/cubie.rs`: FTO cubie state.
- `main/crates/fto-core/src/coord.rs`: compact coordinate representation.
- `main/crates/fto-cli/src/main.rs`: command-line interface and cache/pruning tooling.
- `main/src-tauri/src/lib.rs`: desktop app bridge between the React UI and the release CLI solver.

## Features

Solver app:

- 3D graphical FTO input. Manually color in the pieces, or swap the pieces and twist the corners to do the input.
- Text input for setup moves or algorithms.
- Print-style depth selection: `12-15, 17, 20` searches those exact depths, sorted and deduplicated (0–255). With All solutions enabled, results include every selected depth; otherwise search stops at the first successful depth. Empty input or `auto` finds the first successful depth and returns one or all solutions at that depth.
- Option to return one solution or all found solutions.
- App zoom: **Ctrl+=** / **Ctrl+-**, with **Ctrl+0** to reset. The selected zoom level is remembered.
- Configurable thread count. (Not implemented perfectly, max thread count is the max number of allowed moves)
- Multiple solver instances saved in local storage to make sure of better and more strict pruning (faster solution) and to eliminate unwanted moves.
- Ability to choose the moveset you want the solution to be confined in.
- Supports base moves, wide moves, slice moves, and bundled trigger moves exposed by the UI.
- Restricted pruning mode, where pruning is built around the selected instance move set.
- Last-layer mode: Separates the center piece that goes to the top layer and the center that goes to bottom so that the solution that the app generates is AUF independent.
- Right click on a piece on the graphical FTO to do Partial-position input.
- If only one center is partially defined of a color, the ability to choose exactly where the center should end up after the solve.

Bloom tables are available under **Menu → Pruning → Bloom tables**. Choose the
moves, depth (1–20), and a memory budget in MiB, then generate. There is no fixed
512 MiB ceiling: for example, 12 GiB is 12,288 MiB. Allocation is checked against
currently available system RAM, with headroom for the app and OS. Size estimation
is not included yet. Generation runs in the background with progress and can be
cancelled with **Stop**. If the table fills up, generation stops without saving an
incomplete table or replacing an existing one.

**Use GPU acceleration when available** applies to both generators and to table
builds needed by subsequent solves. The native compute path uses wgpu (Vulkan,
Metal, DX12, or compatible OpenGL), prefers a discrete GPU, and falls back to CPU
when unsupported. Small jobs stay on CPU; larger jobs compare CPU/GPU throughput
and switch to CPU if the pilot favors it. GPU failure or timeout discards the GPU
attempt rather than saving incomplete data.

GPU dispatch sizes adapt to measured completion time (an 8 ms target on integrated or
unclassified graphics, 16 ms on discrete GPUs), with brief scheduling gaps for the desktop.
Working buffers obey device binding limits and a conservative memory budget.
Bloom filters that fit that budget are built directly on the GPU and read back
at completion. Larger filters stay in system RAM and use bounded GPU hash batches,
so a multi-GiB filter does not require matching VRAM. This is a scheduling target, not a hardware power or utilization
limit. The option can be disabled to force CPU generation. Cache contents and
search results are independent of the generation backend.

Hardware parity tests (require the existing transition cache and a GPU):

```sh
cd main
cargo test -p fto-core gpu_ -- --ignored --nocapture --test-threads=1
```

A CPU-only build remains available with `cargo check -p fto-core --no-default-features`.

Select **Last-layer mode** (enabled by default) to build a table for the last-layer
solver's goal: free leading/trailing U or U′, the distinct UF center groups, and the
three fixed RL center identities. Depth counts the moves between the free U
adjustments. These adjustments work even when U is not in the chosen move set.
Uncheck it to build a full-position table instead.

Completed tables are saved by goal mode, move set, and depth. Searches load the
deepest compatible saved table automatically, including tables covering a superset
of the search's allowed moves. These supplement the existing pattern tables;
full-position and last-layer caches are never mixed. Partial-position searches
keep their existing pruning. Manage saved Bloom tables in **Settings → Manage
storage**, or use **Unload pruning table from RAM** to release the loaded tables.
Bloom membership is checked when the remaining search depth reaches the saved
radius; a possible match still undergoes normal search and the mode's exact goal
check.

### Batch solving

Choose **Menu → Switch to batch solving**. Enter one setup per numbered line;
multiline paste adds cases or replaces the current selection. Select across lines
and press **Backspace** or **Delete** to remove them; undo/redo restores edits,
including visual cases. Untouched cases retain their saved results and search
checkpoints. Press **Tab** on a setup to open the visual editor,
including paint, swap, ignored pieces, center targets, and LL setup controls.
Saved visual entries appear as blue **Case N** links that reopen the editor.
The right sidebar holds the shared moves, print-style depth selection, threads,
All solutions, and Last layer mode.

The scheduler searches selected depths in ascending order across cases. A case
with its first solution finishes that depth and waits while other cases find
their first solutions. With **All solutions** and explicit depths, it then
exhausts the remaining selected depths for every case. Cases with no solution
within the selected depths, or invalid input, do not block the remaining work.
With **auto**, each case stops after its first successful depth; single-solution
mode returns one solution per case. Threads run different cases first; spare
threads use the existing parallel search within a case. Shared pattern/Bloom
tables are reused across the batch.

**Defer after (seconds)** defaults to 10; set it to 0 to disable it. In All
solutions mode, a slow first-pass search is interrupted and held at its current
depth. Other cases, including parked ones, exhaust their selected depths up to
that depth before the deferred cases resume last. Catch-up searches can also be
held if they exceed the budget. Deferred continuations run without the time limit so
they can finish. No depth is marked completed
on timeout, and discovered solutions are retained. This catch-up also applies to
All solutions with auto depth. It changes search order, not solvability: a hard
case can still take a long time once everyone has caught up.

**Pause** keeps completed depths, discovered solutions, and versioned DFS stack
checkpoints for each unfinished depth. Resume continues the pending branches;
only the active paths are reconstructed, with no completed subtree searched
again. Checkpoints support full-position, last-layer, and partial-position
searches, including free U ends and center targets. They survive project saves
and JSON export/import, and can resume with a different thread count. While paused, add/remove
cases or edit individual cases; unchanged cases keep their checkpoints. Changing
the search settings resets results and search stacks, except when changing only the thread count or deferral budget.
Projects autosave locally in IndexedDB and survive app restarts. **Download
project** creates a portable checkpoint; **Open project** restores one.
**Export CSV** writes one row per solution, including setup/state, case number,
depth, search settings and progress; unsolved/invalid cases are included too.
Desktop exports go to the Downloads directory without overwriting existing files.

Batch checks:

```sh
cd main
npm run test:batch
cargo test -p optimal-fto-solver-app --lib batch::tests
# Integration check with the existing cache/transition-table.tran:
cargo test -p optimal-fto-solver-app --lib batch::tests::real_search -- --ignored
```

CLI solver:

- Reads a scramble from `--scramble`, JSON from `--json`, JSON from stdin, or solved state by default.
- Supports `--depth` for a maximum depth.
- Supports `--exact` for exact-depth searches.
- Supports `--all` to collect all solutions instead of stopping at the first.
- Supports `--threads`.
- Supports `--moves` to restrict the active move set.
- Supports `--instance-moves` for instance-specific pruning/caches.
- Supports `--ban` for quick move exclusion.
- Supports `--last-layer`.
- Supports `--restricted-pruning`.
- Supports `--no-pruning`.
- Supports automatic transition-table generation/loading.
- Supports automatic pruning-table generation/loading.
- Supports pruning candidate evaluation with `--eval-pruning`.
- Supports automatic pruning candidate search with `--auto-pruning`.
- Supports benchmark mode with warmups and median/mean timing output.
- Supports bidirectional exact-depth search for deeper CLI searches.
- Supports start-centered bidirectional pruning.
- Allows tuning bidirectional memory with `--bidir-max-mib`.
- Allows tuning pruning memory, component count, candidate count, combo size, sample count, sample walk length, output directory, and progress interval.

Alg combiner:

- Separate Combiner mode in the app.
- Applies a scramble to the 3D puzzle.
- Accepts named intermediate algorithms, one per line.
- Tries single algorithms, ordered pairs, and optional triples.
- Inserts optional `U`/`U'` slots around algs while searching combinations.
- Cancels adjacent moves in found combinations.
- Reports progress and found combinations live.
- Sorts found combinations by final move count.
- Lets a found combination be applied back to the viewer.
- Includes resettable default intermediate algorithms.

## Limitations

- This is a **fixed-orientation FTO solver**. It can solve optimally to a specific target orientation.
- It does **not** search every orientation-neutral solved state and then choose the globally shortest orientation-neutral solution. The current solver model is focused on generating algorithms and orientation neutrality is not very necessary for that, and adds additional complexity and potential speed-loss.
- Search cost rises quickly as the allowed move set grows.
- Search cost also rises quickly as the requested depth increases.
- The pruning table is large. Searches can require substantial memory and disk cache.
- Restricted solver tables use compact coordinate orbits when they substantially reduce RAM. Move tables precompute compact pruning indices, keeping heuristic reads direct; generation uses the reduced spaces too. Existing cache files remain compatible. Large coordinate orbits retain dense storage.
- Full-state searches also reuse those tables after a half turn that exchanges the UF and RL center orbits. A small corner mapping and two additional coordinates let the unused orbit strengthen pruning without another large table. This is enabled only when the table's move set preserves the rotation (including the standard faces plus Fw/Rw); last-layer goals and arbitrary start-centered tables keep their existing bounds.
- Because the app is divided into solver instances, different instance move sets may need different pruning-table data. In practice, that can mean more than one set of pruning tables on disk.
- The GUI links the Rust solver core directly, so desktop builds produce a single app binary.
- Very deep exact searches may need bidirectional memory measured in GiB.

## System Requirements

Minimum practical setup:

- 64-bit Linux, Windows, or macOS capable of running Tauri 2 apps.
- Rust toolchain with Edition 2024 support.
- Node.js 18 or newer.
- npm.
- 8 GiB RAM for normal shallow/moderate searches. At least 1.2 GiB dormant/ready to access RAM just to load the pruning tables and transition table comfortably.
- 2 GiB free disk space for dependencies, build output, and normal caches.

Recommended for heavier use:

- 5-10 GiB free disk space if you plan to build several instances and pruning caches or run experiments.
- Multiple CPU cores if using `--threads` or the app thread setting.

Notes:

- Default bidirectional CLI memory is controlled by `--bidir-max-mib` and defaults to 3072 MiB.
- Pruning cache size depends on the selected move set and pruning settings.
- The app stores generated cache files under `main/cache/` when run from this workspace.

## Downloads and releases

Get installers from the [latest GitHub release](https://github.com/Abid-speedcuber/FTO-alg-finder/releases/latest).
The landing page links directly to version-independent asset names under
`releases/latest/download/`, so new stable releases need no website edits.

To prepare the release downloads:

```sh
./build.sh --all
```

Check the latest published tag:

```sh
./release --current
```

After committing and pushing the source used for the builds, publish a new tag:

```sh
./release v1.0.0.209
```

`./release.sh` accepts the same commands. The script verifies every installer
against `builds/release/SHA256SUMS`, uploads the assets as a draft, then publishes
the release and marks it latest. Existing tags are never overwritten. Sign in
with `gh auth login` first. It does not rebuild, change app version metadata,
commit, or push source. The landing-page links automatically follow the latest
published release.

Windows uses `FTO-Alg-Finder-windows-x64.exe`; Linux uses
`FTO-Alg-Finder-linux-x86_64.AppImage`, `FTO-Alg-Finder-linux-amd64.deb`, and
`FTO-Alg-Finder-linux-x86_64.rpm`. Arch users can use the AppImage.

## Build From Source

From a fresh clone, enter the app workspace:

```sh
cd main
```

Install JavaScript dependencies:

```sh
npm install
```

Run the desktop app in development mode:

```sh
npm run tauri dev
```

Build the web frontend only:

```sh
npm run build
```

Type-check the frontend:

```sh
npm run check
```

Run the CLI directly:

```sh
cargo run --release -p fto-cli -- --scramble "U F R" --depth 8 --all
```

Show CLI help:

```sh
cargo run --release -p fto-cli -- --help
```

## Project Layout

```text
main/
  crates/
    fto-core/      Rust solver library
    fto-cli/       command-line solver and tooling
  src/             React frontend
  src-tauri/       Tauri desktop wrapper
  public/          FTO viewer assets
  cache/           generated tables and pruning data, created at runtime
```

## Cache Files

The solver builds tables on demand:

- `main/cache/transition-tables-v4.bin`
- `main/cache/pruning-v4/`

These files are generated artifacts. They can be deleted, but the next run may take time and disk space to rebuild them.

## Development Notes

- The workspace uses Rust lints with `unsafe_code = "forbid"`.
- The frontend is React + Vite + TypeScript.
- The desktop shell is Tauri 2.
- The CLI remains available for command-line solving and cache/pruning tooling, but the GUI no longer shells out to it.
- Keep solver changes benchmarked; small coordinate or pruning changes can have large performance effects.
