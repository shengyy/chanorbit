//! Incremental analyzer: inclusion, stroke search and extension and same-dt
//! tail revision over every input bar. Batch construction is the same per-bar
//! loop.
use crate::bar::{NewBar, RawBar, merge_inclusion};
use crate::fractal::{Fractal, scan_fractals};
use crate::stroke::{Direction, Endpoint, Stroke, beyond, extreme, find_stroke};
use std::sync::Arc;

/// Stroke parameters. Default: 6 NewBars.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    /// Minimum NewBar count of a stroke.
    pub min_stroke_bars: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self { min_stroke_bars: 6 }
    }
}

/// Undo record for exactly the latest input.
#[derive(Debug, Clone, Default)]
struct TailUpdate {
    pending_bars: Vec<Arc<NewBar>>,
    /// Strokes from `revised_from` on were changed or removed by the input;
    /// `revised` holds them as they were before it.
    revised_from: usize,
    revised: Vec<Stroke>,
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

    /// Every input raw bar (the last version of each `dt`).
    pub fn raw_bars(&self) -> &[RawBar] {
        &self.raw_bars
    }

    /// NewBars not yet part of a finished stroke search.
    pub fn pending_bars(&self) -> &[Arc<NewBar>] {
        &self.pending_bars
    }

    /// Every stroke, oldest first; the last one may still be extended.
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
            self.strokes.truncate(previous.revised_from);
            self.strokes.extend(previous.revised);
            self.raw_bars.pop();
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
        if let Some(mut stroke) = stroke {
            stroke.place(&self.raw_bars, None);
            self.strokes.push(stroke);
        }
        self.pending_bars = bars[rest..].to_vec();
    }

    /// While a raw bar after the last stroke's end is beyond it, the stroke is
    /// searched again. If it does not hold its end up to the most extreme such
    /// bar, it cannot extend across that price: it is reopened with the stroke
    /// before it, which is searched again from its start and replaces both
    /// once its end fractal lies beyond that stroke's. Otherwise the last
    /// stroke is searched again from its own start and replaced by a stroke
    /// whose end fractal lies further beyond; else it stays.
    fn extend_last_stroke(&mut self) {
        let Some(to) = self.extension() else {
            return;
        };
        let min = self.config.min_stroke_bars;
        let n = self.strokes.len();
        let last = &self.strokes[n - 1];
        if n >= 2 && !last.holds_end(to.at, None, &self.raw_bars) {
            let before = &self.strokes[n - 2];
            let window = before.reopen(&last.reopen(&self.pending_bars));
            if let (Some(stroke), _) = find_stroke(&window, min)
                && beyond(
                    before.direction,
                    stroke.end_fractal.price,
                    before.end_fractal.price,
                )
            {
                self.pop_stroke();
                self.pop_stroke();
                self.settle(window);
                return;
            }
        }
        let window = last.reopen(&self.pending_bars);
        if let (Some(stroke), _) = find_stroke(&window, min)
            && beyond(
                last.direction,
                stroke.end_fractal.price,
                last.end_fractal.price,
            )
        {
            self.pop_stroke();
            self.settle(window);
        }
    }

    /// Append the stroke found in `window`, which starts at the last stroke's
    /// end fractal, and keep the rest unfinished; its endpoints are placed
    /// after the last stroke's start and the last stroke then ends where the
    /// new one starts. When that would carry the last stroke across a raw
    /// price beyond its own start, the last two strokes are reopened and the
    /// stroke is searched again from the start of the one before them, as
    /// often as needed. A search that does not reach the new stroke's end, or
    /// no stroke before them, keeps the strokes and links the stroke as found;
    /// [`Analyzer::restart_back`] then moves the earlier starts.
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
            if kept == 0 {
                break;
            }
            let previous = &self.strokes[kept - 1];
            let start = stroke.start_after(&self.raw_bars, Some(previous.start.at));
            let end = stroke.end_from(&self.raw_bars, start);
            if previous.holds_end(start.at, Some(end.price), &self.raw_bars) {
                break;
            }
            let reopened = (kept >= 2).then(|| {
                let later = previous.reopen(bars);
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
        let (mut stroke, bars, rest) = merged.unwrap_or((found, window, found_rest));
        self.revise(kept.saturating_sub(1));
        self.strokes.truncate(kept);
        stroke.place(&self.raw_bars, self.strokes.last().map(|s| s.start.at));
        if let Some(last) = self.strokes.last_mut() {
            last.end = stroke.start;
        }
        self.strokes.push(stroke);
        self.pending_bars = bars[rest..].to_vec();
        self.restart_back();
    }

    /// After the last stroke was linked: while the stroke before it would then
    /// span a raw price beyond its own start, that stroke starts again at the
    /// most extreme start-side price up to its new end, and the stroke before
    /// it ends there in turn.
    fn restart_back(&mut self) {
        let mut i = self.strokes.len() - 1;
        while i > 0 {
            let until = self.strokes[i].start.at;
            let previous = &self.strokes[i - 1];
            if previous.holds_end(until, None, &self.raw_bars) {
                return;
            }
            let start = previous.restart(until, &self.raw_bars);
            self.revise((i - 1).saturating_sub(1));
            self.strokes[i - 1].start = start;
            if i >= 2 {
                self.strokes[i - 2].end = start;
            }
            i -= 1;
        }
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

    /// Whether some raw bar after the last stroke's end is beyond it.
    pub fn last_stroke_extends(&self) -> bool {
        self.extension().is_some()
    }

    /// The most extreme raw price after the last stroke's end and beyond it
    /// (see [`Analyzer::after_end`]), or `None`.
    pub fn extension(&self) -> Option<Endpoint> {
        let stroke = self.strokes.last()?;
        self.after_end(stroke.direction)
            .filter(|x| beyond(stroke.direction, x.price, stroke.end.price))
    }

    /// The most extreme raw price on `side` after the last stroke's end, held
    /// by the first raw bar that reaches it.
    pub fn after_end(&self, side: Direction) -> Option<Endpoint> {
        let end = self.strokes.last()?.end.at;
        extreme(
            &self.raw_bars[self.raw_bars.partition_point(|b| b.dt <= end)..],
            side,
        )
    }
}
