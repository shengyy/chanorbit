//! Nest signals of the `signals` command: the completion of a move inside a
//! span, the buy-side third-buy nest (`third_buy_nest`) and the sell-side
//! up-end nest (`up_end_nest`). Definitions are owned by
//! `docs/structure-contract.md#signals`; nothing here is a trading conclusion.
use crate::analyzer::Analyzer;
use crate::bar::RawBar;
use crate::centre::{Centre, canonical_centres};
use crate::fractal::{Fractal, Mark};
use crate::levels::{Pos, Seg, State, counted, read_strokes, tail};
use crate::levels_child::child_time;
use crate::levels_state::centre_state;
use crate::macd::MacdSeries;
use crate::stroke::{Direction, Endpoint, extreme};
use chrono::NaiveDateTime;
use serde::Serialize;

const KEY_TIME: &str = "%Y-%m-%dT%H:%M:%S";

/// One level as the signals read it. `macd` may be the MACD of a longer
/// series that starts with this level's raw bars: MACD is causal, so its
/// values over this level's bars are the same.
pub struct LevelRead<'a> {
    pub(crate) c: &'a Analyzer,
    macd: &'a MacdSeries,
    /// The last stroke's current extreme while it extends.
    extension: Option<Endpoint>,
    /// Strokes counted for centres (the extending last stroke as read).
    pub(crate) finished: usize,
    pub(crate) unfinished: Option<Seg>,
    pub(crate) after: Option<Seg>,
}

impl<'a> LevelRead<'a> {
    pub fn new(c: &'a Analyzer, macd: &'a MacdSeries) -> Self {
        let (extension, finished) = counted(c);
        let (unfinished, after) = match c.strokes().last() {
            Some(last) => {
                let end = extension.unwrap_or(last.end);
                let (u, a) = tail(c, last.direction, end.at, end.price);
                (u, a.map(|a| a.0))
            }
            None => (None, None),
        };
        Self {
            c,
            macd,
            extension,
            finished,
            unfinished,
            after,
        }
    }

    pub fn raw(&self) -> &[RawBar] {
        self.c.raw_bars()
    }

    pub(crate) fn n(&self) -> usize {
        self.c.strokes().len()
    }

    /// Stroke `k`, the last one read to its current extreme while it extends.
    pub(crate) fn seg(&self, k: usize) -> Seg {
        let s = &self.c.strokes()[k];
        let end = match self.extension {
            Some(x) if k + 1 == self.n() => x,
            _ => s.end,
        };
        Seg {
            pos: Pos::Stroke(k),
            direction: s.direction,
            start_at: s.start.at,
            end_at: end.at,
            start_price: s.start.price,
            end_price: end.price,
        }
    }

    /// Element `k`: a stroke, or the unfinished reverse segment at `k = n`.
    fn element(&self, k: usize) -> Option<Seg> {
        if k < self.n() {
            Some(self.seg(k))
        } else {
            self.unfinished.filter(|_| k == self.n())
        }
    }

    /// Whether element `k` is still moving: the extending last stroke or the
    /// unfinished segment.
    pub(crate) fn moving(&self, k: usize) -> bool {
        k == self.n() || (k + 1 == self.n() && self.extension.is_some())
    }

    /// The same-direction MACD area of the interval power over
    /// `(start_at, end_at]` (positive area for up, negative for down), or
    /// `None` when that power is unavailable.
    pub(crate) fn area(&self, s: &Seg) -> Option<f64> {
        let raw = self.raw();
        if s.start_at >= s.end_at {
            return None;
        }
        let a = raw.partition_point(|b| b.dt <= s.start_at);
        let b = raw.partition_point(|b| b.dt <= s.end_at);
        let covered = a > 0 && raw[a - 1].dt == s.start_at && b > a && raw[b - 1].dt == s.end_at;
        let m = self.macd;
        let ready = (a..b).all(|i| {
            i < m.hist.len()
                && m.dif[i].is_finite()
                && m.dea[i].is_finite()
                && m.hist[i].is_finite()
        });
        if !covered || !ready {
            return None;
        }
        let up = s.direction == Direction::Up;
        let sum: f64 = (a..b)
            .map(|i| {
                if up {
                    m.hist[i].max(0.)
                } else {
                    (-m.hist[i]).max(0.)
                }
            })
            .sum();
        sum.is_finite().then_some(sum)
    }

