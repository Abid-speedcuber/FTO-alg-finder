# Pruning-read ablation — 2026-11-12

The slowdown is real and configuration-dependent. Batching was not a general win. Early exit alone also did not consistently improve the app's six-move last-layer searches.

The retained change enables early exit **only for dense full-position SolverPruning searches**. The choice is made outside recursion, with separate compiled loops; compact-table searches retain their unconditional maximum. The app's LL PatternDatabases path, LL resume kernel, and partial solver are unchanged. The previous empty-coordinate guard remains. Batching and prefetch remain reverted.

## Original four-variant sweep

34 state/configuration cases: 6 full/default-20, 14 LL/default-20, and 14 LL/six-move. Two six-move inputs are immediately rejected at the root; they are correctness checks and excluded from timing aggregates. Each case ran once to warm up and three measured times, in each of four separately compiled static variants: 544 searches. Loading and algorithm-file writes are outside the timer. The complete ordered algorithm files and every node count matched; the checked-in rows also include deterministic algorithm hashes.

| Variant | Full, 20 moves: 6 cases | LL, 20 moves: 14 cases | LL, 6 moves: 12 timed cases |
|---|---:|---:|---:|
| Batching only | 34.8% less time | 35.9% less | **57.8% more** |
| Early exit only | 22.0% less | 36.6% less | **10.0% more** |
| Both | 38.8% less | 26.9% less | **57.2% more** |

These are geometric means of per-case median wall-time ratios, not universal promises or rates to add together. Individual results matter: the LL/default-20 early-exit sweep includes one regressing case, and the combined variant regresses on two default-20 mixed cases. The desktop was running normally, so small effects need caution.

The actual app LL entry point is `solve_last_layer_with_pattern_pruning_threads`, with the edge3+llufA and corner+llufB table pair. The full entry point is `solve_with_pruning_threads`, with SolverPruning. This corrects the earlier CLI report's use of the full SolverPruning pair to estimate app LL performance. No GUI timing was substituted for search timing.

## Photographed position

Only RL centres differ from solved. Viewer facelets 65 and 68 are grey; 56 and 61 are orange. In cubie coordinates, `cp`, `co`, `ep`, and `uf` are solved and `rl = [0,1,2,3,4,5,9,11,8,6,10,7]`. This preserves the fixed LL centre identities while exchanging the two orange/grey top centres.

Using the existing U/U′, F/F′, Fw/Fw′ LL tables:

| Variant | Median search time at depth 16 |
|---|---:|
| Baseline | 7.299 s |
| Batching only | 9.759 s |
| Early exit only | 6.881 s |
| Both | 9.999 s |

All four visit **9,741,371 nodes** and return no algorithms at that depth. This is a fixed-depth comparison of the photographed position, not its complete automatic-depth solve. It reproduces the slowdown without claiming that an algorithm was found. Early exit's small benefit here does not override the regressions on other six-move LL states.

## Validation and limits

The retained source is retested separately, including six additional full-position cases using the six-move compact tables. Wall time and Linux process CPU time are recorded for confirmation because desktop scheduling made some shorter wall timings fluctuate substantially. Final results are in `selected-verification.json`. The selected dense/full loop used **7.2–16.7% less process CPU time in all six default-20 cases**, geometric mean 10.8%. The earlier full-only confirmation measured 13–29% lower wall time, but the last run was heavily interrupted by other desktop activity: process CPU time decreased while wall time increased on several cases. These metrics must not be conflated; there is no claim that every app run will finish 10.8% earlier.

Compact six-move searches use the baseline compiled loop (the bounded-read choice is made once before recursion). Their timing differences are not presented as an optimization win; small-table confirmations were noisy. The app LL contexts/enum and LL resume kernel were checked byte-for-byte against the baseline source. Compact traversal, all ordered algorithms and all node counts match as well.

All builds use Rust release optimization, one search thread, no GPU feature, and the same compiler/settings. GPU table generation is outside this experiment. Core tests cover rejection equivalence, exact bounds on surviving branches, byte-255 sentinels, and skipping a second read. Existing full/LL/partial pause/resume integration tests are also run. Matching algorithms includes order, not just counts.

The two table instances are default 20 moves (all face turns plus Fw/Fw′ and Rw/Rw′) and the actual six-move instance. Six-move LL uses its dedicated existing tables, not a restricted benchmark pretending to represent unrestricted solving. No new large tables are generated. This does not establish performance for every possible move set, depth, machine, batch event callback, or partial goal. Partial search has no new optimization in this patch.

## Reproduction and artifacts

`build.py` reconstructs the four static builds from commits `78d6ae2` (baseline) and `ff61193` (the changes being isolated). Early-only disables batching at compile time; batch-only disables bounded-read short circuits at compile time. There are no runtime experiment switches and no other experimental optimizations. Source snapshots, binaries, original algorithm files and compiler logs are retained locally under `/tmp/fto-ablation-results-20261112`.

`harness.rs` is the four-variant wall-time harness; `harness_cpu.rs` adds process CPU timing for final confirmation. `run.py` loads the checked-in fixed depths, runs the suite, and verifies nodes and ordered algorithms. `measurements.json` and `summary.json` contain the original four-variant results. The seeds, state construction, allowed moves and exact entry points are in the harness.

Paths in these local reproduction scripts point to the current checkout and its existing caches. Adjust those paths on another machine; use the same caches and move instances. Compile the harness against each build with `rustc --edition=2024 -C opt-level=3 -L dependency=<target>/release/deps --extern fto_core=<variant.rlib> harness.rs -o <variant>`, then run `python3 run.py`. Do not time searches while compilation is running.
