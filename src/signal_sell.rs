//! The sell-side nest of the `signals` command (`up_end_nest`): the middle
//! level's latest up segment ending, with its attributes, and the middle
//! level's support. Definitions are owned by
//! `docs/structure-contract.md#signals`.
use crate::centre::Centre;
use crate::fractal::Mark;
use crate::levels::{Seg, State};
use crate::levels_state::centre_state;
use crate::signal::{
    Completion, CompletionFact, LevelRead, Stage, completion, covered_child, side_price,
};
use crate::stroke::{Direction, Endpoint};
use chrono::NaiveDateTime;
use serde::Serialize;

const KEY_TIME: &str = "%Y-%m-%dT%H:%M:%S";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SegmentFact {
    pub start_at: NaiveDateTime,
    pub end_at: NaiveDateTime,
    pub high: f64,
    pub extending: bool,
    pub area: Option<f64>,
}

/// `D_P`: the departure of the latest parent centre left upward through
/// `zg` by an element running up.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct UpDeparture {
    pub start_at: NaiveDateTime,
    pub end_at: NaiveDateTime,
    pub high: f64,
}

/// The parent's latest up segment against the departure of the latest
/// parent centre left upward.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct VersusDeparture {
    pub new_high: bool,
    pub area_ratio: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Sell {
    /// `triggered` (the leaf completes, confirmed at the high) or `turned`
    /// (the middle level's top fractal at the high is confirmed).
    pub stage: Stage,
    pub key: String,
    pub segment: SegmentFact,
    pub leaf: Option<CompletionFact>,
    pub mid_divergent: bool,
    pub mid_lower_high: bool,
    pub parent_complete: bool,
    pub parent_departure: Option<UpDeparture>,
    pub parent_vs_departure: Option<VersusDeparture>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Support {
    pub low: f64,
    pub low_at: NaiveDateTime,
}

/// What the sell nest reads from the parent level, which changes only with
/// the parent's bars.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParentFacts {
    pub complete: bool,
    pub up: ParentUp,
}

/// `parent_departure` (`D_P`) and `parent_vs_departure`, read from the
/// parent level alone.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ParentUp {
    pub departure: Option<UpDeparture>,
    pub versus: Option<VersusDeparture>,
}

/// The completion of `level` in direction up over the span of `s`, a segment
/// of the next coarser level `coarse`, mapped into child time.
fn completion_inside(coarse: &LevelRead, level: &LevelRead, s: &Seg) -> Option<Completion> {
    let t0 = covered_child(coarse.raw(), level.raw(), s.start_at, false)?;
    let t1 = covered_child(coarse.raw(), level.raw(), s.end_at, true)?;
    completion(level, t0, Some(t1), Direction::Up)
}

/// The sell-side nest: the middle level's latest up segment `U` ends,
/// `triggered` when its move completes on the leaf confirmed at `U`'s high,
/// otherwise `turned` when the middle level's top fractal there is
/// confirmed. `centres` are the middle level's qualified centres.
pub fn sell(
    m: &LevelRead,
    l: Option<&LevelRead>,
    centres: &[Centre],
    parent: ParentFacts,
) -> Option<Sell> {
    let (ui, u) = m.latest_up()?;
    let leaf = l
        .and_then(|l| completion_inside(m, l, &u))
        .filter(|c| c.extreme.price == u.end_price);
    let high = Endpoint {
        at: u.end_at,
        price: u.end_price,
    };
    let stage = if leaf.as_ref().is_some_and(|c| c.fact.confirmed_at.is_some()) {
        Stage::Triggered
    } else if m.confirmed(ui, Mark::Top, high).is_some() {
        Stage::Turned
    } else {
        return None;
    };
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
        stage,
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
        leaf: leaf.map(|c| c.fact),
        mid_divergent: diverges_previous || departs_centre,
        mid_lower_high,
        parent_complete: parent.complete,
        parent_departure: parent.up.departure,
        parent_vs_departure: parent.up.versus,
    })
}

/// `parent_complete`: the parent's latest up segment completes on the
/// middle level.
pub fn parent_complete(p: &LevelRead, m: &LevelRead) -> bool {
    p.latest_up()
        .and_then(|(_, s)| completion_inside(p, m, &s))
        .is_some()
}

/// `parent_departure` and `parent_vs_departure`: `D_P`, the departure of the
/// latest qualified parent centre departed upward through `zg` by an element
/// running up, and the parent's latest up segment `U` against it: a high
/// strictly over `D_P`'s and the ratio of their up areas. Both `None` without
/// `D_P`; the comparison `None` also when `U` does not start after it.
pub fn versus_departure(p: &LevelRead) -> ParentUp {
    let Some(d) = departure_up(p) else {
        return ParentUp::default();
    };
    let high = side_price(&d, Direction::Up);
    let versus = p
        .latest_up()
        .filter(|(_, u)| u.start_at >= d.end_at)
        .map(|(_, u)| VersusDeparture {
            new_high: u.end_price > high,
            area_ratio: match (p.area(&u), p.area(&d)) {
                (Some(a), Some(b)) if b > 0. => Some(a / b),
                _ => None,
            },
        });
    ParentUp {
        departure: Some(UpDeparture {
            start_at: d.start_at,
            end_at: d.end_at,
            high,
        }),
        versus,
    }
}

/// `D_P` on the parent's canonical sequence, latest qualified centre first.
fn departure_up(p: &LevelRead) -> Option<Seg> {
    let qualified = p.qualified_centres();
    let w0 = qualified.first()?.start - 1;
    let segs = p.sequence(w0);
    (0..qualified.len()).rev().find_map(|k| {
        match centre_state(p.raw(), &segs, w0, &qualified[k], qualified.get(k + 1), k) {
            State::Departed {
                departure,
                direction: Direction::Up,
                edge: "zg",
                ..
            } => segs.iter().find(|x| x.pos == departure).copied(),
            _ => None,
        }
    })
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
