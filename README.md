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
- Exact-depth solving when a depth is supplied. Incremental solving when depth is left as `auto`.
- Option to return one solution or all found solutions.
- Configurable thread count. (Not implemented perfectly, max thread count is the max number of allowed moves)
- Multiple solver instances saved in local storage to make sure of better and more strict pruning (faster solution) and to eliminate unwanted moves.
- Ability to choose the moveset you want the solution to be confined in.
- Supports base moves, wide moves, slice moves, and bundled trigger moves exposed by the UI.
- Restricted pruning mode, where pruning is built around the selected instance move set.
- Last-layer mode: Separates the center piece that goes to the top layer and the center that goes to bottom so that the solution that the app generates is AUF independent.
- Right click on a piece on the graphical FTO to do Partial-position input.
- If only one center is partially defined of a color, the ability to choose exactly where the center should end up after the solve.

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
- Because the app is divided into solver instances, different instance move sets may need different pruning-table data. In practice, that can mean more than one set of pruning tables on disk.
- The GUI launches the release `fto-cli` binary. Build the release CLI once before expecting the desktop app to solve.
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

## Build From Source

From a fresh clone, enter the app workspace:

```sh
cd main
```

Install JavaScript dependencies:

```sh
npm install
```

Build the release solver binary:

```sh
cargo build --release -p fto-cli
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
- The app expects `main/target/release/fto-cli` to exist when solving from the GUI.
- Keep solver changes benchmarked; small coordinate or pruning changes can have large performance effects.
