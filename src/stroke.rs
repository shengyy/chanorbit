//! Strokes (笔) and the single-stroke search over unfinished NewBars.
use crate::bar::NewBar;
use crate::fractal::{Fractal, Mark, check_fxs};
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

/// A stroke from fractal `fx_a` to the opposite fractal `fx_b`.
#[derive(Debug, Clone, PartialEq)]
pub struct Stroke {
    pub fx_a: Fractal,
    pub fx_b: Fractal,
    /// Fractals whose time lies within the stroke's NewBar span.
    pub fxs: Vec<Fractal>,
    pub direction: Direction,
    /// Snapshot of the NewBars from `fx_a`'s first to `fx_b`'s last bar.
    pub bars: Vec<Arc<NewBar>>,
}

impl Stroke {
    pub fn sdt(&self) -> NaiveDateTime {
        self.fx_a.dt
    }

    pub fn edt(&self) -> NaiveDateTime {
        self.fx_b.dt
    }

    /// Endpoint high; the path inside the stroke may exceed it.
    pub fn high(&self) -> f64 {
        self.fx_a.high.max(self.fx_b.high)
    }

    /// Endpoint low; the path inside the stroke may exceed it.
    pub fn low(&self) -> f64 {
        self.fx_a.low.min(self.fx_b.low)
    }

    /// `|fx_b - fx_a|` rounded to two decimals.
    pub fn power_price(&self) -> f64 {
        ((self.fx_b.fx - self.fx_a.fx).abs() * 100.0).round() / 100.0
    }

    pub fn length(&self) -> usize {
        self.bars.len()
    }
}

/// Find the first stroke in `bars`. Returns the stroke and the index where the
/// remaining unfinished bars start (0 when no stroke is formed).
pub fn check_bi(bars: &[Arc<NewBar>], min_bi_len: usize) -> (Option<Stroke>, usize) {
    let fxs = check_fxs(bars);
    if fxs.len() < 2 {
        return (None, 0);
    }
    let fx_a = &fxs[0];
    // The first of equal extremes wins.
    let mut fx_b: Option<&Fractal> = None;
    let direction = match fx_a.mark {
        Mark::D => {
            for x in fxs
                .iter()
                .filter(|x| x.mark == Mark::G && x.dt > fx_a.dt && x.fx > fx_a.fx)
            {
                if fx_b.is_none_or(|best| x.high > best.high) {
                    fx_b = Some(x);
                }
            }
            Direction::Up
        }
        Mark::G => {
            for x in fxs
                .iter()
                .filter(|x| x.mark == Mark::D && x.dt > fx_a.dt && x.fx < fx_a.fx)
            {
                if fx_b.is_none_or(|best| x.low < best.low) {
                    fx_b = Some(x);
                }
            }
            Direction::Down
        }
    };
    let Some(fx_b) = fx_b else {
        return (None, 0);
    };
    let start_dt = fx_a.elements[0].dt;
    let end_dt = fx_b.elements[2].dt;
    let start = bars.partition_point(|b| b.dt < start_dt);
    let end = bars.partition_point(|b| b.dt <= end_dt);
    if start >= end {
        return (None, 0);
    }
    let rest = bars.partition_point(|b| b.dt < fx_b.elements[0].dt);
    let ab_include = (fx_a.high > fx_b.high && fx_a.low < fx_b.low)
        || (fx_a.high < fx_b.high && fx_a.low > fx_b.low);
    if ab_include || end - start < min_bi_len {
        return (None, 0);
    }
    let stroke = Stroke {
        fx_a: fx_a.clone(),
        fx_b: fx_b.clone(),
        fxs: fxs
            .iter()
            .filter(|x| x.dt >= start_dt && x.dt <= end_dt)
            .cloned()
            .collect(),
        direction,
        bars: bars[start..end].to_vec(),
    };
    (Some(stroke), rest)
}
