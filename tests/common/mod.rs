//! Shared helpers for the regression-baseline tests.
#![allow(dead_code)]
use chanorbit::contract::Request;
use chanorbit::{Config, RawBar};
use serde_json::Value;
use std::io::Read;
use std::path::{Path, PathBuf};

pub fn golden_files() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("tests/golden exists")
        .map(|e| e.unwrap().path())
        .filter(|p| p.to_string_lossy().ends_with(".json.gz"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no baselines in {}", dir.display());
    files
}

pub fn load(path: &Path) -> Value {
    let mut text = String::new();
    flate2::read::GzDecoder::new(std::fs::File::open(path).unwrap())
        .read_to_string(&mut text)
        .unwrap();
    serde_json::from_str(&text).unwrap()
}

/// Bars of a baseline's request and the stroke parameters it was recorded with.
pub fn input(doc: &Value) -> (Vec<RawBar>, Config) {
    let req: Request = serde_json::from_value(doc["request"].clone()).unwrap();
    let config = Config {
        min_stroke_bars: doc["config"]["min_stroke_bars"].as_u64().unwrap() as usize,
    };
    (req.bars().unwrap(), config)
}

/// Whether a baseline uses the parameters of the command line.
pub fn default_config(doc: &Value) -> bool {
    input(doc).1 == Config::default()
}

/// Strict comparison: same keys, same lengths, same JSON types (an integer
/// never equals a float, a boolean never equals a number) and floats equal
/// bit for bit. No tolerance.
#[derive(Default, Debug)]
pub struct Diff {
    pub mismatches: Vec<String>,
    pub floats: usize,
}

impl Diff {
    pub fn compare(&mut self, path: &str, got: &Value, want: &Value) {
        if self.mismatches.len() >= 20 {
            return;
        }
        match (got, want) {
            (Value::Object(g), Value::Object(w)) => {
                for k in g.keys().chain(w.keys()) {
                    if !(g.contains_key(k) && w.contains_key(k)) {
                        self.mismatches
                            .push(format!("{path}.{k}: key present on one side only"));
                        return;
                    }
                }
                for (k, wv) in w {
                    self.compare(&format!("{path}.{k}"), &g[k], wv);
                }
            }
            (Value::Array(g), Value::Array(w)) => {
                if g.len() != w.len() {
                    self.mismatches.push(format!(
                        "{path}: length {} != expected {}",
                        g.len(),
                        w.len()
                    ));
                    return;
                }
                for (i, (gv, wv)) in g.iter().zip(w).enumerate() {
                    self.compare(&format!("{path}[{i}]"), gv, wv);
                }
            }
            (Value::Number(g), Value::Number(w)) => {
                let same = match (g.as_f64(), g.is_f64(), w.is_f64()) {
                    (Some(a), true, true) => {
                        self.floats += 1;
                        a.to_bits() == w.as_f64().unwrap().to_bits()
                    }
                    (_, false, false) => g == w,
                    _ => false,
                };
                if !same {
                    self.mismatches.push(format!("{path}: {g} vs expected {w}"));
                }
            }
            _ => {
                if got != want {
                    self.mismatches
                        .push(format!("{path}: {got} vs expected {want}"));
                }
            }
        }
    }
}

/// A synthetic zigzag of `legs` legs of 7 bars each with pseudo-random leg
/// sizes and a slight upward drift, so centres form and close along it.
pub fn zigzag(legs: usize) -> Vec<RawBar> {
    let start = chanorbit::contract::parse_dt("2026-01-05T09:35:00").unwrap();
    let mut seed: u64 = 7;
    let mut next = || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 33) as f64 / (1u64 << 31) as f64
    };
    let cents = |x: f64| (x * 100.0).round() / 100.0;
    let (mut price, mut bars) = (100.0_f64, Vec::new());
    for leg in 0..legs {
        let up = leg % 2 == 0;
        let size = 2.0 + 6.0 * next() + if up { 0.3 } else { 0.0 };
        for _ in 0..7 {
            let from = price;
            price += if up { size / 7.0 } else { -size / 7.0 };
            bars.push(RawBar {
                dt: start + chrono::Duration::minutes(5 * bars.len() as i64),
                high: cents(from.max(price) + 0.05),
                low: cents(from.min(price) - 0.05),
                close: cents(price),
                vol: 100.0,
            });
        }
    }
    bars
}
