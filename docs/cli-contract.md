# Command-line contract

The only owner of the `chanorbit` commands, their JSON request/response shapes, errors and exit codes. Object definitions are in [structure-contract.md](structure-contract.md). Implementation: `src/main.rs` (process I/O) and `src/contract.rs` (request → response).

The command is stateless: every request is recomputed from its complete bar sequence. There is no daemon, cache or state between calls.

## Commands

```sh
chanorbit geometry                   # stdin: one JSON request object (any formatting); stdout: one response line
chanorbit geometry --batch           # stdin: JSON Lines, one request per line; stdout: one response line per request
chanorbit geometry --batch --jobs N  # N ≥ 1 worker threads (default: available CPUs)
chanorbit stages [--batch [--jobs N]]  # the same, answering with the buy-side stages
chanorbit levels [--batch [--jobs N]]  # one instrument, several period series: multi-level structure facts
chanorbit --version | -V             # "chanorbit <version> schemas <geometry schema> <stages schema> <levels schema>"
chanorbit --help | -h
```

`geometry` answers with the structure snapshot of the last input; `stages` replays the inputs and answers with the confirmed buy-side stages and their observation. Both take the same request fields and input checks, except `observe_from`, which only `stages` accepts. `levels` takes a list of such series ([levels request](#levels-request)). All three follow the same batch, error and exit-code rules.

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
| `high`, `low`, `close` | number arrays | Finite prices in `(0, 1e9]`, in the caller's units, with `low ≤ high` and `low ≤ close ≤ high` in every bar (equality allowed) |
| `vol` | number array | Finite volume in `[0, 1e15]`, in the caller's units; checked by `stages` too, though no stage fact uses it |
| `observe_from` | string, `stages` only, optional | `YYYY-MM-DDTHH:MM:SS`; observation starts at the first input at or after it ([structure contract](structure-contract.md#observation)). Malformed or `null`: `invalid_request`; present in a `geometry` request, `null` included: `invalid_request` |

- Length: 1 to 200,000 bars per request; more is an `invalid_input` error.
- Unknown fields are rejected; a repeated key is an `invalid_request` error.
- `dt` must be non-decreasing: adjacent equal times are revisions (the later bar replaces the earlier), and any time earlier than its predecessor — including a repeat that is not adjacent — is an `invalid_bar` error. Deduplication and corrections are the caller's.
- Values are checked before any computation; a bar outside these ranges, with `high < low`, or with `close` outside `[low, high]` fails the request with `invalid_bar` naming the field and index. The engine's `non_finite` reasons therefore cannot arise through the command line.
- Stroke parameters are the defaults of the [structure contract](structure-contract.md#strokes).

Columns rather than per-bar objects: the caller already holds its series as columns and passes them without building one object per bar, the request carries each key once, and parsing fills arrays directly. One shape only is accepted.

## Levels request

One instrument at one moment: `id` (any JSON, echoed) and `levels`, a non-empty array of period series, coarsest first. Each element is an object with `name` (non-empty string, unique within the request, echoed) and the columns `dt`, `high`, `low`, `close`, `vol`, accepted exactly as a `geometry` request (formats, ranges, bar order, revisions, at most 200,000 bars per level). The engine does not know or limit which periods or how many.

```json
{"id": "600000.SH@2026-11-16T11:00:00", "levels": [
  {"name": "daily", "dt": ["…", "2026-11-13T15:00:00", "2026-11-16T15:00:00"], "high": […], "low": […], "close": […], "vol": […]},
  {"name": "30m",   "dt": ["…", "2026-11-16T10:30:00", "2026-11-16T11:00:00"], "high": […], "low": […], "close": […], "vol": […]}
]}
```

Every `dt` labels the **end** of its bar's period, and a bar still forming carries the end of its period: the daily bar forming during the 16 November session above is `2026-11-16T15:00:00`, and the 30-minute bar covering 10:30–11:00 is `11:00:00`. How a finer time maps into a coarser bar is defined in the [structure contract](structure-contract.md#bar-time). `observe_from` is not accepted. Errors in one level fail the request and name it: `levels[1].low[3]: …`.

## Response

`geometry`: one object with `schema` (`chanorbit.geometry/1`), `id` (echo), then the [geometry snapshot](structure-contract.md#geometry-snapshot) fields at the top level.

`stages`: one object with `schema` (`chanorbit.stages/1`), `id` (echo) and `stages` ([stages output](structure-contract.md#stages-output)).

`levels`: one object with `schema` (`chanorbit.levels/1`), `id` (echo) and `levels` ([levels output](structure-contract.md#levels-output)). A complete example (illustrative values; the finest level has no `child`):

```json
{
  "schema": "chanorbit.levels/1",
  "id": "600000.SH@2026-11-16T11:00:00",
  "levels": [
    {
      "name": "daily",
      "status": "ready",
      "last_input_at": "2026-11-16T15:00:00",
      "strokes": [
        {"direction": "向下", "start_at": "2026-09-01T15:00:00", "end_at": "2026-09-10T15:00:00", "start_price": 12.0, "end_price": 10.0, "high": 12.0, "low": 10.0, "newbar_count": 8, "finished": true, "raw_extreme": {"price": 10.0, "at": "2026-09-10T15:00:00"}, "dif_extreme": {"value": -0.21, "at": "2026-09-09T15:00:00"}, "power": {"sample_count": 7, "mean_volume": 81000.0, "macd": {"status": "ready", "reason": null, "positive_area": 0.0, "negative_area": 1.12, "dif_at_end": -0.2, "dea_at_end": -0.11, "ready_count": 7}}},
        {"direction": "向上", "start_at": "2026-09-10T15:00:00", "end_at": "2026-09-18T15:00:00", "start_price": 10.0, "end_price": 11.2, "high": 11.2, "low": 10.0, "newbar_count": 7, "finished": true, "raw_extreme": {"price": 11.2, "at": "2026-09-18T15:00:00"}, "dif_extreme": {"value": 0.08, "at": "2026-09-18T15:00:00"}, "power": {"sample_count": 6, "mean_volume": 90500.0, "macd": {"status": "ready", "reason": null, "positive_area": 0.66, "negative_area": 0.0, "dif_at_end": 0.08, "dea_at_end": -0.05, "ready_count": 6}}},
        {"direction": "向下", "start_at": "2026-09-18T15:00:00", "end_at": "2026-09-25T15:00:00", "start_price": 11.2, "end_price": 10.4, "high": 11.2, "low": 10.4, "newbar_count": 6, "finished": true, "raw_extreme": {"price": 10.4, "at": "2026-09-25T15:00:00"}, "dif_extreme": {"value": -0.04, "at": "2026-09-25T15:00:00"}, "power": {"sample_count": 5, "mean_volume": 70200.0, "macd": {"status": "ready", "reason": null, "positive_area": 0.0, "negative_area": 0.31, "dif_at_end": -0.04, "dea_at_end": -0.01, "ready_count": 5}}},
        {"direction": "向上", "start_at": "2026-09-25T15:00:00", "end_at": "2026-10-09T15:00:00", "start_price": 10.4, "end_price": 11.0, "high": 11.0, "low": 10.4, "newbar_count": 7, "finished": true, "raw_extreme": {"price": 11.04, "at": "2026-10-08T15:00:00"}, "dif_extreme": {"value": 0.05, "at": "2026-10-08T15:00:00"}, "power": {"sample_count": 6, "mean_volume": 65000.0, "macd": {"status": "ready", "reason": null, "positive_area": 0.22, "negative_area": 0.03, "dif_at_end": 0.04, "dea_at_end": 0.01, "ready_count": 6}}},
        {"direction": "向下", "start_at": "2026-10-09T15:00:00", "end_at": "2026-10-16T15:00:00", "start_price": 11.0, "end_price": 10.6, "high": 11.0, "low": 10.6, "newbar_count": 6, "finished": true, "raw_extreme": {"price": 10.6, "at": "2026-10-16T15:00:00"}, "dif_extreme": {"value": -0.03, "at": "2026-10-15T15:00:00"}, "power": {"sample_count": 5, "mean_volume": 60100.0, "macd": {"status": "ready", "reason": null, "positive_area": 0.0, "negative_area": 0.18, "dif_at_end": -0.02, "dea_at_end": 0.0, "ready_count": 5}}},
        {"direction": "向上", "start_at": "2026-10-16T15:00:00", "end_at": "2026-10-30T15:00:00", "start_price": 10.6, "end_price": 11.8, "high": 11.8, "low": 10.6, "newbar_count": 9, "finished": true, "raw_extreme": {"price": 11.82, "at": "2026-10-29T15:00:00"}, "dif_extreme": {"value": 0.19, "at": "2026-10-29T15:00:00"}, "power": {"sample_count": 10, "mean_volume": 98000.0, "macd": {"status": "ready", "reason": null, "positive_area": 0.95, "negative_area": 0.0, "dif_at_end": 0.18, "dea_at_end": 0.1, "ready_count": 10}}},
        {"direction": "向下", "start_at": "2026-10-30T15:00:00", "end_at": "2026-11-06T15:00:00", "start_price": 11.8, "end_price": 11.3, "high": 11.8, "low": 11.3, "newbar_count": 6, "finished": false, "raw_extreme": {"price": 11.3, "at": "2026-11-06T15:00:00"}, "dif_extreme": {"value": 0.02, "at": "2026-11-06T15:00:00"}, "power": {"sample_count": 5, "mean_volume": 52000.0, "macd": {"status": "ready", "reason": null, "positive_area": 0.0, "negative_area": 0.21, "dif_at_end": 0.02, "dea_at_end": 0.07, "ready_count": 5}}}
      ],
      "unfinished": {
        "kind": "reverse",
        "direction": "向上",
        "anchor_at": "2026-11-06T15:00:00",
        "anchor_price": 11.3,
        "end_price": 11.65,
        "end_price_at": "2026-11-13T15:00:00",
        "bars_after_extreme": 1,
        "raw_extreme": {"price": 11.65, "at": "2026-11-13T15:00:00"},
        "dif_extreme": {"value": 0.06, "at": "2026-11-13T15:00:00"},
        "power": {"sample_count": 5, "mean_volume": 61000.0, "macd": {"status": "ready", "reason": null, "positive_area": 0.09, "negative_area": 0.02, "dif_at_end": 0.06, "dea_at_end": 0.05, "ready_count": 5}}
      },
      "after_extreme": {
        "direction": "向下",
        "start_at": "2026-11-13T15:00:00",
        "start_price": 11.65,
        "end_at": "2026-11-16T15:00:00",
        "raw_extreme": {"price": 11.45, "at": "2026-11-16T15:00:00"},
        "bar_count": 1,
        "dif_extreme": {"value": 0.055, "at": "2026-11-16T15:00:00"},
        "power": {"sample_count": 1, "mean_volume": 72000.0, "macd": {"status": "ready", "reason": null, "positive_area": 0.004, "negative_area": 0.0, "dif_at_end": 0.055, "dea_at_end": 0.052, "ready_count": 1}}
      },
      "fractals": [
        {"dt": "2026-09-10T15:00:00", "mark": "底分型", "price": 10.0, "confirmed_at": "2026-09-12T15:00:00"},
        {"dt": "2026-09-18T15:00:00", "mark": "顶分型", "price": 11.2, "confirmed_at": "2026-09-22T15:00:00"},
        {"dt": "2026-09-25T15:00:00", "mark": "底分型", "price": 10.4, "confirmed_at": "2026-09-29T15:00:00"},
        {"dt": "2026-10-09T15:00:00", "mark": "顶分型", "price": 11.0, "confirmed_at": "2026-10-13T15:00:00"},
        {"dt": "2026-10-16T15:00:00", "mark": "底分型", "price": 10.6, "confirmed_at": "2026-10-20T15:00:00"},
        {"dt": "2026-10-30T15:00:00", "mark": "顶分型", "price": 11.8, "confirmed_at": "2026-11-03T15:00:00"},
        {"dt": "2026-11-06T15:00:00", "mark": "底分型", "price": 11.3, "confirmed_at": "2026-11-10T15:00:00"},
        {"dt": "2026-11-13T15:00:00", "mark": "顶分型", "price": 11.65, "confirmed_at": "2026-11-16T15:00:00"}
      ],
      "last_stroke_member": {"member": false, "reason": "not_finished"},
      "centres": [
        {"first_member": 1, "last_member": 5, "member_count": 5, "entry": 0, "start_at": "2026-09-10T15:00:00", "end_at": "2026-10-30T15:00:00", "zd": 10.4, "zg": 11.0, "dd": 10.0, "gg": 11.8, "versus_next": null, "ended_by": null, "price_versus_core": "above", "price_versus_outer": "inside", "state": {"kind": "departed", "departure": 5, "direction": "向上", "edge": "zg", "raw_extreme": {"price": 11.82, "at": "2026-10-29T15:00:00"}, "member": true, "first_beyond_at": "2026-10-22T15:00:00", "retest": 6, "retest_extreme": {"price": 11.3, "at": "2026-11-06T15:00:00"}, "versus_edge": "beyond", "after_retest": ["unfinished"], "retest_role": null}}
      ],
      "comparisons": {
        "completed": [{"pair": "last_vs_previous", "numerator": 6, "denominator": 4, "area": 1.16667, "price": 1.25, "mean_volume": 0.865225, "bar_count": 1.0, "between": {"min_abs_dif": 0.004, "dif_crossed_zero": true, "dea_crossed_zero": false}}],
        "in_progress": [{"pair": "advance_vs_departure", "numerator": "unfinished", "denominator": 5, "area": 0.0947368, "price": 0.291667, "mean_volume": 0.622449, "bar_count": 0.5, "between": {"min_abs_dif": 0.02, "dif_crossed_zero": false, "dea_crossed_zero": false}}, {"pair": "unfinished_vs_previous", "numerator": "unfinished", "denominator": 5, "area": 0.0947368, "price": 0.291667, "mean_volume": 0.622449, "bar_count": 0.5, "between": {"min_abs_dif": 0.02, "dif_crossed_zero": false, "dea_crossed_zero": false}}, {"pair": "after_extreme_vs_previous", "numerator": "after_extreme", "denominator": 6, "area": 0.0, "price": 0.4, "mean_volume": 1.38462, "bar_count": 0.2, "between": {"min_abs_dif": 0.02, "dif_crossed_zero": false, "dea_crossed_zero": false}}]
      },
      "child": {
        "name": "30m",
        "points": {"last_stroke": {"at": "2026-11-06T15:00:00", "price": 11.3, "child_at": "2026-11-06T10:30:00", "child_price": 11.3, "passed_by": null}, "unfinished": {"at": "2026-11-13T15:00:00", "price": 11.65, "child_at": "2026-11-13T14:00:00", "child_price": 11.65, "passed_by": null}, "after_extreme": {"at": "2026-11-16T15:00:00", "price": 11.45, "child_at": "2026-11-16T11:00:00", "child_price": 11.45, "passed_by": null}},
        "forces": {"last_stroke": [], "unfinished": [{"pair": "last_vs_first", "numerator": 2, "denominator": 0, "area": 0.48, "price": 0.6, "mean_volume": 0.91, "bar_count": 0.75, "between": {"min_abs_dif": 0.001, "dif_crossed_zero": true, "dea_crossed_zero": false}}]},
        "zones": {"boundaries": {"last_start": "2026-10-30T13:30:00", "last_end": "2026-11-06T10:30:00", "unfinished_extreme": "2026-11-13T14:00:00"}, "objects": [{"kind": "stroke", "index": 0, "start_at": "2026-11-06T10:30:00", "end_at": "2026-11-09T14:30:00", "zone": "inside_unfinished", "crosses": []}, {"kind": "stroke", "index": 1, "start_at": "2026-11-09T14:30:00", "end_at": "2026-11-11T10:00:00", "zone": "inside_unfinished", "crosses": []}, {"kind": "stroke", "index": 2, "start_at": "2026-11-11T10:00:00", "end_at": "2026-11-13T14:00:00", "zone": "inside_unfinished", "crosses": []}, {"kind": "stroke", "index": 3, "start_at": "2026-11-13T14:00:00", "end_at": "2026-11-16T10:00:00", "zone": "after_extreme", "crosses": []}, {"kind": "unfinished", "index": null, "start_at": "2026-11-16T10:00:00", "end_at": "2026-11-16T10:30:00", "zone": "after_extreme", "crosses": []}, {"kind": "after_extreme", "index": null, "start_at": "2026-11-16T10:30:00", "end_at": "2026-11-16T11:00:00", "zone": "after_extreme", "crosses": []}]},
        "zones_reason": null
      }
    },
    {
      "name": "30m",
      "status": "ready",
      "last_input_at": "2026-11-16T11:00:00",
      "strokes": [
        {"direction": "向上", "start_at": "2026-11-06T10:30:00", "end_at": "2026-11-09T14:30:00", "start_price": 11.3, "end_price": 11.52, "high": 11.52, "low": 11.3, "newbar_count": 7, "finished": true, "raw_extreme": {"price": 11.53, "at": "2026-11-09T14:00:00"}, "dif_extreme": {"value": 0.031, "at": "2026-11-09T14:00:00"}, "power": {"sample_count": 12, "mean_volume": 5200.0, "macd": {"status": "ready", "reason": null, "positive_area": 0.11, "negative_area": 0.01, "dif_at_end": 0.03, "dea_at_end": 0.02, "ready_count": 12}}},
        {"direction": "向下", "start_at": "2026-11-09T14:30:00", "end_at": "2026-11-11T10:00:00", "start_price": 11.52, "end_price": 11.38, "high": 11.52, "low": 11.38, "newbar_count": 6, "finished": true, "raw_extreme": {"price": 11.38, "at": "2026-11-11T10:00:00"}, "dif_extreme": {"value": -0.012, "at": "2026-11-11T10:00:00"}, "power": {"sample_count": 8, "mean_volume": 4100.0, "macd": {"status": "ready", "reason": null, "positive_area": 0.0, "negative_area": 0.05, "dif_at_end": -0.01, "dea_at_end": 0.004, "ready_count": 8}}},
        {"direction": "向上", "start_at": "2026-11-11T10:00:00", "end_at": "2026-11-13T14:00:00", "start_price": 11.38, "end_price": 11.65, "high": 11.65, "low": 11.38, "newbar_count": 9, "finished": true, "raw_extreme": {"price": 11.65, "at": "2026-11-13T14:00:00"}, "dif_extreme": {"value": 0.024, "at": "2026-11-13T13:30:00"}, "power": {"sample_count": 16, "mean_volume": 4700.0, "macd": {"status": "ready", "reason": null, "positive_area": 0.0528, "negative_area": 0.0, "dif_at_end": 0.02, "dea_at_end": 0.012, "ready_count": 16}}},
        {"direction": "向下", "start_at": "2026-11-13T14:00:00", "end_at": "2026-11-16T10:00:00", "start_price": 11.65, "end_price": 11.48, "high": 11.65, "low": 11.48, "newbar_count": 6, "finished": false, "raw_extreme": {"price": 11.48, "at": "2026-11-16T10:00:00"}, "dif_extreme": {"value": -0.009, "at": "2026-11-16T10:00:00"}, "power": {"sample_count": 8, "mean_volume": 6800.0, "macd": {"status": "ready", "reason": null, "positive_area": 0.0, "negative_area": 0.04, "dif_at_end": -0.009, "dea_at_end": 0.003, "ready_count": 8}}}
      ],
      "unfinished": {
        "kind": "reverse",
        "direction": "向上",
        "anchor_at": "2026-11-16T10:00:00",
        "anchor_price": 11.48,
        "end_price": 11.55,
        "end_price_at": "2026-11-16T10:30:00",
        "bars_after_extreme": 1,
        "raw_extreme": {"price": 11.55, "at": "2026-11-16T10:30:00"},
        "dif_extreme": null,
        "power": {"sample_count": 1, "mean_volume": 7100.0, "macd": {"status": "ready", "reason": null, "positive_area": 0.0, "negative_area": 0.002, "dif_at_end": -0.008, "dea_at_end": 0.001, "ready_count": 1}}
      },
      "after_extreme": {
        "direction": "向下",
        "start_at": "2026-11-16T10:30:00",
        "start_price": 11.55,
        "end_at": "2026-11-16T11:00:00",
        "raw_extreme": {"price": 11.45, "at": "2026-11-16T11:00:00"},
        "bar_count": 1,
        "dif_extreme": {"value": -0.01, "at": "2026-11-16T11:00:00"},
        "power": {"sample_count": 1, "mean_volume": 6400.0, "macd": {"status": "ready", "reason": null, "positive_area": 0.0, "negative_area": 0.003, "dif_at_end": -0.01, "dea_at_end": 0.0, "ready_count": 1}}
      },
      "fractals": [
        {"dt": "2026-11-06T10:30:00", "mark": "底分型", "price": 11.3, "confirmed_at": "2026-11-06T13:30:00"},
        {"dt": "2026-11-09T14:30:00", "mark": "顶分型", "price": 11.52, "confirmed_at": "2026-11-10T10:30:00"},
        {"dt": "2026-11-11T10:00:00", "mark": "底分型", "price": 11.38, "confirmed_at": "2026-11-11T11:00:00"},
        {"dt": "2026-11-13T14:00:00", "mark": "顶分型", "price": 11.65, "confirmed_at": "2026-11-13T15:00:00"},
        {"dt": "2026-11-16T10:00:00", "mark": "底分型", "price": 11.48, "confirmed_at": "2026-11-16T11:00:00"}
      ],
      "last_stroke_member": {"member": false, "reason": "no_centre"},
      "centres": [],
      "comparisons": {
        "completed": [{"pair": "last_vs_previous", "numerator": 2, "denominator": 0, "area": 0.48, "price": 1.22727, "mean_volume": 0.903846, "bar_count": 1.33333, "between": {"min_abs_dif": 0.001, "dif_crossed_zero": true, "dea_crossed_zero": false}}],
        "in_progress": [{"pair": "unfinished_vs_previous", "numerator": "unfinished", "denominator": 2, "area": 0.0, "price": 0.259259, "mean_volume": 1.51064, "bar_count": 0.0625, "between": {"min_abs_dif": 0.008, "dif_crossed_zero": true, "dea_crossed_zero": false}}]
      }
    }
  ]
}
```

Every time is written as `YYYY-MM-DDTHH:MM:SS`, the same format as the input.

Error response, one line, carrying the command's schema:

```json
{"schema":"chanorbit.geometry/1","id":"…","error":{"kind":"invalid_request","message":"…"}}
```

| `kind` | Cause |
|---|---|
| `invalid_json` | Not UTF-8, or not parseable JSON |
| `invalid_input` | Valid JSON that is not an object; more than 200,000 bars (in one `levels` level) |
| `invalid_request` | Object shape: missing or unknown field, repeated key, wrong type, empty `dt`, column lengths differ, `observe_from` malformed or sent to `geometry` or `levels`; for `levels`: `levels` empty, a `name` missing, empty or repeated |
| `invalid_bar` | One bar: `dt` not in the format or earlier than the previous one, a price or volume out of range, `high < low`, `close` outside `[low, high]` |
| `internal_error` | The computation of this request panicked (an engine defect); `message` carries the panic message. Other requests in the batch are unaffected and keep their order |

`id` is echoed when the outer object was readable, otherwise `null`. `message` is for humans and not a contract.

## Exit codes and streams

| Code | Meaning |
|---|---|
| 0 | Every response written and successful (including an empty batch) |
| 1 | Every response written; at least one is an error object |
| 2 | Bad command line; stdout is empty |
| 3 | stdin could not be read or stdout could not be written, including a reader that closed the pipe (also for `--version` and `--help`); no panic, and a closed stderr is ignored |

stdout carries only contract JSON, one object per line. stderr carries human-readable diagnostics for codes 2 and 3 and the panic report behind an `internal_error`; it is otherwise empty for 0 and 1, never receives contract JSON, and is not a contract.

## Versioning

Each command has its own schema string; it changes when one of that command's fields is removed, renamed or changes meaning. Adding fields keeps it. `--version` and every response carry them; the package version is in `Cargo.toml`. Before the first release the schema strings are not bumped: they are fixed from the first release on.
