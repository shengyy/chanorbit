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

- **Revisions.** Each `dt` is observed once, after its last version. An earlier version of a time is never observed: it records no first-known time and takes part in no withdrawal. A `stages` request carrying revisions therefore answers exactly as the same request holding only the last version of each `dt`. There is no notion of revising an input that was already observed.
- **Observed inputs.** Observation starts at the first input with `dt ≥ observe_from`, or at the first input when `observe_from` is absent; earlier inputs only build state. When no input is observed (`observe_from` after the last input), every `first_known_at` and `stage_age` is `null` and no withdrawal is recorded.
- **Same stage.** Kind, the centre's `zd`, `zg` and first three members, and every role stroke (entry, departure, rebound, first-point anchor, retest, parent rebound and retest). Strokes compare by direction, `start_at`, `end_at`, start and end price, high and low. A centre taking more members keeps the stage; any role stroke changing — the retest reaching a new low, for example — makes a new stage.
- **`first_known_at`.** The first observed input at which the same stage stood. A stage already standing at the first observed input has `null`: it began before observation. The time is kept even if the stage disappears and comes back.
- **`stage_age`.** Observed inputs after `first_known_at` up to the last input (0 when first known at the last input); `null` with `first_known_at`.
- **Withdrawal.** Between two observed inputs the last stroke goes from an up stroke `U` back to a down stroke `D` that starts earlier, with `U.start_at = D.end_at` and `U.low = D.low`. The first observed input is not compared. A later withdrawal back to the same `D` (same `end_at` and low) replaces the earlier record.
- **`withdrawn_rebound`.** When the stage's retest has the `end_at` and low of such a `D`: the latest withdrawn `U` (`start_at`, `end_at`, `high`, `low`), `observed_at` — the observed input from which that version of `U` was the last stroke — and `withdrawn_at` — the observed input at which it was withdrawn. Otherwise `null`.
- **`beyond_start`**, at the last input: the candidate stroke is available, of kind `reverse`, and its `end_price` is above the retest's high — the move after the retest has passed the retest's start. Independent of `withdrawn_rebound`; both can hold.

