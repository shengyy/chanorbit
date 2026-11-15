//! Incremental analyzer: inclusion, stroke update/withdrawal, retention and
//! same-dt tail revision. Batch construction is the same per-bar loop.
use crate::bar::{NewBar, RawBar, merge_inclusion};
use crate::fractal::{Fractal, check_fxs};
use crate::stroke::{Direction, Stroke, check_bi};
use std::sync::Arc;

/// Stroke parameters. Defaults: 50 strokes, 6 NewBars.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    /// Maximum retained strokes; older strokes and their raw bars are dropped.
    pub max_bi_num: usize,
    /// Minimum NewBar count of a stroke.
    pub min_bi_len: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            max_bi_num: 50,
            min_bi_len: 6,
        }
    }
}

/// Undo record for exactly the latest input, including retention pruning.
#[derive(Debug, Clone, Default)]
struct TailUpdate {
    bars_ubi: Vec<Arc<NewBar>>,
    bi_count: usize,
    removed_bi: Option<Stroke>,
    pruned_bis: Vec<Stroke>,
    pruned_raw: Vec<RawBar>,
}

#[derive(Debug, Clone, Default)]
pub struct Analyzer {
    config: Config,
    bars_raw: Vec<RawBar>,
    bars_ubi: Vec<Arc<NewBar>>,
    bi_list: Vec<Stroke>,
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

    pub fn config(&self) -> Config {
        self.config
    }

    /// Retained raw bars (pruned to the first retained stroke's start).
    pub fn bars_raw(&self) -> &[RawBar] {
        &self.bars_raw
    }

    /// NewBars not yet part of a finished stroke search.
    pub fn bars_ubi(&self) -> &[Arc<NewBar>] {
        &self.bars_ubi
    }

    /// Retained strokes; the last one may still be withdrawn.
    pub fn bi_list(&self) -> &[Stroke] {
        &self.bi_list
    }

    /// Strokes counted as centre members: the last stroke is excluded while
    /// fewer than five unfinished NewBars follow it. Not a no-repaint claim.
    pub fn finished_len(&self) -> usize {
        if self.bars_ubi.len() < 5 {
            self.bi_list.len().saturating_sub(1)
        } else {
            self.bi_list.len()
        }
    }

    pub fn finished_bis(&self) -> &[Stroke] {
        &self.bi_list[..self.finished_len()]
    }

    /// Add one bar. A bar with the same `dt` as the previous input fully
    /// replaces it: the result equals having input only the new version.
    pub fn update(&mut self, bar: RawBar) {
        if self.bars_raw.last().is_some_and(|last| last.dt == bar.dt) {
            let previous = std::mem::take(&mut self.tail);
            self.bars_ubi = previous.bars_ubi;
            self.bi_list.splice(0..0, previous.pruned_bis);
            self.bi_list.truncate(previous.bi_count);
            if let Some(bi) = previous.removed_bi {
                self.bi_list.push(bi);
            }
            self.bars_raw.pop();
            self.bars_raw.splice(0..0, previous.pruned_raw);
        }
        self.tail = TailUpdate::default();
        let mut tail = TailUpdate {
            bars_ubi: self.bars_ubi.clone(),
            bi_count: self.bi_list.len(),
            ..TailUpdate::default()
        };
        self.bars_raw.push(bar);

        if self.bars_ubi.len() < 2 {
            self.bars_ubi.push(Arc::new(NewBar::from_raw(&bar)));
        } else {
            let n = self.bars_ubi.len();
            let (merged, k) = merge_inclusion(&self.bars_ubi[n - 2], &self.bars_ubi[n - 1], &bar);
            if merged {
                self.bars_ubi[n - 1] = Arc::new(k);
            } else {
                self.bars_ubi.push(Arc::new(k));
            }
        }

        let removed = self.update_bi();
        if self.bi_list.len() < tail.bi_count {
            tail.removed_bi = removed;
        }
        if self.bi_list.len() > self.config.max_bi_num {
            let excess = self.bi_list.len() - self.config.max_bi_num;
            tail.pruned_bis = self.bi_list.drain(..excess).collect();
        }
        if let Some(first) = self.bi_list.first() {
            let sdt = first.fx_a.elements[0].dt;
            let keep_from = self.bars_raw.partition_point(|b| b.dt < sdt);
            tail.pruned_raw = self.bars_raw.drain(..keep_from).collect();
        }
        self.tail = tail;
    }

