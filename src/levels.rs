//! Per-level structure facts of the `levels` command: stroke window, raw and
//! DIF extremes, unfinished segment and the path after its extreme, centres
//! with their state, and same-direction comparisons. Definitions are owned by
//! `docs/structure-contract.md#levels`; the centre state is in `levels_state`.
use crate::analyzer::{Analyzer, Config};
use crate::bar::{NewBar, RawBar};
use crate::candidate::candidate_stroke;
use crate::centre::{Centre, canonical_centres};
use crate::geometry::{FractalFact, relation};
use crate::levels_compare::Comparisons;
use crate::levels_state::centre_state;
use crate::macd::{MacdSeries, raw_macd};
use crate::power::{Power, interval_power};
use crate::stroke::{Direction, Endpoint, Stroke, extreme};
use chrono::NaiveDateTime;
use serde::{Serialize, Serializer};
use std::borrow::Cow;
use std::sync::Arc;

/// Centres listed per level: the latest qualified one and up to four before it.
pub const LISTED_CENTRES: usize = 5;

/// A segment position: a window stroke, or one of the level's named objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pos {
    Stroke(usize),
    Unfinished,
    AfterExtreme,
}

impl Serialize for Pos {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Pos::Stroke(i) => s.serialize_u64(*i as u64),
            Pos::Unfinished => s.serialize_str("unfinished"),
            Pos::AfterExtreme => s.serialize_str("after_extreme"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Extreme {
    pub price: f64,
    pub at: NaiveDateTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct DifExtreme {
    pub value: f64,
    pub at: NaiveDateTime,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LevelStroke {
    pub direction: Direction,
    pub start_at: NaiveDateTime,
    pub end_at: NaiveDateTime,
    pub start_price: f64,
    pub end_price: f64,
    pub high: f64,
    pub low: f64,
    pub newbar_count: usize,
    pub finished: bool,
    pub raw_extreme: Option<Extreme>,
    pub dif_extreme: Option<DifExtreme>,
    pub power: Power,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Unfinished {
    pub direction: Direction,
    pub anchor_at: NaiveDateTime,
    pub anchor_price: f64,
    pub end_price: f64,
    pub end_price_at: NaiveDateTime,
    pub bars_after_extreme: usize,
    pub raw_extreme: Option<Extreme>,
    pub dif_extreme: Option<DifExtreme>,
    pub power: Power,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AfterExtreme {
    pub direction: Direction,
    pub start_at: NaiveDateTime,
    pub start_price: f64,
    pub end_at: NaiveDateTime,
    pub raw_extreme: Extreme,
    pub bar_count: usize,
    pub dif_extreme: Option<DifExtreme>,
    pub power: Power,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Membership {
    pub member: bool,
    pub reason: &'static str,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Versus {
    pub core: String,
    pub outer: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RetestRole {
    /// Position in `centres` of the next listed centre.
    pub centre: usize,
    /// `entry` or `member` there.
    pub role: &'static str,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum State {
    Departed {
        departure: Pos,
        direction: Direction,
        edge: &'static str,
        raw_extreme: Option<Extreme>,
        /// Toward the exit side only: the raw extreme strictly beyond every
        /// other member's (`dd`/`gg` when the departure is not a member).
        new_extreme: Option<bool>,
        member: bool,
        first_beyond_at: NaiveDateTime,
        retest: Option<Pos>,
        retest_extreme: Option<Extreme>,
        versus_edge: Option<&'static str>,
        /// Latest centre only; `null` for an ended centre.
        after_retest: Option<Vec<Pos>>,
        /// Ended centre only: the retest's role in the next listed centre.
        retest_role: Option<RetestRole>,
    },
    Inside {
        segment: Pos,
    },
    FormedOut {
        third: Pos,
        after: Vec<Pos>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LevelCentre {
    pub first_member: usize,
    pub last_member: usize,
    pub member_count: usize,
    pub entry: usize,
    pub start_at: NaiveDateTime,
    pub end_at: NaiveDateTime,
    pub zd: f64,
    pub zg: f64,
    pub dd: f64,
    pub gg: f64,
    pub versus_next: Option<Versus>,
    pub ended_by: Option<usize>,
    pub price_versus_core: &'static str,
    pub price_versus_outer: &'static str,
    pub state: State,
}

/// A segment as the centre state and the comparisons read it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Seg {
    pub pos: Pos,
    pub direction: Direction,
    pub start_at: NaiveDateTime,
    pub end_at: NaiveDateTime,
    pub start_price: f64,
    pub end_price: f64,
}

/// One level analysed alone, with what the per-level output and the child
/// placement read.
pub struct Level {
    pub analyzer: Analyzer,
    pub macd: MacdSeries,
    /// Every input bar (last version of each `dt`), for bar-time mapping.
    pub input: Vec<RawBar>,
    /// Retained stroke index of window position 0.
    pub window_start: usize,
    pub strokes: Vec<LevelStroke>,
    /// The last window stroke is extending: it is read to its current extreme.
    pub last_stroke_extending: bool,
    pub unfinished: Option<Unfinished>,
    pub after_extreme: Option<AfterExtreme>,
    pub centres: Vec<LevelCentre>,
    pub membership: Membership,
    pub comparisons: Comparisons,
    pub fractals: Vec<FractalFact>,
}

fn dedup_last(bars: &[RawBar]) -> Vec<RawBar> {
    let mut out: Vec<RawBar> = Vec::with_capacity(bars.len());
    for b in bars {
        match out.last_mut() {
            Some(last) if last.dt == b.dt => *last = *b,
            _ => out.push(*b),
        }
    }
    out
}

/// Index range of raw bars with `start < dt <= end`.
pub(crate) fn rows(
    raw: &[RawBar],
    start: NaiveDateTime,
    end: NaiveDateTime,
) -> std::ops::Range<usize> {
    let a = raw.partition_point(|b| b.dt <= start);
    let b = raw.partition_point(|b| b.dt <= end);
    a..b.max(a)
}

fn up(d: Direction) -> bool {
    d == Direction::Up
}

/// Raw extreme over `(start, end]`, held by the first bar that reaches it.
pub fn raw_extreme(
    raw: &[RawBar],
    d: Direction,
    start: NaiveDateTime,
    end: NaiveDateTime,
) -> Option<Extreme> {
    extreme_of(&raw[rows(raw, start, end)], d)
}

/// Raw extreme over `[from, end]`, the same rule.
pub(crate) fn raw_extreme_from(
    raw: &[RawBar],
    d: Direction,
    from: NaiveDateTime,
    end: NaiveDateTime,
) -> Option<Extreme> {
    let a = raw.partition_point(|b| b.dt < from);
    let b = raw.partition_point(|b| b.dt <= end).max(a);
    extreme_of(&raw[a..b], d)
}

fn extreme_of(bars: &[RawBar], d: Direction) -> Option<Extreme> {
    extreme(bars, d).map(|x| Extreme {
        price: x.price,
        at: x.at,
    })
}

fn dif_extreme(
    raw: &[RawBar],
    s: &MacdSeries,
    d: Direction,
    start: NaiveDateTime,
    end: NaiveDateTime,
) -> Option<DifExtreme> {
    let r = rows(raw, start, end);
    if r.is_empty() || s.dif[r.clone()].iter().any(|v| !v.is_finite()) {
        return None;
    }
    r.map(|i| DifExtreme {
        value: s.dif[i],
        at: raw[i].dt,
    })
    .reduce(|a, b| {
        let beyond = if up(d) {
            b.value > a.value
        } else {
            b.value < a.value
        };
        if beyond { b } else { a }
    })
}

/// Position of the NewBar holding the raw bar at `at` (the first whose last
/// element is at or after it).
fn holding(bars: &[Arc<NewBar>], at: NaiveDateTime) -> usize {
    bars.iter()
        .position(|b| b.elements.last().is_some_and(|r| r.dt >= at))
        .unwrap_or(bars.len().saturating_sub(1))
}

/// The unfinished NewBars a stroke ending at `at` would leave: from its end
/// fractal's first NewBar (the one before the NewBar holding `at`) on, as the
/// finished rule counts them once the stroke ends there.
fn newbars_from_end(pending: &[Arc<NewBar>], at: NaiveDateTime) -> usize {
    pending.len() - holding(pending, at).saturating_sub(1)
}

/// The level's extension and how many strokes count for centres: an
/// extending last stroke is that stroke, not yet ended, read to its current
/// extreme and finished as a stroke ending there is.
pub(crate) fn counted(c: &Analyzer) -> (Option<Endpoint>, usize) {
    let n = c.strokes().len();
    let extension = c.extension();
    let finished = match extension {
        Some(x) => n - 1 + usize::from(newbars_from_end(c.pending_bars(), x.at) >= 5),
        None => c.finished_count(),
    };
    (extension, finished)
}

/// `all[from..to]` with the last stroke read to `extension` when the slice
/// holds it.
pub(crate) fn read_strokes(
    all: &[Stroke],
    from: usize,
    to: usize,
    extension: Option<Endpoint>,
) -> Cow<'_, [Stroke]> {
    match extension {
        Some(x) if to == all.len() && from < to => {
            let mut read = all[from..to].to_vec();
            read.last_mut().expect("non-empty").end = x;
            Cow::Owned(read)
        }
        _ => Cow::Borrowed(&all[from..to.max(from)]),
    }
}

/// The elements after the last stroke, whose read end is `last_at` at
/// `last_price` in direction `last_direction`: the unfinished reverse segment
/// (the available reverse candidate), and the path after the extreme of the
/// last element with that path's raw extreme.
pub(crate) fn tail(
    c: &Analyzer,
    last_direction: Direction,
    last_at: NaiveDateTime,
    last_price: f64,
) -> (Option<Seg>, Option<(Seg, Extreme)>) {
    let raw = c.raw_bars();
    let candidate = candidate_stroke(c);
    let mut unfinished = None;
    // The path after the extreme follows the last element: the unfinished
    // reverse segment, otherwise the last stroke as read.
    let mut path_from = (last_direction.opposite(), last_at, last_price);
    if candidate.state == "available"
        && candidate.kind.as_deref() == Some("reverse")
        && let (Some(d), Some(a), Some(ap), Some(e), Some(et), Some(_)) = (
            candidate.direction,
            candidate.anchor_at,
            candidate.anchor_price,
            candidate.end_price,
            candidate.end_price_at,
            raw.last(),
        )
    {
        unfinished = Some(Seg {
            pos: Pos::Unfinished,
            direction: d,
            start_at: a,
            end_at: et,
            start_price: ap,
            end_price: e,
        });
        path_from = (d.opposite(), et, e);
    }
    let (pd, et, e) = path_from;
    let after = raw.last().and_then(|last| {
        let x = raw_extreme(raw, pd, et, last.dt)?;
        let seg = Seg {
            pos: Pos::AfterExtreme,
            direction: pd,
            start_at: et,
            end_at: last.dt,
            start_price: e,
            end_price: x.price,
        };
        Some((seg, x))
    });
    (unfinished, after)
}

/// NewBars of the extending last stroke ending at `at`: from its first
/// NewBar to the one after the NewBar holding `at`, those that exist.
fn extended_newbars(last: &Stroke, pending: &[Arc<NewBar>], at: NaiveDateTime) -> usize {
    let window = last.reopen(pending);
    (holding(&window, at) + 2).min(window.len())
}

/// This centre (`a`) versus the next (`b`): `above` when `a` lies above `b`.
fn versus(a: &Centre, b: &Centre) -> Versus {
    Versus {
        core: relation(b.core_low, b.core_high, a.core_low, a.core_high),
        outer: relation(b.low, b.high, a.low, a.high),
    }
}

pub(crate) fn position(price: f64, low: f64, high: f64) -> &'static str {
    if price > high {
        "above"
    } else if price < low {
        "below"
    } else {
        "inside"
    }
}

impl Level {
    /// Analyse one level alone; [`Level::build`] then derives its facts.
    pub fn new(input: &[RawBar]) -> Self {
        let input = dedup_last(input);
        let analyzer = Analyzer::from_bars(input.iter().copied(), Config::default());
        let macd = raw_macd(&analyzer);
        Level {
            analyzer,
            macd,
            input,
            window_start: 0,
            strokes: Vec::new(),
            last_stroke_extending: false,
            unfinished: None,
            after_extreme: None,
            centres: Vec::new(),
            membership: Membership {
                member: false,
                reason: "no_centre",
            },
            comparisons: Comparisons::default(),
            fractals: Vec::new(),
        }
    }

    pub fn raw(&self) -> &[RawBar] {
        self.analyzer.raw_bars()
    }

    /// Derive the level's facts. `cover_from`: the window also starts early
    /// enough to hold every stroke ending after this time.
    pub fn build(&mut self, cover_from: Option<NaiveDateTime>) {
        let c = &self.analyzer;
        let all = c.strokes();
        if all.is_empty() {
            return;
        }
        let n = all.len();
        let (extension, finished) = counted(c);
        let fs = read_strokes(all, 0, finished, extension);
        let sequence = canonical_centres(&fs);
        let qualified: Vec<Centre> = sequence
            .iter()
            .filter(|z| z.member_count() >= 3 && z.members_touch_core(&fs))
            .copied()
            .collect();
        let listed = &qualified[qualified.len().saturating_sub(LISTED_CENTRES)..];
        let mut w0 = listed.first().map_or(0, |z| z.start - 1);
        if let Some(t) = cover_from {
            let first_after = all.iter().position(|s| s.end_at() > t).unwrap_or(n);
            w0 = w0.min(first_after.min(n - 1));
        }
        self.window_start = w0;
        self.last_stroke_extending = extension.is_some();
        let raw = c.raw_bars();
        let macd = &self.macd;
        self.strokes = (w0..n)
            .map(|k| {
                let s = &all[k];
                let (start, end) = match extension {
                    Some(x) if k + 1 == n => (s.start, x),
                    _ => (s.start, s.end),
                };
                LevelStroke {
                    direction: s.direction,
                    start_at: start.at,
                    end_at: end.at,
                    start_price: start.price,
                    end_price: end.price,
                    high: start.price.max(end.price),
                    low: start.price.min(end.price),
                    newbar_count: match extension {
                        Some(x) if k + 1 == n => extended_newbars(s, c.pending_bars(), x.at),
                        _ => s.newbar_count(),
                    },
                    finished: k < finished,
                    raw_extreme: raw_extreme(raw, s.direction, start.at, end.at),
                    dif_extreme: dif_extreme(raw, macd, s.direction, start.at, end.at),
                    power: interval_power(c, macd, start.at, end.at),
                }
            })
            .collect();
        let since = all[w0].start_at();
        self.fractals = c
            .fractals()
            .iter()
            .filter(|f| f.dt >= since)
            .map(FractalFact::from)
            .collect();
        let last = self
            .strokes
            .last()
            .expect("the window holds the last stroke");
        let (unfinished, after) = tail(c, last.direction, last.end_at, last.end_price);
        if let (Some(u), Some(last)) = (unfinished, raw.last()) {
            let (d, a, et) = (u.direction, u.start_at, u.end_at);
            self.unfinished = Some(Unfinished {
                direction: d,
                anchor_at: a,
                anchor_price: u.start_price,
                end_price: u.end_price,
                end_price_at: et,
                bars_after_extreme: rows(raw, et, last.dt).len(),
                raw_extreme: raw_extreme(raw, d, a, et),
                dif_extreme: dif_extreme(raw, macd, d, a, et),
                power: interval_power(c, macd, a, et),
            });
        }
        if let Some((p, x)) = after {
            let (pd, et, end) = (p.direction, p.start_at, p.end_at);
            self.after_extreme = Some(AfterExtreme {
                direction: pd,
                start_at: et,
                start_price: p.start_price,
                end_at: end,
                raw_extreme: x,
                bar_count: rows(raw, et, end).len(),
                dif_extreme: dif_extreme(raw, macd, pd, et, end),
                power: interval_power(c, macd, et, end),
            });
        }
        let last_index = n - 1;
        self.membership = match listed.last() {
            None => Membership {
                member: false,
                reason: "no_centre",
            },
            Some(z) if z.start <= last_index && last_index < z.end => Membership {
                member: true,
                reason: "member",
            },
            Some(_) if last_index >= finished => Membership {
                member: false,
                reason: "not_finished",
            },
            Some(_) => Membership {
                member: false,
                reason: "not_returned",
            },
        };
        let close = raw.last().map_or(f64::NAN, |b| b.close);
        let mut centres = Vec::new();
        for (k, z) in listed.iter().enumerate() {
            let next = listed.get(k + 1);
            let state = self.state_of(z, next, k);
            centres.push(LevelCentre {
                first_member: z.start - w0,
                last_member: z.end - 1 - w0,
                member_count: z.member_count(),
                entry: z.start - 1 - w0,
                start_at: z.start_at,
                end_at: z.end_at,
                zd: z.core_low,
                zg: z.core_high,
                dd: z.low,
                gg: z.high,
                versus_next: next.map(|n| versus(z, n)),
                ended_by: next.map(|_| k + 1),
                price_versus_core: position(close, z.core_low, z.core_high),
                price_versus_outer: position(close, z.low, z.high),
                state,
            });
        }
        if let Some(z) = centres.last() {
            // The latest state never contradicts where the last close stands.
            assert_eq!(
                matches!(z.state, State::Inside { .. }),
                z.price_versus_core == "inside",
                "latest centre state {:?} against last close {close} in [{}, {}]",
                z.state,
                z.zd,
                z.zg
            );
        }
        self.centres = centres;
        self.comparisons = self.comparisons_of();
    }

    /// Segment of window position `k`.
    pub fn seg(&self, k: usize) -> Seg {
        let s = &self.strokes[k];
        Seg {
            pos: Pos::Stroke(k),
            direction: s.direction,
            start_at: s.start_at,
            end_at: s.end_at,
            start_price: s.start_price,
            end_price: s.end_price,
        }
    }

    /// The unfinished reverse segment.
    pub fn unfinished_seg(&self) -> Option<Seg> {
        self.unfinished.as_ref().map(|u| Seg {
            pos: Pos::Unfinished,
            direction: u.direction,
            start_at: u.anchor_at,
            end_at: u.end_price_at,
            start_price: u.anchor_price,
            end_price: u.end_price,
        })
    }

    pub(crate) fn after_seg(&self) -> Option<Seg> {
        self.after_extreme.as_ref().map(|p| Seg {
            pos: Pos::AfterExtreme,
            direction: p.direction,
            start_at: p.start_at,
            end_at: p.end_at,
            start_price: p.start_price,
            end_price: p.raw_extreme.price,
        })
    }

    /// Whether `s` is still moving: the extending last stroke, the unfinished
    /// segment or the path after the extreme.
    pub(crate) fn moving(&self, s: &Seg) -> bool {
        match s.pos {
            Pos::Stroke(k) => self.last_stroke_extending && k + 1 == self.strokes.len(),
            _ => true,
        }
    }

    /// Every element of the level in time order: window strokes (the last
    /// one to its current extreme), the unfinished segment, the path after
    /// the extreme.
    pub(crate) fn sequence(&self) -> Vec<Seg> {
        (0..self.strokes.len())
            .map(|k| self.seg(k))
            .chain(self.unfinished_seg())
            .chain(self.after_seg())
            .collect()
    }

    /// The centre's state from its raw path after the third member
    /// (`docs/structure-contract.md#centre-state`).
    fn state_of(&self, z: &Centre, next: Option<&Centre>, k: usize) -> State {
        centre_state(self.raw(), &self.sequence(), self.window_start, z, next, k)
    }
}