    /// Confirmation time of the first `mark` fractal at `ext`'s price from
    /// its time on, among the fractals from stroke `from` on and the
    /// unfinished ones.
    pub(crate) fn confirmed(
        &self,
        from: usize,
        mark: Mark,
        ext: Endpoint,
    ) -> Option<NaiveDateTime> {
        let hit = |f: &&Fractal| f.mark == mark && f.price == ext.price && f.dt >= ext.at;
        let s = self.c.strokes();
        s[from.min(s.len())..]
            .iter()
            .flat_map(|st| st.fractals.iter())
            .find(hit)
            .and_then(Fractal::confirmed_at)
            .or_else(|| {
                self.c
                    .pending_fractals()
                    .iter()
                    .find(hit)
                    .and_then(Fractal::confirmed_at)
            })
    }

    /// The latest up element (stroke as read, or the unfinished segment)
    /// whose high no later raw bar exceeds, with its element index.
    pub(crate) fn latest_up(&self) -> Option<(usize, Seg)> {
        let raw = self.raw();
        (0..=self.n()).rev().find_map(|k| {
            let s = self.element(k).filter(|s| s.direction == Direction::Up)?;
            let later = &raw[raw.partition_point(|b| b.dt <= s.end_at)..];
            (!later.iter().any(|b| b.high > s.end_price)).then_some((k, s))
        })
    }

    /// Whether the element holding the bar at `t` is a counted stroke, so the
    /// structure up to `t` is settled.
    pub(crate) fn counted_at(&self, t: NaiveDateTime) -> bool {
        let k = self.c.strokes().partition_point(|x| x.start_at() < t);
        k.checked_sub(1)
            .is_some_and(|k| k < self.finished && t <= self.seg(k).end_at)
    }

    /// Whether a qualified centre forms among the counted strokes (as read)
    /// that start at or after `from` and end at or before `to`, with no
    /// alignment or entry rule.
    pub(crate) fn centre_within(&self, from: NaiveDateTime, to: NaiveDateTime) -> bool {
        let s = self.c.strokes();
        let k0 = s.partition_point(|x| x.start_at() < from);
        let k1 = (k0..self.finished)
            .take_while(|&k| self.seg(k).end_at <= to)
            .last()
            .map_or(k0, |k| k + 1);
        let sub = read_strokes(s, k0, k1, self.extension);
        canonical_centres(&sub)
            .iter()
            .any(|z| z.member_count() >= 3 && z.members_touch_core(&sub))
    }

    /// The qualified centres over the counted strokes.
    pub(crate) fn qualified_centres(&self) -> Vec<Centre> {
        let fs = read_strokes(self.c.strokes(), 0, self.finished, self.extension);
        canonical_centres(&fs)
            .into_iter()
            .filter(|z| z.member_count() >= 3 && z.members_touch_core(&fs))
            .collect()
    }
}

/// The child time of a parent point, only when the child series covers the
/// whole parent bar: its first bar is not after the parent bar's start.
pub(crate) fn covered_child(
    parent: &[RawBar],
    child: &[RawBar],
    at: NaiveDateTime,
    high: bool,
) -> Option<NaiveDateTime> {
    let k = parent.partition_point(|b| b.dt < at);
    let first = child.first()?.dt;
    let covered = match k.checked_sub(1) {
        Some(p) => first <= parent[p].dt,
        None => first <= at,
    };
    if !covered {
        return None;
    }
    child_time(parent, child, at, high).map(|c| c.0)
}

pub(crate) fn beyond(d: Direction, p: f64, than: f64) -> bool {
    match d {
        Direction::Up => p > than,
        Direction::Down => p < than,
    }
}

