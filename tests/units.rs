//! Small definitional checks that do not need a baseline.
mod common;

use chanorbit::contract::{handle, parse_dt};
use chanorbit::power::{MacdBar, macd_segments};
use chanorbit::{Analyzer, RawBar, geometry};
use serde_json::{Value, json};

#[test]
fn times_are_naive_wall_clock() {
    assert!(parse_dt("2026-01-05T15:00:00").is_some());
    assert!(parse_dt("2026-01-05 09:31:00").is_some());
    assert!(parse_dt("2026-01-05").is_some());
    assert!(parse_dt("2026-01-05T15:00:00Z").is_none());
    assert!(parse_dt("2026-01-05T15:00:00+08:00").is_none());
    let dt = parse_dt("2026-01-05T15:00:00").unwrap();
    assert_eq!(serde_json::to_value(dt).unwrap(), "2026-01-05T15:00:00");
}

#[test]
fn zero_continues_a_run_and_missing_closes_it() {
    let base = parse_dt("2026-01-05T09:30:00").unwrap();
    let hist = [
        None,
        Some(0.),
        Some(0.2),
        Some(0.),
        Some(0.5),
        Some(-0.1),
        Some(-0.4),
        None,
        Some(-0.3),
        Some(0.),
    ];
    let bars: Vec<MacdBar> = hist
        .iter()
        .enumerate()
        .map(|(i, h)| MacdBar {
            dt: base + chrono::Duration::minutes(i as i64),
            dif: *h,
            dea: h.map(|_| 0.),
            hist: *h,
        })
        .collect();
    let rows: Vec<_> = macd_segments(&bars)
        .iter()
        .map(|s| (s.sign, s.sdt, s.edt, s.bars, s.peak, s.peak_at))
        .collect();
    assert_eq!(
        rows,
        vec![
            (1, bars[2].dt, bars[4].dt, 3, 0.5, bars[4].dt),
            (-1, bars[5].dt, bars[6].dt, 2, -0.4, bars[6].dt),
            (-1, bars[8].dt, bars[9].dt, 2, -0.3, bars[8].dt),
        ]
    );
}

/// A same-dt revision fully replaces the previous input: feeding every
/// revision equals feeding only the last version of each time.
#[test]
fn same_dt_revision_equals_single_input() {
    for name in [
        "daily_corrections.json.gz",
        "f5_corrections.json.gz",
        "daily_prune_max5.json.gz",
    ] {
        let path = common::golden_files()
            .into_iter()
            .find(|p| p.ends_with(name))
            .unwrap();
        let (bars, config) = common::input(&common::load(&path));
        let mut last: Vec<RawBar> = Vec::new();
        for b in &bars {
            if last.last().is_some_and(|l| l.dt == b.dt) {
                last.pop();
            }
            last.push(*b);
        }
        assert!(last.len() < bars.len(), "{name} has revisions");
        let all = Analyzer::from_bars(bars, config);
        let deduped = Analyzer::from_bars(last, config);
        assert_eq!(geometry(&all), geometry(&deduped), "{name}");
        assert_eq!(all.bars_raw(), deduped.bars_raw(), "{name}");
    }
}

fn request(dt: &[&str], high: &[f64]) -> String {
    let low: Vec<f64> = high.iter().map(|h| h - 1.0).collect();
    json!({"id": "t", "dt": dt, "high": high, "low": low, "close": high, "vol": vec![100.0; dt.len()]})
        .to_string()
}

fn error_kind(text: &str) -> Option<String> {
    let v: Value = serde_json::from_str(text).unwrap();
    v["error"]["kind"].as_str().map(str::to_owned)
}

#[test]
fn decreasing_time_is_rejected() {
    let (text, ok) = handle(&request(&["2026-01-06", "2026-01-05"], &[10.0, 11.0]));
    assert!(!ok);
    assert_eq!(error_kind(&text).as_deref(), Some("invalid_bar"));
    assert!(text.contains("dt[1]"), "{text}");
}

#[test]
fn adjacent_equal_time_is_a_revision() {
    let (revised, ok) = handle(&request(
        &["2026-01-05", "2026-01-06", "2026-01-06", "2026-01-07"],
        &[10.0, 11.0, 12.5, 12.0],
    ));
    assert!(ok, "{revised}");
    let (last_only, ok) = handle(&request(
        &["2026-01-05", "2026-01-06", "2026-01-07"],
        &[10.0, 12.5, 12.0],
    ));
    assert!(ok);
    assert_eq!(revised, last_only);
}

#[test]
fn non_adjacent_repeated_time_is_rejected() {
    let (text, ok) = handle(&request(
        &["2026-01-05", "2026-01-06", "2026-01-05"],
        &[10.0, 11.0, 12.0],
    ));
    assert!(!ok);
    assert_eq!(error_kind(&text).as_deref(), Some("invalid_bar"));
    assert!(text.contains("dt[2]"), "{text}");
}

/// A NewBar keeps at most 101 raw bars: the first 100 and the latest. The
/// first element (and so a fractal's `confirmed_at`) is never displaced; a
/// raw price owner in the 101st slot is, which the candidate reports.
#[test]
fn newbar_element_cap() {
    let path = common::golden_files()
        .into_iter()
        .find(|p| p.ends_with("f1_long_merge.json.gz"))
        .unwrap();
    let (bars, config) = common::input(&common::load(&path));
    let c = Analyzer::from_bars(bars.iter().copied(), config);
    let capped: Vec<_> = c
        .bars_ubi()
        .iter()
        .filter(|b| b.elements.len() > 100)
        .collect();
    assert_eq!(capped.len(), 1);
    let nb = capped[0];
    assert_eq!(nb.elements.len(), 101);
    let start = bars.iter().position(|b| b.dt == nb.elements[0].dt).unwrap();
    let merged = &bars[start
        ..=bars
            .iter()
            .position(|b| b.dt == nb.elements[100].dt)
            .unwrap()];
    assert!(merged.len() > 101, "more raw bars merged than kept");
    assert_eq!(nb.elements[..100], merged[..100]);
    assert_eq!(nb.elements[100], merged[merged.len() - 1]);
    let displaced = &merged[100..merged.len() - 1];
    let top = c.bi_list().last().unwrap().fx_b.clone();
    assert!(
        displaced.iter().any(|b| b.dt == top.dt && b.high == top.fx),
        "the merged high's raw bar was displaced"
    );
    assert_eq!(top.elements[1].elements[0], merged[0]);
    let g = geometry(&c);
    let last = g.fractals.last().unwrap();
    assert_eq!(last.dt, top.dt);
    assert_eq!(
        last.confirmed_at,
        top.elements[2].elements.first().map(|b| b.dt)
    );
    assert_eq!(g.candidate_stroke.state, "unavailable");
    assert_eq!(
        g.candidate_stroke.reason.as_deref(),
        Some("anchor_price_source_missing")
    );
}
