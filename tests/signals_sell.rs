//! `signals` sell-side rules on synthetic multi-level series.
mod nest;

use chanorbit::contract::Command;
use nest::*;
use serde_json::{Value, json};

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
    // No 30m centre: no departure to compare the upswing with.
    assert_eq!(
        (&s["parent_departure"], &s["parent_vs_departure"]),
        (&Value::Null, &Value::Null)
    );
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
/// 30m centre (to 104.01): a new high or not, read against
/// `parent_departure.high`; while the retest has no 30m up element yet, the
/// latest 30m up element is the departure itself and only the departure is
/// output. A long flat start readies the 30m MACD.
#[test]
fn the_parent_upswing_against_its_departure() {
    let after_retest = |tail: &[(f64, usize)]| {
        let mut legs = vec![(100.0, 3000)];
        legs.extend(parent(95.0).into_iter().skip(1));
        legs.extend(pullback(98.5, 10));
        legs.extend_from_slice(tail);
        let s = sell(&request(&path(100.0, &legs), false));
        assert_eq!(s["stage"], "turned");
        assert_eq!(
            s["parent_departure"],
            json!({"start_at": "2026-01-08T02:00:00", "end_at": "2026-01-08T06:00:00", "high": 104.01})
        );
        s["parent_vs_departure"].clone()
    };
    // The 5m segment holds the 30m upswing's high: 106.01, 104.01 (equal
    // to the departure's, not new) and 103.01.
    let over = after_retest(&[(106.0, 240), (105.0, 60)]);
    assert_eq!(over["new_high"], true);
    assert!(over["area_ratio"].as_f64().unwrap() > 0.0);
    let equal = after_retest(&[(104.0, 240), (103.0, 60)]);
    assert_eq!(equal["new_high"], false);
    let under = after_retest(&[(103.0, 240), (102.0, 60)]);
    assert_eq!(under["new_high"], false);
    assert!(under["area_ratio"].as_f64().unwrap() > 0.0);
    // A 5m top inside the retest, to 99.51: no comparison yet.
    assert_eq!(after_retest(&[(99.5, 20), (99.0, 20)]), Value::Null);
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
