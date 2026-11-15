# Contributing

Use the stable Rust toolchain pinned by `rust-toolchain.toml`. `Cargo.lock` owns dependency versions.

## Local gates

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release --locked
git diff --check
```

Run from the repository root. Documentation-only changes need `git diff --check`.

`cargo test` covers:

- `tests/regression.rs`: every baseline in `tests/golden/` replayed through the library — the snapshot at 5–15 input prefixes, the final response, a per-input state trace, final NewBar detail, batch-versus-incremental equality and, for default stroke parameters, the request text through the contract. Prices, counts, times and flags must be equal; MACD-derived values (`dif*`, `dea*`, `hist*`, `*_area`, `peak`) may differ by at most 1e-9 relative.
- `tests/cli.rs`: the real binary through stdin→stdout, single and batch mode, order, error isolation, determinism across `--jobs`, version and exit codes.
- `tests/units.rs`: time parsing, MACD segments and same-time revision equivalence.

Baselines are gzip-compressed JSON (synthetic series only): `config` the stroke parameters, `request` a columnar request, `response` the expected final response, `prefixes` the expected snapshot after given input counts, `trace` one row per input, `detail` the final NewBars and retained raw times. Only baselines with default parameters go through the command line. A failing baseline means behaviour changed. Do not regenerate baselines or widen tolerances to make a change pass; a definition change is a separate, explicitly approved decision recorded in [docs/structure-contract.md](docs/structure-contract.md).

In-process timing: `cargo run --release --example bench -- requests.jsonl` (one request per line).

## Changes

Use a topic branch. Record behaviour changes in `CHANGELOG.md`, update each fact at its owner document and link rather than duplicate. Keep files within 800 lines, split by responsibility. Never commit real market data, account identifiers, credentials or personal paths.
