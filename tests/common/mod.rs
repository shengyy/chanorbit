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
        max_bi_num: doc["config"]["max_bi_num"].as_u64().unwrap() as usize,
        min_bi_len: doc["config"]["min_bi_len"].as_u64().unwrap() as usize,
    };
    (req.bars().unwrap(), config)
}

/// Whether a baseline uses the parameters of the command line.
pub fn default_config(doc: &Value) -> bool {
    input(doc).1 == Config::default()
}

/// MACD-derived values (EMA chains and their sums) compare with a relative
/// tolerance of 1e-9; every other number must be equal.
fn tolerant(key: &str) -> bool {
    key.starts_with("dif")
        || key.starts_with("dea")
        || key.starts_with("hist")
        || key.ends_with("_area")
        || key == "peak"
}

#[derive(Default, Debug)]
pub struct Diff {
    pub mismatches: Vec<String>,
    pub tolerant_values: usize,
    pub tolerant_inexact: usize,
}

impl Diff {
    pub fn compare(&mut self, path: &str, key: &str, got: &Value, want: &Value) {
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
                    self.compare(&format!("{path}.{k}"), k, &g[k], wv);
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
                    self.compare(&format!("{path}[{i}]"), key, gv, wv);
                }
            }
            (Value::Number(g), Value::Number(w)) => {
                let (a, b) = (g.as_f64().unwrap(), w.as_f64().unwrap());
                if tolerant(key) {
                    self.tolerant_values += 1;
                    if a.to_bits() != b.to_bits() {
                        self.tolerant_inexact += 1;
                    }
                    if (a - b).abs() > 1e-9 * a.abs().max(b.abs()) {
                        self.mismatches.push(format!("{path}: {a} vs expected {b}"));
                    }
                } else if a != b {
                    self.mismatches.push(format!("{path}: {a} vs expected {b}"));
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