    /// Returns the withdrawn tail stroke, if any.
    fn update_bi(&mut self) -> Option<Stroke> {
        if self.bars_ubi.len() < 3 {
            return None;
        }
        if self.bi_list.is_empty() {
            let fxs = check_fxs(&self.bars_ubi);
            let first = fxs.first()?;
            let mut fx_a = first;
            for x in fxs.iter().filter(|x| x.mark == first.mark) {
                let better = match first.mark {
                    crate::fractal::Mark::D => x.low <= fx_a.low,
                    crate::fractal::Mark::G => x.high >= fx_a.high,
                };
                if better {
                    fx_a = x;
                }
            }
            let start = fx_a.elements[0].dt;
            let bars: Vec<_> = self
                .bars_ubi
                .iter()
                .filter(|b| b.dt >= start)
                .cloned()
                .collect();
            let (bi, rest) = check_bi(&bars, self.config.min_bi_len);
            if let Some(bi) = bi {
                self.bi_list.push(bi);
            }
            self.bars_ubi = bars[rest..].to_vec();
            return None;
        }

        let (bi, rest) = check_bi(&self.bars_ubi, self.config.min_bi_len);
        if let Some(bi) = bi {
            self.bi_list.push(bi);
        }
        self.bars_ubi.drain(..rest);

        let last_bi = self.bi_list.last().unwrap();
        let broken = self
            .bars_ubi
            .last()
            .is_some_and(|b| match last_bi.direction {
                Direction::Up => b.high > last_bi.high(),
                Direction::Down => b.low < last_bi.low(),
            });
        if broken {
            let keep = last_bi.bars.len() - 2;
            let merge_point = last_bi.bars[keep].dt;
            self.bars_ubi = last_bi.bars[..keep]
                .iter()
                .chain(self.bars_ubi.iter().filter(|b| b.dt >= merge_point))
                .cloned()
                .collect();
            return self.bi_list.pop();
        }
        None
    }

    /// Fractals in the unfinished NewBars.
    pub fn ubi_fxs(&self) -> Vec<Fractal> {
        check_fxs(&self.bars_ubi)
    }

    /// Stroke-internal fractals (skipping each stroke's first) then unfinished
    /// fractals, keeping only strictly increasing times.
    pub fn fx_list(&self) -> Vec<Fractal> {
        let mut fxs: Vec<Fractal> = Vec::new();
        let strokes = self
            .bi_list
            .iter()
            .flat_map(|bi| bi.fxs.iter().skip(1).cloned());
        for x in strokes.chain(self.ubi_fxs()) {
            if fxs.last().is_none_or(|last| x.dt > last.dt) {
                fxs.push(x);
            }
        }
        fxs
    }

    /// Direction of the unfinished stroke: opposite to the last stroke, once
    /// the unfinished NewBars hold a fractal. `None` before the first stroke.
    pub fn ubi_direction(&self) -> Option<Direction> {
        let last = self.bi_list.last()?;
        (!self.ubi_fxs().is_empty()).then(|| last.direction.opposite())
    }

    /// Whether some unfinished NewBar is beyond the last stroke's end side.
    pub fn last_bi_extend(&self) -> bool {
        self.bi_list.last().is_some_and(|bi| {
            self.bars_ubi.iter().any(|b| match bi.direction {
                Direction::Up => b.high > bi.high(),
                Direction::Down => b.low < bi.low(),
            })
        })
    }
}
