# Release

The package version is in `Cargo.toml`; the output schema strings are `contract::SCHEMA` (`geometry`), `contract::STAGES_SCHEMA` (`stages`) and `contract::LEVELS_SCHEMA` (`levels`) (see [cli-contract.md](cli-contract.md#versioning)). Release notes are in `CHANGELOG.md`.

## Build

Run the gates in [CONTRIBUTING.md](../CONTRIBUTING.md), then:

```sh
cargo build --release --locked          # target/release/chanorbit
```

The release profile uses LTO and one codegen unit. Runtime dependencies are limited to the platform C library.

Toolchain: edition 2024, `rust-version = "1.88"` in `Cargo.toml` (the minimum the language features need; only the stable toolchain is exercised).

## Python packaging

`pyproject.toml` packages the executable as a wheel with maturin (`bindings = "bin"`, version from `Cargo.toml`, no Python code). `uv pip install <path or git URL>` or `maturin build --release` builds it from source with the local Rust toolchain and puts `chanorbit` in the environment's scripts directory, so a project can lock it as a git dependency.

## Targets

| Target | Use | Build |
|---|---|---|
| `aarch64-apple-darwin` | Development machine (macOS arm64) | Native `cargo build --release`; links only `libSystem` |
| `x86_64-unknown-linux-musl` | Linux x86_64 node, static binary | `rustup target add x86_64-unknown-linux-musl` and a musl linker, or build in a `linux/amd64` container (see below) |

Container build of the static Linux binary:

```sh
docker run --rm --platform linux/amd64 -v "$PWD":/src -w /src rust:1-alpine \
  cargo build --release --locked --target x86_64-unknown-linux-musl
# target/x86_64-unknown-linux-musl/release/chanorbit
```

Check the result with `file` (expect "statically linked") and `chanorbit --version`. [status.md](status.md) records which targets have actually been built.
