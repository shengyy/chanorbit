//! Strokes (笔) and the single-stroke search over unfinished NewBars.
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

/// A stroke from fractal `start_fractal` to the opposite fractal `end_fractal`.
#[derive(Debug, Clone, PartialEq)]
pub struct Stroke {
    pub start_fractal: Fractal,
    pub end_fractal: Fractal,
    /// Fractals whose time lies within the stroke's NewBar span.
    pub fractals: Vec<Fractal>,
    pub direction: Direction,
    /// Snapshot of the NewBars from `start_fractal`'s first to `end_fractal`'s last bar.
    pub newbars: Vec<Arc<NewBar>>,
}

impl Stroke {
    pub fn start_at(&self) -> NaiveDateTime {
        self.start_fractal.dt
    }

    pub fn end_at(&self) -> NaiveDateTime {
        self.end_fractal.dt
    }

    /// Endpoint high; the path inside the stroke may exceed it.
    pub fn high(&self) -> f64 {
        self.start_fractal.high.max(self.end_fractal.high)
    }

    /// Endpoint low; the path inside the stroke may exceed it.
    pub fn low(&self) -> f64 {
        self.start_fractal.low.min(self.end_fractal.low)
    }

    /// `|end_fractal - start_fractal|` rounded to two decimals.
    pub fn amplitude(&self) -> f64 {
        ((self.end_fractal.price - self.start_fractal.price).abs() * 100.0).round() / 100.0
    }

    pub fn newbar_count(&self) -> usize {
        self.newbars.len()
    }
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
