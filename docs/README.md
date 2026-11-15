# Documentation map

## Core documents

| Document | Only responsibility |
|---|---|
| [README.md](../README.md) / [README.zh-CN.md](../README.zh-CN.md) | Overview and quick start (kept in sync) |
| [PRODUCT.md](../PRODUCT.md) | Scope, non-goals and intended use |
| [AGENTS.md](../AGENTS.md) | Agent boundaries and routing; [CLAUDE.md](../CLAUDE.md) imports it |
| [CONTRIBUTING.md](../CONTRIBUTING.md) | Toolchain, local gates, baselines and change rules |
| [SECURITY.md](../SECURITY.md) | Trust boundary |
| [LICENSE](../LICENSE) | MIT license terms |
| [CHANGELOG.md](../CHANGELOG.md) | Behaviour changes per version |
| [architecture.md](architecture.md) | Module map and data flow |
| [structure-contract.md](structure-contract.md) | Objects, definitions and output fields |
| [cli-contract.md](cli-contract.md) | Command line, request/response JSON, errors, exit codes, schema version |
| [status.md](status.md) | What the tests cover and what they do not |
| [release.md](release.md) | Building the binary and target platforms |

## Directories

| Directory | Only holds |
|---|---|
| `src/` | Engine library modules, the JSON contract and the binary entry |
| `tests/` | Regression, end-to-end and unit tests; `tests/common/` shared helpers |
| `tests/golden/` | Regression baselines (gzip JSON, synthetic series) |
| `tests/golden/stages/` | `stages` regression baselines (gzip JSON request/response cases, synthetic series) |
| `tests/golden/levels/` | `levels` regression baselines (gzip JSON request/response cases, synthetic series) |
| `examples/` | In-process timing tool |
| `docs/` | Current architecture, contracts, status and release facts |
| `target/`, `.runtime/` | Ignored build output and local scratch |

## Writing rules

- **One owner per fact:** edit the responsible document and link to it elsewhere.
- **Current state only:** no plans or process logs; Git holds history.
- **Verified then written:** distinguish what tests cover from what is assumed.
- **Nothing private:** synthetic fixtures only; no real market data, accounts or personal paths.

Use lowercase kebab-case filenames inside `docs/`. Public documents are in English; `AGENTS.md` is in Chinese.

## Context layers

1. `AGENTS.md` holds persistent boundaries and a routing table; `CLAUDE.md` points to it.
2. This map and the owner documents give task-specific context.
3. Read the owning module and its tests before changing or claiming behaviour.
