//! Display-only tail geometry after the last stroke. It never runs the stroke
//! search and does not claim that a new stroke exists.
use crate::analyzer::Analyzer;
use crate::bar::NewBar;
use crate::stroke::Direction;
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

/// Candidate segment from the last stroke's end fractal to the most extreme
/// later NewBar boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateStroke {
    pub state: String,
    pub reason: Option<String>,
    pub kind: Option<String>,
    pub direction: Option<Direction>,
    /// Natural endpoint of the anchor fractal, not its observation time.
    pub anchor_at: Option<NaiveDateTime>,
    pub anchor_price: Option<f64>,
    pub anchor_price_at: Option<NaiveDateTime>,
    pub end_price: Option<f64>,
    pub end_price_at: Option<NaiveDateTime>,
}

fn boundary(bar: &NewBar, up: bool) -> f64 {
    if up { bar.high } else { bar.low }
}

impl CandidateStroke {
    fn unavailable(mut self, reason: &str) -> Self {
        self.state = "unavailable".into();
        self.reason = Some(reason.into());
        self
    }
}

pub fn candidate_stroke(c: &Analyzer) -> CandidateStroke {
    let mut result = CandidateStroke {
        state: "none".into(),
        reason: Some("no_identified_stroke".into()),
        kind: None,
        direction: None,
        anchor_at: None,
        anchor_price: None,
        anchor_price_at: None,
        end_price: None,
        end_price_at: None,
    };
    let Some(bi) = c.bi_list().last() else {
        return result;
    };
    let anchor = &bi.fx_b;
    let up = bi.direction == Direction::Up;
    result.anchor_at = Some(anchor.dt);
    result.anchor_price = anchor.fx.is_finite().then_some(anchor.fx);
    let owners: Vec<_> = anchor
        .elements
        .iter()
        .flat_map(|b| &b.elements)
        .filter(|b| (if up { b.high } else { b.low }) == anchor.fx)
        .collect();
    let Some(owner) = owners.iter().find(|b| b.dt == anchor.dt).or(owners.first()) else {
        return result.unavailable("anchor_price_source_missing");
    };
    result.anchor_price_at = Some(owner.dt);
    let ubi = c.bars_ubi();
    let Some(central) = ubi
        .iter()
        .position(|b| b.dt == anchor.dt || b.elements.iter().any(|r| r.dt == anchor.dt))
    else {
        return result.unavailable("anchor_not_in_engine_tail");
    };
    let extension = c.last_bi_extend();
    let forward_up = if extension { up } else { !up };
    result.kind = Some(if extension { "extension" } else { "reverse" }.into());
    result.direction = Some(if forward_up {
        Direction::Up
    } else {
        Direction::Down
    });
    let eligible = &ubi[central + usize::from(!extension)..];
    if !anchor.fx.is_finite()
        || eligible
            .iter()
            .any(|b| !b.high.is_finite() || !b.low.is_finite())
    {
        return result.unavailable("non_finite");
    }
    // Stable first extreme on NewBar boundaries; only then locate the raw owner.
    let end = eligible
        .iter()
        .filter(|b| {
            if forward_up {
                b.high > anchor.fx
            } else {
                b.low < anchor.fx
            }
        })
        .reduce(|a, b| {
            let beyond = if forward_up {
                b.high > a.high
            } else {
                b.low < a.low
            };
            if beyond { b } else { a }
        });
    let Some(end) = end else {
        result.reason = Some("no_forward_extreme".into());
        return result;
    };
    let price = boundary(end, forward_up);
    let owners: Vec<_> = end
        .elements
        .iter()
        .filter(|b| b.dt > anchor.dt && (if forward_up { b.high } else { b.low }) == price)
        .collect();
    let Some(owner) = owners.iter().find(|b| b.dt == end.dt).or(owners.first()) else {
        return result.unavailable("no_forward_price_source");
    };
    let raw = c.bars_raw();
    if !raw
        .iter()
        .any(|b| b.dt == owner.dt && (if forward_up { b.high } else { b.low }) == price)
        || !raw.iter().any(|b| Some(b.dt) == result.anchor_price_at)
    {
        return result.unavailable("missing_coverage");
    }
    result.state = "available".into();
    result.reason = None;
    result.end_price = Some(price);
    result.end_price_at = Some(owner.dt);
    result
}
