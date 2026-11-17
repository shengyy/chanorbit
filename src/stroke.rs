//! Strokes (笔), their endpoints and the single-stroke search over unfinished NewBars.
use crate::bar::NewBar;
use crate::fractal::{Fractal, Mark, scan_fractals};
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Direction {
    #[serde(rename = "向上")]
    Up,
    #[serde(rename = "向下")]
    Down,
}

impl Direction {
    pub fn opposite(self) -> Self {
        match self {
            Direction::Up => Direction::Down,
            Direction::Down => Direction::Up,
        }
    }
}

/// A stroke endpoint: a price and the raw bar that holds it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Endpoint {
    pub at: NaiveDateTime,
    pub price: f64,
}

impl Endpoint {
    fn of(fractal: &Fractal) -> Self {
        Self {
            at: fractal.dt,
            price: fractal.price,
        }
    }
}

/// A stroke identified by fractal `start_fractal` and the opposite fractal
/// `end_fractal`, running from `start` to `end`.
#[derive(Debug, Clone, PartialEq)]
pub struct Stroke {
    pub start_fractal: Fractal,
    pub end_fractal: Fractal,
    /// The start fractal's point, or the more extreme raw bar after it (see
    /// [`find_stroke`]); the previous stroke ends here too.
    pub start: Endpoint,
    /// The end fractal's point.
    pub end: Endpoint,
    /// Fractals whose time lies within the stroke's NewBar span.
    pub fractals: Vec<Fractal>,
    pub direction: Direction,
    /// Snapshot of the NewBars from `start_fractal`'s first to `end_fractal`'s last bar.
    pub newbars: Vec<Arc<NewBar>>,
}

impl Stroke {
    pub fn start_at(&self) -> NaiveDateTime {
        self.start.at
    }

    pub fn end_at(&self) -> NaiveDateTime {
        self.end.at
    }

    /// The higher endpoint price; the path may exceed it only on the end side.
    pub fn high(&self) -> f64 {
        self.start.price.max(self.end.price)
    }

    /// The lower endpoint price; the path may exceed it only on the end side.
    pub fn low(&self) -> f64 {
        self.start.price.min(self.end.price)
    }

    /// `|end - start|` rounded to two decimals.
    pub fn amplitude(&self) -> f64 {
        ((self.end.price - self.start.price).abs() * 100.0).round() / 100.0
    }

    pub fn newbar_count(&self) -> usize {
        self.newbars.len()
    }

    /// This stroke's NewBars without the end fractal's middle and last, then
    /// `later` from the end fractal's middle on: the window to search again
    /// from this stroke's start. `later` starts at the end fractal's first NewBar.
    pub fn reopen(&self, later: &[Arc<NewBar>]) -> Vec<Arc<NewBar>> {
        let keep = self.newbars.len() - 2;
        let merge_point = self.newbars[keep].dt;
        self.newbars[..keep]
            .iter()
            .chain(later.iter().filter(|b| b.dt >= merge_point))
            .cloned()
            .collect()
    }

    /// Whether this stroke can end at a raw bar at `until`: no raw bar in
    /// `later` from its end fractal's up to `until` is beyond its start.
    /// `later` starts at the end fractal's first NewBar.
    pub fn holds_end(&self, until: NaiveDateTime, later: &[Arc<NewBar>]) -> bool {
        let side = self.direction.opposite();
        !later
            .iter()
            .flat_map(|b| &b.elements)
            .filter(|r| r.dt >= self.end_fractal.dt && r.dt < until)
            .any(|r| {
                beyond(
                    side,
                    if side == Direction::Up { r.high } else { r.low },
                    self.start.price,
                )
            })
    }
}

/// Whether `price` lies beyond `than` on the side a `direction` stroke ends.
pub fn beyond(direction: Direction, price: f64, than: f64) -> bool {
    match direction {
        Direction::Up => price > than,
        Direction::Down => price < than,
    }
}

/// The start of a stroke identified by `start_fractal` and `end_fractal`: the
/// raw bar most beyond the start fractal's price on its side (the first wins
/// ties) from the start fractal's middle NewBar up to, not including, the end
/// fractal's raw bar; the start fractal's own point when none is beyond it.
/// Inclusion can hide such a bar inside a merged NewBar.
fn true_start(
    bars: &[Arc<NewBar>],
    start_fractal: &Fractal,
    end_fractal: &Fractal,
    direction: Direction,
) -> Endpoint {
    let side = direction.opposite();
    let from = bars.partition_point(|b| b.dt < start_fractal.dt);
    let mut start = Endpoint::of(start_fractal);
    for raw in bars[from..]
        .iter()
        .flat_map(|b| &b.elements)
        .take_while(|r| r.dt < end_fractal.dt)
    {
        let price = if side == Direction::Up {
            raw.high
        } else {
            raw.low
        };
        if beyond(side, price, start.price) {
            start = Endpoint { at: raw.dt, price };
        }
    }
    start
}

/// Find the first stroke in `bars`. Returns the stroke and the index where the
/// remaining unfinished bars start (0 when no stroke is formed).
pub fn find_stroke(bars: &[Arc<NewBar>], min_stroke_bars: usize) -> (Option<Stroke>, usize) {
    let fractals = scan_fractals(bars);
    if fractals.len() < 2 {
        return (None, 0);
    }
    let start_fractal = &fractals[0];
    // The first of equal extremes wins.
    let mut end_fractal: Option<&Fractal> = None;
    let direction = match start_fractal.mark {
        Mark::Bottom => {
            for x in fractals.iter().filter(|x| {
                x.mark == Mark::Top && x.dt > start_fractal.dt && x.price > start_fractal.price
            }) {
                if end_fractal.is_none_or(|best| x.high > best.high) {
                    end_fractal = Some(x);
                }
            }
            Direction::Up
        }
        Mark::Top => {
            for x in fractals.iter().filter(|x| {
                x.mark == Mark::Bottom && x.dt > start_fractal.dt && x.price < start_fractal.price
            }) {
                if end_fractal.is_none_or(|best| x.low < best.low) {
                    end_fractal = Some(x);
                }
            }
            Direction::Down
        }
    };
    let Some(end_fractal) = end_fractal else {
        return (None, 0);
    };
    let start_dt = start_fractal.elements[0].dt;
    let end_dt = end_fractal.elements[2].dt;
    let start = bars.partition_point(|b| b.dt < start_dt);
    let end = bars.partition_point(|b| b.dt <= end_dt);
    if start >= end {
        return (None, 0);
    }
    let rest = bars.partition_point(|b| b.dt < end_fractal.elements[0].dt);
    let ab_include = (start_fractal.high > end_fractal.high && start_fractal.low < end_fractal.low)
        || (start_fractal.high < end_fractal.high && start_fractal.low > end_fractal.low);
    if ab_include || end - start < min_stroke_bars {
        return (None, 0);
    }
    let stroke = Stroke {
        start: true_start(bars, start_fractal, end_fractal, direction),
        end: Endpoint::of(end_fractal),
        start_fractal: start_fractal.clone(),
        end_fractal: end_fractal.clone(),
        fractals: fractals
            .iter()
            .filter(|x| x.dt >= start_dt && x.dt <= end_dt)
            .cloned()
            .collect(),
        direction,
        newbars: bars[start..end].to_vec(),
    };
    (Some(stroke), rest)
}
