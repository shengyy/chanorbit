//! Structure geometry snapshot: strokes with power, fractals, centres,
//! candidate and MACD. No buy/sell stage is evaluated here.
use crate::analyzer::Analyzer;
use crate::candidate::{CandidateStroke, candidate_stroke};
use crate::centre::canonical_centres;
use crate::fractal::{Fractal, Mark};
use crate::macd::retained_macd;
use crate::power::{MacdFacts, Power, Span, interval_power, stroke_power};
use crate::stroke::Direction;
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

/// One retained stroke with its interval power.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StrokeFact {
    pub direction: Direction,
    pub start_at: NaiveDateTime,
    pub end_at: NaiveDateTime,
    /// Endpoint high and low; the path inside the stroke may exceed them.
    pub high: f64,
    pub low: f64,
    pub start_price: f64,
    pub end_price: f64,
    /// `|end_price - start_price|` rounded to two decimals.
    pub price_amplitude: f64,
    pub newbar_count: usize,
    /// Counted as a centre member; see [`Analyzer::finished_count`].
    pub finished: bool,
    pub power: Power,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FractalFact {
    pub dt: NaiveDateTime,
    pub mark: Mark,
    pub price: f64,
    pub confirmed_at: Option<NaiveDateTime>,
}

impl From<&Fractal> for FractalFact {
    fn from(f: &Fractal) -> Self {
        Self {
            dt: f.dt,
            mark: f.mark,
            price: f.price,
            confirmed_at: f.confirmed_at(),
        }
    }
}

/// One centre of the canonical sequence; indices point into `strokes`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CentreFact {
    pub start_at: NaiveDateTime,
    pub end_at: NaiveDateTime,
    pub zd: f64,
    pub zg: f64,
    pub zz: f64,
    pub dd: f64,
    pub gg: f64,
    pub member_count: usize,
    pub member_indices: Vec<usize>,
    /// The stroke before the first member.
    pub entry_index: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CentreRelation {
    pub core: String,
    pub outer: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Geometry {
    /// `ready` with at least one stroke, otherwise `missing_material`.
    pub status: String,
    pub strokes: Vec<StrokeFact>,
    pub pending_direction: Option<Direction>,
    pub pending_newbar_count: usize,
    /// Fractals from the start of the second-to-last stroke on.
    pub fractals: Vec<FractalFact>,
    /// Canonical centres over the finished strokes.
    pub centre_sequence: Vec<CentreFact>,
    /// Indices into `centre_sequence`: the last two centres with at least
    /// three members that all touch `[zd, zg]`.
    pub previous_centre: Option<usize>,
    pub latest_centre: Option<usize>,
    pub centre_relation: Option<CentreRelation>,
    pub candidate_stroke: CandidateStroke,
    /// From `anchor_at` to the last input of an available candidate, the
    /// anchor bar included for a reverse candidate ([`Span`]).
    pub candidate_power: Option<Power>,
    /// From `anchor_at` to `end_price_at`, the same way.
    pub candidate_endpoint_power: Option<Power>,
    pub macd: MacdFacts,
}

/// Interval `[bl, bh]` (the later one) against `[al, ah]`.
pub(crate) fn relation(al: f64, ah: f64, bl: f64, bh: f64) -> String {
    if bl > ah {
        "above"
    } else if bh < al {
        "below"
    } else if bl == ah || bh == al {
        "touch"
    } else {
        "overlap"
    }
    .into()
}

pub fn geometry(c: &Analyzer) -> Geometry {
    let macd = retained_macd(c);
    let strokes = c.strokes();
    let finished = c.finished_count();
    let stroke_facts = strokes
        .iter()
        .enumerate()
        .map(|(i, b)| StrokeFact {
            direction: b.direction,
            start_at: b.start_at(),
            end_at: b.end_at(),
            high: b.high(),
            low: b.low(),
            start_price: b.start.price,
            end_price: b.end.price,
            price_amplitude: b.amplitude(),
            newbar_count: b.newbar_count(),
            finished: i < finished,
            power: stroke_power(c, &macd, b),
        })
        .collect();
    let finished_strokes = &strokes[..finished];
    let centres = canonical_centres(finished_strokes);
    let mut qualified = (0..centres.len()).filter(|&i| {
        centres[i].member_count() >= 3 && centres[i].members_touch_core(finished_strokes)
    });
    let latest_centre = qualified.next_back();
    let previous_centre = qualified.next_back();
    let centre_sequence: Vec<CentreFact> = centres
        .iter()
        .map(|z| CentreFact {
            start_at: z.start_at,
            end_at: z.end_at,
            zd: z.core_low,
            zg: z.core_high,
            zz: z.core_mid,
            dd: z.low,
            gg: z.high,
            member_count: z.member_count(),
            member_indices: (z.start..z.end).collect(),
            entry_index: z.start - 1,
        })
        .collect();
    let centre_relation = previous_centre.zip(latest_centre).map(|(a, b)| {
        let (a, b) = (&centre_sequence[a], &centre_sequence[b]);
        CentreRelation {
            core: relation(a.zd, a.zg, b.zd, b.zg),
            outer: relation(a.dd, a.gg, b.dd, b.gg),
        }
    });
    let since = strokes.len().checked_sub(2).map(|i| strokes[i].start_at());
    let fractals = c
        .fractals()
        .iter()
        .filter(|f| since.is_none_or(|s| f.dt >= s))
        .map(FractalFact::from)
        .collect();
    let candidate = candidate_stroke(c);
    let span = if candidate.kind.as_deref() == Some("reverse") {
        Span::Turn
    } else {
        Span::Extension
    };
    let window = |end: Option<NaiveDateTime>| {
        if candidate.state == "available" {
            candidate
                .anchor_at
                .zip(end)
                .map(|(s, e)| interval_power(c, &macd, span, s, e))
        } else {
            None
        }
    };
    let candidate_power = window(c.raw_bars().last().map(|b| b.dt));
    let candidate_endpoint_power = window(candidate.end_price_at);
    Geometry {
        status: if strokes.is_empty() {
            "missing_material"
        } else {
            "ready"
        }
        .into(),
        strokes: stroke_facts,
        pending_direction: c.pending_direction(),
        pending_newbar_count: c.pending_bars().len(),
        fractals,
        centre_sequence,
        previous_centre,
        latest_centre,
        centre_relation,
        candidate_stroke: candidate,
        candidate_power,
        candidate_endpoint_power,
        macd: MacdFacts::capture(c, &macd),
    }
}
