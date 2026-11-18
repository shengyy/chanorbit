//! Strokes (笔), their raw endpoints and the single-stroke search over unfinished NewBars.
use crate::bar::{NewBar, RawBar};
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
    /// The most extreme start-side raw price of its raw span (see
    /// [`Stroke::place`]); the previous stroke ends here too.
    pub start: Endpoint,
    /// The most extreme end-side raw price from its start up to its end
    /// fractal's bar, until the next stroke starts and it ends there.
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

    /// The higher endpoint price.
    pub fn high(&self) -> f64 {
        self.start.price.max(self.end.price)
    }

    /// The lower endpoint price.
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

    /// The start over the retained raw bars `raws` once this stroke follows a
    /// stroke starting at `after`: the most extreme start-side raw price after
    /// `after` (without a stroke before, from this stroke's first raw bar) up
    /// to, not including, the end fractal's bar.
    pub fn start_after(&self, raws: &[RawBar], after: Option<NaiveDateTime>) -> Endpoint {
        let from = match after {
            Some(after) => raws.partition_point(|r| r.dt <= after),
            None => {
                let first = self.newbars[0].elements[0].dt;
                raws.partition_point(|r| r.dt < first)
            }
        };
        let to = raws.partition_point(|r| r.dt < self.end_fractal.dt);
        let side = self.direction.opposite();
        extreme(&raws[from..to.max(from)], side, Some(self.start_fractal.dt))
            .unwrap_or(Endpoint::of(&self.start_fractal))
    }

    /// Place both endpoints over `raws` (see [`Stroke::start_after`]); the end
    /// is the most extreme end-side raw price after the start up to the end
    /// fractal's bar.
    pub fn place(&mut self, raws: &[RawBar], after: Option<NaiveDateTime>) {
        self.start = self.start_after(raws, after);
        self.end = self.end_from(raws, self.start);
    }

    /// The end once the stroke starts at `start`.
    pub fn end_from(&self, raws: &[RawBar], start: Endpoint) -> Endpoint {
        let from = raws.partition_point(|r| r.dt <= start.at);
        let to = raws.partition_point(|r| r.dt <= self.end_fractal.dt);
        extreme(
            &raws[from..to.max(from)],
            self.direction,
            Some(self.end_fractal.dt),
        )
        .unwrap_or(Endpoint::of(&self.end_fractal))
    }

    /// The start once this stroke ends at `until`: the most extreme start-side
    /// raw price from its start up to, not including, `until`.
    pub fn restart(&self, until: NaiveDateTime, raws: &[RawBar]) -> Endpoint {
        let from = raws.partition_point(|r| r.dt < self.start.at);
        let to = raws.partition_point(|r| r.dt < until);
        extreme(
            &raws[from..to.max(from)],
            self.direction.opposite(),
            Some(self.start.at),
        )
        .unwrap_or(self.start)
    }

    /// Whether this stroke can end at a raw bar at `until`: no raw bar after
    /// its start and before `until` is beyond its start. With `next_end`, the
    /// price where the next stroke from `until` ends, the bar at `until` may
    /// reach beyond the start only when it does not also reach beyond
    /// `next_end`: no order of its high and low would give both strokes their
    /// extremes (see *End bar* in the structure contract).
    pub fn holds_end(&self, until: NaiveDateTime, next_end: Option<f64>, raws: &[RawBar]) -> bool {
        let side = self.direction.opposite();
        let from = raws.partition_point(|r| r.dt <= self.start.at);
        let breaks = |r: &RawBar| beyond(side, price(r, side), self.start.price);
        let mut path = raws[from..].iter().take_while(|r| r.dt <= until);
        !path.any(|r| {
            breaks(r) && (r.dt < until || next_end.is_some_and(|e| beyond(side, price(r, side), e)))
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

/// The raw bar's price on `side`: its high for up, its low for down.
pub fn price(raw: &RawBar, side: Direction) -> f64 {
    match side {
        Direction::Up => raw.high,
        Direction::Down => raw.low,
    }
}

/// The most extreme price on `side` among `raws`: the raw bar at `prefer`
/// when it holds that price, otherwise the earliest that does. `None` when
/// `raws` is empty.
pub fn extreme(
    raws: &[RawBar],
    side: Direction,
    prefer: Option<NaiveDateTime>,
) -> Option<Endpoint> {
    let mut best: Option<Endpoint> = None;
    for raw in raws {
        let p = price(raw, side);
        let wins = best
            .is_none_or(|b| beyond(side, p, b.price) || (p == b.price && Some(raw.dt) == prefer));
        if wins {
            best = Some(Endpoint {
                at: raw.dt,
                price: p,
            });
        }
    }
    best
}

/// Find the first stroke in `bars`. Returns the stroke, its endpoints still at
/// its fractals until [`Stroke::place`], and the index where the remaining
/// unfinished bars start (0 when no stroke is formed).
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
        start: Endpoint::of(start_fractal),
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
