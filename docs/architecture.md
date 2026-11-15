# Architecture

One crate, `chanorbit`: a library (`src/lib.rs`) holding the engine and the JSON contract, and a binary (`src/main.rs`) holding process I/O. The library performs no I/O and reads no clock or environment. Definitions are owned by [structure-contract.md](structure-contract.md); the command line by [cli-contract.md](cli-contract.md).

## Modules

| Module | Owns |
|---|---|
| `bar` | `RawBar`, `NewBar` and the inclusion merge |
| `fractal` | `Mark`, `Fractal`, detection, `confirmed_at` |
| `stroke` | `Direction`, `Stroke` and the single-stroke search over a NewBar window |
| `analyzer` | `Analyzer`: per-bar update, first stroke, withdrawal, retention, same-dt revision, finished strokes, fractal list, unfinished direction |
| `centre` | `Centre` geometry, the touch rule and the canonical centre sequence |
| `macd` | MACD(12, 26, 9) series over retained closes |
| `power` | Interval power, stroke power, MACD bars and segments |
| `candidate` | Candidate stroke after the last stroke |
| `geometry` | The geometry snapshot: stroke facts, fractal window, centre sequence and indices, relation, candidate power |
| `stage` | Confirmed buy-side stages of one analyzer state: first-point anchor, second, third, second-like, path guard |
| `observe` | Per-input replay of the stages: stage identity, first-known time, age, withdrawal record, after-retest state, stage facts |
| `levels` | One level of a `levels` request: stroke window, raw and DIF extremes, unfinished segment, path after its extreme, tail, centre list, last close position, centre state from the raw path |
| `levels_compare` | Same-direction comparison rows of a level's segments |
| `levels_child` | The `levels` output: every level built coarsest first (a child's window covers its parent's last stroke), child times, points with `passed_by`, forces and zones |
| `contract` | Columnar request validation, the three commands' responses and error JSON |
| `main.rs` | Arguments, stdin/stdout, batch threading, exit codes |

## Data flow

```text
columnar request ─► contract::run ─► RawBar ─► Analyzer::update (per bar)
                                                  │  inclusion ─► NewBar window
                                                  │  stroke search / withdrawal ─► strokes
                                                  │  retention pruning
                                                  ▼
                         geometry::geometry: strokes + power, fractals, centres,
                                             candidate + power, MACD ─► geometry response

columnar request ─► contract::run_stages ─► observe::observed_stages
                       per input: Analyzer::update ─► stage::stages ─► identity → first seen
                                  last stroke ─► withdrawal record
                       last input: stage::stages + candidate ─► stage facts ─► stages response

levels request ─► contract::run_levels ─► per level, coarsest first: Analyzer::from_bars
                       ─► levels::Level::build (window covers the parent's last-stroke start)
                  per parent/child pair: levels_child child times, points, forces, zones ─► levels response
```

Strokes, fractals and the unfinished window share NewBars through `Arc`: a merge replaces the last NewBar with a new allocation, so earlier snapshots stay immutable. The undo record for same-dt revision holds only the latest input's prior window, removed stroke and pruned items.

The stages reuse the same objects: `Analyzer` per-input state, `centre::canonical_centres`, `candidate::candidate_stroke`, `macd::retained_macd` (computed lazily, only when a first point's geometry holds) and `power::stroke_power`. `stages` replays the request through one analyzer and evaluates after every observed input; it does not call `geometry`.

Every input clones the pending window and repeats the fractal scan and stroke search over it. While a stroke keeps forming the window stays short; a series that forms no stroke for its whole length makes the work grow quadratically with its length (tens of thousands of bars take seconds). This is the current state and is bounded only by the request length limit.

Batch mode reads all of stdin, splits it into byte lines, hands them to scoped worker threads through an atomic index and writes results in input order.
