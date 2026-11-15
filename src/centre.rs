//! Stroke centres (笔中枢) and the canonical centre sequence.
use crate::stroke::{Direction, Stroke};
use chrono::NaiveDateTime;

/// Geometry of consecutive member strokes. `core_high`/`core_low` come from the first
/// three members only; `high`/`low` from all members.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Centre {
    /// Member range `start..end` in the stroke slice the sequence was built from.
    pub start: usize,
    pub end: usize,
    pub start_at: NaiveDateTime,
    pub end_at: NaiveDateTime,
    pub core_high: f64,
    pub core_low: f64,
    pub core_mid: f64,
    pub high: f64,
    pub low: f64,
}

impl Centre {
    /// Geometry of `strokes[start..end]` (non-empty).
    pub fn new(strokes: &[Stroke], start: usize, end: usize) -> Self {
        let members = &strokes[start..end];
        let first = &members[0];
        let last = &members[members.len() - 1];
        let core_high = members
            .iter()
            .take(3)
            .map(Stroke::high)
            .fold(f64::INFINITY, f64::min);
        let core_low = members
            .iter()
            .take(3)
            .map(Stroke::low)
            .fold(f64::NEG_INFINITY, f64::max);
        Self {
            start,
            end,
            start_at: first.start_at(),
            end_at: last.end_at(),
            core_high,
            core_low,
            core_mid: core_low + (core_high - core_low) * 0.5,
            high: members
                .iter()
                .map(Stroke::high)
                .fold(f64::NEG_INFINITY, f64::max),
            low: members
                .iter()
                .map(Stroke::low)
                .fold(f64::INFINITY, f64::min),
        }
    }

    pub fn member_count(&self) -> usize {
        self.end - self.start
    }

    /// Every member touches `[core_low, core_high]` and `core_high >= core_low`. Does not require three
    /// members; callers that need a qualified centre also check `member_count() >= 3`.
    pub fn members_touch_core(&self, strokes: &[Stroke]) -> bool {
        let (core_low, core_high) = (self.core_low, self.core_high);
        core_high >= core_low
            && strokes[self.start..self.end].iter().all(|stroke| {
                let (h, l) = (stroke.high(), stroke.low());
                (h <= core_high && h >= core_low)
                    || (l <= core_high && l >= core_low)
                    || (h >= core_high && l <= core_low)
            })
    }
}

/// Partition consecutive strokes into non-overlapping centre cores.
///
/// The first stroke is entry context only; a core starts at index 1 at the
/// earliest. Without an active core, each attempt takes three consecutive
/// strokes and advances by two when they do not overlap. An active core keeps
/// its first-three bounds and extends while strokes return into `[core_low, core_high]`.
/// The first strictly non-returning retest closes it and starts the next
/// attempt; members already in the old core are not handed to the new one.
/// The tail group may hold fewer than three strokes. No recursion to higher
/// levels.
pub fn canonical_centres(strokes: &[Stroke]) -> Vec<Centre> {
    let mut out = Vec::new();
    let mut pending = 1;
    let mut active: Option<Centre> = None;
    for (index, stroke) in strokes.iter().enumerate().skip(1) {
        let Some(core) = active.take() else {
            if index + 1 - pending < 3 {
                continue;
            }
            let candidate = Centre::new(strokes, pending, index + 1);
            if candidate.members_touch_core(strokes) {
                active = Some(candidate);
            } else {
                pending += 2;
            }
            continue;
        };
        if (stroke.direction == Direction::Up && stroke.high() < core.core_low)
            || (stroke.direction == Direction::Down && stroke.low() > core.core_high)
        {
            out.push(core);
            pending = index;
        } else {
            active = Some(Centre::new(strokes, core.start, index + 1));
        }
    }
    if let Some(core) = active {
        out.push(core);
    } else if pending < strokes.len() {
        out.push(Centre::new(strokes, pending, strokes.len()));
    }
    out
}
