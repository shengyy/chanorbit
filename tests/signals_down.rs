//! `signals` down-end nest (`down_end`) on synthetic multi-level series.
mod nest;

use chanorbit::RawBar;
use chanorbit::contract::Command;
use nest::*;
use serde_json::{Value, json};

/// The 1m bars before the parent's departure `D` (from 97, bar 5700, the
/// close of a 30m bar).
const D_AT: usize = 5700;

/// `D` inside: a 5m entry to 89.5, a 5m centre (92, 90.3, 91.3) and a last
/// segment holding a 1m five-stroke move to 87, its low at bar 5910 (the
/// close of a 30m bar).
const FAST: [(f64, usize); 9] = [
    (89.5, 40),
    (92.0, 40),
    (90.3, 40),
    (91.3, 40),
    (89.0, 12),
    (89.7, 8),
    (89.2, 8),
    (89.6, 8),
    (87.0, 14),
];

/// As `FAST` with a slow first leg of the last segment that stays above the
/// entry's low (89.49) while it leaves the 5m centre, low 87.49 at bar 5940.
const SLOW: [(f64, usize); 9] = [
    (89.5, 40),
    (92.0, 40),
    (90.3, 40),
    (91.3, 40),
    (89.6, 40),
    (90.2, 8),
    (89.8, 8),
    (90.1, 8),
    (87.5, 16),
];

/// From a flat start long enough for 30m MACD: `head` (ending at the top of
/// the entry stroke), the entry stroke `E` down to 90.4, a 30m centre `Z`
/// (members 90.4 → 98, 95, 97: `zd` 94.99, `zg` 97.01, `dd` 90.39), the
/// departure `d` from 97 and `tail`. Legs of 240 1m bars are 8 30m bars.
fn down_path(head: &[(f64, usize)], d: &[(f64, usize)], tail: &[(f64, usize)]) -> Vec<RawBar> {
    let mut legs = vec![(95.0, 4500)];
    legs.extend_from_slice(head);
    legs.extend([(90.4, 240), (98.0, 240), (95.0, 240), (97.0, 240)]);
    legs.extend_from_slice(d);
    legs.extend_from_slice(tail);
    path(95.0, &legs)
}

const TOP: [(f64, usize); 1] = [(110.0, 240)];

fn fast() -> Vec<RawBar> {
    down_path(&TOP, &FAST, &[(88.0, 20)])
}

fn slow() -> Vec<RawBar> {
    down_path(&TOP, &SLOW, &[(88.0, 20)])
}

fn down(request: &Value) -> Value {
    run(Command::Signals, request)["down_end"].clone()
}

/// The parent level alone through `levels`: its latest centre's state and
/// in-progress comparisons.
fn parent_levels(m: &[RawBar]) -> Value {
    let mut req = request(m, false);
    req["levels"].as_array_mut().unwrap().truncate(1);
    run(Command::Levels, &req)["levels"][0].clone()
}

fn departure_vs_entry(level: &Value) -> f64 {
    level["comparisons"]["in_progress"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["pair"] == "departure_vs_entry")
        .unwrap()["area"]
        .as_f64()
        .unwrap()
}

