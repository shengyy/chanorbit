//! End-to-end: the release contract through the real binary (stdin -> stdout).
mod common;

use common::{Diff, default_config, golden_files, load};
use serde_json::Value;
use std::io::Write;
use std::process::{Command, Output, Stdio};

fn chanorbit(args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_chanorbit"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn assert_same(got: &Value, want: &Value, what: &str) {
    let mut diff = Diff::default();
    diff.compare(what, got, want);
    assert!(diff.mismatches.is_empty(), "{what}: {:#?}", diff.mismatches);
}

#[test]
fn single_request_matches_baseline() {
    let doc = load(
        golden_files()
            .iter()
            .find(|p| p.ends_with("f5_session_1920.json.gz"))
            .unwrap(),
    );
    let request = serde_json::to_string_pretty(&doc["request"]).unwrap();
    let out = chanorbit(&["geometry"], &request);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    assert_eq!(text.lines().count(), 1);
    assert_same(
        &serde_json::from_str(&text).unwrap(),
        &doc["response"],
        "response",
    );
}

#[test]
fn batch_keeps_order_and_isolates_errors() {
    let docs: Vec<Value> = golden_files()
        .iter()
        .map(|p| load(p))
        .filter(default_config)
        .collect();
    let mut lines: Vec<String> = docs
        .iter()
        .map(|d| serde_json::to_string(&d["request"]).unwrap())
        .collect();
    lines.insert(2, "{not json".into());
    lines.insert(4, r#"{"id": "short-high", "dt": ["2026-01-05T15:00:00"], "high": [], "low": [1], "close": [1], "vol": [1]}"#.into());
    let out = chanorbit(
        &["geometry", "--batch", "--jobs", "4"],
        &(lines.join("\n") + "\n"),
    );
    assert_eq!(out.status.code(), Some(1));
    let responses: Vec<Value> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(responses.len(), lines.len());
    assert_eq!(responses[2]["error"]["kind"], "invalid_json");
    assert_eq!(responses[4]["id"], "short-high");
    assert_eq!(responses[4]["error"]["kind"], "invalid_request");
    let ok: Vec<&Value> = responses
        .iter()
        .filter(|r| r.get("error").is_none())
        .collect();
    assert_eq!(ok.len(), docs.len());
    for (got, doc) in ok.iter().zip(&docs) {
        assert_same(
            got,
            &doc["response"],
            doc["request"]["id"].as_str().unwrap(),
        );
    }
    // Same output with one worker: deterministic regardless of threads.
    let serial = chanorbit(
        &["geometry", "--batch", "--jobs", "1"],
        &(lines.join("\n") + "\n"),
    );
    let threaded = chanorbit(
        &["geometry", "--batch", "--jobs", "8"],
        &(lines.join("\n") + "\n"),
    );
    assert_eq!(serial.stdout, threaded.stdout);
}

#[test]
fn version_usage_and_exit_codes() {
    let out = chanorbit(&["--version"], "");
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.starts_with("chanorbit ") && text.contains(chanorbit::contract::SCHEMA));
    assert!(text.trim_end().ends_with(&format!(
        "schemas {} {} {}",
        chanorbit::contract::SCHEMA,
        chanorbit::contract::STAGES_SCHEMA,
        chanorbit::contract::LEVELS_SCHEMA
    )));
    let out = chanorbit(&["geometry", "--bogus"], "");
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty() && !out.stderr.is_empty());
    let out = chanorbit(
        &["geometry"],
        r#"{"dt":["2026-01-05T15:00:00Z"],"high":[1],"low":[1],"close":[1],"vol":[1]}"#,
    );
    assert_eq!(out.status.code(), Some(1));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["error"]["kind"], "invalid_bar");
    assert_eq!(v["schema"], chanorbit::contract::SCHEMA);
    let out = chanorbit(
        &["geometry"],
        r#"{"dt":["2026-01-05T15:00:00"],"high":[1],"low":[1],"close":[1],"vol":[1],"open":[1]}"#,
    );
    assert_eq!(out.status.code(), Some(1));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["error"]["kind"], "invalid_request");
}

fn chanorbit_bytes(args: &[&str], stdin: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_chanorbit"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    child.wait_with_output().unwrap()
}

const OK: &str = r#"{"id":"ok","dt":["2026-03-02T15:00:00","2026-03-03T15:00:00"],"high":[10.3,10.4],"low":[9.9,10.0],"close":[10.2,10.1],"vol":[1000,1200]}"#;

#[test]
fn a_non_utf8_batch_line_fails_alone() {
    let mut input = Vec::new();
    input.extend_from_slice(OK.as_bytes());
    input.extend_from_slice(b"\n\xff\xfe\n");
    input.extend_from_slice(OK.as_bytes());
    input.extend_from_slice(b"\r\n");
    let out = chanorbit_bytes(&["geometry", "--batch"], &input);
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stderr.is_empty());
    let lines: Vec<Value> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0]["id"], "ok");
    assert_eq!(lines[1]["error"]["kind"], "invalid_json");
    assert_eq!(lines[2]["id"], "ok");
    // A single request that is not UTF-8 is an error object too.
    let out = chanorbit_bytes(&["geometry"], b"\xff");
    assert_eq!(out.status.code(), Some(1));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["error"]["kind"], "invalid_json");
}

