//! Structure geometry snapshot: strokes with power, fractals, centres,
//! candidate and MACD. No buy/sell stage is evaluated here.
use crate::analyzer::Analyzer;
use crate::candidate::{CandidateStroke, candidate_stroke};
use crate::centre::canonical_centres;
use crate::fractal::{Fractal, Mark};
use crate::macd::retained_macd;
use crate::power::{MacdFacts, Power, interval_power, stroke_power};
use crate::stroke::Direction;
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

/// One retained stroke with its interval power.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StrokeFact {
    pub direction: Direction,
    pub sdt: NaiveDateTime,
    pub edt: NaiveDateTime,
    /// Endpoint high and low; the path inside the stroke may exceed them.
    pub high: f64,
    pub low: f64,
    pub start_price: f64,
    pub end_price: f64,
    /// `|end_price - start_price|` rounded to two decimals.
    pub price_amplitude: f64,
    pub newbar_count: usize,
    /// Counted as a centre member; see [`Analyzer::finished_count`].
    pub included_in_finished_bis: bool,
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

/// One centre of the canonical sequence; indices point into `bi`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CentreFact {
    pub sdt: NaiveDateTime,
    pub edt: NaiveDateTime,
    pub zd: f64,
    pub zg: f64,
    pub zz: f64,
    pub dd: f64,
    pub gg: f64,
    pub bi_count: usize,
    pub member_indices: Vec<usize>,
    /// The stroke before the first member.
    pub entry_index: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ZsRelation {
    pub core: String,
    pub outer: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Geometry {
    /// `ready` with at least one stroke, otherwise `missing_material`.
    pub status: String,
    pub bi: Vec<StrokeFact>,
    pub ubi_direction: Option<Direction>,
    pub ubi_newbar_count: usize,
    /// Fractals from the start of the second-to-last stroke on.
    pub fractals: Vec<FractalFact>,
    /// Canonical centres over the finished strokes.
    pub centre_sequence: Vec<CentreFact>,
    /// Indices into `centre_sequence`: the last two centres with at least
    /// three members that all touch `[zd, zg]`.
    pub previous_zs: Option<usize>,
    pub latest_finished_zs: Option<usize>,
    pub zs_relation: Option<ZsRelation>,
    pub candidate_stroke: CandidateStroke,
    /// Interval `(anchor_at, last input dt]` of an available candidate.
    pub candidate_power: Option<Power>,
    /// Interval `(anchor_at, end_price_at]` of an available candidate.
    pub candidate_endpoint_power: Option<Power>,
    pub macd: MacdFacts,
}

fn relation(al: f64, ah: f64, bl: f64, bh: f64) -> String {
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
    let bi = strokes
        .iter()
        .enumerate()
        .map(|(i, b)| StrokeFact {
            direction: b.direction,
            sdt: b.sdt(),
            edt: b.edt(),
            high: b.high(),
            low: b.low(),
            start_price: b.start_fractal.price,
            end_price: b.end_fractal.price,
            price_amplitude: b.amplitude(),
            newbar_count: b.newbar_count(),
            included_in_finished_bis: i < finished,
            power: stroke_power(c, &macd, b),
        })
        .collect();
    let finished_strokes = &strokes[..finished];
    let centres = canonical_centres(finished_strokes);
    let mut qualified = (0..centres.len()).filter(|&i| {
        centres[i].member_count() >= 3 && centres[i].members_touch_core(finished_strokes)
    });
    let latest_finished_zs = qualified.next_back();
    let previous_zs = qualified.next_back();
    let centre_sequence: Vec<CentreFact> = centres
        .iter()
        .map(|z| CentreFact {
            sdt: z.sdt,
            edt: z.edt,
            zd: z.core_low,
            zg: z.core_high,
            zz: z.core_mid,
            dd: z.low,
            gg: z.high,
            bi_count: z.member_count(),
            member_indices: (z.start..z.end).collect(),
            entry_index: z.start - 1,
        })
        .collect();
    let zs_relation = previous_zs.zip(latest_finished_zs).map(|(a, b)| {
        let (a, b) = (&centre_sequence[a], &centre_sequence[b]);
        ZsRelation {
            core: relation(a.zd, a.zg, b.zd, b.zg),
            outer: relation(a.dd, a.gg, b.dd, b.gg),
        }
    });
    let since = strokes.len().checked_sub(2).map(|i| strokes[i].sdt());
    let fractals = c
        .fractals()
        .iter()
        .filter(|f| since.is_none_or(|s| f.dt >= s))
        .map(FractalFact::from)
        .collect();
    let candidate = candidate_stroke(c);
    let window = |end: Option<NaiveDateTime>| {
        if candidate.state == "available" {
            candidate
                .anchor_at
                .zip(end)
                .map(|(s, e)| interval_power(c, &macd, s, e))
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
        bi,
        ubi_direction: c.pending_direction(),
        ubi_newbar_count: c.pending_bars().len(),
        fractals,
        centre_sequence,
        previous_zs,
        latest_finished_zs,
        zs_relation,
        candidate_stroke: candidate,
        candidate_power,
        candidate_endpoint_power,
        macd: MacdFacts::capture(c, &macd),
    }
}
