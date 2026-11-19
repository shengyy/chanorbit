//! `levels` regression baselines (tests/golden/levels), its request rules and
//! the state and zone implications on every baseline response.
mod common;

use chanorbit::contract::{Command, LEVELS_SCHEMA, handle_command};
use common::{Diff, load};
use serde_json::{Value, json};
use std::path::Path;

fn cases() -> Vec<Value> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/levels");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("tests/golden/levels exists")
        .map(|e| e.unwrap().path())
        .collect();
    files.sort();
    let cases: Vec<Value> = files
        .iter()
        .flat_map(|p| load(p)["cases"].as_array().unwrap().clone())
        .collect();
    assert!(!cases.is_empty());
    cases
}

fn levels(request: &Value) -> (Value, bool) {
    let (text, ok) = handle_command(Command::Levels, &request.to_string());
    (serde_json::from_str(&text).unwrap(), ok)
}

fn answer(request: &Value) -> Value {
    let (v, ok) = levels(request);
    assert!(ok, "{v}");
    v
}

#[test]
fn baselines_match_field_by_field() {
    for case in cases() {
        let mut diff = Diff::default();
        diff.compare("response", &answer(&case["request"]), &case["response"]);
        assert!(
            diff.mismatches.is_empty(),
            "{}: {:#?}",
            case["request"]["id"],
            diff.mismatches
        );
    }
}

