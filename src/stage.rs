//! Buy-side stages over the current strokes: second (二买), third (三买) and
//! second-like (类二买), each confirmed by a downward retest stroke that is the
//! last stroke. Definitions are owned by `docs/structure-contract.md#stages`.
//! Only confirmed stages are produced; nothing here is a trading conclusion.
use crate::analyzer::Analyzer;
use crate::centre::{Centre, canonical_centres};
use crate::macd::{MacdSeries, retained_macd};
use crate::power::stroke_power;
use crate::stroke::{Direction, Stroke};
use serde::{Deserialize, Serialize};
use std::cell::OnceCell;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StageKind {
    Second,
    Third,
    SecondLike,
}

/// A confirmed stage. Indices point into [`Analyzer::strokes`]; `centre`
/// member indices are in the same list.
#[derive(Debug, Clone, PartialEq)]
pub struct Stage {
    pub kind: StageKind,
    pub centre: Centre,
    pub entry: usize,
    pub departure: usize,
    pub rebound: Option<usize>,
    pub retest: usize,
    pub first_point_anchor: Option<usize>,
    /// The parent second stage's rebound and retest (second-like only).
    pub parent: Option<(usize, usize)>,
}

/// The first point a second stage rests on: a downward departure from the
/// latest qualified centre to a new low, on weaker MACD than its entry.
struct FirstPoint {
    centre: Centre,
    entry: usize,
}

struct Evaluator<'a> {
    c: &'a Analyzer,
    macd: OnceCell<MacdSeries>,
}

/// The last centre of the canonical sequence over `strokes` with at least
/// three members that all touch its core.
fn latest_qualified(strokes: &[Stroke]) -> Option<Centre> {
    canonical_centres(strokes)
        .into_iter()
        .rfind(|z| z.member_count() >= 3 && z.members_touch_core(strokes))
}

impl Evaluator<'_> {
    fn strokes(&self) -> &[Stroke] {
        self.c.strokes()
    }

    /// Negative MACD area of a stroke; `None` when unavailable.
    fn negative_area(&self, index: usize) -> Option<f64> {
        let macd = self.macd.get_or_init(|| retained_macd(self.c));
        stroke_power(self.c, macd, &self.strokes()[index])
            .macd
            .negative_area
    }

    /// First point whose departure is stroke `end - 1`, judged on the strokes
    /// before `end`. Only used as a historical anchor, so no path guard.
    fn first(&self, end: usize) -> Option<FirstPoint> {
        let s = self.strokes();
        let last = &s[end - 1];
        if last.direction != Direction::Down {
            return None;
        }
        let z = latest_qualified(&s[..end - 1])?;
        let entry = z.start - 1;
        let e = &s[entry];
        if z.end != end - 1 || e.direction != Direction::Down {
            return None;
        }
        let new_low = e.high() > z.core_high
            && e.low() <= z.core_high
            && last.high() >= z.core_low
            && last.low() < z.low.min(e.low());
        if !new_low {
            return None;
        }
        let entry_area = self.negative_area(entry)?;
        let departure_area = self.negative_area(end - 1)?;
        let ratio = departure_area / entry_area;
        // A zero entry area or an unready interval cannot be compared.
        (entry_area != 0.0 && ratio.is_finite() && ratio < 1.0)
            .then_some(FirstPoint { centre: z, entry })
    }

    /// Every retained raw bar after stroke `begin` ends, up to the cutoff,
    /// stays strictly above `boundary`. The cutoff is the last input when
    /// `end` is the whole list, otherwise the end of stroke `end - 1`. Raw bars
    /// the strokes or unfinished NewBars hold in that range must be retained.
    fn path_above(&self, end: usize, begin: usize, boundary: f64) -> bool {
        let (s, raw) = (self.strokes(), self.c.raw_bars());
        let start = s[begin].end_at();
        let Some(cutoff) = (if end == s.len() {
            raw.last().map(|b| b.dt)
        } else {
            Some(s[end - 1].end_at())
        }) else {
            return false;
        };
        let held = |dt| raw.binary_search_by_key(&dt, |b| b.dt).is_ok();
        if !held(start) || !held(cutoff) || cutoff < start {
            return false;
        }
        let covered = s[begin..end]
            .iter()
            .flat_map(|stroke| &stroke.newbars)
            .chain(self.c.pending_bars())
            .flat_map(|b| &b.elements)
            .filter(|b| b.dt > start && b.dt <= cutoff)
            .all(|b| held(b.dt));
        covered
            && raw
                .iter()
                .filter(|b| b.dt > start && b.dt <= cutoff)
                .all(|b| b.low > boundary)
    }

    /// Second stage whose retest is stroke `end - 1`.
    fn second(&self, end: usize) -> Option<Stage> {
        if end < 3 {
            return None;
        }
        let first = self.first(end - 2)?;
        let (anchor, rebound, retest) = (end - 3, end - 2, end - 1);
        let s = self.strokes();
        let guard = s[anchor].low();
        if s[retest].direction != Direction::Down || s[retest].low() <= guard {
            return None;
        }
        self.path_above(end, anchor, guard).then_some(Stage {
            kind: StageKind::Second,
            centre: first.centre,
            entry: first.entry,
            departure: anchor,
            rebound: Some(rebound),
            retest,
            first_point_anchor: Some(anchor),
            parent: None,
        })
    }

    fn third(&self) -> Option<Stage> {
        let s = self.strokes();
        let end = s.len();
        let z = latest_qualified(&s[..self.c.finished_count()])?;
        let leave = end.checked_sub(2)?;
        if z.end != leave + 1 || z.member_count() < 4 {
            return None;
        }
        let (l, last) = (&s[leave], &s[end - 1]);
        let departs =
            l.direction == Direction::Up && l.low() <= z.core_high && l.high() > z.core_high;
        if !departs || last.direction != Direction::Down || last.low() <= z.core_high {
            return None;
        }
        self.path_above(end, leave, z.core_high).then_some(Stage {
            kind: StageKind::Third,
            centre: z,
            entry: z.start - 1,
            departure: leave,
            rebound: None,
            retest: end - 1,
            first_point_anchor: None,
            parent: None,
        })
    }

    fn second_like(&self) -> Option<Stage> {
        let s = self.strokes();
        let end = s.len();
        let z = latest_qualified(&s[..self.c.finished_count()])?;
        if z.start + 5 != end || !(4..=5).contains(&z.member_count()) {
            return None;
        }
        let parent = self.second(z.start + 1)?;
        let anchor = parent.first_point_anchor?;
        let (rebound, retest) = (end - 2, end - 1);
        if s[retest].direction != Direction::Down || s[retest].low() <= z.core_low {
            return None;
        }
        let guarded = self.path_above(end, rebound, z.core_low)
            && self.path_above(end, anchor, s[anchor].low());
        guarded.then_some(Stage {
            kind: StageKind::SecondLike,
            centre: z,
            entry: z.start - 1,
            departure: rebound,
            rebound: Some(rebound),
            retest,
            first_point_anchor: Some(anchor),
            parent: Some((parent.rebound?, parent.retest)),
        })
    }
}

/// Confirmed buy-side stages of the analyzer's current state, in the order
/// second, third, second-like.
pub fn stages(c: &Analyzer) -> Vec<Stage> {
    let e = Evaluator {
        c,
        macd: OnceCell::new(),
    };
    [e.second(c.strokes().len()), e.third(), e.second_like()]
        .into_iter()
        .flatten()
        .collect()
}
