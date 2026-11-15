//! Per-level structure facts of the `levels` command: stroke window, raw and
//! DIF extremes, unfinished segment and the path after its extreme, centres
//! with their state, and same-direction comparisons. Definitions are owned by
//! `docs/structure-contract.md#levels`.
use crate::analyzer::{Analyzer, Config};
use crate::bar::RawBar;
use crate::candidate::candidate_stroke;
use crate::centre::{Centre, canonical_centres};
use crate::geometry::{FractalFact, relation};
use crate::levels_compare::Comparisons;
use crate::macd::{MacdSeries, retained_macd};
use crate::power::{Power, interval_power, stroke_power};
use crate::stroke::Direction;
use chrono::NaiveDateTime;
use serde::{Serialize, Serializer};

/// Centres listed per level: the latest qualified one and up to four before it.
pub const LISTED_CENTRES: usize = 5;

/// A segment position: a window stroke, or one of the level's named objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pos {
    Stroke(usize),
    Unfinished,
    AfterExtreme,
    Extension,
}

impl Serialize for Pos {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Pos::Stroke(i) => s.serialize_u64(*i as u64),
            Pos::Unfinished => s.serialize_str("unfinished"),
            Pos::AfterExtreme => s.serialize_str("after_extreme"),
            Pos::Extension => s.serialize_str("extension"),
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
    pub kind: String,
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
        member: bool,
        first_beyond_at: Option<NaiveDateTime>,
        returned_inside_at: Option<NaiveDateTime>,
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

/// Index range of retained raw bars with `start < dt <= end`.
fn rows(raw: &[RawBar], start: NaiveDateTime, end: NaiveDateTime) -> std::ops::Range<usize> {
    let a = raw.partition_point(|b| b.dt <= start);
    let b = raw.partition_point(|b| b.dt <= end);
    a..b.max(a)
}

fn up(d: Direction) -> bool {
    d == Direction::Up
}

pub fn raw_extreme(
    raw: &[RawBar],
    d: Direction,
    start: NaiveDateTime,
    end: NaiveDateTime,
) -> Option<Extreme> {
    raw[rows(raw, start, end)]
        .iter()
        .map(|b| Extreme {
            price: if up(d) { b.high } else { b.low },
            at: b.dt,
        })
        .reduce(|a, b| {
            let beyond = if up(d) {
                b.price > a.price
            } else {
                b.price < a.price
            };
            if beyond { b } else { a }
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

/// Raw range `[lo, hi]` of a segment, widened to its endpoint prices.
pub fn raw_range(raw: &[RawBar], seg: &Seg) -> (f64, f64) {
    let r = rows(raw, seg.start_at, seg.end_at);
    raw[r].iter().fold(
        (
            seg.start_price.min(seg.end_price),
            seg.start_price.max(seg.end_price),
        ),
        |(lo, hi), b| (lo.min(b.low), hi.max(b.high)),
    )
}

/// This centre (`a`) versus the next (`b`): `above` when `a` lies above `b`.
fn versus(a: &Centre, b: &Centre) -> Versus {
    Versus {
        core: relation(b.core_low, b.core_high, a.core_low, a.core_high),
        outer: relation(b.low, b.high, a.low, a.high),
    }
}

fn position(price: f64, low: f64, high: f64) -> &'static str {
    if price > high {
        "above"
    } else if price < low {
        "below"
    } else {
        "inside"
    }
}

/// Positions `j` that overlap the core and the state, per the centre-state rule.
fn classify(
    raw: &[RawBar],
    segs: &[Seg],
    members: usize,
    zd: f64,
    zg: f64,
) -> (Option<usize>, Vec<&'static str>) {
    let ranges: Vec<(f64, f64)> = segs.iter().map(|s| raw_range(raw, s)).collect();
    let overlaps = |k: usize| {
        let (lo, hi) = ranges[k];
        let s = &segs[k];
        if k < members || (k == members && (zd..=zg).contains(&s.start_price)) {
            hi >= zd && lo <= zg
        } else {
            hi > zd && lo < zg
        }
    };
    let exceeds = |k: usize| {
        let s = &segs[k];
        match raw_extreme(raw, s.direction, s.start_at, s.end_at) {
            Some(x) if up(s.direction) => x.price.max(s.end_price) > zg,
            Some(x) => x.price.min(s.end_price) < zd,
            None => {
                if up(s.direction) {
                    s.end_price > zg
                } else {
                    s.end_price < zd
                }
            }
        }
    };
    let j = (0..segs.len()).rev().find(|&k| overlaps(k));
    let last = segs.len() - 1;
    let mut states = Vec::new();
    if j.is_some_and(|j| j >= 3 && exceeds(j)) {
        states.push("departed");
    }
    if overlaps(last) && !exceeds(last) {
        states.push("inside");
    }
    if members == 3 && segs.len() >= 3 && exceeds(2) && !(3..segs.len()).any(overlaps) {
        states.push("formed_out");
    }
    (j, states)
}

fn first_beyond_and_return(
    raw: &[RawBar],
    seg: &Seg,
    zd: f64,
    zg: f64,
) -> (Option<NaiveDateTime>, Option<NaiveDateTime>) {
    let bars = &raw[rows(raw, seg.start_at, seg.end_at)];
    let u = up(seg.direction);
    let Some(k) = bars
        .iter()
        .position(|b| if u { b.high > zg } else { b.low < zd })
    else {
        return (None, None);
    };
    let back = bars[k + 1..]
        .iter()
        .find(|b| if u { b.low < zg } else { b.high > zd })
        .map(|b| b.dt);
    (Some(bars[k].dt), back)
}

impl Level {
    /// Analyse one level alone; [`Level::build`] then derives its facts.
    pub fn new(input: &[RawBar]) -> Self {
        let input = dedup_last(input);
        let analyzer = Analyzer::from_bars(input.iter().copied(), Config::default());
        let macd = retained_macd(&analyzer);
        Level {
            analyzer,
            macd,
            input,
            window_start: 0,
            strokes: Vec::new(),
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
        let finished = c.finished_count();
        let fs = &all[..finished];
        let sequence = canonical_centres(fs);
        let qualified: Vec<Centre> = sequence
            .iter()
            .filter(|z| z.member_count() >= 3 && z.members_touch_core(fs))
            .copied()
            .collect();
        let listed = &qualified[qualified.len().saturating_sub(LISTED_CENTRES)..];
        let mut w0 = listed.first().map_or(0, |z| z.start - 1);
        if let Some(t) = cover_from {
            let first_after = all.iter().position(|s| s.end_at() > t).unwrap_or(all.len());
            w0 = w0.min(first_after.min(all.len() - 1));
        }
        self.window_start = w0;
        let raw = c.raw_bars();
        let macd = &self.macd;
        self.strokes = all[w0..]
            .iter()
            .enumerate()
            .map(|(k, s)| LevelStroke {
                direction: s.direction,
                start_at: s.start_at(),
                end_at: s.end_at(),
                start_price: s.start_fractal.price,
                end_price: s.end_fractal.price,
                high: s.high(),
                low: s.low(),
                newbar_count: s.newbar_count(),
                finished: w0 + k < finished,
                raw_extreme: raw_extreme(raw, s.direction, s.start_at(), s.end_at()),
                dif_extreme: dif_extreme(raw, macd, s.direction, s.start_at(), s.end_at()),
                power: stroke_power(c, macd, s),
            })
            .collect();
        let since = all[w0].start_at();
        self.fractals = c
            .fractals()
            .iter()
            .filter(|f| f.dt >= since)
            .map(FractalFact::from)
            .collect();
        let candidate = candidate_stroke(c);
        let last_dt = raw.last().map(|b| b.dt);
        if candidate.state == "available"
            && let (Some(kind), Some(d), Some(a), Some(ap), Some(e), Some(et), Some(last)) = (
                candidate.kind.clone(),
                candidate.direction,
                candidate.anchor_at,
                candidate.anchor_price,
                candidate.end_price,
                candidate.end_price_at,
                last_dt,
            )
        {
            self.unfinished = Some(Unfinished {
                kind,
                direction: d,
                anchor_at: a,
                anchor_price: ap,
                end_price: e,
                end_price_at: et,
                bars_after_extreme: rows(raw, et, last).len(),
                raw_extreme: raw_extreme(raw, d, a, et),
                dif_extreme: dif_extreme(raw, macd, d, a, et),
                power: interval_power(c, macd, a, et),
            });
            let pd = d.opposite();
            if let Some(x) = raw_extreme(raw, pd, et, last) {
                self.after_extreme = Some(AfterExtreme {
                    direction: pd,
                    start_at: et,
                    start_price: e,
                    end_at: last,
                    raw_extreme: x,
                    bar_count: rows(raw, et, last).len(),
                    dif_extreme: dif_extreme(raw, macd, pd, et, last),
                    power: interval_power(c, macd, et, x.at),
                });
            }
        }
        let last_index = all.len() - 1;
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

    pub fn unfinished_seg(&self) -> Option<Seg> {
        self.unfinished.as_ref().map(|u| Seg {
            pos: if u.kind == "reverse" {
                Pos::Unfinished
            } else {
                Pos::Extension
            },
            direction: u.direction,
            start_at: u.anchor_at,
            end_at: u.end_price_at,
            start_price: u.anchor_price,
            end_price: u.end_price,
        })
    }

    pub(crate) fn reverse_seg(&self) -> Option<Seg> {
        self.unfinished_seg().filter(|s| s.pos == Pos::Unfinished)
    }

    pub(crate) fn after_seg(&self) -> Option<Seg> {
        self.after_extreme.as_ref().map(|p| Seg {
            pos: Pos::AfterExtreme,
            direction: p.direction,
            start_at: p.start_at,
            end_at: p.raw_extreme.at,
            start_price: p.start_price,
            end_price: p.raw_extreme.price,
        })
    }

    fn state_of(&self, z: &Centre, next: Option<&Centre>, k: usize) -> State {
        let raw = self.raw();
        let w0 = self.window_start;
        let (zd, zg) = (z.core_low, z.core_high);
        let first = z.start - w0;
        let segs: Vec<Seg> = match next {
            // Ended: members and the stroke that closed the centre.
            Some(_) => (first..=z.end - w0).map(|p| self.seg(p)).collect(),
            None => (first..self.strokes.len())
                .map(|p| self.seg(p))
                .chain(self.reverse_seg())
                .collect(),
        };
        let members = z.member_count();
        let (j, states) = classify(raw, &segs, members, zd, zg);
        let detail = || {
            let parts: Vec<String> = segs
                .iter()
                .map(|s| {
                    let (lo, hi) = raw_range(raw, s);
                    format!(
                        "{:?} {:?} {}..{} {}->{} raw [{lo}, {hi}]",
                        s.pos, s.direction, s.start_at, s.end_at, s.start_price, s.end_price
                    )
                })
                .collect();
            format!(
                "centre {k}, {members} members, zd {zd}, zg {zg}, j {j:?}: {}",
                parts.join("; ")
            )
        };
        assert!(
            states.len() == 1,
            "centre state not exactly one: {states:?} ({})",
            detail()
        );
        if next.is_some() {
            assert!(
                states[0] != "inside",
                "an ended centre cannot be inside ({})",
                detail()
            );
        }
        match states[0] {
            "inside" => State::Inside {
                segment: segs[segs.len() - 1].pos,
            },
            "formed_out" => State::FormedOut {
                third: segs[2].pos,
                after: segs[3..].iter().map(|s| s.pos).collect(),
            },
            _ => {
                let j = j.expect("departed has a position");
                let x = segs[j];
                let u = up(x.direction);
                let (first_beyond_at, returned_inside_at) =
                    first_beyond_and_return(raw, &x, zd, zg);
                let retest = segs.get(j + 1).copied();
                let retest_extreme =
                    retest.and_then(|r| raw_extreme(raw, r.direction, r.start_at, r.end_at));
                let versus_edge = retest.map(|r| {
                    let (lo, hi) = raw_range(raw, &r);
                    let held_at = if u { lo == zg } else { hi == zd };
                    if held_at { "at" } else { "beyond" }
                });
                let retest_role = next.and_then(|n| {
                    let p = z.end;
                    if p == n.start - 1 {
                        Some(RetestRole {
                            centre: k + 1,
                            role: "entry",
                        })
                    } else if (n.start..n.end).contains(&p) {
                        Some(RetestRole {
                            centre: k + 1,
                            role: "member",
                        })
                    } else {
                        None
                    }
                });
                State::Departed {
                    departure: x.pos,
                    direction: x.direction,
                    edge: if u { "zg" } else { "zd" },
                    raw_extreme: raw_extreme(raw, x.direction, x.start_at, x.end_at),
                    member: j < members,
                    first_beyond_at,
                    returned_inside_at,
                    retest: retest.map(|r| r.pos),
                    retest_extreme,
                    versus_edge,
                    after_retest: next.is_none().then(|| {
                        segs[(j + 2).min(segs.len())..]
                            .iter()
                            .map(|s| s.pos)
                            .collect()
                    }),
                    retest_role,
                }
            }
        }
    }
}
