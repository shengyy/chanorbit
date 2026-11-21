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
    c: &'a Analyzer,
    macd: &'a MacdSeries,
    /// The last stroke's current extreme while it extends.
    extension: Option<Endpoint>,
    /// Strokes counted for centres (the extending last stroke as read).
    finished: usize,
    unfinished: Option<Seg>,
    after: Option<Seg>,
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

    fn n(&self) -> usize {
        self.c.strokes().len()
    }

    /// Stroke `k`, the last one read to its current extreme while it extends.
    fn seg(&self, k: usize) -> Seg {
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
    fn moving(&self, k: usize) -> bool {
        k == self.n() || (k + 1 == self.n() && self.extension.is_some())
    }

    /// The same-direction MACD area of the interval power over
    /// `(start_at, end_at]` (positive area for up, negative for down), or
    /// `None` when that power is unavailable.
    fn area(&self, s: &Seg) -> Option<f64> {
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
    fn confirmed(&self, from: usize, mark: Mark, ext: Endpoint) -> Option<NaiveDateTime> {
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
    fn latest_up(&self) -> Option<(usize, Seg)> {
        let raw = self.raw();
        (0..=self.n()).rev().find_map(|k| {
            let s = self.element(k).filter(|s| s.direction == Direction::Up)?;
            let later = &raw[raw.partition_point(|b| b.dt <= s.end_at)..];
            (!later.iter().any(|b| b.high > s.end_price)).then_some((k, s))
        })
    }

    /// The qualified centres over the counted strokes.
    fn qualified_centres(&self) -> Vec<Centre> {
        let fs = read_strokes(self.c.strokes(), 0, self.finished, self.extension);
        canonical_centres(&fs)
            .into_iter()
            .filter(|z| z.member_count() >= 3 && z.members_touch_core(&fs))
            .collect()
    }
}

/// The child time of a parent point, only when the child series covers the
/// whole parent bar: its first bar is not after the parent bar's start.
fn covered_child(
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

fn beyond(d: Direction, p: f64, than: f64) -> bool {
    match d {
        Direction::Up => p > than,
        Direction::Down => p < than,
    }
}

/// The end price of `s` on side `d`: its high for up, its low for down.
fn side_price(s: &Seg, d: Direction) -> f64 {
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
    pub area: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LastFact {
    pub start_at: NaiveDateTime,
    pub end_at: NaiveDateTime,
    pub extending: bool,
    pub area: f64,
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

/// A completion with what the nests read from it.
pub struct Completion {
    pub fact: CompletionFact,
    /// The span's extreme, held by `last`.
    pub extreme: Endpoint,
    pub last: Seg,
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
    let s = r.c.strokes();
    let n = s.len();
    // 0. The strokes starting inside the span; the first starts at its start.
    let k0 = s.partition_point(|x| x.start_at() < from);
    let first = s.get(k0)?;
    if first.start_at() != from || first.direction != dir {
        return None;
    }
    let k1 = until.map_or(n, |u| s.partition_point(|x| x.start_at() <= u));
    let sub = read_strokes(s, k0, k1.min(r.finished).max(k0), r.extension);
    // 1. The last qualified centre of the local sequence, entered in `dir`.
    let z = canonical_centres(&sub)
        .into_iter()
        .rfind(|z| z.member_count() >= 3 && z.members_touch_core(&sub))?;
    let (first_member, last_member) = (k0 + z.start, k0 + z.end - 1);
    let entry = r.seg(first_member - 1);
    if entry.direction != dir {
        return None;
    }
    // 2. The element holding the span's extreme leaves the centre to a new
    // extreme beyond every other member.
    let raw = r.raw();
    let end = until.or(raw.last().map(|b| b.dt))?;
    let a = raw.partition_point(|b| b.dt <= from);
    let b = raw.partition_point(|b| b.dt <= end).max(a);
    let ext = extreme(&raw[a..b], dir)?;
    let holds = |x: &Seg| x.start_at < ext.at && ext.at <= x.end_at;
    let (xi, x) = (k0..k1)
        .map(|k| (k, r.seg(k)))
        .find(|(_, x)| holds(x))
        .or_else(|| r.unfinished.filter(|u| k1 == n && holds(u)).map(|u| (n, u)))?;
    let departs = (xi == last_member && xi >= first_member + 3) || xi == last_member + 1;
    if x.direction != dir || !departs {
        return None;
    }
    let new_extreme = (first_member..=last_member)
        .filter(|&i| i != xi)
        .all(|i| beyond(dir, ext.price, side_price(&r.seg(i), dir)));
    if !new_extreme {
        return None;
    }
    // 3. Divergence: the last segment's same-direction area is smaller.
    let (entry_area, last_area) = (r.area(&entry)?, r.area(&x)?);
    if last_area.partial_cmp(&entry_area) != Some(std::cmp::Ordering::Less) {
        return None;
    }
    // 4. The opposite fractal at the extreme.
    let mark = match dir {
        Direction::Down => Mark::Bottom,
        Direction::Up => Mark::Top,
    };
    let confirmed_at = r.confirmed(xi, mark, ext);
    Some(Completion {
        fact: CompletionFact {
            centre: CentreFact::from(&z),
            entry: EntryFact {
                start_at: entry.start_at,
                end_at: entry.end_at,
                area: entry_area,
            },
            last: LastFact {
                start_at: x.start_at,
                end_at: x.end_at,
                extending: r.moving(xi),
                area: last_area,
            },
            area_ratio: last_area / entry_area,
            confirmed_at,
        },
        extreme: ext,
        last: x,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Armed,
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
    pub mid: Option<CompletionFact>,
    pub leaf: Option<CompletionFact>,
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
}

/// A1: the parent's latest qualified centre is `departed` through `zg` and
/// its departure is a stroke running up that no longer extends.
pub fn parent_departure(p: &LevelRead) -> Option<ParentDeparture> {
    let s = p.c.strokes();
    let n = s.len();
    let fs = read_strokes(s, 0, p.finished, p.extension);
    let z = canonical_centres(&fs)
        .into_iter()
        .rfind(|z| z.member_count() >= 3 && z.members_touch_core(&fs))?;
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
    if second || passed {
        return None;
    }
    // B: the pullback completes on the middle level at the low.
    let mid = covered_child(p.raw(), m.raw(), d.end_at, true)
        .and_then(|t0| completion(m, t0, None, Direction::Down))
        .filter(|c| c.extreme.price == low.price);
    // C: the middle level's last segment completes on the leaf, confirmed.
    let leaf = match (&mid, l) {
        (Some(mid), Some(l)) => covered_child(m.raw(), l.raw(), mid.last.start_at, true)
            .and_then(|t0| completion(l, t0, None, Direction::Down))
            .filter(|c| c.extreme.price == low.price),
        _ => None,
    };
    let stage = match (&mid, &leaf) {
        (Some(_), Some(leaf)) if leaf.fact.confirmed_at.is_some() => Stage::Triggered,
        (Some(mid), _) if mid.fact.confirmed_at.is_some() => Stage::Turned,
        (Some(_), _) => Stage::Ripe,
        (None, _) => Stage::Armed,
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
        leaf: leaf.map(|c| c.fact),
        mid: mid.map(|c| c.fact),
    })
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SegmentFact {
    pub start_at: NaiveDateTime,
    pub end_at: NaiveDateTime,
    pub high: f64,
    pub extending: bool,
    pub area: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Sell {
    /// Always `triggered`: the nest stands only once confirmed on the leaf.
    pub stage: Stage,
    pub key: String,
    pub segment: SegmentFact,
    pub leaf: CompletionFact,
    pub mid_divergent: bool,
    pub mid_lower_high: bool,
    pub parent_complete: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Support {
    pub low: f64,
    pub low_at: NaiveDateTime,
}

/// The completion of `level` in direction up over the span of `s`, a segment
/// of the next coarser level `coarse`, mapped into child time.
fn completion_inside(coarse: &LevelRead, level: &LevelRead, s: &Seg) -> Option<Completion> {
    let t0 = covered_child(coarse.raw(), level.raw(), s.start_at, false)?;
    let t1 = covered_child(coarse.raw(), level.raw(), s.end_at, true)?;
    completion(level, t0, Some(t1), Direction::Up)
}

/// The sell-side nest: the middle level's latest up segment `U` completes on
/// the leaf, confirmed at `U`'s high. `centres` are the middle level's
/// qualified centres and `parent_complete` the parent's upswing completion.
pub fn sell(
    m: &LevelRead,
    l: Option<&LevelRead>,
    centres: &[Centre],
    parent_complete: bool,
) -> Option<Sell> {
    let l = l?;
    let (ui, u) = m.latest_up()?;
    let leaf = completion_inside(m, l, &u)?;
    if leaf.extreme.price != u.end_price || leaf.fact.confirmed_at.is_none() {
        return None;
    }
    let u_area = m.area(&u);
    let smaller = |other: &Seg| matches!((u_area, m.area(other)), (Some(a), Some(b)) if a < b);
    let previous = (0..ui.min(m.n()))
        .rev()
        .map(|k| m.seg(k))
        .find(|s| s.direction == Direction::Up);
    let mid_lower_high = previous.is_some_and(|q| u.end_price < q.end_price);
    let diverges_previous = previous.is_some_and(|q| u.end_price > q.end_price && smaller(&q));
    let departs_centre = centres.iter().any(|z| {
        let entry = m.seg(z.start - 1);
        let departure = (ui + 1 == z.end && ui >= z.start + 3) || ui == z.end;
        entry.direction == Direction::Up
            && departure
            && u.end_price > z.core_high
            && smaller(&entry)
    });
    Some(Sell {
        stage: Stage::Triggered,
        key: format!(
            "{}|{}",
            u.start_at.format(KEY_TIME),
            u.end_at.format(KEY_TIME)
        ),
        segment: SegmentFact {
            start_at: u.start_at,
            end_at: u.end_at,
            high: u.end_price,
            extending: m.moving(ui),
            area: u_area,
        },
        leaf: leaf.fact,
        mid_divergent: diverges_previous || departs_centre,
        mid_lower_high,
        parent_complete,
    })
}

/// `parent_complete`: the parent's latest up segment completes on the
/// middle level.
pub fn parent_complete(p: &LevelRead, m: &LevelRead) -> bool {
    p.latest_up()
        .and_then(|(_, s)| completion_inside(p, m, &s))
        .is_some()
}

/// The middle level's qualified centres, for `mid_divergent`.
pub fn mid_centres(m: &LevelRead) -> Vec<Centre> {
    m.qualified_centres()
}

/// The low of the middle level's latest counted down stroke.
pub fn mid_support(m: &LevelRead) -> Option<Support> {
    (0..m.finished)
        .rev()
        .map(|k| m.seg(k))
        .find(|s| s.direction == Direction::Down)
        .map(|s| Support {
            low: s.end_price,
            low_at: s.end_at,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::Config;
    use crate::macd::raw_macd;
    use crate::power::interval_power;

    /// The area read here is the interval power's same-direction area, for
    /// every stroke and for intervals inside the MACD warm-up.
    #[test]
    fn area_is_the_interval_power_area() {
        let t0 = crate::contract::parse_dt("2026-01-05T09:35:00").unwrap();
        let mut price = 100.0_f64;
        let bars: Vec<RawBar> = (0..900)
            .map(|i| {
                let from = price;
                price += if (i / 9) % 2 == 0 { 0.37 } else { -0.31 };
                RawBar {
                    dt: t0 + chrono::Duration::minutes(5 * i),
                    high: from.max(price) + 0.05,
                    low: from.min(price) - 0.05,
                    close: price,
                    vol: 1.0,
                }
            })
            .collect();
        let c = Analyzer::from_bars(bars, Config::default());
        let macd = raw_macd(&c);
        let r = LevelRead::new(&c, &macd);
        assert!(r.n() > 40);
        let mut ready = 0;
        for k in 0..r.n() {
            let s = r.seg(k);
            let power = interval_power(&c, &macd, s.start_at, s.end_at).macd;
            let want = match s.direction {
                Direction::Up => power.positive_area,
                Direction::Down => power.negative_area,
            };
            assert_eq!(r.area(&s).map(f64::to_bits), want.map(f64::to_bits), "{k}");
            ready += usize::from(want.is_some());
        }
        assert!(ready > 0 && ready < r.n());
    }
}
