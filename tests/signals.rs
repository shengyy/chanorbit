//! `signals` buy-side rules, replay and requests on synthetic multi-level series.
mod nest;

use chanorbit::contract::{Command, handle_command};
use nest::*;
use serde_json::{Value, json};

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
    assert_eq!(b["leaf_centre"], true);
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
}

/// The leaf does not trigger ahead of the middle level: with the 1m move
/// complete and confirmed (22:28) but the 5m down stroke still extending,
/// the nest is ripe; one 5m bar later the 5m bottom confirms (22:35).
#[test]
fn the_leaf_waits_for_the_mid_bottom() {
    let m = base();
    let before = buy(&request(&cut(&m, 2794), true));
    assert_eq!(before["stage"], "ripe");
    assert_eq!(before["mid"]["last"]["extending"], true);
    assert_eq!(before["leaf"]["confirmed_at"], "2026-01-06T22:28:00");
    // C is not evaluated before the 5m bottom confirms.
    assert_eq!(before["leaf_centre"], Value::Null);
    let after = buy(&request(&cut(&m, 2795), true));
    assert_eq!(after["stage"], "triggered");
    assert_eq!(after["leaf_centre"], true);
    assert_eq!(after["mid"]["confirmed_at"], "2026-01-06T22:35:00");
    assert_eq!(before["key"], after["key"]);
}

/// C is a veto: a last 5m segment that is one straight 1m drop holds no
/// leaf centre and triggers; a 1m centre left without divergence (the last
/// 1m leg over 25 instead of 28 bars) holds the nest at `turned`.
#[test]
fn the_leaf_vetoes_only_a_leaf_still_gaining() {
    let sharp = vec![
        (99.0, 40),
        (102.0, 40),
        (100.0, 40),
        (101.5, 40),
        (98.5, 40),
    ];
    let b = buy(&request(&buy_path(95.0, sharp, &[(99.5, 20)]), true));
    assert_eq!(
        (&b["stage"], &b["leaf_centre"], &b["leaf"]),
        (&json!("triggered"), &json!(false), &Value::Null)
    );
    assert!(b["mid"]["area_ratio"].as_f64().unwrap() < 1.0);
    let leaf = |bars: usize| {
        buy(&request(
            &buy_path(95.0, pullback(97.0, bars), &[(98.0, 20)]),
            true,
        ))
    };
    let gaining = leaf(25);
    assert_eq!(
        (&gaining["stage"], &gaining["leaf_centre"], &gaining["leaf"]),
        (&json!("turned"), &json!(true), &Value::Null)
    );
    let diverging = leaf(28);
    assert_eq!(diverging["stage"], "triggered");
    assert!(diverging["leaf"]["area_ratio"].as_f64().unwrap() < 1.0);
    // Without the leaf level C is not evaluated.
    let two = buy(&request(
        &buy_path(95.0, pullback(97.0, 25), &[(98.0, 20)]),
        false,
    ));
    assert_eq!(
        (&two["stage"], &two["leaf_centre"]),
        (&json!("turned"), &Value::Null)
    );
}

/// C reads the leaf only up to the low: a rally after it that builds 1m up
/// centres (below `H_dep`, no new low) leaves the key, `leaf_centre` and
/// the stage as they were, `turned` (leaf still gaining) and `triggered`.
#[test]
fn a_rally_after_the_low_does_not_move_the_leaf_veto() {
    let rally = [
        (98.0, 20),
        (99.0, 8),
        (98.4, 8),
        (99.4, 8),
        (98.8, 8),
        (99.8, 8),
        (99.2, 8),
        (100.5, 8),
        (99.9, 8),
        (101.0, 8),
    ];
    for (bars, want) in [(25, "turned"), (28, "triggered")] {
        let pull = || pullback(97.0, bars);
        let at = buy(&request(&buy_path(95.0, pull(), &rally[..1]), true));
        let later = buy(&request(&buy_path(95.0, pull(), &rally), true));
        assert_eq!(at["stage"], want);
        for field in ["stage", "key", "leaf_centre", "leaf"] {
            assert_eq!(later[field], at[field], "{bars} {field}");
        }
    }
}

/// A nest capped at `armed` by A4 or A5: the key and A facts stand, the
/// finer levels are not read.
fn assert_capped(b: &Value) {
    assert_eq!(b["stage"], "armed", "{b}");
    for field in ["mid", "leaf", "leaf_centre"] {
        assert_eq!(b[field], Value::Null, "{field}");
    }
    assert!(b["key"].is_string() && b["pullback"]["low"].is_number());
}

/// A5 over 30m legs of 240 bars from the flat start: an earlier 30m centre
/// built from `head`, then `Z` (members from 90: 96.5, 92, 95, 93) left
/// upward to 104, and a pullback to `low` (the base one: 98.49).
fn earlier_centre_to(head: &[f64], low: f64, bars: usize) -> Value {
    let mut legs = vec![(100.0, 900)];
    legs.extend(head.iter().map(|&p| (p, 240)));
    legs.extend([90.0, 96.5, 92.0, 95.0, 93.0, 104.0].map(|p| (p, 240)));
    legs.extend(pullback(low, bars));
    legs.push((low + 1.0, 20));
    buy(&request(&path(100.0, &legs), false))
}

