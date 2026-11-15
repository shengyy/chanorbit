//! Top and bottom fractals over three consecutive NewBars.
use crate::bar::NewBar;
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Mark {
    /// Bottom fractal.
    #[serde(rename = "底分型")]
    Bottom,
    /// Top fractal.
    #[serde(rename = "顶分型")]
    Top,
}

/// A fractal: the middle NewBar is the extreme of three.
#[derive(Debug, Clone, PartialEq)]
pub struct Fractal {
    pub dt: NaiveDateTime,
    pub mark: Mark,
    pub high: f64,
    pub low: f64,
    /// The extreme price: `high` for a top, `low` for a bottom.
    pub price: f64,
    /// Snapshot of the three NewBars at detection time.
    pub elements: [Arc<NewBar>; 3],
}

impl Fractal {
    /// Time of the first raw bar in the third NewBar: when the fractal became visible.
    pub fn confirmed_at(&self) -> Option<NaiveDateTime> {
        self.elements[2].elements.first().map(|b| b.dt)
    }
}

pub fn fractal_at(k1: &Arc<NewBar>, k2: &Arc<NewBar>, k3: &Arc<NewBar>) -> Option<Fractal> {
    let mark = if k1.high < k2.high && k2.high > k3.high && k1.low < k2.low && k2.low > k3.low {
        Mark::Top
    } else if k1.low > k2.low && k2.low < k3.low && k1.high > k2.high && k2.high < k3.high {
        Mark::Bottom
    } else {
        return None;
    };
    Some(Fractal {
        dt: k2.dt,
        mark,
        high: k2.high,
        low: k2.low,
        price: if mark == Mark::Top { k2.high } else { k2.low },
        elements: [k1.clone(), k2.clone(), k3.clone()],
    })
}

/// All fractals in order; a fractal with the same mark as the previous one is dropped.
pub fn scan_fractals(bars: &[Arc<NewBar>]) -> Vec<Fractal> {
    let mut fractals: Vec<Fractal> = Vec::new();
    for w in bars.windows(3) {
        if let Some(fractal) = fractal_at(&w[0], &w[1], &w[2])
            && fractals.last().is_none_or(|last| last.mark != fractal.mark)
        {
            fractals.push(fractal);
        }
    }
    fractals
}