#[test]
fn empty_batch_and_jobs_without_batch() {
    for input in ["", "\n  \n\r\n"] {
        let out = chanorbit(&["geometry", "--batch"], input);
        assert_eq!(out.status.code(), Some(0));
        assert!(out.stdout.is_empty() && out.stderr.is_empty());
    }
    let out = chanorbit(&["geometry", "--jobs", "2"], OK);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty() && !out.stderr.is_empty());
    let out = chanorbit(&["geometry", "--batch", "--jobs", "0"], OK);
    assert_eq!(out.status.code(), Some(2));
    for alias in [["--version"], ["-V"]] {
        let out = chanorbit(&alias, "");
        assert_eq!(out.status.code(), Some(0));
    }
    let out = chanorbit(&["-h"], "");
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8(out.stdout).unwrap().starts_with("usage:"));
}

#[test]
fn stages_batch_matches_baselines_in_order() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/stages");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    files.sort();
    let cases: Vec<Value> = files
        .iter()
        .flat_map(|p| load(p)["cases"].as_array().unwrap().clone())
        .collect();
    let mut lines: Vec<String> = cases.iter().map(|c| c["request"].to_string()).collect();
    lines.insert(1, r#"{"dt":[]}"#.into());
    let input = lines.join("\n") + "\n";
    let serial = chanorbit(&["stages", "--batch", "--jobs", "1"], &input);
    assert_eq!(serial.status.code(), Some(1));
    let got: Vec<Value> = String::from_utf8(serial.stdout.clone())
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(got.len(), cases.len() + 1);
    assert_eq!(got[1]["error"]["kind"], "invalid_request");
    assert_eq!(got[1]["schema"], "chanorbit.stages/2");
    let answered = got.iter().take(1).chain(got.iter().skip(2));
    for (g, c) in answered.zip(&cases) {
        assert_same(g, &c["response"], "stages");
    }
    let threaded = chanorbit(&["stages", "--batch", "--jobs", "8"], &input);
    assert_eq!(serial.stdout, threaded.stdout);
    let single = chanorbit(&["stages"], &cases[0]["request"].to_string());
    assert_eq!(single.status.code(), Some(0));
    assert_same(
        &serde_json::from_slice(&single.stdout).unwrap(),
        &cases[0]["response"],
        "stages single",
    );
}

#[test]
fn observe_from_is_refused_by_geometry_even_as_null() {
    let request = |observe_from: &str| {
        format!(
            r#"{{"id":"g","dt":["2020-01-02T15:00:00"],"high":[1.0],"low":[1.0],"close":[1.0],"vol":[1.0],"observe_from":{observe_from}}}"#
        )
    };
    for value in [r#""2020-01-02T15:00:00""#, "null"] {
        let out = chanorbit(&["geometry"], &request(value));
        assert_eq!(out.status.code(), Some(1), "observe_from {value}");
        let v: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(v["schema"], "chanorbit.geometry/1");
        assert_eq!(v["id"], "g");
        assert_eq!(
            v["error"]["kind"], "invalid_request",
            "observe_from {value}"
        );
    }
    // stages: a string is the parameter, null is a wrong type.
    let out = chanorbit(&["stages"], &request(r#""2020-01-02T15:00:00""#));
    assert_eq!(out.status.code(), Some(0));
    let out = chanorbit(&["stages"], &request("null"));
    assert_eq!(out.status.code(), Some(1));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["error"]["kind"], "invalid_request");
}

/// The reader of stdout (and maybe stderr) went away: exit 3, no panic.
#[test]
fn a_closed_stdout_exits_with_the_io_code() {
    let request = r#"{"id":"p","dt":["2020-01-02T15:00:00"],"high":[2.0],"low":[1.0],"close":[1.5],"vol":[1.0]}"#;
    let input = format!("{request}\n{request}\n");
    for args in [
        &["geometry"][..],
        &["geometry", "--batch"],
        &["stages", "--batch", "--jobs", "2"],
        &["--version"],
        &["--help"],
    ] {
        for close_stderr in [false, true] {
            let mut child = Command::new(env!("CARGO_BIN_EXE_chanorbit"))
                .args(args)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            drop(child.stdout.take());
            if close_stderr {
                drop(child.stderr.take());
            }
            // The process may exit before reading stdin; a failed write here is fine.
            let _ = child.stdin.take().unwrap().write_all(input.as_bytes());
            let out = child.wait_with_output().unwrap();
            assert_eq!(
                out.status.code(),
                Some(3),
                "{args:?} close_stderr={close_stderr}"
            );
            let stderr = String::from_utf8_lossy(&out.stderr);
            assert!(!stderr.contains("panicked"), "{args:?}: {stderr}");
            if !close_stderr {
                assert!(
                    stderr.starts_with("chanorbit: writing stdout:"),
                    "{args:?}: {stderr}"
                );
            }
        }
    }
}

#[test]
fn levels_batch_matches_baselines_in_order() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/levels");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    files.sort();
    let cases: Vec<Value> = files
        .iter()
        .flat_map(|p| load(p)["cases"].as_array().unwrap().clone())
        .collect();
    let mut lines: Vec<String> = cases.iter().map(|c| c["request"].to_string()).collect();
    lines.insert(1, r#"{"id":"bad","levels":[]}"#.into());
    let input = lines.join("\n") + "\n";
    let serial = chanorbit(&["levels", "--batch", "--jobs", "1"], &input);
    assert_eq!(serial.status.code(), Some(1));
    let got: Vec<Value> = String::from_utf8(serial.stdout.clone())
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(got.len(), cases.len() + 1);
    assert_eq!(got[1]["error"]["kind"], "invalid_request");
    assert_eq!(got[1]["schema"], "chanorbit.levels/2");
    assert_eq!(got[1]["id"], "bad");
    let answered = got.iter().take(1).chain(got.iter().skip(2));
    for (g, c) in answered.zip(&cases) {
        assert_same(g, &c["response"], "levels");
    }
    let threaded = chanorbit(&["levels", "--batch", "--jobs", "8"], &input);
    assert_eq!(serial.stdout, threaded.stdout);
}
