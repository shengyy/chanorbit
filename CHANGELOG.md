# Changelog

## [Unreleased]

### Added

- Structure engine: inclusion, fractals, strokes with withdrawal and retention, same-time revision, canonical stroke centres, MACD(12, 26, 9) interval power, candidate stroke and the geometry snapshot.
- `chanorbit geometry` command: one JSON object of columnar bars (`dt` as `YYYY-MM-DDTHH:MM:SS`, positive prices up to 1e9, volume up to 1e15, at most 200,000 bars) with non-decreasing `dt` (adjacent equal times are revisions, a decrease is an error), per-line errors including non-UTF-8 lines, single request or JSON Lines batch on stdin, deterministic output order; one geometry snapshot per request with each fact once; schema `chanorbit.geometry/1`.
- Regression baselines in `tests/golden/` and an end-to-end stdin→stdout test.
