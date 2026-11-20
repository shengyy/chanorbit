# Structure contract

The only owner of the engine's objects, definitions and output fields. Implementation: `src/`; module map in [architecture.md](architecture.md); JSON transport in [cli-contract.md](cli-contract.md).

## Input

A `RawBar` has `dt`, `high`, `low`, `close` and `vol` (input format and ranges: [cli-contract.md](cli-contract.md#request)). `dt` is the exchange-local wall clock without a zone; every output time is the same wall clock written as a naive ISO string (`2026-03-02T09:35:00`). Prices and volume are used in the caller's units. The engine has no notion of symbol or period: the same rules apply to every series.

Bars are fed one at a time with non-decreasing `dt`; deduplication and corrections belong to the caller. A bar whose `dt` equals the previous input's `dt` fully replaces it: the engine restores the exact state before that input (unfinished bars, strokes) and then applies the new version. Feeding several revisions of one time equals feeding only the last one. Only the latest input can be revised. Batch construction is the same per-bar loop, so it always equals incremental updates.

**Window.** The caller owns the window: every command computes over every bar of its request, and the engine drops nothing. Every stroke, raw bar, MACD value and index is taken over the whole request, so bars appended after the window's start never move an earlier stroke, centre or index unless the stroke rules themselves revise it.

## Inclusion (`NewBar`)

The first two inputs become NewBars as they are. Afterwards, with the last two NewBars `k1`, `k2` and the new raw bar `k3`:

- Direction is up when `k1.high < k2.high`, down when `k1.high > k2.high`; equal highs mean no merge.
- `k2` and `k3` are in inclusion when one range contains the other, boundaries included.
- Up merge: high and low are the larger of each; `dt` is `k2.dt` if `k2.high > k3.high`, otherwise `k3.dt`. Down merge: the smaller of each; `dt` is `k2.dt` if `k2.low < k3.low`, otherwise `k3.dt`.
- Its `elements` are the first 100 raw bars of `k2` (minus any with `k3.dt`) followed by `k3`.

Without inclusion `k3` becomes a new NewBar.

**Element cap.** A NewBar therefore keeps at most 101 raw bars: its first 100 and the latest; raw bars merged in between are no longer among its elements. Prices and `dt` of the NewBar are unaffected, and so are fractals, `confirmed_at` (a first element), strokes, the candidate and MACD: they read the raw bars, not a NewBar's elements.

## Fractals

Over three consecutive NewBars, a top (`顶分型`) has the middle high and low both strictly above its neighbours'; a bottom (`底分型`) has both strictly below. The fractal's `dt` is the middle bar's; its price is the middle high for a top and the middle low for a bottom. Scanning a NewBar sequence keeps only alternating marks: a fractal with the same mark as the previous kept one is dropped.

`confirmed_at` is the time of the first raw bar in the third NewBar. The last fractal's third bar can still absorb bars, so its price may change.

The fractal list is every stroke's fractals except each stroke's first, then the fractals of the unfinished window, keeping only strictly increasing times.

## Strokes

**Search.** On a NewBar window, find all fractals. The first fractal `A` fixes the direction (bottom → up stroke `向上`, top → down stroke `向下`). The end `B` is the most extreme opposite fractal after `A` that is beyond it (for an up stroke: a top with price above `A`'s and the highest high; the first wins ties). The stroke spans NewBars from `A`'s first bar to `B`'s third bar. It forms only when that span has at least `min_stroke_bars` NewBars (default 6), the two fractals are not in inclusion (one fractal's range strictly inside the other's), and at least three raw bars lie strictly between the two fractals' **extreme bars**, counted without inclusion. A fractal's extreme bar is the first raw bar of its middle NewBar that holds the fractal's price (the fractal's `dt` when the [element cap](#inclusion-newbar) dropped it); the fractal's own `dt` is the later of equal prices (*Inclusion*), so it is not used for the count. The remaining unfinished window starts at `B`'s first NewBar.

**Raw gap.** This is the second half of the new stroke definition (2007-09-18): after inclusion no NewBar is shared by the two fractals, and between the top fractal's highest bar and the bottom fractal's lowest bar, not counting those two and ignoring inclusion, there are at least three raw bars. With the default six NewBars the span alone leaves two NewBars between the middles, so only a six-NewBar span whose two inner NewBars are single raw bars and whose extreme bars sit at the facing ends of the middles can fail it; seven or more never do. A smaller `min_stroke_bars` leaves the gap to decide more often. When `B`, the most extreme opposite fractal, fails it, no stroke forms and the window waits: a less extreme fractal is never taken instead (that would redefine which extreme ends the stroke), and a later fractal at the same price does not end it either, since the first raw bar reaching a price holds it (*Endpoints*). Only a fractal beyond `B` can end the stroke. The rule constrains the fractals that identify a stroke; the placed endpoints below are raw extremes over the span and may lie closer together.

**First stroke.** Before any stroke exists, the start is the most extreme fractal of the first fractal's mark (later fractals win ties); the window is cut to start at that fractal's first NewBar, then searched.

**Endpoints.** The fractals `A` and `B` identify a stroke; its endpoints (`start_at`/`start_price`, `end_at`/`end_price`) are raw prices over the raw bars. The start is the most extreme raw price on the start side (lowest low for an up stroke, highest high for a down one) after the previous stroke's start — for the first stroke, from the first raw bar of its first NewBar — up to, not including, `B`'s `dt`. The end is the most extreme raw price on the end side over `(start, B's dt]`. On equal prices the first raw bar reaching the price holds it, at both ends and for every raw price below (extension, candidate end, levels' raw extremes): a later bar at the same price is not a new extreme, so it lies after the endpoint, in the next stroke or the path after the extreme. So a high or low that inclusion merged out of sight — in `B`'s first NewBar, in the NewBar before it, or anywhere in the span — is the endpoint, never a NewBar boundary. The previous stroke ends where the next starts: appending a stroke moves the previous stroke's end to the new start (see *Append* for when that moves the previous stroke's start too). For every stroke no raw bar in `(start, end]` lies beyond its end, and no raw bar from its start to the bar before its end lies beyond its start; the bar holding the end may (*End bar*).

**Reopening.** Reopening a stroke gives the window to search again from its start: its NewBars without `B`'s middle and last, then the current window from `B`'s middle NewBar on. A stroke **holds its end** up to a time when no raw bar after its start and before that time is beyond its start; the bar at that time may be (*End bar*).

**Later strokes.** After each input:

1. **Extension.** When a raw bar after the last stroke's end is beyond it (higher for an up stroke, lower for a down stroke), the last stroke is extending; the most extreme such raw price is its extension. If the stroke does not hold its end up to the extension, it cannot extend across that price: the last two strokes are reopened together and searched again from the earlier one's start; when the stroke found has its end fractal beyond the earlier one's, both are replaced by it. Otherwise the last stroke is reopened and searched again from its own start, and replaced when the stroke found has its end fractal further beyond. Else it stays as it is, still extending: an extending stroke is never removed.
2. **Append.** The unfinished window is searched; a found stroke is appended, its endpoints are placed (see *Endpoints*) and the previous stroke's end moves to its start. When that move would carry the previous stroke across a raw price beyond the previous stroke's own start (it does not hold its end up to the new start), the last two strokes are reopened and the stroke is searched again from the start of the one before them, repeated while needed. A search that does not reach the found stroke's end, or no stroke before the two, keeps the strokes and links the found stroke as it is. Then, while the stroke before the found one does not hold its end up to its new end, it **starts again**: its start becomes the most extreme start-side raw price from its start up to, not including, its new end (its start wins ties), and the stroke before it ends there, checked the same way, back to the first stroke, whose start simply moves. Directions keep alternating and every stroke still ends where the next starts.

The unfinished window after a stroke is formed starts at its `B`'s first NewBar. Every change of this input — an extension, reopened strokes, moved ends — is undone by a revision of the same `dt`.

**End bar.** A raw bar holds both a high and a low and their order inside the bar is unknown. When the bar holding a stroke's end also reaches beyond the stroke's start (a wide opening bar, for example), both prices stand: the end is not moved to the bar's other side — unless the bar also reaches beyond the next stroke's end, which it would then hold on both sides; such a bar does not let the previous stroke hold its end at the next start (*Append*). Once the end moves to a later bar, that bar is inside the stroke and the stroke starts again (*Append*).

**Finished strokes.** The last stroke does not count as finished while fewer than five unfinished NewBars follow it. This only selects centre members; any stroke may still be revised.

**Unfinished direction.** Once there is a stroke and the unfinished window holds a fractal, the unfinished direction is opposite to the last stroke.

The command line uses the default `min_stroke_bars = 6`; the library `Config` sets it. Until 0.1.4 the engine kept at most 50 strokes and dropped older strokes and the raw bars before the first kept one, so each new stroke past 50 re-segmented the oldest centres from the cut and shifted every index; a centre is fixed by the stroke sequence, not by a retention boundary, so 0.1.5 removed it ([Window](#input)).

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

MACD(12, 26, 9) over the request's raw closes, from its first bar. Each EMA is seeded with the simple mean of its first `n` values and then `alpha * x + (1 - alpha) * prev` with `alpha = 2 / (n + 1)`. The short EMA starts at index `26 - 12` so both EMAs seed on the same bar. DIF = short − long; DEA is the 9-EMA of DIF from its first finite value; hist = DIF − DEA. The first 33 bars have no histogram.

Segments are maximal runs of same-sign histogram bars: zero continues an open run and never starts one; a missing value closes it; `peak` is the value with the largest magnitude (the first wins ties).

## Interval power

Power over `(start_at, end_at]` uses raw bars with `start_at < dt ≤ end_at`; their count is `sample_count`. The MACD reason, in order:

- `missing_coverage`: `start_at ≥ end_at`, either endpoint is not a raw bar, raw times are not strictly increasing, or no rows.
- `non_finite`: a non-finite close at or before `end_at`.
- `warmup`: some row has no MACD value and lies within the first 33 bars; otherwise `non_finite`.

When ready, `positive_area` and `negative_area` are the sums of `max(hist, 0)` and `max(-hist, 0)` over the rows. `dif_at_end`/`dea_at_end` are the values at `end_at` when present. `ready_count` counts rows with all three MACD values. `mean_volume` needs coverage and rows. Unavailable values are `null`, never 0.

For a stroke the interval is its endpoint times, `(start_at, end_at]`, which follow the [stroke endpoints](#strokes) (raw bars, not necessarily fractal times).

**Why the start bar is left out.** Strokes, the unfinished segment, the path after the extreme and the tail follow one another along the raw bars, each starting at the bar where the one before ends. With `(start_at, end_at]` every bar's histogram is counted once along that sequence: the bar where price turned belongs to the piece ending there, and the next piece starts after it. An extension added to the stroke it extends gives exactly that stroke's new interval. Counting the start bar too would put each turn bar in two adjacent pieces. When a wide bar reaches beyond the previous stroke's end, that stroke ends at it ([endpoints](#strokes)); the bars before it then lie inside that stroke and their histogram is its area, not the next stroke's.

## Candidate stroke

Display-only geometry after the last stroke; it never runs the stroke search and never confirms a stroke.

- No stroke: `state = none`, `reason = no_identified_stroke`.
- The anchor is the last stroke's end: `anchor_at` and `anchor_price_at` are its time, `anchor_price` its price.
- `kind = extension` when the last stroke extends (a raw bar after its end is beyond it and no stroke search has replaced it yet, see [later strokes](#strokes)), in the same direction; otherwise `kind = reverse`, in the opposite direction.
- The end is the most extreme raw price in that direction after the anchor, beyond the anchor price, held by the first raw bar reaching it: a later bar at the same price lies after it, in the path after the extreme. None → `state = none`, `reason = no_forward_extreme`; since the bar confirming the last stroke's end fractal is beyond its end, an identified stroke always has one unless prices are not finite. Non-finite prices → `unavailable / non_finite`.
- Otherwise `state = available` with `end_price` and `end_price_at`.

## Geometry snapshot

Every fact appears once. Field names follow the consumer's vocabulary.

| Field | Meaning |
|---|---|
| `status` | `ready` with at least one stroke, otherwise `missing_material` |
| `strokes` | Every stroke, oldest first: `direction`, `start_at`, `end_at`, `high`, `low` (the higher and lower endpoint; no raw bar in `(start_at, end_at]` lies beyond them, except the end bar on the start side, see *End bar*), `start_price`, `end_price`, `price_amplitude` (`|end_price − start_price|` rounded to two decimals), `newbar_count`, `finished`, `power` |
| `pending_direction` | Unfinished direction, or `null` |
| `pending_newbar_count` | NewBars in the unfinished window |
| `fractals` | Fractal list from the second-to-last stroke's `start_at` on (all of it with fewer than two strokes): `dt`, `mark`, `price`, `confirmed_at` |
| `centre_sequence` | Canonical centres: `start_at`, `end_at`, `zd`, `zg`, `zz`, `dd`, `gg`, `member_count`, `member_indices` (into `strokes`), `entry_index` (the stroke before the first member). The sequence includes tail groups of one or two strokes |
| `latest_centre`, `previous_centre` | Indices into `centre_sequence` of the last and second-to-last centre with at least three members that touch the core, or `null` |
| `centre_relation` | When both exist: latest versus previous, `core` on `[zd, zg]` and `outer` on `[dd, gg]`, each `above`, `below`, `touch` or `overlap` |
| `candidate_stroke` | `state`, `reason`, `kind`, `direction`, `anchor_at`, `anchor_price`, `anchor_price_at`, `end_price`, `end_price_at` |
| `candidate_power` | Power over `(anchor_at, last input dt]` of an available candidate, or `null` |
| `candidate_endpoint_power` | Power over `(anchor_at, end_price_at]` of an available candidate, or `null` |
| `macd.bars` | Every raw bar: `dt`, `dif`, `dea`, `hist` (`null` when not finite) |
| `macd.segments` | `sign`, `start_at`, `end_at`, `bars`, `peak`, `peak_at` |
| power | `sample_count`, `mean_volume`, `macd`: `status` (`ready`/`unavailable`), `reason`, `positive_area`, `negative_area`, `dif_at_end`, `dea_at_end`, `ready_count` |

Indices into `strokes` are positions in the request's stroke list, counted from its first stroke; for a fixed window start they change only when the stroke rules revise an earlier stroke. Stages are a separate output, below.

## Stages

Buy-side stages (买点) of the current strokes, each confirmed by a downward **retest** stroke (回试笔) that is the last stroke. They describe completed structure; they are not trading conclusions. Only confirmed stages are produced: no sell side, no stage whose retest stroke has not formed yet, no reasons for absence.

Notation: `S` is the stroke list, `n` its length, `F` the finished count. A **qualified centre** has at least three members that touch its core; "the latest qualified centre over a slice" is the last such centre of the canonical sequence built over that slice of strokes. A stroke's `high`/`low` are its endpoint extremes.

**Path guard.** "The path after stroke `X` stays above `p` up to the cutoff" means every raw bar with `X.end_at < dt ≤ cutoff` has `low > p` (touching `p` ends the stage). The cutoff is the last input when the stage's retest is the last stroke, otherwise the retest's `end_at`.

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

- **Revisions.** Each `dt` is observed once, after its last version. An earlier version of a time is never observed: it records no first-known time. A `stages` request carrying revisions therefore answers exactly as the same request holding only the last version of each `dt`. There is no notion of revising an input that was already observed.
- **Observed inputs.** Observation starts at the first input with `dt ≥ observe_from`, or at the first input when `observe_from` is absent; earlier inputs only build state. When no input is observed (`observe_from` after the last input), every `first_known_at` and `stage_age` is `null`.
- **Same stage.** Kind, the centre's `zd`, `zg` and first three members, and every role stroke (entry, departure, rebound, first-point anchor, retest, parent rebound and retest). Strokes compare by direction, `start_at`, `end_at`, start and end price, high and low. A centre taking more members keeps the stage; any role stroke changing — the retest reaching a new low, for example — makes a new stage.
- **`first_known_at`.** The first observed input at which the same stage stood. A stage already standing at the first observed input has `null`: it began before observation. The time is kept even if the stage disappears and comes back.
- **`stage_age`.** Observed inputs after `first_known_at` up to the last input (0 when first known at the last input); `null` with `first_known_at`.
- **`beyond_start`**, at the last input: the candidate stroke is available, of kind `reverse`, and its `end_price` is above the retest's high — the move after the retest has passed the retest's start.
- **`retest_broken`**, at the last input: some raw bar with `dt` after the retest's `end_at` has a `low` strictly below the retest's low — the retest's low no longer stands (the retest is still extending, or the stage has failed). Independent of `beyond_start`; both describe only what followed the retest. The stage is still listed; whether to exclude it is the caller's choice. Only the stage's own retest is checked, and a stage stands only while its retest is the last stroke.

Per observed input the engine evaluates the three stages once: the centre sequence over the strokes up to four times (second through its first point, third, second-like, and second-like's parent second through its first point), path guards over the raw bars, and MACD over the raw bars only when a first point's geometry holds. A request of `m` inputs costs `O(m · (s + r))` for `s` strokes and `r` raw bars, both growing with the window the caller sends.

## Stages output

| Field | Meaning |
|---|---|
| `stages` | Confirmed stages after the last input, in the order second, third, second-like |
| `kind` | `second`, `third` or `second_like` |
| `centre` | The stage's centre: `start_at`, `zd`, `zg` |
| `entry`, `departure`, `retest` | Role strokes: `start_at`, `end_at`, `high`, `low` |
| `rebound`, `first_point_anchor` | Role strokes of second and second-like, otherwise `null` |
| `parent` | Second-like only: the parent second's `rebound` and `retest`, otherwise `null` |
| `first_known_at`, `stage_age`, `beyond_start`, `retest_broken` | [Observation](#observation) |

## Levels

One instrument at one moment, seen through several period series (levels), coarsest first. Each level is analysed alone, exactly as a `geometry` request (same strokes, centres, MACD, candidate); levels are not built from each other. What `levels` adds is per-level structure facts over a stroke window, and placement of each finer level inside the next coarser one. Nothing here is a trading conclusion.

### Bar time

Every series labels a bar by the **end** of the period it covers. Bar `i` of a level covers `(dt[i-1], dt[i]]`; the first bar covers everything up to `dt[0]`. A bar still forming carries the end of its period too (a daily bar forming during the session is labelled with that session's close time, for example `T15:00:00`). A finer time `t` lies in the coarser bar `i` with `dt[i-1] < t ≤ dt[i]`; a time after the coarser level's last `dt` lies in no coarser bar.

A **point** of a level is one of its raw bars together with a side, high or low (a stroke end, an extreme). Its **child time** is the first bar of the next finer level inside that raw bar's period whose high (for a high point) is the highest, or whose low (for a low point) is the lowest, among the finer bars in that period; its **child price** is that bar's high or low. With no finer bar in the period the child time is `null`. Prices of different levels are compared only through these child times, never directly: levels may be adjusted differently.

### Segments and ranges

A **segment** of a level is one of its window strokes or its unfinished reverse segment (below). Its interval is `(start_at, end_at]`: the start bar belongs to the segment before. The level's **sequence** is, in time order, its window strokes, the unfinished segment (either kind), the path after the extreme, and the **tail** (`"tail"`: the raw bars after all of these when no available candidate covers them, read opposite to the last stroke; with finite prices the candidate is always available, so a response has no tail); every raw bar after the first window stroke's start lies in exactly one element.

- **Raw extreme** of a segment: over the raw bars with `start_at < dt ≤ end_at`, the highest high for an up segment or the lowest low for a down segment, with `at` the first bar holding it. The same rule holds for every raw extreme below (strokes, unfinished segment, path after the extreme, tail, retest, `passed_by`). For a window stroke and the unfinished segment it is the segment's end, which is held by the first bar reaching it too ([endpoints](#strokes)).
- **DIF extreme**: over the same bars, the largest DIF for an up segment or the smallest for a down segment, with its first bar; `null` when some bar in the interval has no DIF.

**Unfinished segment** (`unfinished`): the [candidate stroke](#candidate-stroke) when it is `available`, interval `(anchor_at, end_price_at]`, `start_price = anchor_price`. Only `kind = reverse` is a segment; `kind = extension` (the last stroke is being extended) is reported and is an element of the sequence (so a latest centre's exit can lie in it), but is never a retest, a zone object or a `passed_by` segment.

**Path after the extreme** (`after_extreme`): when the candidate is available, the raw bars with `end_price_at < dt ≤` the last input, read in the direction opposite to the candidate. Every figure covers exactly those bars: the raw extreme (the farthest opposite price, `at`), `bar_count`, the DIF extreme and `power`; only its `price` comparison stops at the raw extreme (start price to extreme). It is a path, not a stroke and not a segment; `null` when no bar follows `end_price_at`.

### Stroke window

`strokes` lists the strokes from the entry stroke of the oldest listed centre to the last stroke, so every listed centre has its entry, members, departure and retest present; with no listed centre, every stroke. A level with a coarser parent also starts early enough to hold every stroke ending after the child time of the parent's last-stroke start, so the [zones](#child-placement) see every child stroke they place. `fractals` lists every fractal (as in the geometry snapshot) whose `dt` is at or after the first window stroke's `start_at`. Indices (`strokes[i]`, `segment` references) are positions in this window.

### Centre list

`centres` holds the latest qualified centre (at least three members touching the core) and up to four qualified centres before it, oldest first, from the canonical sequence over the finished strokes. Each carries its first and last member, `member_count`, `zd`, `zg`, `dd`, `gg`, `start_at`, `end_at` and entry stroke (as in the geometry snapshot), and every centre but the last `versus_next`: its `core` (`[zd, zg]`) and `outer` (`[dd, gg]`) against the next listed centre, each `above`, `below`, `touch` or `overlap` (the [centre relation](#geometry-snapshot) rule). The core is always `above` or `below`: a centre closes on a stroke wholly beyond its core, every attempt after it starts with that stroke or a later one beyond it, so the next qualified core lies strictly on that side.

**Last stroke membership** (`last_stroke_member`): `member` is whether the last stroke is a member of the latest qualified centre, with `reason`: `member`; `not_finished` (fewer than five unfinished NewBars follow it, so it is not yet eligible); `not_returned` (finished, eligible, and not a member: the canonical sequence has closed the latest qualified centre before it, or it opens a later attempt that has not qualified); `no_centre`.

**Last close position** (every listed centre): `price_versus_core` and `price_versus_outer`, the level's last `close` against `[zd, zg]` and `[dd, gg]`: `above`, `inside` (edges included) or `below`.

### Centre state

A centre's state is read from its **path**: the third member's end price, then every raw bar after the third member's `end_at` — for the latest listed centre up to the last input, for an earlier (ended) centre up to the `end_at` of the stroke that closed it in the canonical sequence. A raw bar is `above` the core when its low is above `zg`, `below` when its high is below `zd`, otherwise in the core (touching an edge counts as in the core); the start price is placed the same way, edges inside. The path's **end side** is, for the latest centre, the [last close position](#centre-list) `price_versus_core`; for an ended centre, the closing stroke's end price against the core, which is always outside it.

Exactly one state holds:

- `inside` — latest centre only: the last close is in the core (`price_versus_core = inside`). A last bar that pierces an edge but closes inside is `inside`; one that touches the core but closes outside is `departed`, leaving in that bar.
- `departed` — the end side is outside and some point of the path is not on it. The **exit** is the last such point when it is a bar that closes beyond the edge on the end side and the element of the sequence holding it runs in the exit direction (price left during that bar); otherwise the bar after it (that point itself when it is the last bar). When price touches the core inside a forming pullback and leaves again, the exit lies in that pullback and the departure runs against its exit side; that is a real departure, reported with its exit-side extreme and without departure comparison rows; the departure is the element of the sequence holding the exit — a window stroke, or for the latest centre possibly `unfinished`, `last_extended`, `after_extreme` or `tail`. It is never the third member or earlier.

  **Extended last stroke** (`last_extended`). An extension is the same stroke continuing: once a reverse move completes it, the last stroke ends at the extension's extreme. So for the latest centre, when the exit lies in the extension or in the last stroke while it extends, the departure is the last stroke taken whole, from its `start_at`/`start_price` to the extension's current extreme (`end_price_at`/`end_price`), interval `(start_at, end_price_at]`, valued in progress; it is that stroke for `member` and `new_extreme` too, and has no retest yet. It is not an element of the sequence, a zone object or a `passed_by` segment; `extension` stays the fragment after the stroke's end for the [points](#child-placement) and `extension_vs_last`.
- `formed_out` — the end side is outside and the whole path is on it: formed and never returned. Only a centre with exactly three members can be `formed_out`.

So `inside` holds exactly when the last close is in the core, and `departed` or `formed_out` exactly when it is outside. An ended centre's state is the fixed record of how it was left and never changes once the centre has ended; it is never `inside`. The engine checks these implications and the three-member rule for every centre; a violation is an `internal_error`. An earlier centre also names `ended_by`, the next listed centre. Positions below are window positions (`strokes[i]`) or the sequence names.

Facts by state:

- `departed`: `departure`, its `direction` and raw extreme — in that direction when the departure runs toward the exit side; when it runs against it (the exit lies in a pullback element), on the exit side over the departure's bars from the exit bar on — `new_extreme`: when the departure runs toward its exit side, whether that raw extreme is strictly below the low (`edge = zd`) or strictly above the high (`edge = zg`) of every member other than the departure itself, member lows and highs as in `dd`/`gg` — for a departure that is not a member, strictly beyond `dd` or `gg`; a member departure is part of `dd`/`gg`, so it is `true` exactly when no other member reaches its extreme (a `last_extended` member reaches past its own end, from which `dd`/`gg` were taken, so it is `true` whenever it is beyond them); an equal price (a double bottom or top) is `false`; `null` when the departure runs against its exit side — `edge` — the exit side, independent of the departure's own direction: `zg` when price left upward (end side above), `zd` when it left downward (a down stroke whose raw bars dip into the core and end above it departs through `zg`) — `member` (whether the departure is a member stroke), `first_beyond_at` (the exit bar), `retest` — the element right after a stroke departure when it is a window stroke or the unfinished reverse segment, otherwise `null` (no retest yet) — with its raw extreme toward the core (`retest_extreme`: its lowest low after an upward exit, its highest high after a downward one) and `versus_edge`: `beyond` (strictly beyond `edge`), `at` (equal) or `inside` (back in the core). Every bar after the exit is strictly beyond the edge, so a retest inside the path is always `beyond`: `at` and `inside` occur only for an ended centre whose retest follows its closing stroke. The latest centre carries `after_retest` (the positions after the retest; empty without a retest) and `retest_role: null`. An ended centre's record stops at the retest, kept even when it is the next listed centre's entry or one of its members: `retest_role` names that role by reference, {`centre` (position in `centres`), `role`: `entry` or `member`}, or `null`; `after_retest` is `null`.
- `inside`: `segment`, the element holding the last bar (the third member when no bar follows it).
- `formed_out`: `third` and `after` (the positions after it: to the latest element for the latest centre, to the closing stroke for an ended one).

### Comparisons

Only segments of the same direction are compared, numerator against denominator: `area` (the same-direction MACD area: positive area for up, negative area for down), `price` (`|end_price − start_price|`; for the path after the extreme, from its start price to its raw extreme), `mean_volume` and `bar_count` (raw bars in the interval). A ratio is `null` when the numerator is negative, the denominator is not positive, or either is unavailable. Between the two (raw bars from the earlier one's end to the later one's start, both included): `min_abs_dif`, `dif_crossed_zero` and `dea_crossed_zero` (both a strictly positive and a strictly negative value occur); `null` values when any of those bars lacks the value.

`completed` rows compare window strokes (the last stroke included, finished or not); `in_progress` rows have a numerator that is still moving (the unfinished segment, the path after the extreme, an extension, the extended last stroke, the tail, or a departure that is one of these), valued at its current extreme. A row comparing the extending last stroke with another stroke reads it whole: the `last_extended` departure, and the advance when it is the extending last stroke (`advance_vs_departure`, numerator `last_extended`); `last_vs_previous` keeps the last stroke to its end and `extension_vs_last` the fragment after it. The tail's `price` runs from the last stroke's end price to the tail's raw extreme. `departure_vs_entry`, `advance_vs_departure` and `previous_departure_vs_entry` exist only when the departure runs toward its exit side (`direction` up with `edge = zg`, down with `zd`). Rows, when both sides exist and share a direction:

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
| `state` | `kind` = `departed`, `inside` or `formed_out` and that state's facts ([centre state](#centre-state)); positions are indices into `strokes` or the sequence names; `departed` carries `new_extreme` (`null` against its exit side) and `after_retest` (latest) or `retest_role` (ended), the other `null` |
| `comparisons` | `completed` and `in_progress`: rows `pair`, `numerator`, `denominator` (positions as above, `"after_extreme"`, `"extension"`, `"last_extended"`), `area`, `price`, `mean_volume`, `bar_count`, `between` {`min_abs_dif`, `dif_crossed_zero`, `dea_crossed_zero`} |
| `child` | Every level but the finest, with `child_reason`: `null` with `child_reason = missing_material` when this level has no strokes, otherwise `child_reason = null` and the object: `name` of the child level, `points` {`last_stroke`, `unfinished`, `extension`, `after_extreme`}: each `at`, `price`, `child_at`, `child_price`, `passed_by` {`segment`, `end_at`, `price`} or `null` (the point is `null` when the parent object is absent), `forces` {`last_stroke`, `unfinished`: comparison rows, `child_starts_after`}, `zones`: {`boundaries` {`last_start`, `last_end`, `unfinished_extreme`, each `null` when before the child series}, `child_starts_after`, `objects`: `kind` (`stroke`, `unfinished`, `after_extreme`, `centre`), `index` (child window position; for a centre its position in the child's `centres`), `start_at`, `end_at`, `zone`, `crosses` (boundary names)} or `null` with `zones_reason` |

Segment positions use one vocabulary: an integer is `strokes[i]` of that level; `"unfinished"`, `"extension"`, `"last_extended"`, `"after_extreme"` and `"tail"` name that level's objects.
