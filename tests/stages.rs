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
            let after = if !s["withdrawn_rebound"].is_null() {
                "withdrawn_rebound"
            } else if s["beyond_start"] == true {
                "beyond_start"
            } else {
                "within"
            };
            seen_states.insert(format!("{}/{after}", s["kind"]));
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
        "\"second\"/withdrawn_rebound",
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
    let clean = baseline("rebound_withdrawn.json.gz", 0);
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
    assert_eq!(stages(&revised), stages(&clean));
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
    assert_eq!(v["schema"], "chanorbit.stages/1");
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
    assert_eq!(v["schema"], "chanorbit.stages/1");
    assert_eq!(v["error"]["kind"], "invalid_input");
}
