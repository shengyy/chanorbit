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
| `contract` | Columnar request validation, response and error JSON |
| `main.rs` | Arguments, stdin/stdout, batch threading, exit codes |

## Data flow

```text
columnar request ─► contract::run ─► RawBar ─► Analyzer::update (per bar)
                                                  │  inclusion ─► NewBar window
                                                  │  stroke search / withdrawal ─► strokes
                                                  │  retention pruning
                                                  ▼
                         geometry::geometry: strokes + power, fractals, centres,
                                             candidate + power, MACD ─► response JSON
```

Strokes, fractals and the unfinished window share NewBars through `Arc`: a merge replaces the last NewBar with a new allocation, so earlier snapshots stay immutable. The undo record for same-dt revision holds only the latest input's prior window, removed stroke and pruned items.

Later layers (buy/sell stages, per-prefix observation) build on the library: `Analyzer::update` for per-bar state, `Analyzer::strokes`/`finished_strokes`/`pending_bars`, `centre::canonical_centres`, `candidate::candidate_stroke`, `macd::retained_macd` and `power::interval_power`. The command line exposes only the snapshot.

Every input clones the pending window and repeats the fractal scan and stroke search over it. While a stroke keeps forming the window stays short; a series that forms no stroke for its whole length makes the work grow quadratically with its length (tens of thousands of bars take seconds). This is the current state and is bounded only by the request length limit.

Batch mode reads all of stdin, splits it into byte lines, hands them to scoped worker threads through an atomic index and writes results in input order.
