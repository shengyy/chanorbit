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
                kinds.insert(format!(
                    "{}/{kind}",
                    if latest { "latest" } else { "ended" }
                ));
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
            let Some(zones) = level["child"]["zones"].as_object() else {
                continue;
            };
            let b = &zones["boundaries"];
            let b1 = b["last_start"].as_str().unwrap();
            for o in zones["objects"].as_array().unwrap() {
                // Placed objects end after the parent's last-stroke start.
                assert!(o["end_at"].as_str().unwrap() > b1);
            }
        }
    }
    for want in [
        "latest/inside",
        "latest/departed",
        "latest/formed_out",
        "ended/departed",
        "ended/formed_out",
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
    assert_eq!(all[0]["child"]["name"], "fine");
    assert_eq!(all[0]["child"]["zones_reason"], "no_parent_stroke");
    assert!(all[1].get("child").is_none());
}