Per observed input the engine evaluates the three stages once: the centre sequence over at most `max_strokes` strokes up to four times (second through its first point, third, second-like, and second-like's parent second through its first point), path guards over the retained raw bars with a binary search per NewBar element, and MACD over the retained raw bars only when a first point's geometry holds. A request of `m` inputs costs `O(m · (s + r log r))` for `s` retained strokes and `r` retained raw bars.

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

## Levels

One instrument at one moment, seen through several period series (levels), coarsest first. Each level is analysed alone, exactly as a `geometry` request (same strokes, centres, MACD, candidate); levels are not built from each other. What `levels` adds is per-level structure facts over a stroke window, and placement of each finer level inside the next coarser one. Nothing here is a trading conclusion.

### Bar time

Every series labels a bar by the **end** of the period it covers. Bar `i` of a level covers `(dt[i-1], dt[i]]`; the first bar covers everything up to `dt[0]`. A bar still forming carries the end of its period too (a daily bar forming during the session is labelled with that session's close time, for example `T15:00:00`). A finer time `t` lies in the coarser bar `i` with `dt[i-1] < t ≤ dt[i]`; a time after the coarser level's last `dt` lies in no coarser bar.

A **point** of a level is one of its raw bars together with a side, high or low (a stroke end, an extreme). Its **child time** is the first bar of the next finer level inside that raw bar's period whose high (for a high point) is the highest, or whose low (for a low point) is the lowest, among the finer bars in that period; its **child price** is that bar's high or low. With no finer bar in the period the child time is `null`. Prices of different levels are compared only through these child times, never directly: levels may be adjusted differently.

### Segments and ranges

A **segment** of a level is one of its window strokes or its unfinished reverse segment (below). Its interval is `(start_at, end_at]`: the start bar belongs to the segment before. The level's **sequence** is, in time order, its window strokes, the unfinished segment (either kind), the path after the extreme, and the **tail** (`"tail"`: the raw bars after all of these when no available candidate covers them, read opposite to the last stroke); every raw bar after the first window stroke's start lies in exactly one element.

- **Raw extreme** of a segment: over the retained raw bars with `start_at < dt ≤ end_at`, the highest high for an up segment or the lowest low for a down segment, with `at` the first bar holding it — except that the segment's own end bar wins a tie: an earlier bar is `at` only when its price is strictly beyond the end bar's. The same rule holds for every raw extreme below (strokes, unfinished segment, path after the extreme, tail, retest, `passed_by`). It can differ from the segment's `end_price`.
- **DIF extreme**: over the same bars, the largest DIF for an up segment or the smallest for a down segment, with its first bar; `null` when some bar in the interval has no DIF.

**Unfinished segment** (`unfinished`): the [candidate stroke](#candidate-stroke) when it is `available`, interval `(anchor_at, end_price_at]`, `start_price = anchor_price`. Only `kind = reverse` is a segment; `kind = extension` (the last stroke is being extended) is reported and is an element of the sequence (so a latest centre's exit can lie in it), but is never a retest, a zone object or a `passed_by` segment.

**Path after the extreme** (`after_extreme`): when the candidate is available, the raw bars with `end_price_at < dt ≤` the last input, read in the direction opposite to the candidate. Every figure covers exactly those bars: the raw extreme (the farthest opposite price, `at`), `bar_count`, the DIF extreme and `power`; only its `price` comparison stops at the raw extreme (start price to extreme). It is a path, not a stroke and not a segment; `null` when no bar follows `end_price_at`.

### Stroke window

`strokes` lists the retained strokes from the entry stroke of the oldest listed centre to the last stroke, so every listed centre has its entry, members, departure and retest present; with no listed centre, every retained stroke. A level with a coarser parent also starts early enough to hold every stroke ending after the child time of the parent's last-stroke start, so the [zones](#child-placement) see every child stroke they place. `fractals` lists every fractal (as in the geometry snapshot) whose `dt` is at or after the first window stroke's `start_at`. Indices (`strokes[i]`, `segment` references) are positions in this window.

### Centre list

`centres` holds the latest qualified centre (at least three members touching the core) and up to four qualified centres before it, oldest first, from the canonical sequence over the finished strokes. Each carries its first and last member, `member_count`, `zd`, `zg`, `dd`, `gg`, `start_at`, `end_at` and entry stroke (as in the geometry snapshot), and every centre but the last `versus_next`: its `core` (`[zd, zg]`) and `outer` (`[dd, gg]`) against the next listed centre, each `above`, `below`, `touch` or `overlap` (the [centre relation](#geometry-snapshot) rule). The core is always `above` or `below`: a centre closes on a stroke wholly beyond its core, every attempt after it starts with that stroke or a later one beyond it, so the next qualified core lies strictly on that side.

**Last stroke membership** (`last_stroke_member`): `member` is whether the last stroke is a member of the latest qualified centre, with `reason`: `member`; `not_finished` (fewer than five unfinished NewBars follow it, so it is not yet eligible); `not_returned` (finished, eligible, and not a member: the canonical sequence has closed the latest qualified centre before it, or it opens a later attempt that has not qualified); `no_centre`.

**Last close position** (every listed centre): `price_versus_core` and `price_versus_outer`, the level's last `close` against `[zd, zg]` and `[dd, gg]`: `above`, `inside` (edges included) or `below`.

### Centre state

A centre's state is read from its **path**: the third member's end price, then every retained raw bar after the third member's `end_at` — for the latest listed centre up to the last input, for an earlier (ended) centre up to the `end_at` of the stroke that closed it in the canonical sequence. A raw bar is `above` the core when its low is above `zg`, `below` when its high is below `zd`, otherwise in the core (touching an edge counts as in the core); the start price is placed the same way, edges inside. The path's **end side** is, for the latest centre, the [last close position](#centre-list) `price_versus_core`; for an ended centre, the closing stroke's end price against the core, which is always outside it.

Exactly one state holds:

- `inside` — latest centre only: the last close is in the core (`price_versus_core = inside`). A last bar that pierces an edge but closes inside is `inside`; one that touches the core but closes outside is `departed`, leaving in that bar.
- `departed` — the end side is outside and some point of the path is not on it. The **exit** is the bar after the last such point (that point itself when it is the last bar); the departure is the element of the sequence holding the exit — a window stroke, or for the latest centre possibly `unfinished`, `extension`, `after_extreme` or `tail`. It is never the third member or earlier.
- `formed_out` — the end side is outside and the whole path is on it: formed and never returned. Only a centre with exactly three members can be `formed_out`.

So `inside` holds exactly when the last close is in the core, and `departed` or `formed_out` exactly when it is outside. An ended centre's state is the fixed record of how it was left and never changes once the centre has ended; it is never `inside`. The engine checks these implications and the three-member rule for every centre; a violation is an `internal_error`. An earlier centre also names `ended_by`, the next listed centre. Positions below are window positions (`strokes[i]`) or the sequence names.

Facts by state:

- `departed`: `departure`, its `direction` and raw extreme — in that direction when the departure runs toward the exit side; when it runs against it (the exit lies in a pullback element), on the exit side over the departure's bars from the exit bar on — `edge` — the exit side, independent of the departure's own direction: `zg` when price left upward (end side above), `zd` when it left downward (a down stroke whose raw bars dip into the core and end above it departs through `zg`) — `member` (whether the departure is a member stroke), `first_beyond_at` (the exit bar), `retest` — the element right after a stroke departure when it is a window stroke or the unfinished reverse segment, otherwise `null` (no retest yet) — with its raw extreme toward the core (`retest_extreme`: its lowest low after an upward exit, its highest high after a downward one) and `versus_edge`: `beyond` (strictly beyond `edge`), `at` (equal) or `inside` (back in the core). Every bar after the exit is strictly beyond the edge, so a retest inside the path is always `beyond`: `at` and `inside` occur only for an ended centre whose retest follows its closing stroke. The latest centre carries `after_retest` (the positions after the retest; empty without a retest) and `retest_role: null`. An ended centre's record stops at the retest, kept even when it is the next listed centre's entry or one of its members: `retest_role` names that role by reference, {`centre` (position in `centres`), `role`: `entry` or `member`}, or `null`; `after_retest` is `null`.
- `inside`: `segment`, the element holding the last bar (the third member when no bar follows it).
- `formed_out`: `third` and `after` (the positions after it: to the latest element for the latest centre, to the closing stroke for an ended one).

### Comparisons

Only segments of the same direction are compared, numerator against denominator: `area` (the same-direction MACD area: positive area for up, negative area for down), `price` (`|end_price − start_price|`; for the path after the extreme, from its start price to its raw extreme), `mean_volume` and `bar_count` (raw bars in the interval). A ratio is `null` when the numerator is negative, the denominator is not positive, or either is unavailable. Between the two (retained raw bars from the earlier one's end to the later one's start, both included): `min_abs_dif`, `dif_crossed_zero` and `dea_crossed_zero` (both a strictly positive and a strictly negative value occur); `null` values when any of those bars lacks the value.

`completed` rows compare window strokes (the last stroke included, finished or not); `in_progress` rows have a numerator that is still moving (the unfinished segment, the path after the extreme, an extension, the tail, or a departure that is one of these), valued at its current extreme. The tail's `price` runs from the last stroke's end price to the tail's raw extreme. `departure_vs_entry`, `advance_vs_departure` and `previous_departure_vs_entry` exist only when the departure runs toward its exit side (`direction` up with `edge = zg`, down with `zd`). Rows, when both sides exist and share a direction:

| `pair` | Numerator / denominator |
|---|---|
| `last_vs_previous` | last stroke / previous same-direction stroke |
| `departure_vs_entry` | latest centre's departure / its entry stroke |
| `advance_vs_departure` | first segment after the retest in the departure's direction / the departure |
| `previous_departure_vs_entry` | the centre before the latest: its departure / its entry stroke |
| `unfinished_vs_previous` | unfinished reverse segment / previous same-direction stroke |
| `after_extreme_vs_previous` | path after the extreme / previous same-direction stroke |
| `extension_vs_last` | extension of the last stroke (anchor to current extreme) / the last stroke |

### Child placement

Every level but the finest has `child`, describing the next finer level inside it.

**Points.** For the parent's last stroke, unfinished reverse segment, extension (an extending last stroke: its current extreme) and path after the extreme: the parent raw extreme (`at`, `price`), its child time and price, and `passed_by`: the first child segment (window stroke or unfinished reverse segment) of the same direction that ends after the child time and whose raw bars after the child time go strictly beyond the child price, whatever its start — its position, `end_at` and that extreme — or `null`. The comparison stays inside the child level's own prices.

**Forces.** For the parent's last stroke and its unfinished segment: the child strokes lying inside the parent segment's child span (zone rules below) that share its direction; comparison rows `last_vs_first` (at least two such strokes) and `last_vs_previous` (at least three) among them, same fields as above. When the child series starts inside the parent's last stroke, `last_stroke` compares the child strokes available (`first` is the first available) and `child_starts_after` says so, as in the zones.

**Zones.** The boundaries are the child times of the parent's last-stroke start, last-stroke end and the unfinished reverse segment's end bar `end_price_at` (`b1 < b2 < b3`; no `b3` without an unfinished reverse segment; an extension adds no boundary, so its child objects fall in `after_last`). The `unfinished` point sits at `b3`'s parent bar unless the segment's raw extreme is strictly beyond its end price, when it precedes it. A boundary whose parent bar ends before the child series' first bar lies before every child object: it is `null` in `boundaries`, nothing crosses it, and `child_starts_after` names the latest such boundary (`last_start`: the child series starts inside the parent's last stroke; `last_end`; `unfinished_extreme`), otherwise `null`. Only the child's listed centres are placed; every child window stroke ending after `b1` is. A child object spans `[a, b]` in child time — a stroke, segment or the path after the extreme from `start_at` to `end_at`, a centre from `start_at` to `end_at`. It **crosses** boundary `x` when `a < x < b`. Objects with `b ≤ b1` lie before the parent's last stroke and are not placed. Every other object is placed in exactly one zone, the first that applies, and lists every boundary it crosses:

1. `crosses_last_start` — crosses `b1`;
2. `inside_last` — `b1 ≤ a` and `b ≤ b2`;
3. `crosses_last_end` — crosses `b2`;
4. `inside_unfinished` — `b2 ≤ a` and `b ≤ b3`;
5. `crosses_unfinished_extreme` — crosses `b3`;
6. `after_extreme` — `a ≥ b3`; without `b3`, `after_last` — `a ≥ b2`.

The engine checks that each placed object lands in exactly one zone. When a boundary inside the child series' span has no child bar, zones are `null` with the reason (`last_start_no_child_bar`, `last_end_no_child_bar`, `unfinished_extreme_no_child_bar`); when the child series ends before a boundary's parent bar, `child_ends_before`.

## Levels output

| Field | Meaning |
|---|---|
| `levels` | One object per request level, in request order (coarsest first) |
| `name` | The level's name, echoed |
| `status` | `ready` with at least one stroke, otherwise `missing_material` (every other field but `last_input_at` empty or `null`) |
| `last_input_at` | The level's last `dt` |
| `strokes` | [Window](#stroke-window) strokes: `direction`, `start_at`, `end_at`, `start_price`, `end_price`, `high`, `low`, `newbar_count`, `finished`, `raw_extreme` {`price`, `at`}, `dif_extreme` {`value`, `at`} or `null`, `power` (as in the geometry snapshot; `sample_count` is the bar count) |
| `unfinished` | Available candidate: `kind`, `direction`, `anchor_at`, `anchor_price`, `end_price`, `end_price_at`, `bars_after_extreme` (raw bars after `end_price_at`), `raw_extreme`, `dif_extreme`, `power` over `(anchor_at, end_price_at]`; otherwise `null` |
| `after_extreme` | [Path after the extreme](#segments-and-ranges): `direction`, `start_at` (`end_price_at`), `start_price` (`end_price`), `end_at` (last input), `raw_extreme`, `bar_count`, `dif_extreme`, `power`, all over `(start_at, end_at]`; or `null` |
| `fractals` | `dt`, `mark`, `price`, `confirmed_at` over the window |
| `last_stroke_member` | `member`, `reason` |
| `centres` | Oldest first: `first_member`, `last_member` (window positions), `member_count`, `entry`, `start_at`, `end_at`, `zd`, `zg`, `dd`, `gg`, `versus_next` {`core`, `outer`} or `null` for the last, `ended_by` (position in `centres` of the next one, `null` for the last), `price_versus_core`, `price_versus_outer`, `state` |
| `state` | `kind` = `departed`, `inside` or `formed_out` and that state's facts ([centre state](#centre-state)); positions are indices into `strokes` or the sequence names; `departed` carries `after_retest` (latest) or `retest_role` (ended), the other `null` |
| `comparisons` | `completed` and `in_progress`: rows `pair`, `numerator`, `denominator` (positions as above, `"after_extreme"`, `"extension"`), `area`, `price`, `mean_volume`, `bar_count`, `between` {`min_abs_dif`, `dif_crossed_zero`, `dea_crossed_zero`} |
| `child` | Every level but the finest, with `child_reason`: `null` with `child_reason = missing_material` when this level has no strokes, otherwise `child_reason = null` and the object: `name` of the child level, `points` {`last_stroke`, `unfinished`, `extension`, `after_extreme`}: each `at`, `price`, `child_at`, `child_price`, `passed_by` {`segment`, `end_at`, `price`} or `null` (the point is `null` when the parent object is absent), `forces` {`last_stroke`, `unfinished`: comparison rows, `child_starts_after`}, `zones`: {`boundaries` {`last_start`, `last_end`, `unfinished_extreme`, each `null` when before the child series}, `child_starts_after`, `objects`: `kind` (`stroke`, `unfinished`, `after_extreme`, `centre`), `index` (child window position; for a centre its position in the child's `centres`), `start_at`, `end_at`, `zone`, `crosses` (boundary names)} or `null` with `zones_reason` |

Segment positions use one vocabulary: an integer is `strokes[i]` of that level; `"unfinished"`, `"extension"`, `"after_extreme"` and `"tail"` name that level's objects.
