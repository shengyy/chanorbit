# Command-line contract

The only owner of the `chanorbit` command, its JSON request/response shapes, errors and exit codes. Object definitions are in [structure-contract.md](structure-contract.md). Implementation: `src/main.rs` (process I/O) and `src/contract.rs` (request → response).

The command is stateless: every request is recomputed from its complete bar sequence. There is no daemon, cache or state between calls.

## Commands

```sh
chanorbit geometry                   # stdin: one JSON request object (any formatting); stdout: one response line
chanorbit geometry --batch           # stdin: JSON Lines, one request per line; stdout: one response line per request
chanorbit geometry --batch --jobs N  # N ≥ 1 worker threads (default: available CPUs)
chanorbit --version | -V             # "chanorbit <version> schema <schema>"
chanorbit --help | -h
```

`--jobs` is accepted only together with `--batch`; alone, or with a value that is not a positive integer, it is a command-line error (exit 2).

In batch mode stdin is split into lines on `\n` (a trailing `\r` is removed); lines that are empty or only ASCII whitespace are skipped, so an empty batch writes nothing and exits 0. Responses keep input order whatever `--jobs` is, so output bytes are deterministic. Each line stands alone: a line that is not UTF-8, not JSON or not a valid request yields an error object on its own output line, and the other lines are answered normally.

## Request

Exactly one JSON object; any other JSON value (for example a positional array) is an `invalid_input` error. Bars are columnar: one array per field, all of the same length, in input order.

```json
{"id": "series-1", "dt": ["2026-03-02T09:35:00", "…"], "high": [10.02, …], "low": [9.98, …], "close": [10.01, …], "vol": [120300, …]}
```

| Field | Type | Accepted values |
|---|---|---|
| `id` | any JSON, default `null` | Echoed in the response |
| `dt` | string array | Exactly `YYYY-MM-DDTHH:MM:SS`: exchange-local wall clock, zero-padded, no fraction, no zone, no surrounding spaces, a real calendar time (no `:60`) |
| `high`, `low`, `close` | number arrays | Finite prices in `(0, 1e9]`, in the caller's units |
| `vol` | number array | Finite volume in `[0, 1e15]`, in the caller's units |

- Length: 1 to 200,000 bars per request; more is an `invalid_input` error.
- Unknown fields are rejected; a repeated key is an `invalid_request` error.
- `dt` must be non-decreasing: adjacent equal times are revisions (the later bar replaces the earlier), and any time earlier than its predecessor — including a repeat that is not adjacent — is an `invalid_bar` error. Deduplication and corrections are the caller's.
- Values are checked before any computation; a bar outside these ranges fails the request with `invalid_bar` naming the field and index. The engine's `non_finite` reasons therefore cannot arise through the command line.
- Stroke parameters are the defaults of the [structure contract](structure-contract.md#strokes).

Columns rather than per-bar objects: the caller already holds its series as columns and passes them without building one object per bar, the request carries each key once, and parsing fills arrays directly. One shape only is accepted.

## Response

One object: `schema` (`chanorbit.geometry/1`), `id` (echo), then the [geometry snapshot](structure-contract.md#geometry-snapshot) fields at the top level. Every time is written as `YYYY-MM-DDTHH:MM:SS`, the same format as the input.

Error response, one line:

```json
{"schema":"chanorbit.geometry/1","id":"…","error":{"kind":"invalid_request","message":"…"}}
```

| `kind` | Cause |
|---|---|
| `invalid_json` | Not UTF-8, or not parseable JSON |
| `invalid_input` | Valid JSON that is not an object; more than 200,000 bars |
| `invalid_request` | Object shape: missing or unknown field, repeated key, wrong type, empty `dt`, column lengths differ |
| `invalid_bar` | One bar: `dt` not in the format or earlier than the previous one, a price or volume out of range |

`id` is echoed when the outer object was readable, otherwise `null`. `message` is for humans and not a contract.

## Exit codes and streams

| Code | Meaning |
|---|---|
| 0 | Every response written and successful (including an empty batch) |
| 1 | Every response written; at least one is an error object |
| 2 | Bad command line; stdout is empty |
| 3 | stdin could not be read or stdout could not be written |

stdout carries only contract JSON, one object per line. stderr carries human-readable diagnostics for codes 2 and 3, is empty for 0 and 1, and is not a contract.

## Versioning

The schema string changes when a field is removed, renamed or changes meaning. Adding fields keeps it. `--version` and every response carry it; the package version is in `Cargo.toml`.