fn earlier_centre(head: &[f64]) -> Value {
    earlier_centre_to(head, 98.5, 10)
}

#[test]
fn a_low_back_in_an_earlier_parent_core_caps_the_nest() {
    // The previous centre's core [97.49, 99.51] holds the low.
    let inside = [103.0, 97.0, 100.0, 97.5, 99.5];
    assert_capped(&earlier_centre(&inside));
    // A lower low below that core's `zd` (96.99, still above `zg` 95.01):
    // A5 passes and the new key reaches `turned`.
    let lower = earlier_centre_to(&inside, 97.0, 25);
    assert_eq!(
        (&lower["stage"], &lower["pullback"]["low"]),
        (&json!("turned"), &json!(96.99))
    );
    // Its core [98.59, 99.51] lies above the low.
    let above = earlier_centre(&[103.0, 98.6, 100.0, 98.6, 99.5]);
    assert_eq!(
        (&above["stage"], &above["pullback"]["low"]),
        (&json!("turned"), &json!(98.49))
    );
    // `zg` at the low (98.49) versus one tick below it (98.48).
    assert_capped(&earlier_centre(&[103.0, 97.0, 100.0, 97.5, 98.48]));
    assert_eq!(
        earlier_centre(&[103.0, 97.0, 100.0, 97.5, 98.47])["stage"],
        "turned"
    );
    // Two centres before `Z`: [97.49, 99.51] holds the low, [99.09, 99.51]
    // does not; the centre between them ([84.99, 88.01]) is below.
    let two = |a: f64| {
        let head = [103.0, a, 100.0, a + 0.5, 99.5, 84.0, 88.5, 85.0, 88.0, 86.0];
        let mut legs = vec![(100.0, 900)];
        legs.extend(head.iter().map(|&p| (p, 240)));
        legs.extend([96.5, 92.0, 95.0, 93.0, 104.0].map(|p| (p, 240)));
        legs.extend(pullback(98.5, 10));
        legs.push((99.5, 20));
        buy(&request(&path(100.0, &legs), false))
    };
    assert_capped(&two(97.0));
    let below = two(98.6);
    assert_eq!(below["stage"], "turned");
    assert_eq!(below["centre"]["zd"], 92.99);
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

/// Already beyond `H_dep`: a later high equal to the departure's high caps
/// the nest at `armed`, with 2 and 3 levels.
#[test]
fn a_move_back_to_the_departure_high_caps_the_nest() {
    let below = buy_path(95.0, pullback(98.5, 10), &[(99.5, 20), (103.99, 44)]);
    assert_eq!(stage(&request(&below, false)), "turned");
    let at = buy_path(95.0, pullback(98.5, 10), &[(99.5, 20), (104.0, 44)]);
    for leaf in [false, true] {
        assert_capped(&buy(&request(&at, leaf)));
    }
}

/// A4 heals with a new low: after the high back at `H_dep` the pullback
/// falls to 96.99, a new key that reaches `turned`.
#[test]
fn a_new_low_after_the_departure_high_heals_the_nest() {
    let tail = [(99.5, 20), (104.0, 44), (97.0, 60), (97.8, 20)];
    let m = buy_path(95.0, pullback(98.5, 10), &tail);
    let capped = buy(&request(&cut(&m, 2850), false));
    assert_capped(&capped);
    let healed = buy(&request(&m, false));
    assert_eq!(
        (&healed["stage"], &healed["pullback"]["low"]),
        (&json!("turned"), &json!(96.99))
    );
    assert_ne!(healed["key"], capped["key"]);
    assert_eq!(healed["centre"], capped["centre"]);
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

/// `departure.entry_ratio` is the departure's area over the up entry
/// stroke's, and `null` when the centre is entered from above.
#[test]
fn the_departure_is_compared_with_an_up_entry() {
    for (high, bars, weaker) in [(104.0, 240, true), (110.0, 480, false)] {
        let b = buy(&request(&entry_up_path(high, bars), false));
        assert_eq!(b["stage"], "turned");
        assert_eq!(b["centre"]["zg"], 95.01);
        let d = &b["departure"];
        let ratio = d["entry_ratio"].as_f64().unwrap();
        assert!(d["area"].as_f64().unwrap() > 0.0);
        assert_eq!(ratio < 1.0, weaker, "{ratio}");
    }
    // Entered by a down stroke: the departure has an area, the ratio none.
    let b = buy(&request(&entry_down_path(), false));
    assert_eq!(b["stage"], "turned");
    assert!(b["departure"]["area"].as_f64().unwrap() > 0.0);
    assert_eq!(b["departure"]["entry_ratio"], Value::Null);
    // Without 30m MACD neither is available.
    let b = buy(&request(&base(), false));
    assert_eq!(
        (&b["departure"]["area"], &b["departure"]["entry_ratio"]),
        (&Value::Null, &Value::Null)
    );
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
    // Buy capped by A4, then healed by a new low.
    replay_equals_truncation(
        &request(
            &buy_path(
                95.0,
                pullback(98.5, 10),
                &[(99.5, 20), (104.0, 44), (97.0, 60), (97.8, 20)],
            ),
            false,
        ),
        500,
    );
    // An up entry: the ratio at each step is read from the strokes then.
    let m = entry_up_path(104.0, 240);
    replay_equals_truncation(&request(&m, false), m.len() / 5 - 60);
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
    // then triggered once the 5m bottom there confirms (the 1m one already
    // has, at 22:32).
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
            r#""2026-01-06T22:40:00" "triggered" "2026-01-06T22:30:00""#,
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

/// A reviewer's synthetic series: a 5m pullback whose last segment ends in
/// a 3-bar 1m drop to 96.09 at 22:00, the 5m bottom confirmed at 22:10, and
/// 20 rising 1m bars after the low.
fn veto_series() -> Value {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/nest/veto-full.json.gz");
    let mut text = String::new();
    use std::io::Read;
    flate2::read::GzDecoder::new(std::fs::File::open(path).unwrap())
        .read_to_string(&mut text)
        .unwrap();
    serde_json::from_str(&text).unwrap()
}

/// The 1m move into the low is still the unfinished segment, with the 1m
/// level cut at the low (30m and 5m whole, as the reviewer sent it) and 20
/// bars later: C waits, so the nest stays `turned` with no `leaf_centre`
/// instead of reading "no centre" from an unsettled leaf.
#[test]
fn the_leaf_veto_waits_for_the_low_stroke() {
    let full = veto_series();
    let mut cut = full.clone();
    for field in ["dt", "high", "low", "close", "vol"] {
        cut["levels"][2][field]
            .as_array_mut()
            .unwrap()
            .truncate(2760);
    }
    assert_eq!(cut["levels"][2]["dt"][2759], "2026-01-06T22:00:00");
    for request in [cut, full] {
        let b = buy(&request);
        assert_eq!(
            b["key"],
            "2026-01-05T23:00:00|2026-01-06T19:00:00|2026-01-06T22:00:00"
        );
        assert_eq!(b["mid"]["confirmed_at"], "2026-01-06T22:10:00");
        assert_eq!(
            (&b["stage"], &b["leaf_centre"], &b["leaf"]),
            (&json!("turned"), &Value::Null, &Value::Null)
        );
    }
}

fn rank(stage: &Value) -> usize {
    ["armed", "primed", "ripe", "turned", "triggered"]
        .iter()
        .position(|s| stage == s)
        .unwrap()
}

/// Over every truncation from `from` on: within a key the stage only moves
/// forward, and once C has been evaluated `leaf_centre` keeps its value.
fn veto_is_stable(request: &Value, from: usize) -> usize {
    let finest = request["levels"][2]["dt"].as_array().unwrap().clone();
    let mut seen: std::collections::HashMap<String, (usize, Value)> = Default::default();
    let mut evaluated = 0;
    for t in &finest[from..] {
        let b = buy(&truncated(request, t.as_str().unwrap()));
        if b.is_null() {
            continue;
        }
        let key = b["key"].as_str().unwrap().to_string();
        let (r, centre) = (rank(&b["stage"]), b["leaf_centre"].clone());
        if let Some((r0, c0)) = seen.get(&key) {
            assert!(r >= *r0, "{t}: {key} went back to {}", b["stage"]);
            if !c0.is_null() {
                assert_eq!(&centre, c0, "{t}: {key}");
            }
        }
        evaluated += usize::from(!centre.is_null());
        let keep = if centre.is_null() {
            seen.get(&key).map_or(Value::Null, |x| x.1.clone())
        } else {
            centre
        };
        seen.insert(key, (r, keep));
    }
    evaluated
}

#[test]
fn the_leaf_veto_never_changes_within_a_key() {
    let rally = [
        (98.0, 20),
        (99.0, 8),
        (98.4, 8),
        (99.4, 8),
        (98.8, 8),
        (99.8, 8),
        (99.2, 8),
        (100.5, 8),
        (99.9, 8),
        (101.0, 8),
    ];
    let sharp = vec![
        (99.0, 40),
        (102.0, 40),
        (100.0, 40),
        (101.5, 40),
        (98.5, 40),
    ];
    assert_eq!(veto_is_stable(&veto_series(), 2740), 0);
    let mut evaluated = 0;
    for m in [
        base(),
        buy_path(95.0, pullback(97.0, 25), &rally),
        buy_path(95.0, pullback(97.0, 28), &rally),
        buy_path(95.0, sharp, &rally),
    ] {
        evaluated += veto_is_stable(&request(&m, true), 2760);
    }
    assert!(evaluated > 0);
}
