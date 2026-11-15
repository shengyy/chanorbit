# Security

`chanorbit` performs no network access and opens no files. It reads requests from stdin and writes responses to stdout; diagnostics go to stderr. The library reads no environment variables or clock. Callers own where input data comes from and where output goes.

Requests are untrusted input: they are parsed with bounded types, unknown fields are rejected, values are range-checked before computation, and a bad request — including a batch line that is not UTF-8 — yields an error object without affecting other requests. A request holds at most 200,000 bars; memory is proportional to request size, and time can grow quadratically with the length of a series that forms no stroke (see [architecture](docs/architecture.md#data-flow)).

Report vulnerabilities privately to the maintainer; do not include market data or account information in reports.
