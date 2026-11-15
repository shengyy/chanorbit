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

A centre over consecutive member strokes has `zg` = lowest high and `zd` = highest low of its **first three** members, `zz` = `zd + (zg - zd) * 0.5`, `gg`/`dd` = highest high / lowest low of all members, `start_at`/`end_at` = first member start / last member end. Its members touch the core when `zg ≥ zd` and each member's high or low lies in `[zd, zg]` or the member spans the whole range.

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

Power over `(start_at, end_at]` uses retained raw bars with `start_at < dt ≤ end_at`; their count is `sample_count`. The MACD reason, in order:

- `missing_coverage`: `start_at ≥ end_at`, either endpoint is not a retained raw bar, raw times are not strictly increasing, or no rows.
- `non_finite`: a non-finite close at or before `end_at`.
- `warmup`: some row has no MACD value and lies within the first 33 retained bars; otherwise `non_finite`.

When ready, `positive_area` and `negative_area` are the sums of `max(hist, 0)` and `max(-hist, 0)` over the rows. `dif_at_end`/`dea_at_end` are the values at `end_at` when present. `ready_count` counts rows with all three MACD values. `mean_volume` needs coverage and rows. Unavailable values are `null`, never 0.

For a stroke the interval is its endpoint fractal times. If a raw bar inside the stroke's NewBars with time in `(start_at, end_at]` is no longer retained, areas and `mean_volume` are unavailable with `missing_coverage`.

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
| `strokes` | Every retained stroke, oldest first: `direction`, `start_at`, `end_at`, `high`, `low` (endpoint extremes; the path inside can exceed them), `start_price`, `end_price`, `price_amplitude` (`|end_price − start_price|` rounded to two decimals), `newbar_count`, `finished`, `power` |
| `pending_direction` | Unfinished direction, or `null` |
| `pending_newbar_count` | NewBars in the unfinished window |
| `fractals` | Fractal list from the second-to-last stroke's `start_at` on (all of it with fewer than two strokes): `dt`, `mark`, `price`, `confirmed_at` |
| `centre_sequence` | Canonical centres: `start_at`, `end_at`, `zd`, `zg`, `zz`, `dd`, `gg`, `member_count`, `member_indices` (into `strokes`), `entry_index` (the stroke before the first member). The sequence includes tail groups of one or two strokes |
| `latest_centre`, `previous_centre` | Indices into `centre_sequence` of the last and second-to-last centre with at least three members that touch the core, or `null` |
| `centre_relation` | When both exist: latest versus previous, `core` on `[zd, zg]` and `outer` on `[dd, gg]`, each `above`, `below`, `touch` or `overlap` |
| `candidate_stroke` | `state`, `reason`, `kind`, `direction`, `anchor_at`, `anchor_price`, `anchor_price_at`, `end_price`, `end_price_at` |
| `candidate_power` | Power over `(anchor_at, last input dt]` of an available candidate, or `null` |
| `candidate_endpoint_power` | Power over `(anchor_at, end_price_at]` of an available candidate, or `null` |
| `macd.bars` | Every retained raw bar: `dt`, `dif`, `dea`, `hist` (`null` when not finite) |
| `macd.segments` | `sign`, `start_at`, `end_at`, `bars`, `peak`, `peak_at` |
| power | `sample_count`, `mean_volume`, `macd`: `status` (`ready`/`unavailable`), `reason`, `positive_area`, `negative_area`, `dif_at_end`, `dea_at_end`, `ready_count` |

Indices into `strokes` are positions in the current retained list; they shift when retention drops old strokes and are not stable identifiers. Stages are a separate output, below.

## Stages

Buy-side stages (买点) of the current strokes, each confirmed by a downward **retest** stroke (回试笔) that is the last stroke. They describe completed structure; they are not trading conclusions. Only confirmed stages are produced: no sell side, no stage whose retest stroke has not formed yet, no reasons for absence.

Notation: `S` is the retained stroke list, `n` its length, `F` the finished count. A **qualified centre** has at least three members that touch its core; "the latest qualified centre over a slice" is the last such centre of the canonical sequence built over that slice of strokes. A stroke's `high`/`low` are its endpoint extremes.

**Path guard.** "The path after stroke `X` stays above `p` up to the cutoff" means every retained raw bar with `X.end_at < dt ≤ cutoff` has `low > p` (touching `p` ends the stage). The cutoff is the last input when the stage's retest is the last stroke, otherwise the retest's `end_at`. Both `X.end_at` and the cutoff must be retained raw bars, and every raw bar that the strokes from `X` to the retest or the unfinished NewBars hold in that range must still be retained; otherwise the guard fails.

**First point (一买), used only as an anchor.** For a down stroke `D = S[e-1]`:

