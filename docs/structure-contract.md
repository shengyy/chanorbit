# Structure contract

The only owner of the engine's objects, definitions and output fields. Implementation: `src/`; module map in [architecture.md](architecture.md); JSON transport in [cli-contract.md](cli-contract.md).

## Input

A `RawBar` has `dt`, `high`, `low`, `close` and `vol` (input format and ranges: [cli-contract.md](cli-contract.md#request)). `dt` is the exchange-local wall clock without a zone; every output time is the same wall clock written as a naive ISO string (`2026-03-02T09:35:00`). Prices and volume are used in the caller's units. The engine has no notion of symbol or period: the same rules apply to every series.

Bars are fed one at a time with non-decreasing `dt`; deduplication and corrections belong to the caller. A bar whose `dt` equals the previous input's `dt` fully replaces it: the engine restores the exact state before that input (unfinished bars, strokes, retention pruning) and then applies the new version. Feeding several revisions of one time equals feeding only the last one. Only the latest input can be revised. Batch construction is the same per-bar loop, so it always equals incremental updates.

## Inclusion (`NewBar`)

The first two inputs become NewBars as they are. Afterwards, with the last two NewBars `k1`, `k2` and the new raw bar `k3`:

- Direction is up when `k1.high < k2.high`, down when `k1.high > k2.high`; equal highs mean no merge.
- `k2` and `k3` are in inclusion when one range contains the other, boundaries included.
- Up merge: high and low are the larger of each; `dt` is `k2.dt` if `k2.high > k3.high`, otherwise `k3.dt`. Down merge: the smaller of each; `dt` is `k2.dt` if `k2.low < k3.low`, otherwise `k3.dt`.
- Its `elements` are the first 100 raw bars of `k2` (minus any with `k3.dt`) followed by `k3`.

Without inclusion `k3` becomes a new NewBar.

**Element cap.** A NewBar therefore keeps at most 101 raw bars: its first 100 and the latest; raw bars merged in between are no longer among its elements. Prices and `dt` of the NewBar are unaffected, and so are fractals, `confirmed_at` (a first element), strokes and MACD (computed over retained raw bars). Only lookups of a raw bar inside a NewBar see the cap: the candidate's `anchor_price_at` and `end_price_at`. When the raw bar holding the price was displaced, an earlier element with the same price is used, or the candidate is `unavailable` (`anchor_price_source_missing` / `no_forward_price_source`).

## Fractals

Over three consecutive NewBars, a top (`顶分型`) has the middle high and low both strictly above its neighbours'; a bottom (`底分型`) has both strictly below. The fractal's `dt` is the middle bar's; its price is the middle high for a top and the middle low for a bottom. Scanning a NewBar sequence keeps only alternating marks: a fractal with the same mark as the previous kept one is dropped.

`confirmed_at` is the time of the first raw bar in the third NewBar. The last fractal's third bar can still absorb bars, so its price may change.

The fractal list is every stroke's fractals except each stroke's first, then the fractals of the unfinished window, keeping only strictly increasing times.

## Strokes

**Search.** On a NewBar window, find all fractals. The first fractal `A` fixes the direction (bottom → up stroke `向上`, top → down stroke `向下`). The end `B` is the most extreme opposite fractal after `A` that is beyond it (for an up stroke: a top with price above `A`'s and the highest high; the first wins ties). The stroke spans NewBars from `A`'s first bar to `B`'s third bar. It forms only when that span has at least `min_stroke_bars` NewBars (default 6) and the two fractals are not in inclusion (one fractal's range strictly inside the other's). The remaining unfinished window starts at `B`'s first NewBar.

**First stroke.** Before any stroke exists, the start is the most extreme fractal of the first fractal's mark (later fractals win ties); the window is cut to start at that fractal's first NewBar, then searched.

**Later strokes.** After each input the unfinished window is searched again; a found stroke is appended.

**Withdrawal.** After the search, if the latest unfinished NewBar is beyond the last stroke's end (higher than its high for an up stroke, lower than its low for a down stroke), the last stroke is removed and the unfinished window becomes that stroke's NewBars without its last two, followed by the current unfinished NewBars from the stroke's second-to-last NewBar onward. Only the latest NewBar is tested, and the search is not repeated in the same input.

**Retention.** At most `max_strokes` strokes are kept (default 50); older ones are dropped, and raw bars before the first retained stroke's first raw bar are dropped with them. MACD, power and indices are computed over what is retained.

**Finished strokes.** The last stroke does not count as finished while fewer than five unfinished NewBars follow it. This only selects centre members; any stroke may still be revised.

**Unfinished direction.** Once there is a stroke and the unfinished window holds a fractal, the unfinished direction is opposite to the last stroke.

The command line uses the defaults `max_strokes = 50`, `min_stroke_bars = 6`; the library `Config` sets them.

## Centres

A centre over consecutive member strokes has `zg` = lowest high and `zd` = highest low of its **first three** members, `zz` = `zd + (zg - zd) * 0.5`, `gg`/`dd` = highest high / lowest low of all members, `sdt`/`edt` = first member start / last member end. Its members touch the core when `zg ≥ zd` and each member's high or low lies in `[zd, zg]` or the member spans the whole range.

The canonical sequence over the finished strokes:

1. The first stroke is entry context only; the earliest core starts at the second stroke.
2. Without an active core, the next three consecutive strokes are tried; if their members do not touch the core, the attempt advances by two strokes.
3. An active core keeps its first-three bounds and takes each following stroke that returns to it.
4. The first stroke that does not return — an up stroke whose high is below `zd`, or a down stroke whose low is above `zg` — closes the core and starts the next attempt; the stroke before it is that attempt's entry. Members of the closed core are not handed back.
5. The open core, or the pending tail (one or two strokes), ends the sequence.

A core keeps its first-three bounds while extending. There is no recursion to higher levels.

## MACD

MACD(12, 26, 9) over retained raw closes. Each EMA is seeded with the simple mean of its first `n` values and then `alpha * x + (1 - alpha) * prev` with `alpha = 2 / (n + 1)`. The short EMA starts at index `26 - 12` so both EMAs seed on the same bar. DIF = short − long; DEA is the 9-EMA of DIF from its first finite value; hist = DIF − DEA. The first 33 retained bars have no histogram.

Segments are maximal runs of same-sign histogram bars: zero continues an open run and never starts one; a missing value closes it; `peak` is the value with the largest magnitude (the first wins ties).

## Interval power

Power over `(sdt, edt]` uses retained raw bars with `sdt < dt ≤ edt`; their count is `sample_count`. The MACD reason, in order:

- `missing_coverage`: `sdt ≥ edt`, either endpoint is not a retained raw bar, raw times are not strictly increasing, or no rows.
- `non_finite`: a non-finite close at or before `edt`.
- `warmup`: some row has no MACD value and lies within the first 33 retained bars; otherwise `non_finite`.

When ready, `positive_area` and `negative_area` are the sums of `max(hist, 0)` and `max(-hist, 0)` over the rows. `dif_at_end`/`dea_at_end` are the values at `edt` when present. `ready_count` counts rows with all three MACD values. `mean_volume` needs coverage and rows. Unavailable values are `null`, never 0.

For a stroke the interval is its endpoint fractal times. If a raw bar inside the stroke's NewBars with time in `(sdt, edt]` is no longer retained, areas and `mean_volume` are unavailable with `missing_coverage`.

## Candidate stroke

Display-only geometry after the last stroke; it never runs the stroke search and never confirms a stroke.

- No stroke: `state = none`, `reason = no_identified_stroke`.
- The anchor is the last stroke's end fractal (`anchor_at`, `anchor_price`). `anchor_price_at` is the raw bar inside the anchor's NewBars at that price (the one at `anchor_at` first, else the earliest); none → `unavailable / anchor_price_source_missing`. The anchor must be in the unfinished window, else `unavailable / anchor_not_in_engine_tail`.
- `kind = extension` when the last stroke extends (some unfinished NewBar is beyond its end), searching the same direction from the anchor's NewBar; otherwise `kind = reverse`, searching the opposite direction after it.
- The end is the most extreme unfinished NewBar boundary beyond the anchor price (the first wins ties); none → `state = none`, `reason = no_forward_extreme`. `end_price_at` is the raw bar after the anchor holding that price (the one at the NewBar's time first); none → `unavailable / no_forward_price_source`; if that raw bar or the anchor raw bar is not retained → `unavailable / missing_coverage`. Non-finite prices → `unavailable / non_finite`.
- Otherwise `state = available` with `end_price` and `end_price_at`.

## Geometry snapshot

Every fact appears once. Field names follow the consumer's vocabulary.

| Field | Meaning |
|---|---|
| `status` | `ready` with at least one stroke, otherwise `missing_material` |
| `bi` | Every retained stroke, oldest first: `direction`, `sdt`, `edt`, `high`, `low` (endpoint extremes; the path inside can exceed them), `start_price`, `end_price`, `price_amplitude` (`|end_price − start_price|` rounded to two decimals), `newbar_count`, `included_in_finished_bis`, `power` |
| `ubi_direction` | Unfinished direction, or `null` |
| `ubi_newbar_count` | NewBars in the unfinished window |
| `fractals` | Fractal list from the second-to-last stroke's `sdt` on (all of it with fewer than two strokes): `dt`, `mark`, `price`, `confirmed_at` |
| `centre_sequence` | Canonical centres: `sdt`, `edt`, `zd`, `zg`, `zz`, `dd`, `gg`, `bi_count`, `member_indices` (into `bi`), `entry_index` (the stroke before the first member). The sequence includes tail groups of one or two strokes |
| `latest_finished_zs`, `previous_zs` | Indices into `centre_sequence` of the last and second-to-last centre with at least three members that touch the core, or `null` |
| `zs_relation` | When both exist: latest versus previous, `core` on `[zd, zg]` and `outer` on `[dd, gg]`, each `above`, `below`, `touch` or `overlap` |
| `candidate_stroke` | `state`, `reason`, `kind`, `direction`, `anchor_at`, `anchor_price`, `anchor_price_at`, `end_price`, `end_price_at` |
| `candidate_power` | Power over `(anchor_at, last input dt]` of an available candidate, or `null` |
| `candidate_endpoint_power` | Power over `(anchor_at, end_price_at]` of an available candidate, or `null` |
| `macd.bars` | Every retained raw bar: `dt`, `dif`, `dea`, `hist` (`null` when not finite) |
| `macd.segments` | `sign`, `sdt`, `edt`, `bars`, `peak`, `peak_at` |
| power | `sample_count`, `mean_volume`, `macd`: `status` (`ready`/`unavailable`), `reason`, `positive_area`, `negative_area`, `dif_at_end`, `dea_at_end`, `ready_count` |

Indices into `bi` are positions in the current retained list; they shift when retention drops old strokes and are not stable identifiers. No buy/sell stage is evaluated.