#[test]
fn every_stage_of_the_down_end() {
    let m = slow();
    // The 30m centre is left downward by the unfinished segment to 89.49.
    let armed = down(&request(&cut(&m, 5820), false));
    assert_eq!(armed["stage"], "armed");
    assert_eq!(armed["mid"], Value::Null);
    assert_eq!(
        armed["key"],
        "2026-01-08T11:00:00|2026-01-08T23:00:00|2026-01-08T23:40:00"
    );
    assert_eq!(
        armed["centre"],
        json!({"start_at": "2026-01-08T11:00:00", "zd": 94.99, "zg": 97.01, "dd": 90.39, "gg": 98.01, "member_count": 3})
    );
    assert_eq!(armed["trend_centres"], 1);
    let d = &armed["departure"];
    assert_eq!(
        (&d["start_at"], &d["low"], &d["extending"]),
        (&json!("2026-01-08T23:00:00"), &json!(89.49), &json!(true))
    );
    assert_eq!(
        armed["low"],
        json!({"price": 89.49, "at": "2026-01-08T23:40:00"})
    );
    // The 5m centre built after the low is not part of the move to it:
    // the 5m reading stops at the low, which has no local centre before it.
    let later = down(&request(&cut(&m, 5880), false));
    assert_eq!(
        (&later["stage"], &later["key"]),
        (&json!("armed"), &armed["key"])
    );
    assert_eq!(later["mid"], Value::Null);
    // With 1m: a new 1m low inside a 5m bar that has not closed (and a 30m
    // bar, so below the departure's low); the 5m move to it leaves the 5m
    // centre to the new low without having completed yet.
    let primed = down(&request(&cut(&m, 5931), true));
    assert_eq!(primed["stage"], "primed");
    assert_eq!(
        (&primed["low"]["price"], &primed["departure"]["low"]),
        (&json!(88.95), &json!(89.49))
    );
    assert_eq!(primed["mid"]["new_extreme"], true);
    assert!(primed["mid"]["last"].is_object());
    // The 30m bar holding the new low has closed: a new key, complete on 5m.
    let ripe = down(&request(&cut(&m, 5940), false));
    assert_eq!(ripe["stage"], "ripe");
    assert_eq!(
        ripe["key"],
        "2026-01-08T11:00:00|2026-01-08T23:00:00|2026-01-09T03:00:00"
    );
    assert_eq!(ripe["mid"]["confirmed_at"], Value::Null);
    // The next 5m bar confirms the 5m bottom.
    let turned = down(&request(&cut(&m, 5950), false));
    assert_eq!(turned["stage"], "turned");
    assert_eq!(turned["key"], ripe["key"]);
    assert!(turned["mid"]["confirmed_at"].is_string());
    assert_eq!(turned["leaf_centre"], Value::Null);
    // With 1m: the leaf centre is left without divergence, still `turned`.
    let gaining = down(&request(&cut(&m, 5950), true));
    assert_eq!(
        (&gaining["stage"], &gaining["leaf_centre"], &gaining["leaf"]),
        (&json!("turned"), &json!(true), &Value::Null)
    );
    // The fast series diverges on 1m too: `triggered`.
    let m = fast();
    let full = down(&request(&m, true));
    assert_eq!(full["stage"], "triggered");
    assert_eq!(full["leaf_centre"], true);
    assert!(full["leaf"]["area_ratio"].as_f64().unwrap() < 1.0);
    assert_eq!(
        full["low"],
        json!({"price": 86.99, "at": "2026-01-09T02:30:00"})
    );
    // The departure's area ratio is the parent's `departure_vs_entry`.
    let level = parent_levels(&m);
    let state = &level["centres"].as_array().unwrap().last().unwrap()["state"];
    assert_eq!(
        (&state["departure"], &state["edge"], &state["new_extreme"]),
        (&json!("unfinished"), &json!("zd"), &json!(true))
    );
    let ratio = full["departure"]["area_ratio"].as_f64().unwrap();
    assert_eq!(ratio.to_bits(), departure_vs_entry(&level).to_bits());
    assert!(ratio < 1.0);
    // A straight 1m drop holds no leaf centre and triggers (bar 5880).
    let straight = down(&request(&cut(&m, 5880), true));
    assert_eq!(
        (&straight["stage"], &straight["leaf_centre"]),
        (&json!("triggered"), &json!(false))
    );
    // `buy` and `sell` do not stand on this series.
    let all = run(Command::Signals, &request(&m, true));
    assert_eq!((&all["buy"], &all["sell"]), (&Value::Null, &Value::Null));
}

/// A finest low below the parent's, inside a 30m bar that has not closed,
/// belongs to the departure: it is a new key at once, below
/// `departure.low` until that bar closes, with the same centre and
/// departure.
#[test]
fn a_new_low_is_a_new_key() {
    let m = fast();
    let before = down(&request(&cut(&m, 5880), false));
    assert_eq!(
        before["key"],
        "2026-01-08T11:00:00|2026-01-08T23:00:00|2026-01-09T01:55:00"
    );
    let inside = down(&request(&cut(&m, 5905), false));
    assert_eq!(
        inside["key"],
        "2026-01-08T11:00:00|2026-01-08T23:00:00|2026-01-09T02:25:00"
    );
    assert_eq!(
        (
            &inside["stage"],
            &inside["low"]["price"],
            &inside["departure"]["low"]
        ),
        (&json!("ripe"), &json!(87.92), &json!(88.99))
    );
    let after = down(&request(&cut(&m, 5910), false));
    assert_eq!(
        after["key"],
        "2026-01-08T11:00:00|2026-01-08T23:00:00|2026-01-09T02:30:00"
    );
    assert_eq!(after["centre"], before["centre"]);
    assert_eq!(
        after["departure"]["start_at"],
        before["departure"]["start_at"]
    );
    assert_eq!(
        (&after["low"]["price"], &after["departure"]["low"]),
        (&json!(86.99), &json!(86.99))
    );
}

