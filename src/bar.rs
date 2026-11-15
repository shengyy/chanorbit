//! Input bars, inclusion-processed bars and the inclusion merge.
use chrono::NaiveDateTime;

/// One input bar. `dt` is the exchange-local wall clock, without a zone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RawBar {
    pub dt: NaiveDateTime,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub vol: f64,
}

/// An inclusion-processed bar and the raw bars merged into it.
#[derive(Debug, Clone, PartialEq)]
pub struct NewBar {
    pub dt: NaiveDateTime,
    pub high: f64,
    pub low: f64,
    pub elements: Vec<RawBar>,
}

/// Raw bars kept from the previous NewBar when merging.
const MAX_CARRIED_ELEMENTS: usize = 100;

impl NewBar {
    pub fn from_raw(bar: &RawBar) -> Self {
        Self {
            dt: bar.dt,
            high: bar.high,
            low: bar.low,
            elements: vec![*bar],
        }
    }
}

/// Merge `k3` into `k2` when they are in an inclusion relation; the direction
/// comes from `k1`→`k2` highs. Returns `(merged, bar)`: `merged` means `bar`
/// replaces `k2`, otherwise `bar` is a fresh NewBar for `k3`.
pub fn merge_inclusion(k1: &NewBar, k2: &NewBar, k3: &RawBar) -> (bool, NewBar) {
    let up = if k1.high < k2.high {
        true
    } else if k1.high > k2.high {
        false
    } else {
        return (false, NewBar::from_raw(k3));
    };
    let included =
        (k2.high <= k3.high && k2.low >= k3.low) || (k2.high >= k3.high && k2.low <= k3.low);
    if !included {
        return (false, NewBar::from_raw(k3));
    }
    let (high, low, dt) = if up {
        let dt = if k2.high > k3.high { k2.dt } else { k3.dt };
        (k2.high.max(k3.high), k2.low.max(k3.low), dt)
    } else {
        let dt = if k2.low < k3.low { k2.dt } else { k3.dt };
        (k2.high.min(k3.high), k2.low.min(k3.low), dt)
    };
    let elements = k2
        .elements
        .iter()
        .take(MAX_CARRIED_ELEMENTS)
        .filter(|x| x.dt != k3.dt)
        .copied()
        .chain(std::iter::once(*k3))
        .collect();
    (
        true,
        NewBar {
            dt,
            high,
            low,
            elements,
        },
    )
}
