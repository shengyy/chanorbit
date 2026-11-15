//! Per-bar replay of the buy-side stages: first-known time, stage age and
//! what followed the retest. Definitions are owned by
//! `docs/structure-contract.md#observation`.
use crate::analyzer::{Analyzer, Config};
use crate::bar::RawBar;
use crate::candidate::candidate_stroke;
use crate::stage::{Stage, StageKind, stages};
use crate::stroke::{Direction, Stroke};
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A stroke named by a stage role.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StrokeRef {
    pub start_at: NaiveDateTime,
    pub end_at: NaiveDateTime,
    /// Endpoint high and low, as in the geometry snapshot.
    pub high: f64,
    pub low: f64,
}

impl From<&Stroke> for StrokeRef {
    fn from(s: &Stroke) -> Self {
        Self {
            start_at: s.start_at(),
            end_at: s.end_at(),
            high: s.high(),
            low: s.low(),
        }
    }
}

/// The centre a stage refers to: its start and core bounds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CentreRef {
    pub start_at: NaiveDateTime,
    pub zd: f64,
    pub zg: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParentRef {
    pub rebound: StrokeRef,
    pub retest: StrokeRef,
}

/// An upward stroke from the retest's end that was the last stroke and was
/// then withdrawn, as last seen before the withdrawal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WithdrawnRebound {
    pub start_at: NaiveDateTime,
    pub end_at: NaiveDateTime,
    pub high: f64,
    pub low: f64,
    /// Observed input from which the last stroke was this version of it.
    pub observed_at: NaiveDateTime,
    /// Observed input at which it was withdrawn.
    pub withdrawn_at: NaiveDateTime,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StageFact {
    pub kind: StageKind,
    pub centre: CentreRef,
    pub entry: StrokeRef,
    pub departure: StrokeRef,
    pub rebound: Option<StrokeRef>,
    pub retest: StrokeRef,
    pub first_point_anchor: Option<StrokeRef>,
    pub parent: Option<ParentRef>,
    /// First observed input at which this same stage stood; `null` when it
    /// already stood at the first observed input or was never observed.
    pub first_known_at: Option<NaiveDateTime>,
    /// Observed inputs after `first_known_at`, up to the last input.
    pub stage_age: Option<usize>,
    /// The available reverse candidate's end price is above the retest's high.
    pub beyond_start: bool,
    /// The latest withdrawn rebound from the retest's end, if any.
    pub withdrawn_rebound: Option<WithdrawnRebound>,
}

/// Exact stroke identity: direction, endpoint times and prices, high, low.
type StrokeKey = (Direction, NaiveDateTime, NaiveDateTime, [u64; 4]);

fn stroke_key(s: &Stroke) -> StrokeKey {
    let prices = [
        s.start_fractal.price,
        s.end_fractal.price,
        s.high(),
        s.low(),
    ];
    (
        s.direction,
        s.start_at(),
        s.end_at(),
        prices.map(f64::to_bits),
    )
}

/// The same stage: kind, the centre's core bounds and first three members,
/// and every role stroke. A centre growing more members keeps its identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Identity {
    kind: StageKind,
    core: (u64, u64, Vec<StrokeKey>),
    roles: Vec<Option<StrokeKey>>,
}

fn identity(s: &[Stroke], st: &Stage) -> Identity {
    let key = |i: usize| stroke_key(&s[i]);
    let core = (st.centre.start..st.centre.end).take(3).map(key).collect();
    let (parent_rebound, parent_retest) = st.parent.unzip();
    Identity {
        kind: st.kind,
        core: (
            st.centre.core_low.to_bits(),
            st.centre.core_high.to_bits(),
            core,
        ),
        roles: [
            Some(st.entry),
            Some(st.departure),
            st.rebound,
            st.first_point_anchor,
            Some(st.retest),
            parent_rebound,
            parent_retest,
        ]
        .into_iter()
        .map(|i| i.map(key))
        .collect(),
    }
}

