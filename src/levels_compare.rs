//! Same-direction comparisons of a level's segments
//! (`docs/structure-contract.md#comparisons`).
use crate::levels::{Level, Pos, Seg, State};
use crate::power::interval_power;
use crate::stroke::Direction;
use chrono::NaiveDateTime;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Between {
    pub min_abs_dif: Option<f64>,
    pub dif_crossed_zero: Option<bool>,
    pub dea_crossed_zero: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Comparison {
    pub pair: &'static str,
    pub numerator: Pos,
    pub denominator: Pos,
    pub area: Option<f64>,
    pub price: Option<f64>,
    pub mean_volume: Option<f64>,
    pub bar_count: Option<f64>,
    pub between: Between,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Comparisons {
    pub completed: Vec<Comparison>,
    pub in_progress: Vec<Comparison>,
}

fn ratio(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (Some(a), Some(b)) if a >= 0.0 && b > 0.0 => Some(a / b),
        _ => None,
    }
}

/// A numerator that is still moving: anything but a window stroke.
fn moving(s: &Seg) -> bool {
    !matches!(s.pos, Pos::Stroke(_))
}

impl Level {
    /// Same-direction area, price, mean volume and bar count of a segment.
    fn metrics(&self, s: &Seg) -> [Option<f64>; 4] {
        let power = match s.pos {
            Pos::Stroke(k) => self.strokes[k].power.clone(),
            _ => interval_power(&self.analyzer, &self.macd, s.start_at, s.end_at),
        };
        let area = if s.direction == Direction::Up {
            power.macd.positive_area
        } else {
            power.macd.negative_area
        };
        [
            area,
            Some((s.end_price - s.start_price).abs()),
            power.mean_volume,
            Some(power.sample_count as f64),
        ]
    }

    /// One comparison row of `num` against `den`, same direction.
    pub fn compare(&self, pair: &'static str, num: &Seg, den: &Seg) -> Comparison {
        assert_eq!(num.direction, den.direction, "{pair}: cross-direction pair");
        let (n, d) = (self.metrics(num), self.metrics(den));
        let (early, late) = if den.end_at <= num.start_at {
            (den, num)
        } else {
            (num, den)
        };
        Comparison {
            pair,
            numerator: num.pos,
            denominator: den.pos,
            area: ratio(n[0], d[0]),
            price: ratio(n[1], d[1]),
            mean_volume: ratio(n[2], d[2]),
            bar_count: ratio(n[3], d[3]),
            between: self.between(early.end_at, late.start_at),
        }
    }

    /// Retained raw bars from `from` to `to`, both included.
    fn between(&self, from: NaiveDateTime, to: NaiveDateTime) -> Between {
        let raw = self.raw();
        let a = raw.partition_point(|b| b.dt < from);
        let b = raw.partition_point(|b| b.dt <= to).max(a);
        let dif = &self.macd.dif[a..b];
        let dea = &self.macd.dea[a..b];
        let ready = |v: &[f64]| !v.is_empty() && v.iter().all(|x| x.is_finite());
        let crossed = |v: &[f64]| v.iter().any(|x| *x > 0.0) && v.iter().any(|x| *x < 0.0);
        Between {
            min_abs_dif: ready(dif)
                .then(|| dif.iter().map(|x| x.abs()).fold(f64::INFINITY, f64::min)),
            dif_crossed_zero: ready(dif).then(|| crossed(dif)),
            dea_crossed_zero: ready(dea).then(|| crossed(dea)),
        }
    }

    /// The latest window stroke before position `before` with direction `d`.
    fn previous_same(&self, before: usize, d: Direction) -> Option<Seg> {
        (0..before)
            .rev()
            .find(|&k| self.strokes[k].direction == d)
            .map(|k| self.seg(k))
    }

    /// A departed centre's departure and retest segments.
    fn departure_of(&self, c: usize) -> Option<(Seg, Option<Seg>)> {
        let State::Departed {
            departure, retest, ..
        } = &self.centres[c].state
        else {
            return None;
        };
        let get = |p: &Pos| match p {
            Pos::Stroke(k) => Some(self.seg(*k)),
            Pos::Unfinished => self.reverse_seg(),
            _ => None,
        };
        Some((get(departure)?, retest.as_ref().and_then(get)))
    }

    pub(crate) fn comparisons_of(&self) -> Comparisons {
        let mut out = Comparisons::default();
        let mut push = |row: Comparison, num: &Seg| {
            if moving(num) {
                out.in_progress.push(row);
            } else {
                out.completed.push(row);
            }
        };
        let n = self.strokes.len();
        let last = self.seg(n - 1);
        if let Some(prev) = self.previous_same(n - 1, last.direction) {
            push(self.compare("last_vs_previous", &last, &prev), &last);
        }
        let count = self.centres.len();
        if count >= 1
            && let Some((dep, retest)) = self.departure_of(count - 1)
        {
            let entry = self.seg(self.centres[count - 1].entry);
            if entry.direction == dep.direction {
                push(self.compare("departure_vs_entry", &dep, &entry), &dep);
            }
            if let Some(Seg {
                pos: Pos::Stroke(k),
                ..
            }) = retest
            {
                let advance = (k + 1..n)
                    .map(|p| self.seg(p))
                    .chain(self.reverse_seg())
                    .find(|s| s.direction == dep.direction);
                if let Some(a) = advance {
                    push(self.compare("advance_vs_departure", &a, &dep), &a);
                }
            }
        }
        if count >= 2
            && let Some((dep, _)) = self.departure_of(count - 2)
        {
            let entry = self.seg(self.centres[count - 2].entry);
            if entry.direction == dep.direction {
                push(
                    self.compare("previous_departure_vs_entry", &dep, &entry),
                    &dep,
                );
            }
        }
        if let Some(u) = self.unfinished_seg() {
            if u.pos == Pos::Unfinished {
                if let Some(prev) = self.previous_same(n, u.direction) {
                    push(self.compare("unfinished_vs_previous", &u, &prev), &u);
                }
            } else if u.direction == last.direction {
                push(self.compare("extension_vs_last", &u, &last), &u);
            }
        }
        if let Some(p) = self.after_seg()
            && let Some(prev) = self.previous_same(n, p.direction)
        {
            push(self.compare("after_extreme_vs_previous", &p, &prev), &p);
        }
        out
    }
}
