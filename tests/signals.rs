//! `signals` rules on synthetic multi-level series.
use chanorbit::RawBar;
use chanorbit::contract::{Command, handle_command};
use serde_json::{Value, json};

const T0: &str = "2026-01-05T00:00:00";

/// A 1m path from `start` through `(price, bars)` legs: each bar moves a
/// straight step, spans its two prices one tick wider each side and closes
/// at the step's end; a flat leg is one-price bars.
fn path(start: f64, legs: &[(f64, usize)]) -> Vec<RawBar> {
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
fn aggregate(bars: &[RawBar], k: usize) -> Vec<RawBar> {
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

fn columns(name: &str, bars: &[RawBar]) -> Value {
    let f = |g: fn(&RawBar) -> f64| bars.iter().map(g).collect::<Vec<_>>();
    json!({
        "name": name,
        "dt": bars.iter().map(|b| b.dt.format("%Y-%m-%dT%H:%M:%S").to_string()).collect::<Vec<_>>(),
        "high": f(|b| b.high), "low": f(|b| b.low), "close": f(|b| b.close), "vol": f(|b| b.vol),
    })
}

/// A request over the 1m path: 30m and 5m, plus 1m when `leaf`.
fn request(minutes: &[RawBar], leaf: bool) -> Value {
    let mut levels = vec![
        columns("30m", &aggregate(minutes, 30)),
        columns("5m", &aggregate(minutes, 5)),
    ];
    if leaf {
        levels.push(columns("1m", minutes));
    }
    json!({"id": "nest", "levels": levels})
}

fn run(command: Command, request: &Value) -> Value {
    let (text, ok) = handle_command(command, &request.to_string());
    assert!(ok, "{text}");
    serde_json::from_str(&text).unwrap()
}

/// The first `n` 1m bars.
fn cut(minutes: &[RawBar], n: usize) -> Vec<RawBar> {
    minutes[..n].to_vec()
}

/// The parent structure: from a flat start, a 30m centre (members from 90
/// to 99.6, 92, `p3` and 93, so `zg` = `p3` + 0.01) left upward by a stroke
/// to 104 (`H_dep` = 104.01).
fn parent(p3: f64) -> Vec<(f64, usize)> {
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
const PULLBACK_AT: usize = 900 + 7 * 240;

/// The 5m pullback: an entry to 99, a centre (102, 100, 101.5) and a last
/// segment that holds a 1m five-stroke move whose last leg of `bars` 1m bars
/// reaches `low`.
fn pullback(low: f64, bars: usize) -> Vec<(f64, usize)> {
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
fn buy_path(p3: f64, pull: Vec<(f64, usize)>, tail: &[(f64, usize)]) -> Vec<RawBar> {
    let mut legs = parent(p3);
    legs.extend(pull);
    legs.extend_from_slice(tail);
    path(100.0, &legs)
}

/// The base buy series: pullback low 98.49 on the 1m bar ending at bar
/// 2786 (the first of its 5m bar), then a rise.
fn base() -> Vec<RawBar> {
    buy_path(95.0, pullback(98.5, 10), &[(99.5, 20)])
}

fn buy(request: &Value) -> Value {
    run(Command::Signals, request)["buy"].clone()
}

fn stage(request: &Value) -> Value {
    buy(request)["stage"].clone()
}

#[test]
fn every_stage_on_the_base_series() {
    let m = base();
    // After the entry stroke only: A holds, no completion yet.
    let early = buy(&request(&cut(&m, PULLBACK_AT + 60), false));
    assert_eq!(early["stage"], "armed");
    assert_eq!(early["mid"], Value::Null);
    assert_eq!(early["pullback"]["low"], 98.99);
    // The 5m bar holding the low has closed: complete, not confirmed.
    let ripe = buy(&request(&cut(&m, 2790), false));
    assert_eq!(ripe["stage"], "ripe");
    assert_eq!(ripe["mid"]["confirmed_at"], Value::Null);
    assert_eq!(ripe["leaf"], Value::Null);
    // The next 5m bar confirms the 5m bottom fractal.
    let turned = buy(&request(&cut(&m, 2795), false));
    assert_eq!(turned["stage"], "turned");
    assert_eq!(turned["mid"]["confirmed_at"], "2026-01-06T22:35:00");
    let full = run(Command::Signals, &request(&m, false));
    assert_eq!(full["buy"]["stage"], "turned");
    assert_eq!(full["sell"], Value::Null);
    assert_eq!(full["observed"], Value::Null);
    let b = &full["buy"];
    assert_eq!(
        b["key"],
        "2026-01-05T23:00:00|2026-01-06T19:00:00|2026-01-06T22:30:00"
    );
    assert_eq!(
        b["centre"],
        json!({"start_at": "2026-01-05T23:00:00", "zd": 91.99, "zg": 95.01, "dd": 89.99, "gg": 104.01, "member_count": 5})
    );
    assert_eq!(
        (&b["departure"]["high"], &b["departure"]["new_extreme"]),
        (&json!(104.01), &json!(true))
    );
    // The 30m series is too short for MACD: no area, the departure stands.
    assert_eq!(b["departure"]["area"], Value::Null);
    assert_eq!(
        b["pullback"],
        json!({"low": 98.49, "low_at": "2026-01-06T22:30:00"})
    );
    let mid = &b["mid"];
    assert_eq!(
        mid["centre"],
        json!({"start_at": "2026-01-06T19:40:00", "zd": 99.99, "zg": 101.51, "dd": 98.49, "gg": 102.01, "member_count": 4})
    );
    assert_eq!(
        (&mid["entry"]["start_at"], &mid["entry"]["end_at"]),
        (&json!("2026-01-06T19:00:00"), &json!("2026-01-06T19:40:00"))
    );
    assert_eq!(
        (&mid["last"]["start_at"], &mid["last"]["end_at"]),
        (&json!("2026-01-06T21:40:00"), &json!("2026-01-06T22:30:00"))
    );
    let ratio = mid["last"]["area"].as_f64().unwrap() / mid["entry"]["area"].as_f64().unwrap();
    assert_eq!(mid["area_ratio"].as_f64().unwrap(), ratio);
    assert!(ratio < 1.0);
    // With the 1m level: the 1m move inside the last 5m segment confirms.
    let three = run(Command::Signals, &request(&m, true));
    let b = &three["buy"];
    assert_eq!(b["stage"], "triggered");
    assert_eq!(b["pullback"]["low_at"], "2026-01-06T22:26:00");
    assert_eq!(b["leaf"]["last"]["start_at"], "2026-01-06T22:16:00");
    assert_eq!(b["leaf"]["confirmed_at"], "2026-01-06T22:28:00");
    assert_eq!(b["leaf"]["centre"]["start_at"], "2026-01-06T21:52:00");
    assert_eq!(b["mid"], full["buy"]["mid"]);
    assert_eq!(
        three["mid_support"],
        json!({"low": 98.49, "low_at": "2026-01-06T22:30:00"})
    );
}

/// Fractal not confirmed: the same series one bar before its confirmation.
#[test]
fn an_unconfirmed_fractal_holds_the_stage_back() {
    let m = base();
    assert_eq!(stage(&request(&cut(&m, 2790), false)), "ripe");
    assert_eq!(stage(&request(&cut(&m, 2795), false)), "turned");
    // The low on the last 1m bar of its 5m bar: the 1m bottom confirms two
    // 1m bars later (the bar after the low ties it and merges).
    let m = buy_path(95.0, pullback(98.5, 14), &[(99.5, 20)]);
    let before = buy(&request(&cut(&m, 2790), true));
    assert_eq!(before["stage"], "ripe");
    assert_eq!(before["leaf"]["confirmed_at"], Value::Null);
    assert!(before["leaf"]["area_ratio"].as_f64().unwrap() < 1.0);
    let after = buy(&request(&cut(&m, 2792), true));
    assert_eq!(after["stage"], "triggered");
    assert_eq!(after["leaf"]["confirmed_at"], "2026-01-06T22:32:00");
    assert_eq!(before["key"], after["key"]);
}

/// Retest touches `zg`: `zg` raised to the pullback low.
#[test]
fn a_retest_touching_zg_stands_no_nest() {
    let tail = [(99.5, 20)];
    let above = buy(&request(&buy_path(98.47, pullback(98.5, 10), &tail), false));
    assert_eq!(
        (&above["stage"], &above["centre"]["zg"]),
        (&json!("turned"), &json!(98.48))
    );
    let touch = buy(&request(&buy_path(98.48, pullback(98.5, 10), &tail), false));
    assert_eq!(touch, Value::Null);
}

/// Second retest: a completed 30m up stroke before the low. Without the
/// lower low the first retest's low stands and the nest is armed.
#[test]
fn a_second_retest_stands_no_nest() {
    let mut legs = parent(95.0);
    legs.extend([(99.0, 240), (102.0, 240)]);
    let mut second = legs.clone();
    second.extend(pullback(98.5, 10));
    second.push((99.5, 20));
    assert_eq!(buy(&request(&path(100.0, &second), false)), Value::Null);
    let mut first = legs;
    first.extend([
        (99.5, 40),
        (101.0, 40),
        (99.9, 40),
        (100.6, 40),
        (99.1, 40),
        (99.6, 20),
    ]);
    let armed = buy(&request(&path(100.0, &first), false));
    assert_eq!(armed["stage"], "armed");
    assert_eq!(armed["pullback"]["low"], 98.99);
}

/// Already beyond `H_dep`: a later high equal to the departure's high.
#[test]
fn a_move_back_to_the_departure_high_ends_the_nest() {
    let below = buy_path(95.0, pullback(98.5, 10), &[(99.5, 20), (103.99, 44)]);
    assert_eq!(stage(&request(&below, false)), "turned");
    let at = buy_path(95.0, pullback(98.5, 10), &[(99.5, 20), (104.0, 44)]);
    assert_eq!(buy(&request(&at, false)), Value::Null);
}

/// No centre inside the span: a three-stroke pullback.
#[test]
fn a_pullback_without_a_centre_stays_armed() {
    let pull = vec![
        (99.0, 40),
        (101.5, 40),
        (99.2, 12),
        (99.9, 8),
        (99.4, 8),
        (99.8, 8),
        (98.5, 10),
    ];
    let three = buy(&request(&buy_path(95.0, pull, &[(99.5, 20)]), false));
    assert_eq!(
        (&three["stage"], &three["mid"]),
        (&json!("armed"), &Value::Null)
    );
    assert_eq!(three["pullback"]["low"], 98.49);
}

/// No new extreme: the last segment's low equals the entry's low, so the
/// first bar holding the span's low is the entry's.
#[test]
fn an_equal_low_is_no_new_extreme() {
    let tail = [(99.5, 20)];
    let lower = buy(&request(&buy_path(95.0, pullback(98.99, 10), &tail), false));
    assert_eq!(
        (&lower["stage"], &lower["pullback"]["low"]),
        (&json!("turned"), &json!(98.98))
    );
    let equal = buy(&request(&buy_path(95.0, pullback(99.0, 10), &tail), false));
    assert_eq!(equal["stage"], "armed");
    assert_eq!(equal["pullback"]["low_at"], "2026-01-06T19:40:00");
}

/// Area not smaller: the same last leg to 96, over 30 or 40 1m bars. While
/// the slow leg still falls it is primed; once the 5m move turns up the
/// latest element runs up and the nest is armed with its local centre.
#[test]
fn a_last_segment_without_divergence_stays_armed() {
    let tail = [(97.0, 20)];
    let fast = buy(&request(&buy_path(95.0, pullback(96.0, 30), &tail), false));
    assert_eq!(fast["stage"], "turned");
    assert!(fast["mid"]["area_ratio"].as_f64().unwrap() > 0.9);
    let slow = buy_path(95.0, pullback(96.0, 40), &tail);
    let falling = buy(&request(&cut(&slow, 2820), false));
    assert_eq!(falling["stage"], "primed");
    assert_eq!(falling["mid"]["new_extreme"], true);
    assert!(falling["mid"]["area_ratio"].as_f64().unwrap() > 1.0);
    assert_eq!(falling["mid"]["confirmed_at"], Value::Null);
    let up = buy(&request(&slow, false));
    assert_eq!(up["stage"], "armed");
    assert_eq!(up["pullback"]["low"], 95.99);
    let mid = &up["mid"];
    assert_eq!(
        mid["centre"]["start_at"],
        falling["mid"]["centre"]["start_at"]
    );
    assert_eq!(
        (&mid["last"], &mid["new_extreme"], &mid["area_ratio"]),
        (&Value::Null, &json!(false), &Value::Null)
    );
}

/// Primed: the latest 5m element leaves the local centre downward before
/// the new low (bar 2760). Without the local centre (a three-stroke
/// pullback, cut on its last down leg) the nest stays armed with no `mid`.
#[test]
fn a_departure_from_the_local_centre_primes_the_nest() {
    let primed = buy(&request(&cut(&base(), 2760), false));
    assert_eq!(primed["stage"], "primed");
    assert_eq!(primed["pullback"]["low"], 98.99);
    let mid = &primed["mid"];
    assert_eq!(mid["centre"]["start_at"], "2026-01-06T19:40:00");
    assert_eq!(
        (&mid["last"]["start_at"], &mid["last"]["extending"]),
        (&json!("2026-01-06T21:40:00"), &json!(true))
    );
    assert_eq!(
        (&mid["new_extreme"], &mid["confirmed_at"]),
        (&json!(false), &Value::Null)
    );
    let ratio = mid["last"]["area"].as_f64().unwrap() / mid["entry"]["area"].as_f64().unwrap();
    assert_eq!(mid["area_ratio"].as_f64().unwrap(), ratio);
    let pull = vec![
        (99.0, 40),
        (101.5, 40),
        (99.2, 12),
        (99.9, 8),
        (99.4, 8),
        (99.8, 8),
        (98.5, 10),
    ];
    let three = buy(&request(&cut(&buy_path(95.0, pull, &[]), 2705), false));
    assert_eq!(
        (&three["stage"], &three["mid"]),
        (&json!("armed"), &Value::Null)
    );
}

/// The sell series: a 30m upswing from 92 whose 5m move (up to 97, 95,
/// `m3`, 95.5 or 96) ends in a last up segment holding a 1m five-stroke
/// move to `high`, then `tail`.
fn sell_path(
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

fn sell_base(high: (f64, usize), tail: &[(f64, usize)]) -> Vec<RawBar> {
    sell_path((96.5, 40), 95.5, [97.3, 96.6, 97.1, 96.7], high, tail)
}

fn sell(request: &Value) -> Value {
    run(Command::Signals, request)["sell"].clone()
}

#[test]
fn the_sell_nest_mirrors_the_buy_side() {
    let m = sell_base((98.0, 10), &[(97.3, 20)]);
    let got = run(Command::Signals, &request(&m, true));
    let s = &got["sell"];
    assert_eq!(s["stage"], "triggered");
    assert_eq!(s["key"], "2026-01-06T01:40:00|2026-01-06T02:30:00");
    assert_eq!(s["segment"]["high"], 98.01);
    assert_eq!(s["leaf"]["centre"]["start_at"], "2026-01-06T01:52:00");
    assert!(s["leaf"]["area_ratio"].as_f64().unwrap() < 1.0);
    assert!(s["leaf"]["confirmed_at"].is_string());
    // The 30m upswing from 92 completes on 5m too; the last 5m up segment
    // makes a higher high than the previous one without less area.
    assert_eq!(
        (
            &s["parent_complete"],
            &s["mid_divergent"],
            &s["mid_lower_high"]
        ),
        (&json!(true), &json!(false), &json!(false))
    );
    assert_eq!(
        got["mid_support"],
        json!({"low": 95.49, "low_at": "2026-01-06T01:40:00"})
    );
    assert_eq!(got["buy"], Value::Null);
    // No 30m centre: nothing to compare the upswing with.
    assert_eq!(s["parent_vs_departure"], Value::Null);
    // Without the leaf level the 5m top at the high turns it, same key.
    let two = sell(&request(&m, false));
    assert_eq!(
        (&two["stage"], &two["key"], &two["leaf"]),
        (&json!("turned"), &s["key"], &Value::Null)
    );
    assert_eq!(two["segment"], s["segment"]);
}

/// Turned: the 5m top at the high confirms on the 5m bar after the one
/// holding it (bar 1600, not 1595); with the leaf's completion the same key
/// is triggered.
#[test]
fn a_confirmed_mid_top_turns_the_sell_nest() {
    let m = sell_base((98.0, 14), &[(97.3, 20)]);
    assert_eq!(sell(&request(&cut(&m, 1595), false)), Value::Null);
    let turned = sell(&request(&cut(&m, 1600), false));
    assert_eq!(turned["stage"], "turned");
    assert_eq!(turned["key"], "2026-01-06T01:40:00|2026-01-06T02:30:00");
    let triggered = sell(&request(&cut(&m, 1600), true));
    assert_eq!(triggered["stage"], "triggered");
    assert_eq!(triggered["key"], turned["key"]);
    assert!(triggered["leaf"]["confirmed_at"].is_string());
}

/// The 30m upswing after the retest against the departure that left the
/// 30m centre (to 104.01): a new high or not. A long flat start readies
/// the 30m MACD.
#[test]
fn the_parent_upswing_against_its_departure() {
    let versus = |high: f64| {
        let mut legs = vec![(100.0, 3000)];
        legs.extend(parent(95.0).into_iter().skip(1));
        legs.extend(pullback(98.5, 10));
        legs.extend([(high, 240), (high - 1.0, 60)]);
        let s = sell(&request(&path(100.0, &legs), false));
        assert_eq!(s["stage"], "turned");
        s["parent_vs_departure"].clone()
    };
    let over = versus(106.0);
    assert_eq!(over["new_high"], true);
    assert!(over["area_ratio"].as_f64().unwrap() > 0.0);
    let under = versus(103.0);
    assert_eq!(under["new_high"], false);
    assert!(under["area_ratio"].as_f64().unwrap() > 0.0);
}

#[test]
fn the_sell_nest_attributes() {
    let inner = [97.5, 97.0, 97.4, 97.1];
    // A steep previous up stroke: the last one is higher on less area.
    let m = sell_path((97.8, 25), 96.0, inner, (98.0, 10), &[(97.3, 24)]);
    let s = sell(&request(&m, true));
    assert_eq!(
        (
            &s["mid_divergent"],
            &s["mid_lower_high"],
            &s["parent_complete"]
        ),
        (&json!(true), &json!(false), &json!(true))
    );
    // A previous up stroke to 98.5: a lower high, and the 30m upswing's
    // extreme is that stroke, so it has not completed on 5m.
    let m = sell_path((98.5, 25), 96.0, inner, (98.0, 10), &[(97.3, 24)]);
    let s = sell(&request(&m, true));
    assert_eq!(
        (
            &s["mid_divergent"],
            &s["mid_lower_high"],
            &s["parent_complete"]
        ),
        (&json!(false), &json!(true), &json!(false))
    );
}

/// Mirrored negatives: no centre, no new high, area not smaller, top not
/// confirmed.
#[test]
fn sell_side_conditions_each_matter() {
    let tail = [(97.3, 20)];
    let straight = sell_path(
        (96.5, 40),
        95.5,
        [96.0, 96.0, 96.0, 96.0],
        (98.0, 10),
        &tail,
    );
    // Without the leaf's completion the 5m top still turns the nest.
    let leafless = |s: Value| {
        assert_eq!((&s["stage"], &s["leaf"]), (&json!("turned"), &Value::Null));
    };
    leafless(sell(&request(&straight, true)));
    assert_eq!(
        sell(&request(&sell_base((97.31, 10), &[(96.9, 20)]), true))["stage"],
        "triggered"
    );
    leafless(sell(&request(&sell_base((97.3, 10), &[(96.9, 20)]), true)));
    let smaller = sell(&request(&sell_base((98.8, 30), &[(98.1, 20)]), true));
    assert!(smaller["leaf"]["area_ratio"].as_f64().unwrap() > 0.9);
    leafless(sell(&request(&sell_base((98.9, 30), &[(98.2, 20)]), true)));
    // The high on the last 1m bar of its 5m bar (bar 1590).
    let m = sell_base((98.0, 14), &tail);
    assert_eq!(sell(&request(&cut(&m, 1590), true)), Value::Null);
    let s = sell(&request(&cut(&m, 1592), true));
    assert_eq!(s["stage"], "triggered");
}

/// Each level truncated to the bars labelled no later than `t`.
fn truncated(request: &Value, t: &str) -> Value {
    let mut out = request.clone();
    for level in out["levels"].as_array_mut().unwrap() {
        let keep = level["dt"]
            .as_array()
            .unwrap()
            .iter()
            .take_while(|d| d.as_str().unwrap() <= t)
            .count();
        for field in ["dt", "high", "low", "close", "vol"] {
            level[field].as_array_mut().unwrap().truncate(keep);
        }
    }
    out
}

/// The replay at each step equals the stateless call on the series truncated
/// there: the first appearances of every `(side, key, stage)` are the same,
/// with the same objects, and the final answer is the stateless one.
fn replay_equals_truncation(request: &Value, from: usize) {
    let finest = request["levels"].as_array().unwrap().last().unwrap()["dt"]
        .as_array()
        .unwrap()
        .clone();
    let mut expected = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for t in &finest[from..] {
        let t = t.as_str().unwrap();
        let now = run(Command::Signals, &truncated(request, t));
        for side in ["buy", "sell"] {
            let s = &now[side];
            if s.is_null() || s["stage"] == "armed" {
                continue;
            }
            if seen.insert((side, s["key"].clone(), s["stage"].clone())) {
                expected.push(json!({"at": t, "side": side, "signal": s}));
            }
        }
    }
    assert!(!expected.is_empty());
    let mut observed = request.clone();
    observed["observe_from"] = finest[from].clone();
    let got = run(Command::Signals, &observed);
    assert_eq!(got["observed"], Value::Array(expected));
    let stateless = run(Command::Signals, request);
    for field in ["buy", "sell", "mid_support"] {
        assert_eq!(got[field], stateless[field], "{field}");
    }
}

#[test]
fn stateless_truncation_equals_the_replay() {
    // Buy: 2 levels (ripe, turned) and 3 levels (ripe, triggered).
    replay_equals_truncation(
        &request(&buy_path(95.0, pullback(98.5, 14), &[(99.5, 20)]), false),
        500,
    );
    replay_equals_truncation(
        &request(&buy_path(95.0, pullback(98.5, 14), &[(99.5, 20)]), true),
        2700,
    );
    // Sell, 3 levels (triggered) and 2 levels (turned).
    let m = sell_base((98.0, 14), &[(97.3, 20)]);
    replay_equals_truncation(&request(&m, true), 1550);
    replay_equals_truncation(&request(&m, false), 300);
}

#[test]
fn observation_lists_each_stage_once_in_time_order() {
    let mut req = request(&buy_path(95.0, pullback(98.5, 14), &[(99.5, 20)]), true);
    req["observe_from"] = json!("2026-01-06T19:00:00");
    let got = run(Command::Signals, &req);
    let rows: Vec<String> = got["observed"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|o| o["side"] == "buy")
        .map(|o| {
            format!(
                "{} {} {}",
                o["at"], o["signal"]["stage"], o["signal"]["pullback"]["low_at"]
            )
        })
        .collect();
    // The last 5m segment is primed once it leaves the local centre, ripe
    // at its first new low; each lower 1m low inside the next 5m bar is a
    // new key that is only primed until that 5m bar closes at it (ripe),
    // then triggered once the 1m bottom there confirms.
    assert_eq!(
        rows,
        [
            r#""2026-01-06T22:00:00" "primed" "2026-01-06T19:40:00""#,
            r#""2026-01-06T22:25:00" "ripe" "2026-01-06T22:25:00""#,
            r#""2026-01-06T22:26:00" "primed" "2026-01-06T22:26:00""#,
            r#""2026-01-06T22:27:00" "primed" "2026-01-06T22:27:00""#,
            r#""2026-01-06T22:28:00" "primed" "2026-01-06T22:28:00""#,
            r#""2026-01-06T22:29:00" "primed" "2026-01-06T22:29:00""#,
            r#""2026-01-06T22:30:00" "ripe" "2026-01-06T22:30:00""#,
            r#""2026-01-06T22:32:00" "triggered" "2026-01-06T22:30:00""#,
        ]
    );
    // The sell side records its turned rows alongside.
    assert!(
        got["observed"]
            .as_array()
            .unwrap()
            .iter()
            .any(|o| o["side"] == "sell" && o["signal"]["stage"] == "turned")
    );
    // An observation start after the last bar observes nothing.
    req["observe_from"] = json!("2099-01-01T00:00:00");
    assert_eq!(run(Command::Signals, &req)["observed"], json!([]));
}

#[test]
fn signals_requests_are_checked() {
    let m = base();
    let check = |req: &Value, kind: &str| {
        let (text, ok) = handle_command(Command::Signals, &req.to_string());
        let v: Value = serde_json::from_str(&text).unwrap();
        assert!(!ok, "{text}");
        assert_eq!(v["schema"], "chanorbit.signals/1");
        assert_eq!(v["error"]["kind"], kind, "{text}");
    };
    let mut one = request(&m, false);
    one["levels"].as_array_mut().unwrap().truncate(1);
    check(&one, "invalid_request");
    let mut four = request(&m, true);
    let mut extra = four["levels"][2].clone();
    extra["name"] = json!("x");
    four["levels"].as_array_mut().unwrap().push(extra);
    check(&four, "invalid_request");
    let mut bad = request(&m, false);
    bad["observe_from"] = json!("2026-01-06 19:00:00");
    check(&bad, "invalid_request");
    bad["observe_from"] = Value::Null;
    check(&bad, "invalid_request");
    let mut same = request(&m, false);
    same["levels"][1]["name"] = json!("30m");
    check(&same, "invalid_request");
    let mut low = request(&m, false);
    low["levels"][1]["low"][3] = json!(-1.0);
    let (text, _) = handle_command(Command::Signals, &low.to_string());
    assert!(text.contains("levels[1].low[3]"), "{text}");
    check(&json!([1]), "invalid_input");
}

/// The real binary: a batch answers line by line as the library does, a bad
/// line fails alone, and `--jobs` does not change the bytes.
#[test]
fn signals_batch_through_the_binary() {
    let mut observed = request(&base(), true);
    observed["observe_from"] = json!("2026-01-06T22:00:00");
    let lines = [
        request(&base(), false).to_string(),
        r#"{"id":"bad","levels":[]}"#.to_string(),
        observed.to_string(),
        request(&sell_base((98.0, 10), &[(97.3, 20)]), true).to_string(),
    ];
    let input = lines.join("\n") + "\n";
    let exe = env!("CARGO_BIN_EXE_chanorbit");
    let run_with = |jobs: &str| {
        let mut child = std::process::Command::new(exe)
            .args(["signals", "--batch", "--jobs", jobs])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        use std::io::Write;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    };
    let serial = run_with("1");
    assert_eq!(serial.status.code(), Some(1));
    let out: Vec<&str> = std::str::from_utf8(&serial.stdout)
        .unwrap()
        .lines()
        .collect();
    assert_eq!(out.len(), lines.len());
    for (line, got) in lines.iter().zip(&out) {
        assert_eq!(*got, handle_command(Command::Signals, line).0);
    }
    assert!(out[1].contains("invalid_request"));
    assert_eq!(run_with("8").stdout, serial.stdout);
}
