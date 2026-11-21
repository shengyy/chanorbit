//! Synthetic multi-level series shared by the `signals` tests.
#![allow(dead_code)]
use chanorbit::RawBar;
use chanorbit::contract::{Command, handle_command};
use serde_json::{Value, json};

const T0: &str = "2026-01-05T00:00:00";

/// A 1m path from `start` through `(price, bars)` legs: each bar moves a
/// straight step, spans its two prices one tick wider each side and closes
/// at the step's end; a flat leg is one-price bars.
pub fn path(start: f64, legs: &[(f64, usize)]) -> Vec<RawBar> {
    let t0 = chanorbit::contract::parse_dt(T0).unwrap();
    let cents = |x: f64| (x * 100.0).round() / 100.0;
    let (mut price, mut out) = (start, Vec::new());
    for &(to, bars) in legs {
        let step = (to - price) / bars as f64;
        for i in 0..bars {
            let from = price;
            price = if i + 1 == bars { to } else { price + step };
            let (high, low) = if from == price {
                (cents(price), cents(price))
            } else {
                (cents(from.max(price) + 0.01), cents(from.min(price) - 0.01))
            };
            out.push(RawBar {
                dt: t0 + chrono::Duration::minutes(out.len() as i64 + 1),
                high,
                low,
                close: cents(price),
                vol: 100.0,
            });
        }
    }
    out
}

/// Complete buckets of `k` bars, each labelled by its last bar.
pub fn aggregate(bars: &[RawBar], k: usize) -> Vec<RawBar> {
    bars.chunks_exact(k)
        .map(|c| RawBar {
            dt: c[k - 1].dt,
            high: c.iter().map(|b| b.high).fold(f64::MIN, f64::max),
            low: c.iter().map(|b| b.low).fold(f64::MAX, f64::min),
            close: c[k - 1].close,
            vol: c.iter().map(|b| b.vol).sum(),
        })
        .collect()
}

pub fn columns(name: &str, bars: &[RawBar]) -> Value {
    let f = |g: fn(&RawBar) -> f64| bars.iter().map(g).collect::<Vec<_>>();
    json!({
        "name": name,
        "dt": bars.iter().map(|b| b.dt.format("%Y-%m-%dT%H:%M:%S").to_string()).collect::<Vec<_>>(),
        "high": f(|b| b.high), "low": f(|b| b.low), "close": f(|b| b.close), "vol": f(|b| b.vol),
    })
}

/// A request over the 1m path: 30m and 5m, plus 1m when `leaf`.
pub fn request(minutes: &[RawBar], leaf: bool) -> Value {
    let mut levels = vec![
        columns("30m", &aggregate(minutes, 30)),
        columns("5m", &aggregate(minutes, 5)),
    ];
    if leaf {
        levels.push(columns("1m", minutes));
    }
    json!({"id": "nest", "levels": levels})
}

pub fn run(command: Command, request: &Value) -> Value {
    let (text, ok) = handle_command(command, &request.to_string());
    assert!(ok, "{text}");
    serde_json::from_str(&text).unwrap()
}

/// The first `n` 1m bars.
pub fn cut(minutes: &[RawBar], n: usize) -> Vec<RawBar> {
    minutes[..n].to_vec()
}

/// The parent structure: from a flat start, a 30m centre (members from 90
/// to 99.6, 92, `p3` and 93, so `zg` = `p3` + 0.01) left upward by a stroke
/// to 104 (`H_dep` = 104.01).
pub fn parent(p3: f64) -> Vec<(f64, usize)> {
    vec![
        (100.0, 900),
        (104.0, 240),
        (90.0, 240),
        (99.6, 240),
        (92.0, 240),
        (p3, 240),
        (93.0, 240),
        (104.0, 240),
    ]
}

/// 1m bars before the pullback.
pub const PULLBACK_AT: usize = 900 + 7 * 240;

/// The 5m pullback: an entry to 99, a centre (102, 100, 101.5) and a last
/// segment that holds a 1m five-stroke move whose last leg of `bars` 1m bars
/// reaches `low`.
pub fn pullback(low: f64, bars: usize) -> Vec<(f64, usize)> {
    vec![
        (99.0, 40),
        (102.0, 40),
        (100.0, 40),
        (101.5, 40),
        (99.2, 12),
        (99.9, 8),
        (99.4, 8),
        (99.8, 8),
        (low, bars),
    ]
}

/// The parent, the pullback and `tail`.
pub fn buy_path(p3: f64, pull: Vec<(f64, usize)>, tail: &[(f64, usize)]) -> Vec<RawBar> {
    let mut legs = parent(p3);
    legs.extend(pull);
    legs.extend_from_slice(tail);
    path(100.0, &legs)
}

/// The base buy series: pullback low 98.49 on the 1m bar ending at bar
/// 2786 (the first of its 5m bar), then a rise.
pub fn base() -> Vec<RawBar> {
    buy_path(95.0, pullback(98.5, 10), &[(99.5, 20)])
}

pub fn buy(request: &Value) -> Value {
    run(Command::Signals, request)["buy"].clone()
}

pub fn stage(request: &Value) -> Value {
    buy(request)["stage"].clone()
}

/// The sell series: a 30m upswing from 92 whose 5m move (up to 97, 95,
/// `m3`, 95.5 or 96) ends in a last up segment holding a 1m five-stroke
/// move to `high`, then `tail`.
pub fn sell_path(
    m3: (f64, usize),
    m4: f64,
    inner: [f64; 4],
    high: (f64, usize),
    tail: &[(f64, usize)],
) -> Vec<RawBar> {
    let mut legs = vec![
        (100.0, 900),
        (104.0, 240),
        (92.0, 240),
        (97.0, 40),
        (95.0, 40),
        m3,
        (m4, 40),
        (inner[0], 12),
        (inner[1], 8),
        (inner[2], 8),
        (inner[3], 8),
        high,
    ];
    legs.extend_from_slice(tail);
    path(100.0, &legs)
}

pub fn sell_base(high: (f64, usize), tail: &[(f64, usize)]) -> Vec<RawBar> {
    sell_path((96.5, 40), 95.5, [97.3, 96.6, 97.1, 96.7], high, tail)
}

pub fn sell(request: &Value) -> Value {
    run(Command::Signals, request)["sell"].clone()
}
