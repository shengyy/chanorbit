//! `stages` regression baselines (tests/golden/stages) and its contract rules.
mod common;

use chanorbit::contract::{Command, handle_command};
use common::{Diff, load};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn stage_files() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/stages");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("tests/golden/stages exists")
        .map(|e| e.unwrap().path())
        .filter(|p| p.to_string_lossy().ends_with(".json.gz"))
        .collect();
    files.sort();
    assert!(!files.is_empty());
    files
}

fn cases() -> Vec<(String, Value)> {
    stage_files()
        .iter()
        .flat_map(|path| {
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            load(path)["cases"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| (name.clone(), c.clone()))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn stages(request: &Value) -> Value {
    let (text, ok) = handle_command(Command::Stages, &request.to_string());
    assert!(ok, "{text}");
    serde_json::from_str(&text).unwrap()
}

#[test]
fn baselines_match_field_by_field() {
    let mut report = Vec::new();
    let mut failed = false;
    let mut seen_states = std::collections::BTreeSet::new();
    for (name, case) in cases() {
        let mut diff = Diff::default();
        let got = stages(&case["request"]);
        diff.compare("response", &got, &case["response"]);
        for s in got["stages"].as_array().unwrap() {
            let after = if s["beyond_start"] == true {
                "beyond_start"
            } else {
                "within"
            };
            seen_states.insert(format!("{}/{after}", s["kind"]));
            if s["retest_broken"] == true {
                seen_states.insert(format!("{}/retest_broken", s["kind"]));
            }
            if s["first_known_at"].is_null() {
                seen_states.insert(format!("{}/first_known_null", s["kind"]));
            }
        }
        let dt = case["request"]["dt"].as_array().unwrap();
        if dt.windows(2).any(|w| w[0] == w[1]) {
            seen_states.insert("revised_input".into());
        }
        failed |= !diff.mismatches.is_empty();
        report.push(format!(
            "{name} {}: mismatches={} floats_bit_equal_checked={}{}",
            case["request"]["id"],
            diff.mismatches.len(),
            diff.floats,
            diff.mismatches
                .iter()
                .map(|m| format!("\n    {m}"))
                .collect::<String>()
        ));
    }
    eprintln!("{}", report.join("\n"));
    assert!(!failed, "baseline mismatches:\n{}", report.join("\n"));
    for want in [
        "\"second\"/within",
        "\"third\"/within",
        "\"second_like\"/within",
        "\"second\"/beyond_start",
        "\"second_like\"/beyond_start",
        "\"second\"/first_known_null",
        "\"third\"/first_known_null",
        "\"second_like\"/first_known_null",
        "revised_input",
        "\"third\"/retest_broken",
        "\"second_like\"/retest_broken",
    ] {
        assert!(seen_states.contains(want), "no baseline covers {want}");
    }
}

fn baseline(file: &str, index: usize) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/stages")
        .join(file);
    load(&path)["cases"][index]["request"].clone()
}

#[test]
fn revisions_are_observed_once_after_the_last_version() {
    let clean = baseline("second.json.gz", 0);
    let mut revised = clean.clone();
    // Insert an earlier, different version before every fifth bar.
    let n = clean["dt"].as_array().unwrap().len();
    for field in ["dt", "high", "low", "close", "vol"] {
        let column = clean[field].as_array().unwrap();
        let mut out = Vec::new();
        for (i, v) in column.iter().enumerate() {
            if i % 5 == 0 && i + 1 < n {
                out.push(match field {
                    "dt" | "vol" => v.clone(),
                    "high" => json!(v.as_f64().unwrap() * 1.05),
                    "low" => json!(v.as_f64().unwrap() * 0.95),
                    _ => json!(v.as_f64().unwrap() * 1.01),
                });
            }
            out.push(v.clone());
        }
        revised[field] = Value::Array(out);
    }
    assert!(revised["dt"].as_array().unwrap().len() > n);
    let answer = stages(&clean);
    assert!(!answer["stages"].as_array().unwrap().is_empty());
    assert_eq!(stages(&revised), answer);
}

#[test]
fn observation_after_the_last_input_knows_no_first_time() {
    let mut request = baseline("second.json.gz", 0);
    request["observe_from"] = json!("2099-01-01T00:00:00");
    let got = stages(&request);
    let all = got["stages"].as_array().unwrap();
    assert!(!all.is_empty());
    for s in all {
        assert_eq!(
            (&s["first_known_at"], &s["stage_age"]),
            (&Value::Null, &Value::Null)
        );
    }
}

#[test]
fn observe_from_is_checked_and_belongs_to_stages_only() {
    let mut request = baseline("second.json.gz", 0);
    request["observe_from"] = json!("2026-01-05 15:00:00");
    let (text, ok) = handle_command(Command::Stages, &request.to_string());
    let v: Value = serde_json::from_str(&text).unwrap();
    assert!(!ok);
    assert_eq!(v["schema"], "chanorbit.stages/2");
    assert_eq!(v["error"]["kind"], "invalid_request");

    request["observe_from"] = json!("2026-01-05T15:00:00");
    let (text, ok) = handle_command(Command::Geometry, &request.to_string());
    let v: Value = serde_json::from_str(&text).unwrap();
    assert!(!ok);
    assert_eq!(v["schema"], "chanorbit.geometry/1");
    assert_eq!(v["error"]["kind"], "invalid_request");
}

#[test]
fn stage_errors_carry_the_stages_schema() {
    let (text, ok) = handle_command(Command::Stages, "[1]");
    let v: Value = serde_json::from_str(&text).unwrap();
    assert!(!ok);
    assert_eq!(v["schema"], "chanorbit.stages/2");
    assert_eq!(v["error"]["kind"], "invalid_input");
}

/// `request` with one daily bar per `(high, low)` after its last input.
fn append(request: &Value, bars: &[(f64, f64)]) -> Value {
    let mut out = request.clone();
    let last = out["dt"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .as_str()
        .unwrap();
    let mut dt = chanorbit::contract::parse_dt(last).unwrap();
    for &(high, low) in bars {
        dt += chrono::Duration::days(1);
        let close = ((high + low) * 50.0).round() / 100.0;
        for (field, v) in [
            ("dt", json!(dt.format("%Y-%m-%dT%H:%M:%S").to_string())),
            ("high", json!(high)),
            ("low", json!(low)),
            ("close", json!(close)),
            ("vol", json!(1000.0)),
        ] {
            out[field].as_array_mut().unwrap().push(v);
        }
    }
    out
}

/// The base second stage and its retest's low.
fn second_base() -> (Value, f64) {
    let base = baseline("second.json.gz", 0);
    let got = stages(&base);
    let stage = &got["stages"][0];
    assert_eq!(stage["kind"], "second");
    assert_eq!(stage["retest_broken"], false);
    let low = stage["retest"]["low"].as_f64().unwrap();
    (base, low)
}

/// `low` moved by `offset`, rounded to a tick.
fn at(low: f64, offset: f64) -> f64 {
    ((low + offset) * 100.0).round() / 100.0
}

/// A raw low below the retest's low before it is a fractal: the retest stays
/// the last stroke (being extended) and the stage stands, marked broken.
#[test]
fn a_new_low_after_the_retest_marks_it_broken() {
    let (base, l) = second_base();
    assert_eq!(l, 8.68);
    // Touching the low does not break it; going below does.
    for (low, broken) in [(l, false), (at(l, -0.09), true)] {
        let got = stages(&append(&base, &[(at(l, 0.21), low)]));
        let stage = &got["stages"][0];
        assert_eq!(stage["retest"]["low"], l, "{low}");
        assert_eq!(stage["retest_broken"], broken, "{low}");
        assert_eq!(stage["beyond_start"], false, "{low}");
    }
}

/// A rebound stroke after the retest, then a raw low below the retest's low
/// with no down stroke yet: the retest is no longer the last stroke, so no
/// stage stands and there is nothing to mark.
#[test]
fn a_new_low_after_a_rebound_stroke_leaves_no_stage() {
    let (base, l) = second_base();
    let rebound: Vec<(f64, f64)> = [
        (0.46, 0.21),
        (0.56, 0.36),
        (0.66, 0.46),
        (0.76, 0.56),
        (0.86, 0.66),
        (0.81, 0.61),
    ]
    .iter()
    .map(|&(h, lo)| (at(l, h), at(l, lo)))
    .collect();
    let mut request = append(&base, &rebound);
    request = append(
        &request,
        &[(at(l, 0.51), at(l, 0.11)), (at(l, 0.21), at(l, -0.09))],
    );
    let mut geometry_request = request.clone();
    geometry_request
        .as_object_mut()
        .unwrap()
        .remove("observe_from");
    let (text, ok) = handle_command(Command::Geometry, &geometry_request.to_string());
    assert!(ok, "{text}");
    let g: Value = serde_json::from_str(&text).unwrap();
    let last = g["strokes"].as_array().unwrap().last().unwrap();
    assert_eq!(
        (&last["direction"], &last["start_price"]),
        (&json!("向上"), &json!(l))
    );
    assert_eq!(stages(&request)["stages"], json!([]));
}
