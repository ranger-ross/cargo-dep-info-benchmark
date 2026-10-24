# Cargo dep-info collection benchmark

Benchmarks for [rust-lang/cargo#16693](https://github.com/rust-lang/cargo/issues/16693), comparing BTreeMap, sorted IndexMap, and HashMap with sort-on-write against an unsorted HashMap baseline.

The workspace includes 316 real rustc dep-info files and the extracted Cargo parser and binary encoder. A Cargo source checkout is not required for the default benchmarks.

## Run

Requires Rust 1.99 or newer. Python 3 is needed only for the helper scripts. The captured inputs and published measurements are from Linux x86-64.

```sh
cargo build --release --locked
./target/release/cargo-16693-bench --verify
./target/release/cargo-16693-bench --bench --save-baseline local
./target/release/cargo-16693-bench --interleaved
```

On Linux, prefix the benchmark commands with `taskset -c 0` to pin them to CPU 0, or choose another allowed CPU. Run without other CPU-intensive work.

To run only the Cargo parse-and-encode cases:

```sh
./target/release/cargo-16693-bench --bench --save-baseline local 'parse_encode/cargo/'
```

Criterion writes its reports under `target/criterion/`. The interleaved runner writes `results/local/interleaved.csv`. Generated results and build artifacts are excluded by `.gitignore`.

```sh
python3 summarize.py local
python3 summarize.py --interleaved results/local/interleaved.csv
```

Both commands write JSON summaries under `results/local/`. Re-running a command replaces its previous local output.

## What is measured

The cases use the same parsing and binary-encoding logic, with these collection choices:

- `hash_fx`: the current FxHashMap baseline, without sorting.
- `btree`: BTreeMap with ordered iteration.
- `index_fx_sort_load`: Fx-hashed IndexMap, sorted after parsing.
- `hash_fx_sort_write`: FxHashMap, sorting the existing encoded-files vector before serialization.
- `hash_std`, `index_std_sort_load`, and `hash_std_sort_write`: corresponding randomized standard-hasher cases for comparison with the original issue.

`parse_encode` measures parsing preloaded dep-info text and encoding it into Cargo's binary format. `cargo_stages` separates text parsing from decoding and reconstructing an encoded fingerprint. `corpus_316` processes all captured files once per iteration. Source files referenced by the inputs do not need to exist for these cases.

Criterion uses 50 samples, a 0.5-second warmup, and a requested 2-second measurement period per case. The interleaved runner uses 30 rounds, shuffles case order each round with a fixed seed, and calibrates batches to approximately 20 milliseconds. Its printed medians are medians of per-round times per operation.

The primary measurements exclude filesystem I/O, source-path canonicalization, and compilation. They retain all paths and environment entries, whereas Cargo filters some of these before writing fingerprints. The captured corpus has no checksum comments; checksum handling is checked separately. These timings do not measure end-to-end Cargo build performance.

Every run checks semantic equivalence across all variants and byte equivalence across the sorted variants. It also checks reversed dependency order, escaped paths, duplicate paths, checksum updates, and environment values. `--verify` runs these checks without timing.

## Captured inputs and results

`provenance.json` records the upstream revision, source checksums, extraction adaptations, and SHA-256 hashes of every fixture. The captured files contain the original machine's absolute paths. Those strings, including `capture-root.txt`, are intentionally preserved as benchmark data: rewriting them changes path comparisons and hashing. They are not local filesystem requirements for the default benchmarks.

`results/original/` contains the original Criterion estimates and samples, interleaved CSV, environment metadata, and eight-clean-build reproduction results. These measurements predate the standalone packaging. Packaging removed the local cargo-util dependency, made filesystem cases opt-in, and moved generated output under `results/local/`. The fixture bytes were not changed. The original filesystem cases used the capturing machine's checkout and are not portable timing baselines.

Inspect the original measurements with:

```sh
python3 summarize.py run1 --criterion-root results/original/criterion
python3 summarize.py --interleaved results/original/interleaved.csv
```

The original randomized-HashMap issue is distinct from input-order sensitivity. [Cargo #17169](https://github.com/rust-lang/cargo/pull/17169) changed these maps to FxHash. Eight clean builds with Cargo 1.99.0 produced identical metadata in the captured experiment. Reversing dependency order demonstrates order sensitivity, not a same-input clean-build reproducer.

## Optional filesystem cases

To include warm canonicalization of the Cargo fixture's source paths, supply a checkout matching the captured source layout:

```sh
DEPINFO_SOURCE_ROOT=/path/to/cargo ./target/release/cargo-16693-bench --bench 'warm_canonicalize_encode'
DEPINFO_SOURCE_ROOT=/path/to/cargo ./target/release/cargo-16693-bench --interleaved
```

The runner checks that every referenced Cargo source file exists before timing. Missing source files are errors rather than a fallback to a different workload. These cases still exclude dep-info disk reads and writes.

## Clean-build reproduction

```sh
python3 reproduce.py
```

This uses the Cargo executable on `PATH`, performs eight builds in a temporary workspace at the same source and target paths, and saves hashes and parsed rustc-cache contents to `results/local/reproduction.json`. Build directories are removed afterward. The script can be run repeatedly.

## Regenerate the extracted inputs

The extraction script is pinned to Cargo revision `cf29d0e0b1f7b03c4ceda5211cc4dc493bfc542e`. It checks source hashes before writing files.

```sh
cargo check --manifest-path /path/to/cargo/Cargo.toml --lib --locked --target-dir /path/to/corpus
python3 extract.py /path/to/cargo /path/to/corpus
```

This overwrites the extracted modules, fixtures, capture root, and provenance. It leaves the original results unchanged. Use `--output /path/to/fresh-directory` to extract into a separate directory instead. That directory contains extracted inputs only, not a complete benchmark workspace. A different toolchain or source location can produce a different corpus and timings.

## License

MIT OR Apache-2.0, matching the Cargo source from which the parser, encoder, checksum handling, and path-conversion helpers were extracted. See `LICENSE-MIT` and `LICENSE-APACHE`.
