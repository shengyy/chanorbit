//! In-process timing over a JSON Lines file of `geometry` requests:
//! `cargo run --release --example bench -- requests.jsonl`, or of `signals`
//! requests with `--signals requests.jsonl` (per-request time, parse included).
use chanorbit::contract::{Command, Request, handle_command};
use chanorbit::{Analyzer, Config, geometry};
use std::time::Instant;

/// Per-request time of `signals` requests, one JSON line each.
fn signals(text: &str) {
    let mut times: Vec<f64> = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let t = Instant::now();
        let (out, ok) = handle_command(Command::Signals, line);
        times.push(t.elapsed().as_secs_f64());
        assert!(ok, "{out}");
        std::hint::black_box(out);
    }
    let n = times.len().max(1) as f64;
    let max = times.iter().copied().fold(0.0, f64::max);
    println!(
        "{{\"requests\":{},\"mean_ms\":{:.3},\"max_ms\":{:.3}}}",
        times.len(),
        times.iter().sum::<f64>() / n * 1e3,
        max * 1e3
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--signals") {
        let path = args
            .get(1)
            .expect("usage: bench --signals <requests.jsonl>");
        signals(&std::fs::read_to_string(path).expect("readable file"));
        return;
    }
    let path = args.first().expect("usage: bench <requests.jsonl>");
    let text = std::fs::read_to_string(path).expect("readable file");
    let (mut series, mut bars_total) = (0usize, 0usize);
    let (mut parse, mut build, mut update, mut snap) = (0f64, 0f64, 0f64, 0f64);
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let t = Instant::now();
        let req: Request = serde_json::from_str(line).expect("request");
        let bars = req.bars().expect("valid bars");
        parse += t.elapsed().as_secs_f64();
        series += 1;
        bars_total += bars.len();
        let t = Instant::now();
        let c = Analyzer::from_bars(bars.iter().copied(), Config::default());
        build += t.elapsed().as_secs_f64();
        let t = Instant::now();
        let mut inc = Analyzer::new(Config::default());
        for bar in bars {
            inc.update(bar);
        }
        update += t.elapsed().as_secs_f64();
        let t = Instant::now();
        std::hint::black_box(serde_json::to_string(&geometry(&c)).unwrap());
        snap += t.elapsed().as_secs_f64();
    }
    println!(
        "{{\"series\":{series},\"bars\":{bars_total},\"parse_s\":{parse:.4},\"batch_construct_s\":{build:.4},\"incremental_update_s\":{update:.4},\"geometry_serialize_s\":{snap:.4}}}"
    );
}
