# Changelog

## [Unreleased]

### Added

- Structure engine: inclusion, fractals, strokes with withdrawal and retention, same-time revision, canonical stroke centres, MACD(12, 26, 9) interval power, candidate stroke and the geometry snapshot.
- `chanorbit geometry` command: columnar bars (`dt`, `high`, `low`, `close`, `vol`) with non-decreasing `dt` (adjacent equal times are revisions, a decrease is an error), single request or JSON Lines batch on stdin, deterministic output order, per-line errors; one geometry snapshot per request with each fact once; schema `chanorbit.geometry/1`.
- Regression baselines in `tests/golden/` and an end-to-end stdin→stdout test.
