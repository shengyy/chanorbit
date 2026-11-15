# Changelog

## [Unreleased]

### Added

- Structure engine: inclusion, fractals, strokes with withdrawal and retention, same-time revision, canonical stroke centres, MACD(12, 26, 9) interval power, candidate stroke and the geometry snapshot.
- `chanorbit geometry` command: one JSON object of columnar bars (`dt` as `YYYY-MM-DDTHH:MM:SS`, positive prices up to 1e9, volume up to 1e15, at most 200,000 bars) with non-decreasing `dt` (adjacent equal times are revisions, a decrease is an error), per-line errors including non-UTF-8 lines, single request or JSON Lines batch on stdin, deterministic output order; one geometry snapshot per request with each fact once; schema `chanorbit.geometry/1`.
- Regression baselines in `tests/golden/` and an end-to-end stdin→stdout test.
- `chanorbit stages` command (schema `chanorbit.stages/1`): replays the request one input at a time and answers with the confirmed buy-side second, third and second-like stages of the last input — centre, role strokes, first-known time, age in observed inputs, whether the move after the retest has passed the retest's start, and the withdrawn rebound stroke from the retest with when it was seen and withdrawn; optional `observe_from` (a string; `null` is refused) starts observation later, and `geometry` refuses the field, `null` included. A time revised within one request is observed once, after its last version. `--version` lists both schemas.
- Stage regression baselines in `tests/golden/stages/`: each kind, after-retest states, no stage, first-known and observation-start boundaries, first time unknown at the observation start for each kind, revisions, more second-like series, retest low exactly at and one tick above the guard.
