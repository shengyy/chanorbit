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
    Tail,
}

impl Serialize for Pos {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Pos::Stroke(i) => s.serialize_u64(*i as u64),
            Pos::Unfinished => s.serialize_str("unfinished"),
            Pos::AfterExtreme => s.serialize_str("after_extreme"),
            Pos::Extension => s.serialize_str("extension"),
            Pos::Tail => s.serialize_str("tail"),
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

/// Raw extreme over `(start, end]`: the first bar holding it, except that
/// the end bar wins a tie (an earlier bar only when strictly beyond it).
pub fn raw_extreme(
    raw: &[RawBar],
    d: Direction,
    start: NaiveDateTime,
    end: NaiveDateTime,
) -> Option<Extreme> {
    extreme_of(&raw[rows(raw, start, end)], d, end)
}

/// Raw extreme over `[from, end]`, the same tie rule.
fn raw_extreme_from(
    raw: &[RawBar],
    d: Direction,
    from: NaiveDateTime,
    end: NaiveDateTime,
) -> Option<Extreme> {
    let a = raw.partition_point(|b| b.dt < from);
    let b = raw.partition_point(|b| b.dt <= end).max(a);
    extreme_of(&raw[a..b], d, end)
}

fn extreme_of(bars: &[RawBar], d: Direction, end: NaiveDateTime) -> Option<Extreme> {
    let price = |b: &RawBar| if up(d) { b.high } else { b.low };
    let mut x = bars
        .iter()
        .map(|b| Extreme {
            price: price(b),
            at: b.dt,
        })
        .reduce(|a, b| {
            let beyond = if up(d) {
                b.price > a.price
            } else {
                b.price < a.price
            };
            if beyond { b } else { a }
        })?;
    if let Some(last) = bars.last()
        && last.dt == end
        && price(last) == x.price
    {
        x.at = last.dt;
    }
    Some(x)
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

/// Side of a raw bar against the core: `above` (low over `zg`), `below`
/// (high under `zd`), otherwise `inside`.
fn side(b: &RawBar, zd: f64, zg: f64) -> &'static str {
    if b.low > zg {
        "above"
    } else if b.high < zd {
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
                start_price: s.start.price,
                end_price: s.end.price,
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
                    power: interval_power(c, macd, et, last),
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
            end_at: p.end_at,
            start_price: p.start_price,
            end_price: p.raw_extreme.price,
        })
    }

    /// Raw bars after everything above, when no candidate covers them.
    fn tail_seg(&self) -> Option<Seg> {
        let raw = self.raw();
        let last = raw.last()?;
        let stroke = self.strokes.last()?;
        let covered = [self.unfinished_seg(), self.after_seg()]
            .into_iter()
            .flatten()
            .map(|s| s.end_at)
            .fold(stroke.end_at, NaiveDateTime::max);
        let d = stroke.direction.opposite();
        let x = raw_extreme(raw, d, covered, last.dt)?;
        Some(Seg {
            pos: Pos::Tail,
            direction: d,
            start_at: covered,
            end_at: last.dt,
            start_price: stroke.end_price,
            end_price: x.price,
        })
    }

    /// Every segment and path of the level in time order: window strokes,
    /// the unfinished segment, the path after its extreme, the tail.
    pub(crate) fn sequence(&self) -> Vec<Seg> {
        (0..self.strokes.len())
            .map(|k| self.seg(k))
            .chain(self.unfinished_seg())
            .chain(self.after_seg())
            .chain(self.tail_seg())
            .collect()
    }

    /// The centre's state from its raw path after the third member
    /// (`docs/structure-contract.md#centre-state`).
    fn state_of(&self, z: &Centre, next: Option<&Centre>, k: usize) -> State {
        let raw = self.raw();
        let w0 = self.window_start;
        let (zd, zg) = (z.core_low, z.core_high);
        let members = z.member_count();
        let third = z.start + 2 - w0;
        let segs = self.sequence();
        let last = raw.last().expect("a level with strokes has bars");
        let (until, end_side) = match next {
            Some(_) => {
                let closing = &self.strokes[z.end - w0];
                (closing.end_at, position(closing.end_price, zd, zg))
            }
            None => (last.dt, position(last.close, zd, zg)),
        };
        let containing = |t: NaiveDateTime| {
            segs.iter()
                .position(|s| s.start_at < t && t <= s.end_at)
                .unwrap_or_else(|| panic!("no segment holds {t} (centre {k})"))
        };
        if end_side == "inside" {
            assert!(
                next.is_none(),
                "centre {k}: the stroke that closed it ends inside its core [{zd}, {zg}]"
            );
            let at = if last.dt > self.strokes[third].end_at {
                containing(last.dt)
            } else {
                third
            };
            return State::Inside {
                segment: segs[at].pos,
            };
        }
        let path = &raw[rows(raw, self.strokes[third].end_at, until)];
        let after_third = |to: usize| segs[third + 1..to].iter().map(|s| s.pos).collect();
        // The path starts at the third member's end price, then its raw bars.
        let start_met = position(self.strokes[third].end_price, zd, zg) != end_side;
        let above = end_side == "above";
        let exit_direction = if above {
            Direction::Up
        } else {
            Direction::Down
        };
        // The exit is the last bar touching the core when it closes beyond
        // the edge and the element holding it runs in the exit direction;
        // otherwise the bar after it (that bar itself when it is the last).
        let star = match path.iter().rposition(|b| side(b, zd, zg) != end_side) {
            Some(i) => {
                let b = &path[i];
                let closed_out = if above { b.close > zg } else { b.close < zd };
                let runs_out = segs
                    .iter()
                    .find(|s| s.start_at < b.dt && b.dt <= s.end_at)
                    .is_some_and(|s| s.direction == exit_direction);
                if closed_out && runs_out {
                    Some(b)
                } else {
                    path.get(i + 1).or(Some(b))
                }
            }
            None if start_met => path.first(),
            None => None,
        };
        let Some(exit) = star else {
            assert!(
                members == 3,
                "centre {k}: {members} members but the path never meets the core [{zd}, {zg}] (ended {}, path {} bars after {}, until {until}, side {end_side})",
                next.is_some(),
                path.len(),
                self.strokes[third].end_at
            );
            let to = match next {
                Some(_) => z.end - w0 + 1,
                None => segs.len(),
            };
            return State::FormedOut {
                third: Pos::Stroke(2 + z.start - w0),
                after: after_third(to),
            };
        };
        let j = containing(exit.dt);
        let x = segs[j];
        let retest = match x.pos {
            Pos::Stroke(_) => segs
                .get(j + 1)
                .filter(|r| matches!(r.pos, Pos::Stroke(_) | Pos::Unfinished))
                .copied(),
            _ => None,
        };
        // Toward the core: the retest's lowest low after an upward exit,
        // its highest high after a downward one.
        let toward = if above {
            Direction::Down
        } else {
            Direction::Up
        };
        let retest_extreme = retest.and_then(|r| raw_extreme(raw, toward, r.start_at, r.end_at));
        let versus_edge = retest_extreme.map(|e| {
            let (edge, beyond) = if above {
                (zg, e.price > zg)
            } else {
                (zd, e.price < zd)
            };
            if e.price == edge {
                "at"
            } else if beyond {
                "beyond"
            } else {
                "inside"
            }
        });
        let retest_role = match (next, retest.map(|r| r.pos)) {
            (Some(n), Some(Pos::Stroke(p))) => {
                let p = p + w0;
                let role = if p + 1 == n.start {
                    Some("entry")
                } else if (n.start..n.end).contains(&p) {
                    Some("member")
                } else {
                    None
                };
                role.map(|role| RetestRole {
                    centre: k + 1,
                    role,
                })
            }
            _ => None,
        };
        State::Departed {
            departure: x.pos,
            direction: x.direction,
            edge: if above { "zg" } else { "zd" },
            // Against the exit side: the extreme on the exit side from the exit bar on.
            raw_extreme: if x.direction == exit_direction {
                raw_extreme(raw, x.direction, x.start_at, x.end_at)
            } else {
                raw_extreme_from(raw, exit_direction, exit.dt, x.end_at)
            },
            member: matches!(x.pos, Pos::Stroke(p) if p + w0 < z.end),
            first_beyond_at: exit.dt,
            retest: retest.map(|r| r.pos),
            retest_extreme,
            versus_edge,
            after_retest: next.is_none().then(|| match retest {
                Some(_) => segs[j + 2..].iter().map(|s| s.pos).collect(),
                None => Vec::new(),
            }),
            retest_role,
        }
    }
}
