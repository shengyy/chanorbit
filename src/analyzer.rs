//! Incremental analyzer: inclusion, stroke search and extension, retention and
//! same-dt tail revision. Batch construction is the same per-bar loop.
use crate::bar::{NewBar, RawBar, merge_inclusion};
use crate::fractal::{Fractal, scan_fractals};
use crate::stroke::{Direction, Stroke, beyond, find_stroke};
use chrono::NaiveDateTime;
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
    /// Strokes from `revised_from` on were changed or removed by the input;
    /// `revised` holds them as they were before it.
    revised_from: usize,
    revised: Vec<Stroke>,
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

    /// Retained strokes; the last one may still be extended.
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
            self.strokes.truncate(previous.revised_from);
            self.strokes.extend(previous.revised);
            self.raw_bars.pop();
            self.raw_bars.splice(0..0, previous.pruned_raw);
        }
        self.tail = TailUpdate {
            pending_bars: self.pending_bars.clone(),
            revised_from: self.strokes.len(),
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

        self.update_strokes();
        if self.strokes.len() > self.config.max_strokes {
            let excess = self.strokes.len() - self.config.max_strokes;
            self.tail.pruned_strokes = self.strokes.drain(..excess).collect();
        }
        if let Some(first) = self.strokes.first() {
            let start_at = first.start_fractal.elements[0].dt;
            let keep_from = self.raw_bars.partition_point(|b| b.dt < start_at);
            self.tail.pruned_raw = self.raw_bars.drain(..keep_from).collect();
        }
    }

    /// Record strokes from `i` on before this input changes or removes them.
    /// Strokes before `revised_from` are still as they were before the input.
    fn revise(&mut self, i: usize) {
        let from = self.tail.revised_from;
        if i < from {
            let saved = self.strokes[i..from].to_vec();
            self.tail.revised.splice(0..0, saved);
            self.tail.revised_from = i;
        }
    }

    fn pop_stroke(&mut self) -> Option<Stroke> {
        self.revise(self.strokes.len().checked_sub(1)?);
        self.strokes.pop()
    }

    fn update_strokes(&mut self) {
        if self.pending_bars.len() < 3 {
            return;
        }
        if self.strokes.is_empty() {
            self.first_stroke();
            return;
        }
        self.extend_last_stroke();
        let window = std::mem::take(&mut self.pending_bars);
        self.settle(window);
    }

    /// Before any stroke: start at the most extreme fractal of the first
    /// fractal's mark (later ones win ties) and search from there.
    fn first_stroke(&mut self) {
        let fractals = scan_fractals(&self.pending_bars);
        let Some(first) = fractals.first() else {
            return;
        };
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
    }

    /// While an unfinished NewBar is beyond the last stroke's end, the stroke
    /// is searched again. If a raw bar after its end is already beyond its
    /// start, it cannot extend: it is reopened with the stroke before it, which
    /// is searched again from its start and replaces both once it ends further
    /// beyond. Otherwise the last stroke is searched again from its own start
    /// and replaced by a stroke ending further beyond; else it stays.
    fn extend_last_stroke(&mut self) {
        if !self.last_stroke_extends() {
            return;
        }
        let min = self.config.min_stroke_bars;
        let n = self.strokes.len();
        let last = &self.strokes[n - 1];
        if n >= 2 && !last.holds_end(NaiveDateTime::MAX, &self.pending_bars) {
            let before = &self.strokes[n - 2];
            let window = before.reopen(&last.reopen(&self.pending_bars));
            if let (Some(stroke), _) = find_stroke(&window, min)
                && beyond(before.direction, stroke.end.price, before.end_fractal.price)
            {
                self.pop_stroke();
                self.pop_stroke();
                self.settle(window);
                return;
            }
        }
        let window = last.reopen(&self.pending_bars);
        if let (Some(stroke), _) = find_stroke(&window, min)
            && beyond(last.direction, stroke.end.price, last.end.price)
        {
            self.pop_stroke();
            self.settle(window);
        }
    }

    /// Append the stroke found in `window`, which starts at the last stroke's
    /// end fractal, and keep the rest unfinished; the last stroke then ends
    /// where the new one starts. When that would carry the last stroke across a
    /// raw price beyond its own start, the last two strokes are reopened and the
    /// stroke is searched again from the start of the one before them, as often
    /// as needed. A search that does not reach the new stroke's end, or no
    /// stroke before them, keeps the strokes and links the stroke as found.
    fn settle(&mut self, window: Vec<Arc<NewBar>>) {
        let min = self.config.min_stroke_bars;
        let (Some(found), found_rest) = find_stroke(&window, min) else {
            self.pending_bars = window;
            return;
        };
        // `merged`: the stroke searched again over reopened strokes, its window
        // and rest; `kept`: the strokes it follows.
        let mut merged: Option<(Stroke, Vec<Arc<NewBar>>, usize)> = None;
        let mut kept = self.strokes.len();
        loop {
            let (stroke, bars) = merged.as_ref().map_or((&found, &window), |m| (&m.0, &m.1));
            if kept == 0 || self.strokes[kept - 1].holds_end(stroke.start.at, bars) {
                break;
            }
            let reopened = (kept >= 2).then(|| {
                let later = self.strokes[kept - 1].reopen(bars);
                self.strokes[kept - 2].reopen(&later)
            });
            match reopened.map(|bars| (find_stroke(&bars, min), bars)) {
                Some(((Some(again), r), bars)) if again.end_fractal.dt >= stroke.end_fractal.dt => {
                    merged = Some((again, bars, r));
                    kept -= 2;
                }
                _ => {
                    merged = None;
                    kept = self.strokes.len();
                    break;
                }
            }
        }
        let (stroke, bars, rest) = merged.unwrap_or((found, window, found_rest));
        self.revise(kept.saturating_sub(1));
        self.strokes.truncate(kept);
        if let Some(last) = self.strokes.last_mut() {
            last.end = stroke.start;
        }
        self.strokes.push(stroke);
        self.pending_bars = bars[rest..].to_vec();
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
            self.pending_bars.iter().any(|b| {
                let boundary = match stroke.direction {
                    Direction::Up => b.high,
                    Direction::Down => b.low,
                };
                beyond(stroke.direction, boundary, stroke.end.price)
            })
        })
    }
}
