//! Raw-interval power over a stroke's or piece's bars and the retained MACD view.
use crate::analyzer::Analyzer;
use crate::macd::{LOOKBACK, MacdSeries};
use crate::stroke::Stroke;
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

fn finite(value: f64) -> Option<f64> {
    value.is_finite().then_some(value)
}

/// MACD over an interval. Areas sum the positive and negative histogram parts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PowerMacd {
    pub status: String,
    pub reason: Option<String>,
    pub positive_area: Option<f64>,
    pub negative_area: Option<f64>,
    pub dif_at_end: Option<f64>,
    pub dea_at_end: Option<f64>,
    /// Rows in the interval with DIF, DEA and histogram all finite.
    pub ready_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Power {
    /// Retained raw bars in the interval (see [`Span`]).
    pub sample_count: usize,
    pub mean_volume: Option<f64>,
    pub macd: PowerMacd,
}

/// Whether an interval's start bar is one of its rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Span {
    /// `start ≤ dt ≤ end`: a stroke, or a piece starting where price turned.
    /// The bar holding the turn opens it, as it opens the stroke it would become.
    Turn,
    /// `start < dt ≤ end`: an extension, the bars after the end it extends.
    Extension,
}

impl Span {
    fn holds(self, dt: NaiveDateTime, start: NaiveDateTime, end: NaiveDateTime) -> bool {
        dt <= end
            && match self {
                Span::Turn => dt >= start,
                Span::Extension => dt > start,
            }
    }
}

/// Power over the retained raw bars of `span` from `start` to `end`. Missing
/// endpoints or non-increasing times mean `missing_coverage`; any non-finite
/// close up to `end` means `non_finite`; unready rows before [`LOOKBACK`] mean
/// `warmup`. Unavailable areas are `None`, never 0.
pub fn interval_power(
    c: &Analyzer,
    s: &MacdSeries,
    span: Span,
    start: NaiveDateTime,
    end: NaiveDateTime,
) -> Power {
    let raw = c.raw_bars();
    let rows: Vec<usize> = (0..raw.len())
        .filter(|&i| span.holds(raw[i].dt, start, end))
        .collect();
    let at = |dt| raw.iter().position(|b| b.dt == dt);
    let value =
        |v: &[f64], idx: Option<usize>| idx.and_then(|i| v.get(i)).copied().and_then(finite);
    let ready = |i: usize| {
        s.dif.get(i).is_some_and(|x| x.is_finite())
            && s.dea.get(i).is_some_and(|x| x.is_finite())
            && s.hist.get(i).is_some_and(|x| x.is_finite())
    };
    let ready_count = rows.iter().filter(|&&i| ready(i)).count();
    let coverage = start < end
        && at(start).is_some()
        && at(end).is_some()
        && s.hist.len() == raw.len()
        && raw.windows(2).all(|w| w[0].dt < w[1].dt);
    let non_finite = raw
        .iter()
        .filter(|b| b.dt <= end)
        .any(|b| !b.close.is_finite());
    let mut reason = if !coverage || rows.is_empty() {
        Some("missing_coverage")
    } else if non_finite {
        Some("non_finite")
    } else if ready_count != rows.len() {
        Some(if rows.iter().any(|&i| i < LOOKBACK) {
            "warmup"
        } else {
            "non_finite"
        })
    } else {
        None
    };
    let area = |positive: bool| {
        finite(
            rows.iter()
                .map(|&i| {
                    if positive {
                        s.hist[i].max(0.)
                    } else {
                        (-s.hist[i]).max(0.)
                    }
                })
                .sum(),
        )
    };
    let (mut positive_area, mut negative_area) = if reason.is_none() {
        (area(true), area(false))
    } else {
        (None, None)
    };
    if reason.is_none() && (positive_area.is_none() || negative_area.is_none()) {
        reason = Some("non_finite");
        positive_area = None;
        negative_area = None;
    }
    let volume = if coverage && !rows.is_empty() {
        finite(rows.iter().map(|&i| raw[i].vol).sum())
    } else {
        None
    };
    Power {
        sample_count: rows.len(),
        mean_volume: volume.map(|v| v / rows.len() as f64),
        macd: PowerMacd {
            status: if reason.is_none() {
                "ready"
            } else {
                "unavailable"
            }
            .into(),
            reason: reason.map(str::to_owned),
            positive_area,
            negative_area,
            dif_at_end: value(&s.dif, at(end)),
            dea_at_end: value(&s.dea, at(end)),
            ready_count,
        },
    }
}

/// Stroke power over `[start_at, end_at]`; a raw bar the stroke snapshot holds
/// there that is no longer retained makes areas and the mean volume unavailable.
pub fn stroke_power(c: &Analyzer, s: &MacdSeries, stroke: &Stroke) -> Power {
    let (start, end) = (stroke.start_at(), stroke.end_at());
    let mut power = interval_power(c, s, Span::Turn, start, end);
    let raw = c.raw_bars();
    let missing = stroke
        .newbars
        .iter()
        .flat_map(|b| &b.elements)
        .filter(|b| Span::Turn.holds(b.dt, start, end))
        .any(|b| raw.binary_search_by_key(&b.dt, |r| r.dt).is_err());
    if missing {
        power.macd.status = "unavailable".into();
        power.macd.reason = Some("missing_coverage".into());
        power.macd.positive_area = None;
        power.macd.negative_area = None;
        power.mean_volume = None;
    }
    power
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MacdBar {
    pub dt: NaiveDateTime,
    pub dif: Option<f64>,
    pub dea: Option<f64>,
    pub hist: Option<f64>,
}

/// A maximal run of same-sign histogram bars. Zero continues an open run and
/// never starts one; a missing value closes it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MacdSegment {
    pub sign: i8,
    pub start_at: NaiveDateTime,
    pub end_at: NaiveDateTime,
    pub bars: usize,
    /// Signed value with the largest magnitude; the first one wins ties.
    pub peak: f64,
    pub peak_at: NaiveDateTime,
}

/// MACD of every retained raw bar and its histogram segments.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MacdFacts {
    pub bars: Vec<MacdBar>,
    pub segments: Vec<MacdSegment>,
}

pub fn macd_segments(bars: &[MacdBar]) -> Vec<MacdSegment> {
    let mut out: Vec<MacdSegment> = Vec::new();
    let mut open = false;
    for b in bars {
        let Some(h) = b.hist else {
            open = false;
            continue;
        };
        let sign: i8 = if h > 0. {
            1
        } else if h < 0. {
            -1
        } else {
            0
        };
        let joins = open && out.last().is_some_and(|s| sign == 0 || sign == s.sign);
        if joins {
            let s = out.last_mut().unwrap();
            s.end_at = b.dt;
            s.bars += 1;
            if h.abs() > s.peak.abs() {
                s.peak = h;
                s.peak_at = b.dt;
            }
        } else if sign != 0 {
            out.push(MacdSegment {
                sign,
                start_at: b.dt,
                end_at: b.dt,
                bars: 1,
                peak: h,
                peak_at: b.dt,
            });
            open = true;
        }
    }
    out
}

impl MacdFacts {
    pub fn capture(c: &Analyzer, s: &MacdSeries) -> Self {
        let bars: Vec<MacdBar> = c
            .raw_bars()
            .iter()
            .enumerate()
            .map(|(i, b)| MacdBar {
                dt: b.dt,
                dif: s.dif.get(i).copied().and_then(finite),
                dea: s.dea.get(i).copied().and_then(finite),
                hist: s.hist.get(i).copied().and_then(finite),
            })
            .collect();
        Self {
            segments: macd_segments(&bars),
            bars,
        }
    }
}
