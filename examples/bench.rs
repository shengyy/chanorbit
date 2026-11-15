//! In-process timing over a JSON Lines file of `geometry` requests:
//! `cargo run --release --example bench -- requests.jsonl`.
use chanorbit::contract::Request;
use chanorbit::{Analyzer, Config, geometry};
use std::time::Instant;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: bench <requests.jsonl>");
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
