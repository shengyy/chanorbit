# Architecture

One crate, `chanorbit`: a library (`src/lib.rs`) holding the engine and the JSON contract, and a binary (`src/main.rs`) holding process I/O. The library performs no I/O and reads no clock or environment. Definitions are owned by [structure-contract.md](structure-contract.md); the command line by [cli-contract.md](cli-contract.md).

## Modules

| Module | Owns |
|---|---|
| `bar` | `RawBar`, `NewBar` and the inclusion merge |
| `fractal` | `Mark`, `Fractal`, detection, `confirmed_at` |
| `stroke` | `Direction`, `Stroke`, its raw endpoints, reopening and the single-stroke search over a NewBar window |
| `analyzer` | `Analyzer`: per-bar update, first stroke, extension, appending with linked endpoints, same-dt revision, finished strokes, fractal list, unfinished direction |
| `centre` | `Centre` geometry, the touch rule and the canonical centre sequence |
| `macd` | MACD(12, 26, 9) series over the raw closes |
| `power` | Interval power, stroke power, MACD bars and segments |
| `candidate` | Candidate stroke after the last stroke |
| `geometry` | The geometry snapshot: stroke facts, fractal window, centre sequence and indices, relation, candidate power |
| `stage` | Confirmed buy-side stages of one analyzer state: first-point anchor, second, third, second-like, path guard |
| `observe` | Per-input replay of the stages: stage identity, first-known time, age, after-retest state, stage facts |
| `levels` | One level of a `levels` request: stroke window (an extending last stroke read to its current extreme), raw and DIF extremes, unfinished segment, path after the extreme, centre list, last close position, centre state from the raw path |
| `levels_state` | A centre's state (`inside`, `departed`, `formed_out`) from its raw path over a level's sequence |
| `levels_compare` | Same-direction comparison rows of a level's segments |
| `levels_child` | The `levels` output: every level built coarsest first (a child's window covers its parent's last stroke), child times, points with `passed_by`, forces and zones |
| `signal` | One level as the nests read it (elements as read, areas, fractals), `completion` of a move inside a span, the third-buy and up-end nests, `mid_support` |
| `signal_observe` | The `signals` output: one evaluation over every level, or the replay along the finest level with first observations and per-level memos |
| `contract` | Columnar request validation, the four commands' responses and error JSON |
| `main.rs` | Arguments, stdin/stdout, batch threading, exit codes |

## Data flow

```text
columnar request ─► contract::run ─► RawBar ─► Analyzer::update (per bar)
                                                  │  inclusion ─► NewBar window
                                                  │  stroke search / extension / linking ─► strokes
                                                  ▼
                         geometry::geometry: strokes + power, fractals, centres,
                                             candidate + power, MACD ─► geometry response

columnar request ─► contract::run_stages ─► observe::observed_stages
                       per input: Analyzer::update ─► stage::stages ─► identity → first seen
                       last input: stage::stages + candidate ─► stage facts ─► stages response

levels request ─► contract::run_levels ─► per level, coarsest first: Analyzer::from_bars
                       ─► levels::Level::build (window covers the parent's last-stroke start)
                  per parent/child pair: levels_child child times, points, forces, zones ─► levels response

signals request ─► contract::run_signals ─► signal_observe::signals: per level dedup + MACD once
                       no observe_from: Analyzer::from_bars per level ─► evaluate
                       observe_from: per finest bar feed every level up to its time ─► evaluate ─► first (side, key, stage)
                  evaluate: signal::parent_departure (memo) ─► buy: A on P, completion on M, completion on L
                            sell: latest up element of M, completion on L, attributes (memo) ─► signals response
```

Strokes, fractals and the unfinished window share NewBars through `Arc`: a merge replaces the last NewBar with a new allocation, so earlier snapshots stay immutable. The undo record for same-dt revision holds only the latest input's prior window and the strokes it changed or removed. Nothing is pruned: strokes and raw bars cover the whole request.

The stages reuse the same objects: `Analyzer` per-input state, `centre::canonical_centres`, `candidate::candidate_stroke`, `macd::raw_macd` (computed lazily, only when a first point's geometry holds) and `power::stroke_power`. `stages` replays the request through one analyzer and evaluates after every observed input; it does not call `geometry`.

Every input clones the pending window and repeats the fractal scan and stroke search over it. While a stroke keeps forming the window stays short; a series that forms no stroke for its whole length makes the work grow quadratically with its length (tens of thousands of bars take seconds). Each input also rescans the raw bars after the last stroke's end for its extension, so a long run that neither extends the last stroke nor forms a new one grows the same way (32,000 one-price bars after a stroke: 0.03 s in 0.1.1, 0.6 s now; a 1,920-bar window stays under 10 ms). This is the current state and is bounded only by the request length limit.

`signals` reuses `levels`' reading of a level (`levels::counted`, `read_strokes`, `tail`), `levels_state::centre_state` for the parent's departure, `centre::canonical_centres` for the local centres and `levels_child::child_time`. Its area is the interval power's same-direction area summed over the rows found by binary search, so a replay step does not scan the whole series; a unit test holds it equal to `power::interval_power`. A replay evaluates once per finest bar; when A fails the buy side returns after the parent's memo, so the cost is the analyzers' per-bar updates plus the spans of standing nests. Local readings with `cargo run --release --example bench -- --signals` on synthetic random walks: 2,880 bars (960 30m, 1,920 5m) without observation about 2 ms per request, a 240-day 5m replay (11,520 bars and its 3,840 30m bars) observed from day 121 about 15 ms per request.

Batch mode reads all of stdin, splits it into byte lines, hands them to scoped worker threads through an atomic index and writes results in input order.
