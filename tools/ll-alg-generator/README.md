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

- Moves are primitive only: `U U' F F' BR BR' BL BL' D D' B B' R R' L L'`.
- Before the first `R`/`F` move, only `U U'` may appear.
- The first non-`U`/`D` move must be `R`, `R'`, `F`, or `F'`.
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
```

`--center-mode exact` is the default and treats center pieces as distinct.
`--center-mode color` allows same-colored centers outside the U-affected slots
to swap.