fn rank(stage: &Value) -> usize {
    ["armed", "primed", "ripe", "turned", "triggered"]
        .iter()
        .position(|s| stage == s)
        .unwrap()
}

/// Over every truncation from `from` on, the stage of a key never moves
/// back; returns the highest rank reached.
fn never_steps_back(request: &Value, from: usize) -> usize {
    let finest = request["levels"].as_array().unwrap().last().unwrap()["dt"]
        .as_array()
        .unwrap()
        .clone();
    let mut seen = std::collections::HashMap::new();
    let mut top = 0;
    for t in &finest[from..] {
        let d = down(&truncated(request, t.as_str().unwrap()));
        if d.is_null() {
            continue;
        }
        let r = rank(&d["stage"]);
        let r0 = seen.entry(d["key"].to_string()).or_insert(r);
        assert!(r >= *r0, "{t}: {} back to {}", d["key"], d["stage"]);
        *r0 = r;
        top = top.max(r);
    }
    top
}

/// The 5m reading stops at the low: a rally after it that builds 5m
/// structure while the 30m rebound is still uncounted (bar 6170, where the
/// open span went back to `primed`) leaves the key at `turned`, and no key
/// of these series steps back.
#[test]
fn the_stage_of_a_key_never_steps_back() {
    let m = down_path(&TOP, &FAST, &[(93.0, 240), (90.0, 240)]);
    let rally = down(&request(&cut(&m, 6170), false));
    assert_eq!(
        (&rally["stage"], &rally["low"]["price"]),
        (&json!("turned"), &json!(86.99))
    );
    assert_eq!(never_steps_back(&request(&m, false), D_AT / 5), 3);
    assert_eq!(
        never_steps_back(&request(&no_mid_divergence(), false), D_AT / 5),
        2
    );
    assert_eq!(never_steps_back(&request(&slow(), true), D_AT + 200), 3);
}

/// A1: a departure to the centre's lowest low (90.39, `new_extreme` false)
/// versus one tick below it.
#[test]
fn an_equal_low_is_no_down_end() {
    let tail = [(91.5, 60)];
    let equal = down(&request(&down_path(&TOP, &[(90.4, 120)], &tail), false));
    assert_eq!(equal, Value::Null);
    let lower = down(&request(&down_path(&TOP, &[(90.39, 120)], &tail), false));
    assert_eq!(
        (&lower["stage"], &lower["low"]["price"]),
        (&json!("armed"), &json!(90.38))
    );
}

/// A2: the same departure after an entry from 98.5 instead of 110 has a
/// larger area than the entry.
#[test]
fn a_departure_without_divergence_is_no_down_end() {
    let weak = down_path(&[(98.5, 240)], &FAST, &[(88.0, 20)]);
    let level = parent_levels(&weak);
    let state = &level["centres"].as_array().unwrap().last().unwrap()["state"];
    assert_eq!(
        (&state["kind"], &state["edge"], &state["new_extreme"]),
        (&json!("departed"), &json!("zd"), &json!(true))
    );
    assert!(departure_vs_entry(&level) > 1.0);
    for leaf in [false, true] {
        assert_eq!(down(&request(&weak, leaf)), Value::Null);
    }
    assert!(departure_vs_entry(&parent_levels(&fast())) < 1.0);
}

/// A3: a 30m rebound to 93 is still uncounted at its top (the departure a
/// member stroke by then); once a decline after it counts it, the nest is
/// gone.
#[test]
fn a_counted_rebound_ends_the_down_end() {
    let m = down_path(&TOP, &FAST, &[(93.0, 240), (90.0, 240)]);
    let top = cut(&m, 5910 + 240);
    let at_top = down(&request(&top, false));
    assert_eq!(at_top["low"]["price"], 86.99);
    // The departure is now a stroke that no longer moves, a member of `Z`.
    assert_eq!(at_top["departure"]["extending"], false);
    let level = parent_levels(&top);
    let state = &level["centres"].as_array().unwrap().last().unwrap()["state"];
    assert_eq!(
        (&state["member"], &state["edge"]),
        (&json!(true), &json!("zd"))
    );
    for leaf in [false, true] {
        assert_eq!(down(&request(&m, leaf)), Value::Null);
    }
    let level = parent_levels(&m);
    let strokes = level["strokes"].as_array().unwrap();
    let up = strokes.last().unwrap();
    assert_eq!(
        (&up["direction"], &up["end_price"], &up["finished"]),
        (&json!("向上"), &json!(93.01), &json!(true))
    );
}

