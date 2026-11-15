# Test status

Version 0.1.0, schema `chanorbit.geometry/1`. This document records what this repository's own tests and builds cover. Gate commands are in [CONTRIBUTING.md](../CONTRIBUTING.md).

## Covered

| Area | Evidence |
|---|---|
| Regression baselines | 15 synthetic series in `tests/golden/` (5 to 1920 inputs, 1.0 MiB compressed): daily random walk, long up and down trends, narrow range with flat bars, one-price limit bars and opening gaps, same-time revisions (daily and 5m), very short series (no stroke, MACD warm-up), retention with `max_bi_num = 5` plus revisions, `min_bi_len = 4`, 30m, 5m (1920 bars, retention at 50 strokes) and 1m session labels, and a 1m NewBar merging 103 raw bars whose price owner is displaced by the element cap |
| Per baseline | Snapshot after 5–15 input prefixes and the final response, every field; one state row per input; final NewBars of every stroke and of the unfinished window with their raw elements; retained raw times; batch equals incremental; default-parameter baselines also through the request text |
| Numeric match | All baselines pass with every MACD-derived value bit-identical (the 1e-9 relative allowance is unused); all other numbers equal |
| Revision and time order | Feeding every same-time revision equals feeding only the last version, including when revisions undo retention pruning and stroke withdrawal; adjacent equal times through the contract equal the last version; a decreasing time and a non-adjacent repeat are `invalid_bar` |
| Element cap | A NewBar keeps its first 100 and latest raw bars; `confirmed_at` is unchanged and the candidate reports `anchor_price_source_missing` when the owner was displaced |
| CLI | Real binary: single request against a baseline, batch of the default-parameter baselines with malformed, short-column and unknown-field lines, input order, error isolation, identical bytes for `--jobs 1` and `--jobs 8`, `--version`, exit codes 0/1/2, zone-suffixed time rejected |
| Builds | `aarch64-apple-darwin` release binary (links only `libSystem`); `x86_64-unknown-linux-musl` static-pie binary built in a `linux/amd64` container and run there under emulation, producing byte-identical batch output to the macOS binary on a local 1677-request batch (not committed) |

## Not covered

- Non-finite prices or closes. JSON cannot carry them, so the `non_finite` reasons and the non-finite candidate path are reachable only through the library and have no test.
- `max_bi_num` other than 5 and 50, `min_bi_len` other than 4 and 6 (library only; the command line uses 50 and 6).
- Native execution on a Linux x86_64 host; only container emulation was run. Windows is not a target.
- Performance has no threshold or test; `cargo run --release --example bench` gives local readings only.
