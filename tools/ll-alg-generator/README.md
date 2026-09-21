# LL alg generator

Generates primitive FTO move sequences that only affect pieces moved by a solved
`U` turn.

From the repository root:

```sh
./generate_ll_algs.sh 11 ll_algs_11_or_under.txt
./generate_ll_algs.sh 12 ll_algs_12_or_under.txt
```

The output is one algorithm per line.

Rules baked in:

- Moves are primitive only:
  `U U' F F' BR BR' BL BL' D D' B B' R R' L L' Uw Uw' Fw Fw' Rw Rw' Lw Lw'`.
- Wide moves are included by default. Use `--no-wide-moves` to exclude them,
  or `--only-wide-moves` to use `U/U'` plus wide moves only.
- Before the first counted move, only `U U'` may appear.
- The first counted move must be `R`, `R'`, `F`, `F'`, `Uw`, `Uw'`, `Fw`,
  `Fw'`, `Rw`, `Rw'`, `Lw`, or `Lw'`.
- `Uw`/`Uw'` are normal counted moves, not free AUF.
- Final `U`/`U'` is omitted by default because LL AUF is free.
- Same-face repeats and commuting move-order duplicates are skipped.
- If two algs are identical after stripping leading/trailing `U`/`U'`, only
  the first one is printed.
- `U alg U'` and `U' alg U` case duplicates are skipped, including forms
  where those `U` turns cancel into the algorithm text.

Useful options:

```sh
cargo run --release --manifest-path tools/ll-alg-generator/Cargo.toml -- --help
./generate_ll_algs.sh 12 ll_algs_12_color_centers.txt --center-mode color
./generate_ll_algs.sh 12 ll_algs_exact_12.txt --exact-depth 12
./generate_ll_algs.sh 12 ll_algs_keep_trailing_auf.txt --allow-trailing-u
./generate_ll_algs.sh 12 ll_algs_12_or_under.txt --threads 8
./generate_ll_algs.sh 11 ll_algs_exact_11.txt --exact-depth 11 --seed ll_algs_10_or_under.txt
./generate_ll_algs.sh 11 ll_algs_exact_11.txt --exact-depth 11 --resume
./generate_ll_algs.sh 11 ll_algs_no_wide_11.txt --exact-depth 11 --no-wide-moves
./generate_ll_algs.sh 11 ll_algs_only_wide_11.txt --exact-depth 11 --only-wide-moves
```

`--center-mode exact` is the default and treats center pieces as distinct.
`--center-mode color` allows same-colored centers outside the U-affected slots
to swap.

Accepted algs are flushed to disk immediately. Use `--resume` after an
interrupted run to append to the same output file and avoid reprinting algs that
are already there. Use `--seed` to dedupe a later exact-depth run against an
earlier file.
