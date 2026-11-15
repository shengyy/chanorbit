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
