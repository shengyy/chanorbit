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

`cargo test` runs `tests/regression.rs` (geometry baselines), `tests/stages.rs` (stage baselines and rules), `tests/cli.rs` (the real binary) and `tests/units.rs`; what they cover and how values are compared is owned by [docs/status.md](docs/status.md).

Baselines are gzip-compressed JSON (synthetic series only): `config` the stroke parameters, `request` a columnar request, `response` the expected final response, `prefixes` the expected snapshot after given input counts, `trace` one row per input, `detail` the final NewBars and retained raw times. Only baselines with default parameters go through the command line. Stage baselines (`tests/golden/stages/*.json.gz`) hold `description` and `cases`, each case a `request` (default parameters, optional `observe_from`) and the expected `stages` `response`. A failing baseline means behaviour changed. Do not regenerate baselines or loosen the comparison to make a change pass; a definition change is a separate, explicitly approved decision recorded in [docs/structure-contract.md](docs/structure-contract.md).

In-process timing: `cargo run --release --example bench -- requests.jsonl` (one request per line).

## Changes

Use a topic branch. Record behaviour changes in `CHANGELOG.md`, update each fact at its owner document and link rather than duplicate. Keep files within 800 lines, split by responsibility. Never commit real market data, account identifiers, credentials or personal paths.
