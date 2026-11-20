//! Replays every baseline in tests/golden through the engine and compares
//! responses, prefix snapshots, the per-input trace and the final NewBar
//! detail field by field.
mod common;

use chanorbit::contract::{handle, respond};
use chanorbit::{Analyzer, NewBar, RawBar, geometry};
use common::{Diff, default_config, golden_files, input, load};
use serde_json::{Value, json};

fn iso(dt: chrono::NaiveDateTime) -> Value {
    serde_json::to_value(dt).unwrap()
}

fn trace_row(c: &Analyzer) -> Value {
    let last = c.strokes().last();
    let cand = chanorbit::candidate::candidate_stroke(c);
    json!([
        c.strokes().len(),
        c.pending_bars().len(),
        c.raw_bars().len(),
        last.map(|b| iso(b.start_at())),
        last.map(|b| iso(b.end_at())),
        c.fractals().len(),
        c.finished_count(),
        cand.state
    ])
}

fn newbar_full(nb: &NewBar) -> Value {
    let raw = |b: &RawBar| json!({"dt": iso(b.dt), "high": b.high, "low": b.low, "close": b.close, "vol": b.vol});
    json!({"dt": iso(nb.dt), "high": nb.high, "low": nb.low,
           "elements": nb.elements.iter().map(raw).collect::<Vec<_>>()})
}

#[test]
fn baselines_match_field_by_field() {
    let mut report = Vec::new();
    let mut failed = false;
    for path in golden_files() {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let doc = load(&path);
        let mut diff = Diff::default();
        let (bars, config) = input(&doc);

        // 1. Incremental analyzer: trace row per input, geometry at each prefix.
        let mut prefixes = doc["prefixes"].as_array().unwrap().iter().peekable();
        let mut c = Analyzer::new(config);
        let mut trace = Vec::new();
        for (i, bar) in bars.iter().enumerate() {
            c.update(*bar);
            trace.push(trace_row(&c));
            while let Some(p) = prefixes.next_if(|p| p["inputs"] == i + 1) {
                let got = serde_json::to_value(geometry(&c)).unwrap();
                diff.compare(&format!("prefix[{}]", i + 1), &got, &p["geometry"]);
            }
        }
        if prefixes.next().is_some() {
            diff.mismatches.push("prefix beyond input".into());
        }
        diff.compare("trace", &Value::Array(trace), &doc["trace"]);

        // 2. Final response.
        let got = serde_json::to_value(respond(doc["request"]["id"].clone(), &c)).unwrap();
        diff.compare("response", &got, &doc["response"]);

        // 3. Final NewBar detail and raw bars.
        let detail = json!({
            "pending_newbars": c.pending_bars().iter().map(|b| newbar_full(b)).collect::<Vec<_>>(),
            "stroke_newbars": c.strokes().iter()
                .map(|stroke| stroke.newbars.iter().map(|b| newbar_full(b)).collect::<Vec<_>>())
                .collect::<Vec<_>>(),
            "raw_bars": c.raw_bars().iter().map(|b| iso(b.dt)).collect::<Vec<_>>(),
        });
        diff.compare("detail", &detail, &doc["detail"]);

        // 4. Batch construction equals the incremental result.
        if geometry(&Analyzer::from_bars(bars, config)) != geometry(&c) {
            diff.mismatches
                .push("batch geometry differs from incremental".into());
        }

        // 5. The request text through the contract, for command-line parameters.
        if default_config(&doc) {
            let (text, ok) = handle(&serde_json::to_string(&doc["request"]).unwrap());
            let got: Value = serde_json::from_str(&text).unwrap();
            if !ok {
                diff.mismatches.push(format!("contract error: {text}"));
            } else {
                diff.compare("contract", &got, &doc["response"]);
            }
        }

        failed |= !diff.mismatches.is_empty();
        report.push(format!(
            "{name}: mismatches={} floats_bit_equal_checked={}{}",
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
}
