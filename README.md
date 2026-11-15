# chanOrbit

A small, pure-Rust Chan-theory structure engine: inclusion, fractals, strokes, stroke centres, MACD interval power, a structure geometry snapshot, the confirmed buy-side stages with their per-bar observation and multi-level structure facts, delivered as one executable that speaks JSON over stdin/stdout.

[简体中文](README.zh-CN.md) · [Product scope](PRODUCT.md) · [Documentation](docs/README.md) · [Test status](docs/status.md)

> **Research & Experimental Use Only — Non-commercial intended use.** No investment advice or trading signals; provided AS IS. See [PRODUCT.md](PRODUCT.md#intended-use-and-disclaimer).

## Build

```sh
cargo build --release      # target/release/chanorbit
```

Targets and static Linux builds: [docs/release.md](docs/release.md).

## Use

```sh
echo '{"id":"demo","dt":["2026-03-02T15:00:00","2026-03-03T15:00:00"],
  "high":[10.3,10.4],"low":[9.9,10.0],"close":[10.2,10.1],"vol":[1000,1200]}' | target/release/chanorbit geometry
```

The response holds the geometry snapshot: strokes with interval power, the recent fractals, the canonical centre sequence with the latest two centres, the candidate stroke with its power, and MACD bars and segments. Batch mode takes JSON Lines:

```sh
target/release/chanorbit geometry --batch < requests.jsonl > responses.jsonl
```

`chanorbit stages` takes the same request fields plus an optional `observe_from`, and answers with the confirmed second, third and second-like stages of the last bar, each with its centre, role strokes, first-known time, age and what followed the retest.

`chanorbit levels` takes one instrument's series for several periods, coarsest first, and answers per level with the stroke window, recent centres and their state against the latest price, same-direction comparisons, and how the next finer level sits inside it.

Requests, responses, errors and exit codes: [docs/cli-contract.md](docs/cli-contract.md). Definitions of every object: [docs/structure-contract.md](docs/structure-contract.md).

## Properties

- Stateless command: each request is recomputed from its full bar sequence.
- Deterministic: batch output order and bytes do not depend on thread count.
- Pure engine: no network, files, clock or environment in the library.
- Dependencies: `serde`, `serde_json`, `chrono`.

## License

Licensed under [MIT](LICENSE).