#[test]
fn states_agree_with_the_last_close_and_zones_partition() {
    let mut kinds = std::collections::BTreeSet::new();
    for case in cases() {
        for level in case["response"]["levels"].as_array().unwrap() {
            let centres = level["centres"].as_array().unwrap();
            for (k, z) in centres.iter().enumerate() {
                let kind = z["state"]["kind"].as_str().unwrap();
                let latest = k + 1 == centres.len();
                let role = if latest { "latest" } else { "ended" };
                kinds.insert(format!("{role}/{kind}"));
                let st = &z["state"];
                if kind == "departed" {
                    if let Some(d) = st["departure"].as_str() {
                        kinds.insert(format!("{role}/departure/{d}"));
                    }
                    kinds.insert(format!("{role}/versus_edge/{}", st["versus_edge"]));
                    if let Some(r) = st["retest_role"]["role"].as_str() {
                        kinds.insert(format!("retest_role/{r}"));
                    }
                    // `edge` is the exit side, whatever the element's direction.
                    let against = (st["edge"] == "zg") != (st["direction"] == "向上");
                    if against {
                        kinds.insert(format!("{role}/against_exit_side"));
                    }
                    // `new_extreme` exists only toward the exit side; a
                    // non-member is new exactly beyond `dd`/`gg`, and a new
                    // member extreme is `dd`/`gg` itself.
                    assert_eq!(st["new_extreme"].is_null(), against, "{st}");
                    if let Some(new) = st["new_extreme"].as_bool() {
                        let price = st["raw_extreme"]["price"].as_f64().unwrap();
                        let (outer, beyond) = if st["edge"] == "zg" {
                            let gg = z["gg"].as_f64().unwrap();
                            (gg, price > gg)
                        } else {
                            let dd = z["dd"].as_f64().unwrap();
                            (dd, price < dd)
                        };
                        if st["member"] == true {
                            assert!(!new || price == outer, "{st}");
                        } else {
                            assert_eq!(new, beyond, "{st}");
                        }
                        kinds.insert(format!("{role}/new_extreme/{new}/member/{}", st["member"]));
                    }
                }
                if latest && kind == "departed" {
                    // Every bar after the exit is strictly beyond the edge.
                    assert!(st["versus_edge"].is_null() || st["versus_edge"] == "beyond");
                }
                if let Some(v) = z["versus_next"].as_object() {
                    assert!(v["core"] == "above" || v["core"] == "below", "{v:?}");
                }
                if latest {
                    assert_eq!(kind == "inside", z["price_versus_core"] == "inside");
                    assert!(z["state"]["retest_role"].is_null());
                } else {
                    assert_ne!(kind, "inside");
                    assert_eq!(z["ended_by"], json!(k + 1));
                }
                if kind == "formed_out" {
                    assert_eq!(z["member_count"], 3);
                }
            }
            if let Some(reason) = level["child"]["zones_reason"].as_str() {
                kinds.insert(format!("zones_reason/{reason}"));
            }
            let Some(zones) = level["child"]["zones"].as_object() else {
                continue;
            };
            let b = &zones["boundaries"];
            let objects = zones["objects"].as_array().unwrap();
            if let Some(b1) = b["last_start"].as_str() {
                for o in objects {
                    // Placed objects end after the parent's last-stroke start.
                    assert!(o["end_at"].as_str().unwrap() > b1);
                }
            } else {
                kinds.insert(format!(
                    "child_starts_after/{}",
                    zones["child_starts_after"]
                ));
                assert!(objects.iter().all(|o| o["zone"] != "crosses_last_start"));
            }
            // Forces: last_vs_first from two same-direction strokes in the
            // zone, last_vs_previous from three.
            let parent_dir = &level["strokes"].as_array().unwrap().last().unwrap()["direction"];
            let child = &case["response"]["levels"]
                .as_array()
                .unwrap()
                .iter()
                .find(|l| l["name"] == level["child"]["name"])
                .unwrap()["strokes"];
            let same = objects
                .iter()
                .filter(|o| o["kind"] == "stroke" && o["zone"] == "inside_last")
                .filter(|o| {
                    child[o["index"].as_u64().unwrap() as usize]["direction"] == *parent_dir
                })
                .count();
            let rows = level["child"]["forces"]["last_stroke"].as_array().unwrap();
            assert_eq!(rows.len(), [0, 0, 1].get(same).copied().unwrap_or(2));
            if same == 2 {
                kinds.insert("forces/two_strokes".into());
            }
        }
    }
    for want in [
        "latest/inside",
        "latest/departed",
        "latest/formed_out",
        "ended/departed",
        "ended/formed_out",
        "latest/departure/unfinished",
        "latest/departure/extension",
        "latest/departure/after_extreme",
        "latest/against_exit_side",
        "ended/against_exit_side",
        "ended/versus_edge/\"beyond\"",
        "ended/new_extreme/true/member/true",
        "latest/new_extreme/true/member/true",
        "retest_role/entry",
        "retest_role/member",
        "zones_reason/last_start_no_child_bar",
        "zones_reason/last_end_no_child_bar",
        "zones_reason/unfinished_extreme_no_child_bar",
        "forces/two_strokes",
        "child_starts_after/\"last_start\"",
    ] {
        assert!(kinds.contains(want), "no baseline covers {want}");
    }
}

#[test]
fn revisions_answer_as_the_series_without_them() {
    let clean = cases()[0]["request"].clone();
    let mut revised = clean.clone();
    for level in revised["levels"].as_array_mut().unwrap() {
        let n = level["dt"].as_array().unwrap().len();
        for field in ["dt", "high", "low", "close", "vol"] {
            let column = level[field].as_array().unwrap().clone();
            let mut out = Vec::new();
            for (i, v) in column.iter().enumerate() {
                if i % 5 == 0 && i + 1 < n {
                    out.push(match field {
                        "dt" | "vol" => v.clone(),
                        // A wider earlier version that still holds its close.
                        "high" => json!(v.as_f64().unwrap() * 1.05),
                        "low" => json!(v.as_f64().unwrap() * 0.95),
                        _ => v.clone(),
                    });
                }
                out.push(v.clone());
            }
            level[field] = Value::Array(out);
        }
    }
    assert_eq!(answer(&revised), answer(&clean));
}

fn series(name: &str) -> Value {
    json!({"name": name, "dt": ["2026-01-05T15:00:00", "2026-01-06T15:00:00"],
           "high": [2.0, 2.1], "low": [1.0, 1.1], "close": [1.5, 1.6], "vol": [1.0, 1.0]})
}

