//! Stroke centres (笔中枢) and the canonical centre sequence.
use crate::stroke::{Direction, Stroke};
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

/// Geometry of consecutive member strokes. `zg`/`zd` come from the first
/// three members only; `gg`/`dd` from all members.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Centre {
    /// Member range `start..end` in the stroke slice the sequence was built from.
    pub start: usize,
    pub end: usize,
    pub sdt: NaiveDateTime,
    pub edt: NaiveDateTime,
    pub zg: f64,
    pub zd: f64,
    pub zz: f64,
    pub gg: f64,
    pub dd: f64,
}

impl Centre {
    /// Geometry of `bis[start..end]` (non-empty).
    pub fn new(bis: &[Stroke], start: usize, end: usize) -> Self {
        let members = &bis[start..end];
        let first = &members[0];
        let last = &members[members.len() - 1];
        let zg = members
            .iter()
            .take(3)
            .map(Stroke::high)
            .fold(f64::INFINITY, f64::min);
        let zd = members
            .iter()
            .take(3)
            .map(Stroke::low)
            .fold(f64::NEG_INFINITY, f64::max);
        Self {
            start,
            end,
            sdt: first.sdt(),
            edt: last.edt(),
            zg,
            zd,
            zz: zd + (zg - zd) * 0.5,
            gg: members
                .iter()
                .map(Stroke::high)
                .fold(f64::NEG_INFINITY, f64::max),
            dd: members
                .iter()
                .map(Stroke::low)
                .fold(f64::INFINITY, f64::min),
        }
    }

    pub fn bi_count(&self) -> usize {
        self.end - self.start
    }

    /// Every member touches `[zd, zg]` and `zg >= zd`. Does not require three
    /// members; callers that need a qualified centre also check `bi_count() >= 3`.
    pub fn is_valid(&self, bis: &[Stroke]) -> bool {
        let (zd, zg) = (self.zd, self.zg);
        zg >= zd
            && bis[self.start..self.end].iter().all(|bi| {
                let (h, l) = (bi.high(), bi.low());
                (h <= zg && h >= zd) || (l <= zg && l >= zd) || (h >= zg && l <= zd)
            })
    }
}

/// Partition consecutive strokes into non-overlapping centre cores.
///
/// The first stroke is entry context only; a core starts at index 1 at the
/// earliest. Without an active core, each attempt takes three consecutive
/// strokes and advances by two when they do not overlap. An active core keeps
/// its first-three bounds and extends while strokes return into `[zd, zg]`.
/// The first strictly non-returning retest closes it and starts the next
/// attempt; members already in the old core are not handed to the new one.
/// The tail group may hold fewer than three strokes. No recursion to higher
/// levels.
pub fn zs_seq(bis: &[Stroke]) -> Vec<Centre> {
    let mut out = Vec::new();
    let mut pending = 1;
    let mut active: Option<Centre> = None;
    for (index, bi) in bis.iter().enumerate().skip(1) {
        let Some(core) = active.take() else {
            if index + 1 - pending < 3 {
                continue;
            }
            let candidate = Centre::new(bis, pending, index + 1);
            if candidate.is_valid(bis) {
                active = Some(candidate);
            } else {
                pending += 2;
            }
            continue;
        };
        if (bi.direction == Direction::Up && bi.high() < core.zd)
            || (bi.direction == Direction::Down && bi.low() > core.zg)
        {
            out.push(core);
            pending = index;
        } else {
            active = Some(Centre::new(bis, core.start, index + 1));
        }
    }
    if let Some(core) = active {
        out.push(core);
    } else if pending < bis.len() {
        out.push(Centre::new(bis, pending, bis.len()));
    }
    out
}
