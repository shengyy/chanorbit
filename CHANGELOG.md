# Changelog

## [Unreleased]

### Added

- Structure engine: inclusion, fractals, strokes with extension and retention, same-time revision, canonical stroke centres, MACD(12, 26, 9) interval power, candidate stroke and the geometry snapshot.
- `chanorbit geometry` command: one JSON object of columnar bars (`dt` as `YYYY-MM-DDTHH:MM:SS`, positive prices up to 1e9, volume up to 1e15, at most 200,000 bars) with non-decreasing `dt` (adjacent equal times are revisions, a decrease is an error), per-line errors including non-UTF-8 lines, single request or JSON Lines batch on stdin, deterministic output order; one geometry snapshot per request with each fact once; schema `chanorbit.geometry/1`.
- Regression baselines in `tests/golden/` and an end-to-end stdin→stdout test.
- `pyproject.toml`: the executable as a maturin `bin` wheel, installable with `uv pip install` or as a git dependency.
- `chanorbit stages` command (schema `chanorbit.stages/1`): replays the request one input at a time and answers with the confirmed buy-side second, third and second-like stages of the last input — centre, role strokes, first-known time, age in observed inputs, whether the move after the retest has passed the retest's start, optional `observe_from` (a string; `null` is refused) starts observation later, and `geometry` refuses the field, `null` included. A time revised within one request is observed once, after its last version. `--version` lists both schemas.
- `chanorbit levels` command (schema `chanorbit.levels/1`): one instrument, several period series coarsest first, bars labelled by the end of their period. Per level: a stroke window, raw and DIF extremes, the unfinished segment and the path after its extreme, up to five recent centres with last-close position and state (`departed`, `inside`, `formed_out`) read from the raw path after the third member — an ended centre's state is the fixed record of how it was left, with its retest's role in the next centre — and same-direction comparisons. Each finer level is placed in the next coarser one: child times of the parent's points (last stroke, unfinished segment, extension, path after the extreme), whether the child has passed them, child forces and zones; a child series that starts inside the parent's last stroke is still zoned and says so. `--version` lists the third schema.
- Levels regression baselines in `tests/golden/levels/`.
- Stage regression baselines in `tests/golden/stages/`: each kind, after-retest states, no stage, first-known and observation-start boundaries, first time unknown at the observation start for each kind, revisions, more second-like series, retest low exactly at and one tick above the guard.

### Changed

- Strokes (version 0.1.1, `chanorbit.stages/2`): a stroke's start is the most extreme raw price between its start fractal and its end fractal, so a low or high that the inclusion merge hid becomes the endpoint the previous stroke ends at; appending a stroke never carries the previous one across a price beyond its start (the last strokes are searched again from earlier); a last stroke whose end is exceeded is searched again from its start and replaced by the further end, or stays as it is (`kind = extension`) — it is no longer withdrawn; when a raw bar after its end has already passed its start, the last two strokes are searched again as one, so the earlier stroke ends at the true extreme. `geometry` and `levels` fields keep their meaning (schemas unchanged), and their values follow the new strokes. `stages` drops `withdrawn_rebound`, which the new strokes can no longer produce. Baselines regenerated; the stage baseline `rebound_withdrawn` and the two withdrawn-rebound cases are removed, and one transformed levels case is cut earlier to keep a departure in the extension.
- Input contract: a bar with `high < low`, or with `close` outside `[low, high]`, is an `invalid_bar` error naming the field and index (equal ends stay valid).
- `rust-toolchain.toml` no longer lists clippy and rustfmt, so building the binary from source fetches only the minimal toolchain; the gates need `rustup component add clippy rustfmt`.

### Fixed

- A panic while computing one request becomes that request's `internal_error` (exit code 1) for every command, single and batch; the other lines are answered as before, in order. The release profile states `panic = "unwind"`.
- A stdout that can no longer be written, including a reader that closed the pipe, exits with code 3 without a panic, also for `--version` and `--help`; a closed stderr is ignored.