/// The end price of `s` on side `d`: its high for up, its low for down.
pub(crate) fn side_price(s: &Seg, d: Direction) -> f64 {
    match d {
        Direction::Up => s.start_price.max(s.end_price),
        Direction::Down => s.start_price.min(s.end_price),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CentreFact {
    pub start_at: NaiveDateTime,
    pub zd: f64,
    pub zg: f64,
    pub dd: f64,
    pub gg: f64,
    pub member_count: usize,
}

impl From<&Centre> for CentreFact {
    fn from(z: &Centre) -> Self {
        Self {
            start_at: z.start_at,
            zd: z.core_low,
            zg: z.core_high,
            dd: z.low,
            gg: z.high,
            member_count: z.member_count(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EntryFact {
    pub start_at: NaiveDateTime,
    pub end_at: NaiveDateTime,
    pub area: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LastFact {
    pub start_at: NaiveDateTime,
    pub end_at: NaiveDateTime,
    pub extending: bool,
    pub area: Option<f64>,
}

/// A completed move: its local centre, entry, last segment, their area
/// ratio and the confirmation time of the opposite fractal, if any.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CompletionFact {
    pub centre: CentreFact,
    pub entry: EntryFact,
    pub last: LastFact,
    pub area_ratio: f64,
    pub confirmed_at: Option<NaiveDateTime>,
}

/// A move in progress around its local centre: `last` is the latest
/// element when it runs in the move's direction from the departure
/// position, `new_extreme` whether it holds the span's extreme.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MoveFact {
    pub centre: CentreFact,
    pub entry: EntryFact,
    pub last: Option<LastFact>,
    pub new_extreme: bool,
    pub area_ratio: Option<f64>,
    pub confirmed_at: Option<NaiveDateTime>,
}

impl From<CompletionFact> for MoveFact {
    fn from(c: CompletionFact) -> Self {
        Self {
            centre: c.centre,
            entry: c.entry,
            last: Some(c.last),
            new_extreme: true,
            area_ratio: Some(c.area_ratio),
            confirmed_at: c.confirmed_at,
        }
    }
}

/// A completion with what the nests read from it.
pub struct Completion {
    pub fact: CompletionFact,
    /// The span's extreme, held by `last`.
    pub extreme: Endpoint,
    pub last: Seg,
}

/// Items 0–1 of a move over a span: the strokes `k0..k1` starting inside
/// it, the last qualified centre of their local sequence (members
/// `first..=last`) and its entry, which runs in the move's direction.
struct Local {
    k0: usize,
    k1: usize,
    centre: Centre,
    first: usize,
    last: usize,
    entry: Seg,
}

impl Local {
    /// Whether element `k` sits where it leaves the centre: its last member
    /// from the fourth on, or the element right after the last member.
    fn departs(&self, k: usize) -> bool {
        (k == self.last && k >= self.first + 3) || k == self.last + 1
    }

    /// Whether `x`, element `k` holding `ext`, reaches a new extreme
    /// beyond every other member.
    fn new_extreme(&self, r: &LevelRead, k: usize, ext: Endpoint, dir: Direction) -> bool {
        (self.first..=self.last)
            .filter(|&i| i != k)
            .all(|i| beyond(dir, ext.price, side_price(&r.seg(i), dir)))
    }
}

/// `before`: the span ends at its extreme, so a stroke starting at `until`
/// (the move after the extreme) is left out.
fn local(
    r: &LevelRead,
    from: NaiveDateTime,
    until: Option<NaiveDateTime>,
    dir: Direction,
    before: bool,
) -> Option<Local> {
    let s = r.c.strokes();
    let n = s.len();
    // 0. The strokes starting inside the span; the first starts at its start.
    let k0 = s.partition_point(|x| x.start_at() < from);
    let first = s.get(k0)?;
    if first.start_at() != from || first.direction != dir {
        return None;
    }
    let k1 = until.map_or(n, |u| {
        s.partition_point(|x| x.start_at() < u || (!before && x.start_at() == u))
    });
    let sub = read_strokes(s, k0, k1.min(r.finished).max(k0), r.extension);
    // 1. The last qualified centre of the local sequence, entered in `dir`.
    let z = canonical_centres(&sub)
        .into_iter()
        .rfind(|z| z.member_count() >= 3 && z.members_touch_core(&sub))?;
    let (first_member, last_member) = (k0 + z.start, k0 + z.end - 1);
    let entry = r.seg(first_member - 1);
    (entry.direction == dir).then_some(Local {
        k0,
        k1,
        centre: z,
        first: first_member,
        last: last_member,
        entry,
    })
}

/// The span's extreme in direction `dir` over the raw bars after `from` up
/// to `until` (the last input when `None`).
fn span_extreme(
    r: &LevelRead,
    from: NaiveDateTime,
    until: Option<NaiveDateTime>,
    dir: Direction,
) -> Option<Endpoint> {
    let raw = r.raw();
    let end = until.or(raw.last().map(|b| b.dt))?;
    let a = raw.partition_point(|b| b.dt <= from);
    let b = raw.partition_point(|b| b.dt <= end).max(a);
    extreme(&raw[a..b], dir)
}

fn holds(x: &Seg, ext: Endpoint) -> bool {
    x.start_at < ext.at && ext.at <= x.end_at
}

fn opposite_mark(dir: Direction) -> Mark {
    match dir {
        Direction::Down => Mark::Bottom,
        Direction::Up => Mark::Top,
    }
}

fn entry_fact(entry: &Seg, area: Option<f64>) -> EntryFact {
    EntryFact {
        start_at: entry.start_at,
        end_at: entry.end_at,
        area,
    }
}

fn last_fact(r: &LevelRead, k: usize, x: &Seg, area: Option<f64>) -> LastFact {
    LastFact {
        start_at: x.start_at,
        end_at: x.end_at,
        extending: r.moving(k),
        area,
    }
}

/// `completion(level, span, dir)`: the move in direction `dir` over the
/// span from `from` (the start of its first stroke) to `until` (the last
/// input when `None`) has completed (items 1–3); `confirmed_at` is item 4.
pub fn completion(
    r: &LevelRead,
    from: NaiveDateTime,
    until: Option<NaiveDateTime>,
    dir: Direction,
) -> Option<Completion> {
    completion_in(r, from, until, dir, false)
}

/// The completion of the move from `from` that ends at its extreme at
/// `to`: nothing after `to` enters it.
fn completion_to(
    r: &LevelRead,
    from: NaiveDateTime,
    to: NaiveDateTime,
    dir: Direction,
) -> Option<Completion> {
    completion_in(r, from, Some(to), dir, true)
}

fn completion_in(
    r: &LevelRead,
    from: NaiveDateTime,
    until: Option<NaiveDateTime>,
    dir: Direction,
    before: bool,
) -> Option<Completion> {
    let n = r.n();
    let loc = local(r, from, until, dir, before)?;
    // 2. The element holding the span's extreme leaves the centre to a new
    // extreme beyond every other member.
    let ext = span_extreme(r, from, until, dir)?;
    let (xi, x) = (loc.k0..loc.k1)
        .map(|k| (k, r.seg(k)))
        .find(|(_, x)| holds(x, ext))
        .or_else(|| {
            r.unfinished
                .filter(|u| loc.k1 == n && holds(u, ext))
                .map(|u| (n, u))
        })?;
    if x.direction != dir || !loc.departs(xi) || !loc.new_extreme(r, xi, ext, dir) {
        return None;
    }
    // 3. Divergence: the last segment's same-direction area is smaller.
    let (entry_area, last_area) = (r.area(&loc.entry)?, r.area(&x)?);
    if last_area.partial_cmp(&entry_area) != Some(std::cmp::Ordering::Less) {
        return None;
    }
    // 4. The opposite fractal at the extreme.
    let confirmed_at = r.confirmed(xi, opposite_mark(dir), ext);
    Some(Completion {
        fact: CompletionFact {
            centre: CentreFact::from(&loc.centre),
            entry: entry_fact(&loc.entry, Some(entry_area)),
            last: last_fact(r, xi, &x, Some(last_area)),
            area_ratio: last_area / entry_area,
            confirmed_at,
        },
        extreme: ext,
        last: x,
    })
}

/// The move over the span from `from` to the last input while it has not
/// completed: items 0–1 hold, and `last` is the latest element when it runs
/// in `dir` from the departure position; `None` without the local centre.
pub fn progress(r: &LevelRead, from: NaiveDateTime, dir: Direction) -> Option<MoveFact> {
    let loc = local(r, from, None, dir, false)?;
    let n = r.n();
    let latest = match r.unfinished {
        Some(u) => (n, u),
        None => (n - 1, r.seg(n - 1)),
    };
    let entry_area = r.area(&loc.entry);
    let candidate = Some(latest).filter(|(k, x)| x.direction == dir && loc.departs(*k));
    let (last, new_extreme, area_ratio, confirmed_at) = match candidate {
        Some((k, x)) => {
            let area = r.area(&x);
            let ext = span_extreme(r, from, None, dir)
                .filter(|&e| holds(&x, e) && loc.new_extreme(r, k, e, dir));
            let ratio = match (area, entry_area) {
                (Some(a), Some(e)) if e > 0. => Some(a / e),
                _ => None,
            };
            let confirmed = ext.and_then(|e| r.confirmed(k, opposite_mark(dir), e));
            (
                Some(last_fact(r, k, &x, area)),
                ext.is_some(),
                ratio,
                confirmed,
            )
        }
        None => (None, false, None, None),
    };
    Some(MoveFact {
        centre: CentreFact::from(&loc.centre),
        entry: entry_fact(&loc.entry, entry_area),
        last,
        new_extreme,
        area_ratio,
        confirmed_at,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Armed,
    Primed,
    Ripe,
    Turned,
    Triggered,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DepartureFact {
    pub start_at: NaiveDateTime,
    pub end_at: NaiveDateTime,
    pub high: f64,
    pub new_extreme: bool,
    pub area: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Pullback {
    pub low: f64,
    pub low_at: NaiveDateTime,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Buy {
    pub stage: Stage,
    pub key: String,
    pub centre: CentreFact,
    pub departure: DepartureFact,
    pub pullback: Pullback,
    pub mid: Option<MoveFact>,
    pub leaf: Option<CompletionFact>,
    /// Whether a qualified leaf centre lies inside the leaf span up to the
    /// low; `null` when C is not evaluated.
    pub leaf_centre: Option<bool>,
}

/// A1 on the parent level: the latest qualified centre departed upward
/// through `zg` by a formed up stroke.
#[derive(Debug, Clone, PartialEq)]
pub struct ParentDeparture {
    centre: Centre,
    index: usize,
    stroke: Seg,
    new_extreme: bool,
    area: Option<f64>,
    /// `[zd, zg]` of every qualified centre before `centre`.
    earlier: Vec<(f64, f64)>,
}

/// A1: the parent's latest qualified centre is `departed` through `zg` and
/// its departure is a stroke running up that no longer extends. The cores
/// of the earlier qualified centres are kept for A5.
pub fn parent_departure(p: &LevelRead) -> Option<ParentDeparture> {
    let s = p.c.strokes();
    let n = s.len();
    let mut qualified = p.qualified_centres();
    let z = qualified.pop()?;
    let earlier = qualified
        .iter()
        .map(|c| (c.core_low, c.core_high))
        .collect();
    let w0 = z.start - 1;
    let segs: Vec<Seg> = (w0..n)
        .map(|k| Seg {
            pos: Pos::Stroke(k - w0),
            ..p.seg(k)
        })
        .chain(p.unfinished)
        .chain(p.after)
        .collect();
    let State::Departed {
        departure: Pos::Stroke(i),
        direction: Direction::Up,
        edge: "zg",
        new_extreme: Some(new_extreme),
        ..
    } = centre_state(p.raw(), &segs, w0, &z, None, 0)
    else {
        return None;
    };
    let index = i + w0;
    if p.moving(index) {
        return None;
    }
    let stroke = p.seg(index);
    Some(ParentDeparture {
        centre: z,
        index,
        stroke,
        new_extreme,
        area: p.area(&stroke),
        earlier,
    })
}

/// The buy-side nest over the parent `p`, the middle level `m` and the
/// optional leaf `l`, given A1 of `p`.
pub fn buy(
    p: &LevelRead,
    m: &LevelRead,
    l: Option<&LevelRead>,
    dep: Option<&ParentDeparture>,
) -> Option<Buy> {
    let dep = dep?;
    let d = dep.stroke;
    let h_dep = d.end_price;
    let finest = l.unwrap_or(m).raw();
    // A2: the pullback low on the finest level stays above `zg`.
    let t_high = covered_child(p.raw(), finest, d.end_at, true)?;
    let low = extreme(
        &finest[finest.partition_point(|b| b.dt <= t_high)..],
        Direction::Down,
    )?;
    if low.price.partial_cmp(&dep.centre.core_high) != Some(std::cmp::Ordering::Greater) {
        return None;
    }
    // A3: no parent up stroke has completed between the high and the low.
    let second = (dep.index + 1..p.n()).map(|k| p.seg(k)).any(|s| {
        s.direction == Direction::Up
            && child_time(p.raw(), finest, s.end_at, true).map_or(s.end_at, |c| c.0) < low.at
    });
    // A4: the move after the low has not reached the departure's high.
    let passed = finest[finest.partition_point(|b| b.dt <= low.at)..]
        .iter()
        .any(|b| b.high >= h_dep);
    // A5: the low is not back inside an earlier parent centre's core.
    let returned = dep
        .earlier
        .iter()
        .any(|&(zd, zg)| zd <= low.price && low.price <= zg);
    if second {
        return None;
    }
    // A4 or A5 failing caps the nest at `armed`: both are read on a
    // provisional low and heal with a new low, which is a new key.
    let (stage, mid, leaf, leaf_centre) = if passed || returned {
        (Stage::Armed, None, None, None)
    } else {
        // B: the pullback completes on the middle level at the low.
        let t0 = covered_child(p.raw(), m.raw(), d.end_at, true);
        let mid = t0
            .and_then(|t0| completion(m, t0, None, Direction::Down))
            .filter(|c| c.extreme.price == low.price);
        // C, a veto on the leaf over the down move from the start of the middle
        // level's last segment to the low (the finest level is the leaf, so the
        // low's time is a leaf time): it completes there at the low, or no leaf
        // centre lies inside it.
        let leaf_span = match (&mid, l) {
            (Some(mid), Some(l)) => {
                covered_child(m.raw(), l.raw(), mid.last.start_at, true).map(|t0| (l, t0))
            }
            _ => None,
        };
        let leaf = leaf_span
            .and_then(|(l, t0)| completion_to(l, t0, low.at, Direction::Down))
            .filter(|c| c.extreme.price == low.price);
        let turned = mid.as_ref().is_some_and(|c| c.fact.confirmed_at.is_some());
        // C waits until the leaf element holding the low is a counted stroke.
        let leaf_centre = leaf_span
            .filter(|(l, _)| turned && l.counted_at(low.at))
            .map(|(l, t0)| l.centre_within(t0, low.at));
        let stage = match (&mid, leaf_centre) {
            (Some(_), Some(centre)) if leaf.is_some() || !centre => Stage::Triggered,
            (Some(_), _) if turned => Stage::Turned,
            (Some(_), _) => Stage::Ripe,
            (None, _) => Stage::Armed,
        };
        // Without B the move in progress, primed once its latest element leaves
        // the local centre downward.
        let mid = match mid {
            Some(c) => Some(MoveFact::from(c.fact)),
            None => t0.and_then(|t0| progress(m, t0, Direction::Down)),
        };
        let stage = match &mid {
            Some(x) if stage == Stage::Armed && x.last.is_some() => Stage::Primed,
            _ => stage,
        };
        (stage, mid, leaf.map(|c| c.fact), leaf_centre)
    };
    let z = &dep.centre;
    Some(Buy {
        stage,
        key: format!(
            "{}|{}|{}",
            z.start_at.format(KEY_TIME),
            d.end_at.format(KEY_TIME),
            low.at.format(KEY_TIME)
        ),
        centre: CentreFact::from(z),
        departure: DepartureFact {
            start_at: d.start_at,
            end_at: d.end_at,
            high: h_dep,
            new_extreme: dep.new_extreme,
            area: dep.area,
        },
        pullback: Pullback {
            low: low.price,
            low_at: low.at,
        },
        leaf,
        mid,
        leaf_centre,
    })
}

#[cfg(test)]
#[path = "signal_tests.rs"]
mod tests;