1. the latest qualified centre over `S[..e-1]` has `S[e-2]` as its last member, and its entry (the stroke before its first member) `E` is a down stroke;
2. `E` enters from above and `D` leaves to a new low: `E.high > zg`, `E.low ≤ zg`, `D.high ≥ zd`, `D.low < min(dd, E.low)`;
3. divergence (背驰): `D` and `E` both have a negative MACD area (interval power of the stroke), `E`'s is not zero, and `D`'s divided by `E`'s is below 1.

`D` is the first-point anchor (一买点所在离开段). No path guard applies to it as an anchor.

**Second (二买).** The last three strokes are the anchor `A = S[n-3]`, the rebound `R = S[n-2]` and the retest `T = S[n-1]`, where `A` is the anchor of a first point with `e = n-2`. `T` is down, `T.low > A.low`, and the path after `A` stays above `A.low`. Roles: `centre` and `entry` from the first point; `departure` = `first_point_anchor` = `A`; `rebound` = `R`; `retest` = `T`.

**Third (三买).** The latest qualified centre over `S[..F]` has at least four members and its last member is `L = S[n-2]`, an up stroke that leaves it upward (`L.low ≤ zg < L.high`). The retest `T = S[n-1]` is down with `T.low > zg`, and the path after `L` stays above `zg`. Roles: `entry` = the stroke before the centre; `departure` = `L`; `retest` = `T`.

**Second-like (类二买).** The latest qualified centre over `S[..F]` has four or five members and starts at `S[n-5]`. A second stage exists whose retest is that first member: anchor `S[n-7]` (a first point with `e = n-6`), rebound `S[n-6]` (also the centre's entry), retest `S[n-5]`, its path guard cut off at the end of `S[n-5]`. The retest `T = S[n-1]` is down with `T.low > zd`; the path after `R = S[n-2]` stays above `zd`, and the path after the parent anchor stays above the anchor's low. Roles: `entry` = `S[n-6]`; `departure` = `rebound` = `R`; `retest` = `T`; `first_point_anchor` = the parent anchor; `parent` = the parent's rebound and retest.

## Observation

A stage's history comes from replaying the request one input at a time with the same analyzer; nothing is inferred backwards.

- **Observed inputs.** Each input time is observed once, after its last version (a revision replaces the earlier version before observation). Observation starts at the first input with `dt ≥ observe_from`, or at the first input when `observe_from` is absent; earlier inputs only build state.
- **Same stage.** Kind, the centre's `zd`, `zg` and first three members, and every role stroke (entry, departure, rebound, first-point anchor, retest, parent rebound and retest). Strokes compare by direction, `start_at`, `end_at`, start and end price, high and low. A centre taking more members keeps the stage; any role stroke changing — the retest reaching a new low, for example — makes a new stage.
- **`first_known_at`.** The first observed input at which the same stage stood. A stage already standing at the first observed input has `null`: it began before observation. The time is kept even if the stage disappears and comes back.
- **`stage_age`.** Observed inputs after `first_known_at` up to the last input (0 when first known at the last input); `null` with `first_known_at`.
- **Withdrawal.** Between two observed inputs the last stroke goes from an up stroke `U` back to a down stroke `D` that starts earlier, with `U.start_at = D.end_at` and `U.low = D.low`. The first observed input is not compared. A later withdrawal back to the same `D` (same `end_at` and low) replaces the earlier record.
- **`withdrawn_rebound`.** When the stage's retest has the `end_at` and low of such a `D`: the latest withdrawn `U` (`start_at`, `end_at`, `high`, `low`), `observed_at` — the observed input from which that version of `U` was the last stroke — and `withdrawn_at` — the observed input at which it was withdrawn. Otherwise `null`.
- **`beyond_start`**, at the last input: the candidate stroke is available, of kind `reverse`, and its `end_price` is above the retest's high — the move after the retest has passed the retest's start. Independent of `withdrawn_rebound`; both can hold.

Per observed input the engine evaluates the three stages once: the centre sequence over at most `max_strokes` strokes up to three times, path guards over the retained raw bars, and MACD over the retained raw bars only when a first point's geometry holds. A request of `m` inputs costs `O(m · (s + r))` for `s` retained strokes and `r` retained raw bars.

## Stages output

| Field | Meaning |
|---|---|
| `stages` | Confirmed stages after the last input, in the order second, third, second-like |
| `kind` | `second`, `third` or `second_like` |
| `centre` | The stage's centre: `start_at`, `zd`, `zg` |
| `entry`, `departure`, `retest` | Role strokes: `start_at`, `end_at`, `high`, `low` |
| `rebound`, `first_point_anchor` | Role strokes of second and second-like, otherwise `null` |
| `parent` | Second-like only: the parent second's `rebound` and `retest`, otherwise `null` |
| `first_known_at`, `stage_age`, `withdrawn_rebound`, `beyond_start` | [Observation](#observation) |