#[test]
fn request_rules() {
    let reject = |request: Value, kind: &str, message: &str| {
        let (v, ok) = levels(&request);
        assert!(!ok);
        assert_eq!(v["schema"], LEVELS_SCHEMA);
        assert_eq!(v["error"]["kind"], kind, "{v}");
        let text = v["error"]["message"].as_str().unwrap();
        assert!(text.contains(message), "{text}");
        v
    };
    let v = reject(
        json!({"id": 7, "levels": []}),
        "invalid_request",
        "levels is empty",
    );
    assert_eq!(v["id"], 7);
    reject(json!({"id": 1}), "invalid_request", "levels");
    reject(
        json!({"levels": [series("")]}),
        "invalid_request",
        "levels[0].name is empty",
    );
    reject(
        json!({"levels": [series("a"), series("a")]}),
        "invalid_request",
        "levels[1].name repeats levels[0].name",
    );
    let mut with_observe = series("a");
    with_observe["observe_from"] = json!("2026-01-05T15:00:00");
    reject(
        json!({"levels": [with_observe]}),
        "invalid_request",
        "observe_from",
    );
    reject(
        json!({"levels": [series("a")], "observe_from": "2026-01-05T15:00:00"}),
        "invalid_request",
        "observe_from",
    );
    let mut bad = series("b");
    bad["low"] = json!([1.0, 2.5]);
    reject(
        json!({"levels": [series("a"), bad]}),
        "invalid_bar",
        "levels[1].high[1]",
    );
    let mut no_name = series("a");
    no_name.as_object_mut().unwrap().remove("name");
    reject(json!({"levels": [no_name]}), "invalid_request", "name");
}

#[test]
fn short_levels_are_missing_material_and_the_finest_has_no_child() {
    let v = answer(&json!({"id": "x", "levels": [series("coarse"), series("fine")]}));
    assert_eq!(v["schema"], LEVELS_SCHEMA);
    assert_eq!(v["id"], "x");
    let all = v["levels"].as_array().unwrap();
    assert_eq!(all.len(), 2);
    for level in all {
        assert_eq!(level["status"], "missing_material");
        assert_eq!(level["last_input_at"], "2026-01-06T15:00:00");
        assert!(level["strokes"].as_array().unwrap().is_empty());
        assert!(level["last_stroke_member"].is_null());
    }
    // A parent without strokes carries no child object, only the reason.
    assert!(all[0]["child"].is_null());
    assert_eq!(all[0]["child_reason"], "missing_material");
    assert!(all[1].get("child").is_none() && all[1].get("child_reason").is_none());
}

/// One daily level from price pivots: legs of `bars` inputs each, linear in
/// between, every bar 0.1 wide around its close.
fn zigzag(pivots: &[(f64, usize)]) -> Value {
    let mut prices = vec![pivots[0].0];
    for w in pivots.windows(2) {
        let ((a, _), (b, n)) = (w[0], w[1]);
        // Each pivot exactly, so equal pivots give equal prices.
        prices.extend((1..=n).map(|k| {
            if k == n {
                b
            } else {
                a + (b - a) * k as f64 / n as f64
            }
        }));
    }
    let start = chanorbit::contract::parse_dt("2026-01-05T15:00:00").unwrap();
    let dt: Vec<String> = (0..prices.len())
        .map(|i| {
            (start + chrono::Duration::days(i as i64))
                .format("%Y-%m-%dT%H:%M:%S")
                .to_string()
        })
        .collect();
    let column = |f: fn(f64) -> f64| prices.iter().map(|&p| f(p)).collect::<Vec<_>>();
    json!({"levels": [{"name": "d", "dt": dt, "high": column(|p| p + 0.05),
        "low": column(|p| p - 0.05), "close": prices, "vol": vec![100.0; prices.len()]}]})
}

