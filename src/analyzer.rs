//! Incremental analyzer: inclusion, stroke update/withdrawal, retention and
//! same-dt tail revision. Batch construction is the same per-bar loop.
use crate::bar::{NewBar, RawBar, merge_inclusion};
use crate::fractal::{Fractal, scan_fractals};
use crate::stroke::{Direction, Stroke, find_stroke};
use std::sync::Arc;

/// Stroke parameters. Defaults: 50 strokes, 6 NewBars.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    /// Maximum retained strokes; older strokes and their raw bars are dropped.
    pub max_strokes: usize,
    /// Minimum NewBar count of a stroke.
    pub min_stroke_bars: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            max_strokes: 50,
            min_stroke_bars: 6,
        }
    }
}

/// Undo record for exactly the latest input, including retention pruning.
#[derive(Debug, Clone, Default)]
struct TailUpdate {
    pending_bars: Vec<Arc<NewBar>>,
    stroke_count: usize,
    withdrawn: Option<Stroke>,
    pruned_strokes: Vec<Stroke>,
    pruned_raw: Vec<RawBar>,
}

#[derive(Debug, Clone, Default)]
pub struct Analyzer {
    config: Config,
    raw_bars: Vec<RawBar>,
    pending_bars: Vec<Arc<NewBar>>,
    strokes: Vec<Stroke>,
    tail: TailUpdate,
}

