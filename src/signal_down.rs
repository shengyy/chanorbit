//! The down-end nest of the `signals` command (`down_end`): the parent's
//! down move leaves its latest centre to a new low on a smaller area than the
//! centre's entry, and that low completes on the finer levels. Definitions
//! are owned by `docs/structure-contract.md#down-end-nest-down_end`; nothing
//! here is a trading conclusion.
use crate::centre::Centre;
use crate::levels::{Extreme, Pos, Seg, State};
use crate::levels_state::centre_state;
use crate::signal::{
    CentreFact, CompletionFact, LevelRead, MoveFact, Stage, covered_child, nest_below,
};
use crate::stroke::{Direction, extreme};
use chrono::NaiveDateTime;
use serde::Serialize;

const KEY_TIME: &str = "%Y-%m-%dT%H:%M:%S";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DownDeparture {
    pub start_at: NaiveDateTime,
    pub end_at: NaiveDateTime,
    pub low: f64,
    pub extending: bool,
    pub area: f64,
    /// `area` over the entry stroke's down area.
    pub area_ratio: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Low {
    pub price: f64,
    pub at: NaiveDateTime,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DownEnd {
    pub stage: Stage,
    pub key: String,
    pub centre: CentreFact,
    /// Qualified centres in a row, each core below the one before, ending at
    /// the centre: 1 a consolidation, 2 or more a trend.
    pub trend_centres: usize,
    pub departure: DownDeparture,
    pub low: Low,
    pub mid: Option<MoveFact>,
    pub leaf: Option<CompletionFact>,
    pub leaf_centre: Option<bool>,
}

/// A1–A3 on the parent level, which change only with its bars.
#[derive(Debug, Clone, PartialEq)]
pub struct ParentDown {
    centre: Centre,
    trend_centres: usize,
    departure: Seg,
    low: Extreme,
    extending: bool,
    area: f64,
    area_ratio: f64,
}

/// The qualified centres in a row ending at the last, each core strictly
/// below the one before.
fn trend_centres(qualified: &[Centre]) -> usize {
    1 + qualified
        .windows(2)
        .rev()
        .take_while(|w| w[0].core_low > w[1].core_high)
        .count()
}

/// A1–A3: the parent's latest qualified centre is `departed` through `zd`
/// by an element running down to a new low (A1), on a smaller down area
/// than the centre's down entry stroke (A2), and no counted up stroke
/// follows it (A3).
pub fn parent_down(p: &LevelRead) -> Option<ParentDown> {
    let qualified = p.qualified_centres();
    let z = *qualified.last()?;
    let w0 = z.start - 1;
    let segs = p.sequence(w0);
    // A1
    let State::Departed {
        departure,
        direction: Direction::Down,
        edge: "zd",
        new_extreme: Some(true),
        raw_extreme: Some(low),
        ..
    } = centre_state(p.raw(), &segs, w0, &z, None, 0)
    else {
        return None;
    };
    let (k, d) = match departure {
        Pos::Stroke(i) => (i + w0, p.seg(i + w0)),
        Pos::Unfinished => (p.n(), p.unfinished?),
        Pos::AfterExtreme => return None,
    };
    // A2
    let entry = p.seg(w0);
    if entry.direction != Direction::Down {
        return None;
    }
    let (area, entry_area) = (p.area(&d)?, p.area(&entry)?);
    if area.partial_cmp(&entry_area) != Some(std::cmp::Ordering::Less) {
        return None;
    }
    // A3
    if (k + 1..p.finished).any(|j| p.seg(j).direction == Direction::Up) {
        return None;
    }
    Some(ParentDown {
        centre: z,
        trend_centres: trend_centres(&qualified),
        departure: d,
        low,
        extending: p.moving(k),
        area,
        area_ratio: area / entry_area,
    })
}

/// The down-end nest over the parent `p`, the middle level `m` and the
/// optional leaf `l`, given A1–A3 of `p`.
pub fn down_end(
    p: &LevelRead,
    m: &LevelRead,
    l: Option<&LevelRead>,
    parent: Option<&ParentDown>,
) -> Option<DownEnd> {
    let pd = parent?;
    let d = pd.departure;
    let z = &pd.centre;
    let finest = l.unwrap_or(m).raw();
    // A4: the finest low after `D`'s start (below `D`'s own low when a parent
    // bar holding it has not closed), and no finest bar after it has reached
    // back to `zd`.
    let t_start = covered_child(p.raw(), finest, d.start_at, true)?;
    let low = extreme(
        &finest[finest.partition_point(|b| b.dt <= t_start)..],
        Direction::Down,
    )?;
    if finest[finest.partition_point(|b| b.dt <= low.at)..]
        .iter()
        .any(|b| b.high >= z.core_low)
    {
        return None;
    }
    let (stage, mid, leaf, leaf_centre) = nest_below(p, m, l, d.start_at, low, true);
    Some(DownEnd {
        stage,
        key: format!(
            "{}|{}|{}",
            z.start_at.format(KEY_TIME),
            d.start_at.format(KEY_TIME),
            low.at.format(KEY_TIME)
        ),
        centre: CentreFact::from(z),
        trend_centres: pd.trend_centres,
        departure: DownDeparture {
            start_at: d.start_at,
            end_at: d.end_at,
            low: pd.low.price,
            extending: pd.extending,
            area: pd.area,
            area_ratio: pd.area_ratio,
        },
        low: Low {
            price: low.price,
            at: low.at,
        },
        mid,
        leaf,
        leaf_centre,
    })
}