#[test]
fn new_extreme_is_strictly_beyond_every_other_member() {
    // Entry 20 -> 10, members 10 -> 14 -> 11 -> 13.5: core [10.95, 13.55],
    // first member low 9.95. Each case leaves downward, and mirrored upward.
    let leg = |p: &[f64]| p.iter().map(|&x| (x, 6)).collect::<Vec<_>>();
    let centre = [16.0, 20.0, 10.0, 14.0, 11.0, 13.5];
    let cases = [
        (
            "member, new low",
            leg(&[9.0, 10.5, 9.5]),
            json!(4),
            json!(true),
        ),
        (
            "member, equal to an earlier member",
            leg(&[10.0, 10.5, 10.2]),
            json!(4),
            json!(false),
        ),
        (
            "member, above an earlier member",
            leg(&[10.5, 10.8, 10.6]),
            json!(4),
            json!(false),
        ),
        (
            "non-member, below dd",
            leg(&[9.0]),
            json!("unfinished"),
            json!(true),
        ),
        (
            "non-member, equal to dd",
            leg(&[10.0]),
            json!("unfinished"),
            json!(false),
        ),
        // An up candidate whose last bar touches the core and closes below it;
        // the down stroke before it has three raw bars between its extremes.
        (
            "against the exit side",
            vec![(10.5, 4), (10.92, 2)],
            json!("unfinished"),
            Value::Null,
        ),
    ];
    for (name, tail, departure, want) in cases {
        let pivots: Vec<(f64, usize)> = leg(&centre).into_iter().chain(tail).collect();
        let mirrored: Vec<(f64, usize)> = pivots.iter().map(|&(p, n)| (30.0 - p, n)).collect();
        for (side, pivots) in [("down", pivots), ("up", mirrored)] {
            let v = answer(&zigzag(&pivots));
            let centres = v["levels"][0]["centres"].as_array().unwrap();
            assert_eq!(centres.len(), 1, "{name} ({side}): {v}");
            let st = &centres[0]["state"];
            assert_eq!(st["kind"], "departed", "{name} ({side}): {st}");
            assert_eq!(
                st["edge"],
                if side == "down" { "zd" } else { "zg" },
                "{name} ({side})"
            );
            assert_eq!(st["departure"], departure, "{name} ({side}): {st}");
            assert_eq!(st["member"], departure.is_number(), "{name} ({side}): {st}");
            assert_eq!(st["new_extreme"], want, "{name} ({side}): {st}");
        }
    }
}

/// Power covers the bars after a piece's start up to its end, so every bar is
/// counted once along the sequence: the turn bar belongs to the piece ending there.
#[test]
fn power_windows_leave_out_the_start_bar() {
    for case in cases() {
        for (input, level) in case["request"]["levels"]
            .as_array()
            .unwrap()
            .iter()
            .zip(case["response"]["levels"].as_array().unwrap())
        {
            assert_eq!(input["name"], level["name"]);
            let mut times: Vec<&str> = input["dt"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect();
            times.dedup();
            let count = |from: &Value, to: &Value| {
                let (from, to) = (from.as_str().unwrap(), to.as_str().unwrap());
                times.iter().filter(|&&t| t > from && t <= to).count()
            };
            let id = &case["request"]["id"];
            for s in level["strokes"].as_array().unwrap() {
                let n = count(&s["start_at"], &s["end_at"]);
                assert_eq!(s["power"]["sample_count"], json!(n), "{id} stroke {s}");
            }
            if let Some(u) = level["unfinished"].as_object() {
                let n = count(&u["anchor_at"], &u["end_price_at"]);
                assert_eq!(u["power"]["sample_count"], json!(n), "{id} unfinished");
                let last = json!(times.last().unwrap());
                let after = count(&u["end_price_at"], &last);
                assert_eq!(u["bars_after_extreme"], json!(after), "{id} bars after");
            }
            if let Some(a) = level["after_extreme"].as_object() {
                let n = count(&a["start_at"], &a["end_at"]);
                assert_eq!(a["bar_count"], json!(n), "{id} after_extreme");
                assert_eq!(a["power"]["sample_count"], json!(n), "{id} after_extreme");
            }
        }
    }
}
