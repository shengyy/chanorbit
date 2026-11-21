//! The `signals` output: one evaluation of the nests over every level, and
//! with `observe_from` a per-step replay along the finest level that records
//! when each `(side, key, stage)` first stood. Definitions are owned by
//! `docs/structure-contract.md#signals`.
use crate::analyzer::{Analyzer, Config};
use crate::bar::RawBar;
use crate::centre::Centre;
use crate::macd::{MacdSeries, PARAMS, macd_series};
use crate::signal::{
    Buy, LevelRead, ParentDeparture, Sell, Stage, Support, buy, mid_centres, mid_support,
    parent_complete, parent_departure, sell,
};
use chrono::NaiveDateTime;
use serde::Serialize;
use std::collections::HashSet;

/// The nests after one step: `buy` (`third_buy_nest`), `sell`
/// (`up_end_nest`) and the middle level's support.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Signals {
    pub buy: Option<Buy>,
    pub sell: Option<Sell>,
    pub mid_support: Option<Support>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Signal {
    Buy(Box<Buy>),
    Sell(Box<Sell>),
}

/// The first observed step at which a `(side, key, stage)` stood, with the
/// whole object at that step.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Observed {
    pub at: NaiveDateTime,
    pub side: Side,
    pub signal: Signal,
}

/// What one evaluation reads from a level that has not changed since the
/// previous step, keyed by the raw bar counts it was computed from. Levels
/// only grow (revisions are resolved before the replay), so a count names a
/// state.
#[derive(Default)]
struct Memo {
    parent: Option<(usize, Option<ParentDeparture>)>,
    centres: Option<(usize, Vec<Centre>)>,
    parent_complete: Option<((usize, usize), bool)>,
}

fn evaluate(levels: &[Analyzer], macd: &[MacdSeries], memo: &mut Memo) -> Signals {
    let read: Vec<LevelRead> = levels
        .iter()
        .zip(macd)
        .map(|(c, m)| LevelRead::new(c, m))
        .collect();
    let (p, m, l) = (&read[0], &read[1], read.get(2));
    let lens = (p.raw().len(), m.raw().len());
    if memo.parent.as_ref().is_none_or(|x| x.0 != lens.0) {
        memo.parent = Some((lens.0, parent_departure(p)));
    }
    let dep = memo.parent.as_ref().and_then(|x| x.1.as_ref());
    let sell = l.and_then(|l| {
        if memo.centres.as_ref().is_none_or(|x| x.0 != lens.1) {
            memo.centres = Some((lens.1, mid_centres(m)));
        }
        if memo.parent_complete.is_none_or(|x| x.0 != lens) {
            memo.parent_complete = Some((lens, parent_complete(p, m)));
        }
        let centres = &memo.centres.as_ref().expect("set above").1;
        let complete = memo.parent_complete.expect("set above").1;
        sell(m, Some(l), centres, complete)
    });
    Signals {
        buy: buy(p, m, l, dep),
        sell,
        mid_support: mid_support(m),
    }
}

/// The last version of each `dt`.
fn dedup_last(bars: &[RawBar]) -> Vec<RawBar> {
    let mut out: Vec<RawBar> = Vec::with_capacity(bars.len());
    for b in bars {
        match out.last_mut() {
            Some(last) if last.dt == b.dt => *last = *b,
            _ => out.push(*b),
        }
    }
    out
}

fn macd_of(bars: &[RawBar]) -> MacdSeries {
    let closes: Vec<f64> = bars.iter().map(|b| b.close).collect();
    let [short, long, m] = PARAMS;
    macd_series(&closes, short, long, m)
}

/// The nests over 2 or 3 levels, coarsest first, and with `observe_from`
/// the replay's first observations.
pub fn signals(
    inputs: &[Vec<RawBar>],
    observe_from: Option<NaiveDateTime>,
) -> (Signals, Option<Vec<Observed>>) {
    assert!(
        (2..=3).contains(&inputs.len()),
        "signals takes 2 or 3 levels"
    );
    let series: Vec<Vec<RawBar>> = inputs.iter().map(|b| dedup_last(b)).collect();
    let macd: Vec<MacdSeries> = series.iter().map(|s| macd_of(s)).collect();
    let config = Config::default();
    let Some(from) = observe_from else {
        let levels: Vec<Analyzer> = series
            .iter()
            .map(|s| Analyzer::from_bars(s.iter().copied(), config))
            .collect();
        return (evaluate(&levels, &macd, &mut Memo::default()), None);
    };
    let finest = series.len() - 1;
    let mut levels: Vec<Analyzer> = series.iter().map(|_| Analyzer::new(config)).collect();
    let mut fed = vec![0usize; series.len()];
    let mut memo = Memo::default();
    let mut seen: HashSet<(bool, String, Stage)> = HashSet::new();
    let mut observed = Vec::new();
    let feed_until = |levels: &mut [Analyzer], fed: &mut [usize], t: Option<NaiveDateTime>| {
        for (k, s) in series.iter().enumerate().take(finest) {
            while fed[k] < s.len() && t.is_none_or(|t| s[fed[k]].dt <= t) {
                levels[k].update(s[fed[k]]);
                fed[k] += 1;
            }
        }
    };
    for bar in &series[finest] {
        // Each level holds only the bars labelled no later than this step.
        feed_until(&mut levels, &mut fed, Some(bar.dt));
        levels[finest].update(*bar);
        if bar.dt < from {
            continue;
        }
        let now = evaluate(&levels, &macd, &mut memo);
        if let Some(b) = now.buy.as_ref().filter(|b| b.stage != Stage::Armed)
            && seen.insert((true, b.key.clone(), b.stage))
        {
            observed.push(Observed {
                at: bar.dt,
                side: Side::Buy,
                signal: Signal::Buy(Box::new(b.clone())),
            });
        }
        if let Some(s) = &now.sell
            && seen.insert((false, s.key.clone(), s.stage))
        {
            observed.push(Observed {
                at: bar.dt,
                side: Side::Sell,
                signal: Signal::Sell(Box::new(s.clone())),
            });
        }
    }
    feed_until(&mut levels, &mut fed, None);
    (evaluate(&levels, &macd, &mut memo), Some(observed))
}