impl Analyzer {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            ..Self::default()
        }
    }

    /// Batch construction; identical to calling [`Analyzer::update`] per bar.
    pub fn from_bars<I: IntoIterator<Item = RawBar>>(bars: I, config: Config) -> Self {
        let mut analyzer = Self::new(config);
        for bar in bars {
            analyzer.update(bar);
        }
        analyzer
    }

    /// Retained raw bars (pruned to the first retained stroke's start).
    pub fn raw_bars(&self) -> &[RawBar] {
        &self.raw_bars
    }

    /// NewBars not yet part of a finished stroke search.
    pub fn pending_bars(&self) -> &[Arc<NewBar>] {
        &self.pending_bars
    }

    /// Retained strokes; the last one may still be withdrawn.
    pub fn strokes(&self) -> &[Stroke] {
        &self.strokes
    }

    /// Strokes counted as centre members: the last stroke is excluded while
    /// fewer than five unfinished NewBars follow it. Not a no-repaint claim.
    pub fn finished_count(&self) -> usize {
        if self.pending_bars.len() < 5 {
            self.strokes.len().saturating_sub(1)
        } else {
            self.strokes.len()
        }
    }

    pub fn finished_strokes(&self) -> &[Stroke] {
        &self.strokes[..self.finished_count()]
    }

    /// Add one bar. A bar with the same `dt` as the previous input fully
    /// replaces it: the result equals having input only the new version.
    pub fn update(&mut self, bar: RawBar) {
        if self.raw_bars.last().is_some_and(|last| last.dt == bar.dt) {
            let previous = std::mem::take(&mut self.tail);
            self.pending_bars = previous.pending_bars;
            self.strokes.splice(0..0, previous.pruned_strokes);
            self.strokes.truncate(previous.stroke_count);
            if let Some(stroke) = previous.withdrawn {
                self.strokes.push(stroke);
            }
            self.raw_bars.pop();
            self.raw_bars.splice(0..0, previous.pruned_raw);
        }
        self.tail = TailUpdate::default();
        let mut tail = TailUpdate {
            pending_bars: self.pending_bars.clone(),
            stroke_count: self.strokes.len(),
            ..TailUpdate::default()
        };
        self.raw_bars.push(bar);

        if self.pending_bars.len() < 2 {
            self.pending_bars.push(Arc::new(NewBar::from_raw(&bar)));
        } else {
            let n = self.pending_bars.len();
            let (merged, k) =
                merge_inclusion(&self.pending_bars[n - 2], &self.pending_bars[n - 1], &bar);
            if merged {
                self.pending_bars[n - 1] = Arc::new(k);
            } else {
                self.pending_bars.push(Arc::new(k));
            }
        }

        let removed = self.update_strokes();
        if self.strokes.len() < tail.stroke_count {
            tail.withdrawn = removed;
        }
        if self.strokes.len() > self.config.max_strokes {
            let excess = self.strokes.len() - self.config.max_strokes;
            tail.pruned_strokes = self.strokes.drain(..excess).collect();
        }
        if let Some(first) = self.strokes.first() {
            let start_at = first.start_fractal.elements[0].dt;
            let keep_from = self.raw_bars.partition_point(|b| b.dt < start_at);
            tail.pruned_raw = self.raw_bars.drain(..keep_from).collect();
        }
        self.tail = tail;
    }

    /// Returns the withdrawn tail stroke, if any.
    fn update_strokes(&mut self) -> Option<Stroke> {
        if self.pending_bars.len() < 3 {
            return None;
        }
        if self.strokes.is_empty() {
            let fractals = scan_fractals(&self.pending_bars);
            let first = fractals.first()?;
            let mut start_fractal = first;
            for x in fractals.iter().filter(|x| x.mark == first.mark) {
                let better = match first.mark {
                    crate::fractal::Mark::Bottom => x.low <= start_fractal.low,
                    crate::fractal::Mark::Top => x.high >= start_fractal.high,
                };
                if better {
                    start_fractal = x;
                }
            }
            let start = start_fractal.elements[0].dt;
            let bars: Vec<_> = self
                .pending_bars
                .iter()
                .filter(|b| b.dt >= start)
                .cloned()
                .collect();
            let (stroke, rest) = find_stroke(&bars, self.config.min_stroke_bars);
            if let Some(stroke) = stroke {
                self.strokes.push(stroke);
            }
            self.pending_bars = bars[rest..].to_vec();
            return None;
        }

        let (stroke, rest) = find_stroke(&self.pending_bars, self.config.min_stroke_bars);
        if let Some(stroke) = stroke {
            self.strokes.push(stroke);
        }
        self.pending_bars.drain(..rest);

        let last_stroke = self.strokes.last().unwrap();
        let broken = self
            .pending_bars
            .last()
            .is_some_and(|b| match last_stroke.direction {
                Direction::Up => b.high > last_stroke.high(),
                Direction::Down => b.low < last_stroke.low(),
            });
        if broken {
            let keep = last_stroke.newbars.len() - 2;
            let merge_point = last_stroke.newbars[keep].dt;
            self.pending_bars = last_stroke.newbars[..keep]
                .iter()
                .chain(self.pending_bars.iter().filter(|b| b.dt >= merge_point))
                .cloned()
                .collect();
            return self.strokes.pop();
        }
        None
    }

    /// Fractals in the unfinished NewBars.
    pub fn pending_fractals(&self) -> Vec<Fractal> {
        scan_fractals(&self.pending_bars)
    }

    /// Stroke-internal fractals (skipping each stroke's first) then unfinished
    /// fractals, keeping only strictly increasing times.
    pub fn fractals(&self) -> Vec<Fractal> {
        let mut fractals: Vec<Fractal> = Vec::new();
        let strokes = self
            .strokes
            .iter()
            .flat_map(|stroke| stroke.fractals.iter().skip(1).cloned());
        for x in strokes.chain(self.pending_fractals()) {
            if fractals.last().is_none_or(|last| x.dt > last.dt) {
                fractals.push(x);
            }
        }
        fractals
    }

    /// Direction of the unfinished stroke: opposite to the last stroke, once
    /// the unfinished NewBars hold a fractal. `None` before the first stroke.
    pub fn pending_direction(&self) -> Option<Direction> {
        let last = self.strokes.last()?;
        (!self.pending_fractals().is_empty()).then(|| last.direction.opposite())
    }

    /// Whether some unfinished NewBar is beyond the last stroke's end side.
    pub fn last_stroke_extends(&self) -> bool {
        self.strokes.last().is_some_and(|stroke| {
            self.pending_bars.iter().any(|b| match stroke.direction {
                Direction::Up => b.high > stroke.high(),
                Direction::Down => b.low < stroke.low(),
            })
        })
    }
}
