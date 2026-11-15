# Test status

Version 0.1.0, schemas `chanorbit.geometry/1` and `chanorbit.stages/1`. This document records what this repository's own tests and builds cover. Gate commands are in [CONTRIBUTING.md](../CONTRIBUTING.md).

## Covered

| Area | Evidence |
|---|---|
| Regression baselines | 15 synthetic series in `tests/golden/` (5 to 1920 inputs, 1.0 MiB compressed): daily random walk, long up and down trends, narrow range with flat bars, one-price limit bars and opening gaps, same-time revisions (daily and 5m), very short series (no stroke, MACD warm-up), retention with `max_strokes = 5` plus revisions, `min_stroke_bars = 4`, 30m, 5m (1920 bars, retention at 50 strokes) and 1m session labels, and a 1m NewBar merging 103 raw bars whose price owner is displaced by the element cap |
| Stage baselines | 20 synthetic daily cases in `tests/golden/stages/` (176 to 250 inputs, plus 3 and 40): second, third and second-like stages; `beyond_start`; a withdrawn rebound with its times; a stage at the input it first stands and one input earlier; `observe_from` on that input (first time unknown) and one input before it (known); retest low exactly at the path guard (no stage) and one tick above (stage); no stage, no stroke and MACD warm-up. Compared strictly, every field |
| Stage rules | Inserting an earlier different version before every fifth input leaves the stages unchanged; `observe_from` after the last input leaves every first time and age `null`; a malformed `observe_from` and one in a `geometry` request are `invalid_request`; `stages` errors carry the stages schema |
| Per baseline | Snapshot after 5–15 input prefixes and the final response, every field; one state row per input; final NewBars of every stroke and of the unfinished window with their raw elements; retained raw times; batch equals incremental; default-parameter baselines also through the request text |
| Comparison | Strict: same keys and lengths, same JSON types (an integer never equals a float, a boolean never equals a number), floats equal bit for bit, no tolerance. All baselines pass |
| Revision and time order | Feeding every same-time revision equals feeding only the last version, including when revisions undo retention pruning and stroke withdrawal; adjacent equal times through the contract equal the last version; a decreasing time and a non-adjacent repeat are `invalid_bar` |
| Element cap | A NewBar keeps its first 100 and latest raw bars; `confirmed_at` is unchanged and the candidate reports `anchor_price_source_missing` when the owner was displaced |
| Input contract | Only `YYYY-MM-DDTHH:MM:SS` parses (space, bare date, single-digit hour, padding, fraction, zone, leap second and invalid dates rejected); every output time in the baselines has the same format; prices and volumes out of range are `invalid_bar` naming field and index; a positional array or other non-object JSON and a request over 200,000 bars are `invalid_input` |
| CLI | Real binary: single request against a baseline, batch of the default-parameter baselines with malformed, short-column and unknown-field lines, input order, error isolation, identical bytes for `--jobs 1` and `--jobs 8` (both commands); a non-UTF-8 batch line fails alone (exit 1) and a non-UTF-8 single request is an error object; empty and whitespace-only batches exit 0 with no output; `--jobs` without `--batch` and `--jobs 0` exit 2; `--version`/`-V`, `-h`; zone-suffixed time and unknown field rejected |
| Builds | `aarch64-apple-darwin` release binary (links only `libSystem`); `x86_64-unknown-linux-musl` static-pie binary built in a `linux/amd64` container and run there under emulation, producing byte-identical `geometry` and `stages` batch output to the macOS binary on a local 1677-request batch (not committed) |

## Not covered

- The engine's `non_finite` reasons and the non-finite candidate path. The command line rejects such input before computation, so they are reachable only through the library, and have no test there.
- `max_strokes` other than 5 and 50, `min_stroke_bars` other than 4 and 6 (library only; the command line uses 50 and 6).
- `stages` baselines are synthetic daily series only; no intraday or real-data stage case is committed.
- Native execution on a Linux x86_64 host; only container emulation was run. Windows is not a target.
- Long series that form no stroke (quadratic time, see [architecture.md](architecture.md#data-flow)) have no timing test. Performance has no threshold or test; `cargo run --release --example bench` gives local readings only.