/// The last stroke as the withdrawal rule compares it, and the observed
/// input from which it was the last stroke.
#[derive(Debug, Clone, PartialEq)]
struct LastStroke {
    direction: Direction,
    stroke: StrokeRef,
    observed_at: NaiveDateTime,
}

/// Replay `bars` one input at a time and return the confirmed stages after
/// the last input. Observation starts at the first input with
/// `dt >= observe_from` (the first input when absent); earlier inputs only
/// build state. A revision is observed once, after its last version.
pub fn observed_stages(
    bars: &[RawBar],
    observe_from: Option<NaiveDateTime>,
    config: Config,
) -> Vec<StageFact> {
    let mut c = Analyzer::new(config);
    let mut times: Vec<NaiveDateTime> = Vec::new();
    // Identity -> index into `times` of its first observation, or `None`
    // when it already stood at the first observed input.
    let mut seen: HashMap<Identity, Option<usize>> = HashMap::new();
    // Downward stroke (end, low) -> the latest withdrawn upward stroke from it.
    let mut withdrawn: HashMap<(NaiveDateTime, u64), WithdrawnRebound> = HashMap::new();
    let mut previous_last: Option<LastStroke> = None;
    for (i, bar) in bars.iter().enumerate() {
        c.update(*bar);
        let revised_next = bars.get(i + 1).is_some_and(|b| b.dt == bar.dt);
        if revised_next || observe_from.is_some_and(|t| bar.dt < t) {
            continue;
        }
        let index = times.len();
        times.push(bar.dt);
        for st in stages(&c) {
            seen.entry(identity(c.strokes(), &st))
                .or_insert((index > 0).then_some(index));
        }
        let Some(last) = c.strokes().last() else {
            continue;
        };
        let (direction, now) = (last.direction, StrokeRef::from(last));
        if let Some(prior) = &previous_last
            && index > 0
            && now.start_at < prior.stroke.start_at
            && prior.direction == Direction::Up
            && direction == Direction::Down
            && prior.stroke.start_at == now.end_at
            && prior.stroke.low == now.low
        {
            let p = &prior.stroke;
            withdrawn.insert(
                (now.end_at, now.low.to_bits()),
                WithdrawnRebound {
                    start_at: p.start_at,
                    end_at: p.end_at,
                    high: p.high,
                    low: p.low,
                    observed_at: prior.observed_at,
                    withdrawn_at: bar.dt,
                },
            );
        }
        if previous_last
            .as_ref()
            .is_none_or(|p| (p.direction, &p.stroke) != (direction, &now))
        {
            previous_last = Some(LastStroke {
                direction,
                stroke: now,
                observed_at: bar.dt,
            });
        }
    }
    let s = c.strokes();
    let candidate = candidate_stroke(&c);
    let last_index = times.len().checked_sub(1);
    stages(&c)
        .iter()
        .map(|st| {
            let known = seen.get(&identity(s, st)).copied().flatten();
            let retest = &s[st.retest];
            let beyond = candidate.state == "available"
                && candidate.kind.as_deref() == Some("reverse")
                && candidate.end_price.is_some_and(|p| p > retest.high());
            let r = |i: usize| StrokeRef::from(&s[i]);
            StageFact {
                kind: st.kind,
                centre: CentreRef {
                    start_at: st.centre.start_at,
                    zd: st.centre.core_low,
                    zg: st.centre.core_high,
                },
                entry: r(st.entry),
                departure: r(st.departure),
                rebound: st.rebound.map(r),
                retest: r(st.retest),
                first_point_anchor: st.first_point_anchor.map(r),
                parent: st.parent.map(|(rebound, retest)| ParentRef {
                    rebound: r(rebound),
                    retest: r(retest),
                }),
                first_known_at: known.map(|k| times[k]),
                stage_age: known.zip(last_index).map(|(k, last)| last - k),
                beyond_start: beyond,
                withdrawn_rebound: withdrawn
                    .get(&(retest.end_at(), retest.low().to_bits()))
                    .cloned(),
            }
        })
        .collect()
}
