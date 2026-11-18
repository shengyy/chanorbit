//! Display-only tail geometry after the last stroke. It never runs the stroke
//! search and does not claim that a new stroke exists.
use crate::analyzer::Analyzer;
use crate::stroke::{Direction, beyond};
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

/// Candidate segment from the last stroke's end to the most extreme later raw price.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateStroke {
    pub state: String,
    pub reason: Option<String>,
    pub kind: Option<String>,
    pub direction: Option<Direction>,
    /// The last stroke's end, not its observation time.
    pub anchor_at: Option<NaiveDateTime>,
    pub anchor_price: Option<f64>,
    pub anchor_price_at: Option<NaiveDateTime>,
    pub end_price: Option<f64>,
    pub end_price_at: Option<NaiveDateTime>,
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
    let Some(stroke) = c.strokes().last() else {
        return result;
    };
    let anchor = stroke.end;
    result.anchor_at = Some(anchor.at);
    result.anchor_price = anchor.price.is_finite().then_some(anchor.price);
    result.anchor_price_at = Some(anchor.at);
    let extension = c.extension();
    let forward = if extension.is_some() {
        stroke.direction
    } else {
        stroke.direction.opposite()
    };
    result.kind = Some(
        if extension.is_some() {
            "extension"
        } else {
            "reverse"
        }
        .into(),
    );
    result.direction = Some(forward);
    let raw = c.raw_bars();
    let after = &raw[raw.partition_point(|b| b.dt <= anchor.at)..];
    if !anchor.price.is_finite()
        || after
            .iter()
            .any(|b| !b.high.is_finite() || !b.low.is_finite())
    {
        result.state = "unavailable".into();
        result.reason = Some("non_finite".into());
        return result;
    }
    let end = extension.or_else(|| {
        c.after_end(forward)
            .filter(|end| beyond(forward, end.price, anchor.price))
    });
    let Some(end) = end else {
        result.reason = Some("no_forward_extreme".into());
        return result;
    };
    result.state = "available".into();
    result.reason = None;
    result.end_price = Some(end.price);
    result.end_price_at = Some(end.at);
    result
}
