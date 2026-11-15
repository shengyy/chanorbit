//! Small definitional checks that do not need a baseline.
mod common;

use chanorbit::contract::{handle, parse_dt};
use chanorbit::power::{MacdBar, macd_segments};
use chanorbit::{Analyzer, RawBar, geometry};
use serde_json::{Value, json};

#[test]
fn times_parse_only_one_format() {
    assert!(parse_dt("2026-01-05T15:00:00").is_some());
    for bad in [
        "2026-01-05 09:31:00",
        "2026-01-05",
        "2026-01-05T9:31:00",
        " 2026-01-05T09:31:00",
        "2026-01-05T09:31:00 ",
        "2026-01-05T09:31:00.5",
        "2026-01-05T15:00:00Z",
        "2026-01-05T15:00:00+08:00",
        "2026-02-30T15:00:00",
        "2026-01-05T24:00:00",
        "2026-06-30T23:59:60",
        "+2026-01-05T15:00",
    ] {
        assert!(parse_dt(bad).is_none(), "{bad}");
    }
}

/// Every time in a response is written as `YYYY-MM-DDTHH:MM:SS`.
#[test]
fn output_times_have_one_format() {
    fn walk(v: &Value, key: &str, seen: &mut usize) {
        match v {
            Value::Object(m) => m.iter().for_each(|(k, v)| walk(v, k, seen)),
            Value::Array(a) => a.iter().for_each(|v| walk(v, key, seen)),
            Value::String(s) if key.ends_with("dt") || key.ends_with("_at") => {
                *seen += 1;
                assert!(parse_dt(s).is_some(), "{key}: {s}");
            }
            _ => {}
        }
    }
    let mut seen = 0;
    for path in common::golden_files() {
        let doc = common::load(&path);
        if !common::default_config(&doc) {
            continue;
        }
        let (text, ok) = handle(&doc["request"].to_string());
        assert!(ok);
        walk(&serde_json::from_str(&text).unwrap(), "", &mut seen);
    }
    assert!(seen > 5_000, "{seen}");
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
        assert_eq!(all.raw_bars(), deduped.raw_bars(), "{name}");
    }
}

const D1: &str = "2026-01-05T15:00:00";
const D2: &str = "2026-01-06T15:00:00";
const D3: &str = "2026-01-07T15:00:00";

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
    let (text, ok) = handle(&request(&[D2, D1], &[10.0, 11.0]));
    assert!(!ok);
    assert_eq!(error_kind(&text).as_deref(), Some("invalid_bar"));
    assert!(text.contains("dt[1]"), "{text}");
}

#[test]
fn adjacent_equal_time_is_a_revision() {
    let (revised, ok) = handle(&request(&[D1, D2, D2, D3], &[10.0, 11.0, 12.5, 12.0]));
    assert!(ok, "{revised}");
    let (last_only, ok) = handle(&request(&[D1, D2, D3], &[10.0, 12.5, 12.0]));
    assert!(ok);
    assert_eq!(revised, last_only);
}

#[test]
fn non_adjacent_repeated_time_is_rejected() {
    let (text, ok) = handle(&request(&[D1, D2, D1], &[10.0, 11.0, 12.0]));
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
        .pending_bars()
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
    let top = c.strokes().last().unwrap().end_fractal.clone();
    assert!(
        displaced
            .iter()
            .any(|b| b.dt == top.dt && b.high == top.price),
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

#[test]
fn prices_and_volumes_must_be_in_range() {
    let ok = r#"{"dt":["2026-01-05T15:00:00"],"high":[10],"low":[9],"close":[9.5],"vol":[100]}"#;
    assert!(handle(ok).1);
    for (field, value) in [
        ("high", "1e10"),
        ("low", "0"),
        ("close", "-1"),
        ("high", "1e306"),
        ("vol", "-1"),
        ("vol", "1e16"),
    ] {
        let mut v: Value = serde_json::from_str(ok).unwrap();
        v[field] = serde_json::from_str(&format!("[{value}]")).unwrap();
        let (text, good) = handle(&v.to_string());
        assert!(!good, "{field}={value}");
        assert_eq!(error_kind(&text).as_deref(), Some("invalid_bar"), "{text}");
        assert!(text.contains(&format!("{field}[0]")), "{text}");
    }
}

#[test]
fn request_must_be_an_object_within_the_length_limit() {
    let positional = r#"["pos",["2026-03-02T15:00:00"],[10.3],[9.9],[10.2],[1000]]"#;
    let (text, ok) = handle(positional);
    assert!(!ok);
    assert_eq!(error_kind(&text).as_deref(), Some("invalid_input"));
    assert_eq!(
        error_kind(&handle("[1, 2").0).as_deref(),
        Some("invalid_json")
    );
    assert_eq!(
        error_kind(&handle("42").0).as_deref(),
        Some("invalid_input")
    );

    let n = chanorbit::contract::MAX_BARS + 1;
    let start = parse_dt(D1).unwrap();
    let dt: Vec<String> = (0..n)
        .map(|i| {
            (start + chrono::Duration::minutes(i as i64))
                .format("%Y-%m-%dT%H:%M:%S")
                .to_string()
        })
        .collect();
    let big = json!({"dt": dt, "high": vec![2.0; n], "low": vec![1.0; n], "close": vec![1.5; n], "vol": vec![1.0; n]});
    let (text, ok) = handle(&big.to_string());
    assert!(!ok);
    assert_eq!(
        error_kind(&text).as_deref(),
        Some("invalid_input"),
        "{text}"
    );
}