/// A4: a rebound inside the 30m bar after the low (the parent has not seen
/// it) to one tick below `zd` versus at `zd`; a rebound into the core that
/// the parent has seen.
#[test]
fn a_rebound_to_zd_ends_the_down_end() {
    for leaf in [false, true] {
        let below = down(&request(&down_path(&TOP, &FAST, &[(94.97, 15)]), leaf));
        assert_eq!(below["low"]["price"], 86.99);
        let at = down(&request(&down_path(&TOP, &FAST, &[(94.98, 15)]), leaf));
        assert_eq!(at, Value::Null);
        let back = down(&request(&down_path(&TOP, &FAST, &[(95.5, 30)]), leaf));
        assert_eq!(back, Value::Null);
    }
}

/// `trend_centres`: an earlier 30m centre (core 104.99–109.01) above `Z`.
#[test]
fn trend_centres_count_cores_stepping_down() {
    assert_eq!(down(&request(&fast(), false))["trend_centres"], 1);
    let head = [
        (112.0, 240),
        (104.0, 240),
        (110.0, 240),
        (105.0, 240),
        (109.0, 240),
    ];
    let trend = down(&request(&down_path(&head, &FAST, &[(88.0, 20)]), true));
    assert_eq!(
        (&trend["stage"], &trend["trend_centres"]),
        (&json!("triggered"), &json!(2))
    );
    assert_eq!(trend["centre"]["zd"], 94.99);
}

/// The replay records `down_end` rows as the stateless calls on each
/// truncation do, between `buy` and `sell` within a step; on the slow
/// series each 5m low below the departure's low inside the open 30m bar is a
/// `ripe` key, then the low's `turned`.
#[test]
fn down_end_replay_equals_truncation() {
    replay_equals_truncation(&request(&fast(), false), D_AT / 5);
    replay_equals_truncation(&request(&fast(), true), D_AT + 100);
    replay_equals_truncation(&request(&slow(), false), D_AT / 5);
    let mut req = request(&slow(), false);
    req["observe_from"] = json!("2026-01-08T23:00:00");
    let rows: Vec<String> = run(Command::Signals, &req)["observed"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|o| o["side"] == "down_end")
        .map(|o| {
            format!(
                "{} {} {} {}",
                o["at"], o["side"], o["signal"]["stage"], o["signal"]["low"]["at"]
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            r#""2026-01-09T02:50:00" "down_end" "ripe" "2026-01-09T02:50:00""#,
            r#""2026-01-09T02:55:00" "down_end" "ripe" "2026-01-09T02:55:00""#,
            r#""2026-01-09T03:00:00" "down_end" "ripe" "2026-01-09T03:00:00""#,
            r#""2026-01-09T03:10:00" "down_end" "turned" "2026-01-09T03:00:00""#,
        ]
    );
}

/// A 5m last segment to 80 without divergence (the entry stroke from 125
/// keeps A2), then a rebound to 82.
fn no_mid_divergence() -> Vec<RawBar> {
    let mut d = FAST[..4].to_vec();
    d.push((80.0, 60));
    down_path(&[(125.0, 240)], &d, &[(82.0, 40)])
}

/// B fails at the low (5m area ratio above 1): `primed`, also once the next
/// 5m bars rebound into an up unfinished segment from the low, which lies
/// after the span and is not its last element.
#[test]
fn a_rebound_after_the_low_keeps_the_move_in_progress() {
    let m = no_mid_divergence();
    let at = down(&request(&cut(&m, 5920), false));
    assert_eq!(
        (&at["stage"], &at["low"]["price"]),
        (&json!("primed"), &json!(79.99))
    );
    assert!(at["mid"]["area_ratio"].as_f64().unwrap() > 1.0);
    for n in (5925..=m.len()).step_by(5) {
        let d = down(&request(&cut(&m, n), false));
        assert_eq!((&d["stage"], &d["key"]), (&at["stage"], &at["key"]), "{n}");
        assert_eq!(d["mid"]["last"]["start_at"], at["mid"]["last"]["start_at"]);
    }
}
