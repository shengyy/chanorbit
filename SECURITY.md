# Security

`chanorbit` performs no network access and opens no files. It reads requests from stdin and writes responses to stdout; diagnostics go to stderr. The library reads no environment variables or clock. Callers own where input data comes from and where output goes.

Requests are untrusted input: they are parsed with bounded types, unknown fields are rejected, and a bad request yields an error object without affecting other requests. Very large requests use memory proportional to their size.

Report vulnerabilities privately to the maintainer; do not include market data or account information in reports.
