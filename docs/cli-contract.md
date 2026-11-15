# Command-line contract

The only owner of the `chanorbit` command, its JSON request/response shapes, errors and exit codes. Object definitions are in [structure-contract.md](structure-contract.md). Implementation: `src/main.rs` (process I/O) and `src/contract.rs` (request → response).

The command is stateless: every request is recomputed from its complete bar sequence. There is no daemon, cache or state between calls.

## Commands

```sh
chanorbit geometry                 # stdin: one JSON request object (any formatting); stdout: one response line
chanorbit geometry --batch         # stdin: JSON Lines, one request per line; stdout: one response line per request
chanorbit geometry --batch --jobs N
chanorbit --version                # "chanorbit <version> schema <schema>"
chanorbit --help
```

In batch mode empty lines are skipped; responses keep input order whatever `--jobs` is (default: available CPUs), so output bytes are deterministic. A failing request yields an error object on its own line and does not affect other lines.

## Request

Bars are columnar: one array per field, all of the same non-empty length, in input order.

```json
{"id": "series-1", "dt": ["2026-03-02T09:35:00", "…"], "high": [10.02, …], "low": [9.98, …], "close": [10.01, …], "vol": [120300, …]}
```

| Field | Type | Meaning |
|---|---|---|
| `id` | any JSON, default `null` | Echoed in the response |
| `dt` | string array | Naive wall clock: `YYYY-MM-DDTHH:MM:SS`, a space instead of `T`, optional fraction, or a bare date. A zone suffix (`Z`, `+08:00`) is rejected |
| `high`, `low`, `close` | number arrays | Prices in the caller's units |
| `vol` | number array | Volume in the caller's units |

Unknown fields are rejected. Stroke parameters are the defaults of the [structure contract](structure-contract.md#strokes). `dt` must be non-decreasing: adjacent equal times are revisions (the later bar replaces the earlier), and any time earlier than its predecessor — including a repeat that is not adjacent — makes the request an `invalid_bar` error. Deduplication and corrections are the caller's.

Columns rather than per-bar objects: the caller already holds its series as columns and passes them without building one object per bar, the request carries each key once, and parsing fills arrays directly. One shape only is accepted.

## Response

One object: `schema` (`chanorbit.geometry/1`), `id` (echo), then the [geometry snapshot](structure-contract.md#geometry-snapshot) fields at the top level.

Error response, one line:

```json
{"schema":"chanorbit.geometry/1","id":"…","error":{"kind":"invalid_request","message":"…"}}
```

`kind` is `invalid_json` (not parseable), `invalid_request` (shape, unknown field, empty `dt`, column lengths differ), or `invalid_bar` (a `dt` that is not a naive time or is earlier than the previous one). `id` is echoed when the outer object was readable.

## Exit codes and streams

| Code | Meaning |
|---|---|
| 0 | Every response written and successful |
| 1 | Every response written; at least one is an error object |
| 2 | Bad command line; stdout is empty |
| 3 | stdin could not be read (including non-UTF-8) or stdout could not be written |

stdout carries only contract JSON, one object per line. stderr carries human-readable diagnostics for codes 2 and 3 and is not a contract.

## Versioning

The schema string changes when a field is removed, renamed or changes meaning. Adding fields keeps it. `--version` and every response carry it; the package version is in `Cargo.toml`.
