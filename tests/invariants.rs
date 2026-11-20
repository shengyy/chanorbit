//! Standing invariants over every baseline input series and the synthetic
//! generator: MACD suffix convergence, in-progress equals completed for an
//! extending last stroke, power recomputed from the bars, and append
//! invariance.
mod common;

use chanorbit::contract::{Command, Request, handle_command};
use chanorbit::macd::raw_macd;
use chanorbit::{Analyzer, Config, RawBar, canonical_centres, geometry};
use chrono::NaiveDateTime;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::Path;

struct Series {
    name: String,
    bars: Vec<RawBar>,
    config: Config,
}

fn bars_of(level: &Value) -> Vec<RawBar> {
    let req: Request = serde_json::from_value(json!({"dt": level["dt"], "high": level["high"],
        "low": level["low"], "close": level["close"], "vol": level["vol"]}))
    .unwrap();
    req.bars().unwrap()
}

/// Every geometry baseline input, the longest cut of each levels baseline
/// path per level, and the synthetic zigzag.
fn all_series() -> Vec<Series> {
    let mut out: Vec<Series> = common::golden_files()
        .iter()
        .map(|p| {
            let (bars, config) = common::input(&common::load(p));
            Series {
                name: p.file_name().unwrap().to_string_lossy().into(),
                bars,
                config,
            }
        })
        .collect();
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/levels");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    files.sort();
    let mut longest: HashMap<String, Vec<RawBar>> = HashMap::new();
    for p in files {
        for case in common::load(&p)["cases"].as_array().unwrap() {
            let id = case["request"]["id"].as_str().unwrap();
            let path = id.rsplit_once('/').map_or(id, |(a, _)| a);
            for level in case["request"]["levels"].as_array().unwrap() {
                let key = format!("{path}/{}", level["name"].as_str().unwrap());
                let bars = bars_of(level);
                if longest.get(&key).is_none_or(|b| b.len() < bars.len()) {
                    longest.insert(key, bars);
                }
            }
        }
    }
    let mut keys: Vec<_> = longest.keys().cloned().collect();
    keys.sort();
    for key in keys {
        out.push(Series {
            bars: longest.remove(&key).unwrap(),
            name: key,
            config: Config::default(),
        });
    }
    out.push(Series {
        name: "zigzag(160)".into(),
        bars: common::zigzag(160),
        config: Config::default(),
    });
    out
}

fn fmt(t: NaiveDateTime) -> String {
    t.format("%Y-%m-%dT%H:%M:%S").to_string()
}

/// The `levels` response of one level over `bars`, as JSON.
fn level_frame(bars: &[RawBar]) -> Value {
    let column = |f: fn(&RawBar) -> f64| bars.iter().map(f).collect::<Vec<_>>();
    let req = json!({"levels": [{"name": "x",
        "dt": bars.iter().map(|b| fmt(b.dt)).collect::<Vec<_>>(),
        "high": column(|b| b.high), "low": column(|b| b.low),
        "close": column(|b| b.close), "vol": column(|b| b.vol)}]});
    let (text, ok) = handle_command(Command::Levels, &req.to_string());
    assert!(ok, "{text}");
    let mut v: Value = serde_json::from_str(&text).unwrap();
    v["levels"][0].take()
}

/// Deduplicated bars (last version of each time) with their reported MACD.
fn macd_by_dt(bars: &[RawBar]) -> Vec<(RawBar, [Option<f64>; 3])> {
    let c = Analyzer::from_bars(bars.iter().copied(), Config::default());
    let m = raw_macd(&c);
    let ok = |v: f64| v.is_finite().then_some(v);
    c.raw_bars()
        .iter()
        .enumerate()
        .map(|(i, b)| (*b, [ok(m.dif[i]), ok(m.dea[i]), ok(m.hist[i])]))
        .collect()
}

/// Every DIF, DEA and histogram value a suffix reports equals the full
/// series' value at the same bar within the seed's remaining weight: from
/// the first reported bar the long EMA's seed weighs below
/// `macd::SEED_TOLERANCE`, and the seed's error is at most the series' close
/// range, so DIF is within `SEED_TOLERANCE × range` and DEA (an EMA of DIF
/// errors that decay more slowly than it forgets them, about 1.5 times DIF's)
/// and the histogram within twice that. Measured on these series: at most
/// 1e-5 of the range, but up to 2e-3 of the series' median |hist|.
#[test]
fn macd_of_every_suffix_converges_to_the_full_series() {
    let mut checked = 0usize;
    for s in all_series() {
        let full = macd_by_dt(&s.bars);
        let by_dt: HashMap<NaiveDateTime, [Option<f64>; 3]> =
            full.iter().map(|(b, v)| (b.dt, *v)).collect();
        let hi = s.bars.iter().map(|b| b.close).fold(f64::MIN, f64::max);
        let lo = s.bars.iter().map(|b| b.close).fold(f64::MAX, f64::min);
        let tolerance = 2.0 * chanorbit::macd::SEED_TOLERANCE * (hi - lo);
        let n = s.bars.len();
        for k in (1..=8).map(|j| j * n / 9).filter(|&k| k + 146 <= n) {
            let suffix = macd_by_dt(&s.bars[k..]);
            for (i, (b, got)) in suffix.iter().enumerate() {
                if i < 145 {
                    assert!(got.iter().all(Option::is_none), "{} k={k} i={i}", s.name);
                    continue;
                }
                let want = by_dt[&b.dt];
                for (f, (g, w)) in got.iter().zip(want).enumerate() {
                    let (g, w) = (g.unwrap(), w.unwrap());
                    assert!(
                        (g - w).abs() <= tolerance,
                        "{} suffix {k} bar {} field {f}: {g} vs {w} (tolerance {tolerance:e})",
                        s.name,
                        fmt(b.dt)
                    );
                    checked += 1;
                }
            }
        }
    }
    assert!(checked > 10_000, "{checked}");
}

/// The extending last stroke at the cut `p` and the cut `q` where it
/// completes at the same extreme.
fn completion_pairs(bars: &[RawBar]) -> Vec<(usize, usize)> {
    let mut c = Analyzer::new(Config::default());
    let mut pairs = Vec::new();
    // Cuts of the current episode: the extending stroke's start and its
    // current extreme at each cut.
    let mut open: Vec<(usize, NaiveDateTime, (NaiveDateTime, f64))> = Vec::new();
    for (i, b) in bars.iter().enumerate() {
        c.update(*b);
        let p = i + 1;
        let now = c
            .extension()
            .zip(c.strokes().last())
            .map(|(x, s)| (s.start_at(), (x.at, x.price)));
        let continues = matches!((now, open.first()), (Some((s, _)), Some(o)) if s == o.1);
        if !continues && let Some(&(_, start, _)) = open.first() {
            let n = c.strokes().len();
            let done =
                c.strokes().iter().enumerate().find(|(k, s)| {
                    s.start_at() == start && !(k + 1 == n && c.extension().is_some())
                });
            if let Some((_, s)) = done {
                let end = (s.end_at(), s.end.price);
                pairs.extend(open.iter().filter(|o| o.2 == end).map(|o| (o.0, p)));
            }
            open.clear();
        }
        if let Some((s, x)) = now {
            open.push((p, s, x));
        }
    }
    pairs
}

/// A stroke or named element of a level by identity: a stroke by its start.
fn identity(level: &Value, pos: &Value) -> Value {
    let strokes = level["strokes"].as_array().unwrap();
    match pos {
        Value::Number(i) => strokes[i.as_u64().unwrap() as usize]["start_at"].clone(),
        Value::String(name) if name == "last_extended" => {
            strokes.last().unwrap()["start_at"].clone()
        }
        other => other.clone(),
    }
}

/// The extending last stroke as the response reads it: before 0.1.6 the
/// stroke ended at its confirmed end and `unfinished.kind = extension` held
/// the current extreme.
fn extending(level: &Value) -> Option<&Value> {
    let extends =
        level["last_stroke_extending"] == true || level["unfinished"]["kind"] == "extension";
    extends.then(|| level["strokes"].as_array().unwrap().last().unwrap())
}

/// For every cut where the last stroke extends and that stroke later
/// completes at the same extreme with nothing before it changed: the
/// stroke's power, raw extreme and DIF extreme, and the latest centre's
/// state (kind, departure, `new_extreme`, exit) when no bar after the
/// extreme touches its core, read the same in progress as completed.
/// Returns (pairs, state pairs, mismatches).
fn in_progress_mismatches(bars: &[RawBar]) -> (usize, usize, Vec<String>) {
    let mut frames: HashMap<usize, Value> = HashMap::new();
    let mut frame = |p: usize| {
        frames
            .entry(p)
            .or_insert_with(|| level_frame(&bars[..p]))
            .clone()
    };
    let (mut pairs, mut states, mut bad) = (0, 0, Vec::new());
    for (p, q) in completion_pairs(bars) {
        let (e, c) = (frame(p), frame(q));
        let Some(live) = extending(&e) else {
            bad.push(format!(
                "cut {p}: the analyzer extends, the response does not"
            ));
            continue;
        };
        let start = &live["start_at"];
        let strokes_c = c["strokes"].as_array().unwrap();
        let Some(done) = strokes_c.iter().find(|s| s["start_at"] == *start) else {
            continue;
        };
        // Nothing before it changed: the same strokes before its start.
        let before = |v: &Value| -> Vec<Value> {
            let all = v["strokes"].as_array().unwrap();
            all.iter()
                .filter(|s| s["start_at"].as_str() < start.as_str())
                .map(|s| json!([s["start_at"], s["end_at"], s["end_price"]]))
                .collect()
        };
        let (be, bc) = (before(&e), before(&c));
        let common = be.len().min(bc.len());
        if be[be.len() - common..] != bc[bc.len() - common..] {
            continue;
        }
        pairs += 1;
        for key in ["end_at", "end_price", "raw_extreme", "dif_extreme", "power"] {
            if live[key] != done[key] {
                bad.push(format!(
                    "cut {p}/{q}: stroke {key} {} vs {}",
                    live[key], done[key]
                ));
            }
        }
        let latest = |v: &Value| v["centres"].as_array().unwrap().last().cloned();
        let (Some(ze), Some(zc)) = (latest(&e), latest(&c)) else {
            continue;
        };
        if ["start_at", "zd", "zg"].iter().any(|k| ze[k] != zc[k]) {
            continue;
        }
        let (zd, zg) = (zc["zd"].as_f64().unwrap(), zc["zg"].as_f64().unwrap());
        let extreme_at = done["end_at"].as_str().unwrap();
        // The rest of the path stays off the core and the last close on the
        // same side: the state is then decided by the path up to the extreme.
        let touched = bars[..q]
            .iter()
            .filter(|b| fmt(b.dt).as_str() >= extreme_at)
            .any(|b| b.low <= zg && b.high >= zd);
        if touched || ze["price_versus_core"] != zc["price_versus_core"] {
            continue;
        }
        states += 1;
        let (se, sc) = (&ze["state"], &zc["state"]);
        let read = |v: &Value, st: &Value| {
            json!({"kind": st["kind"], "new_extreme": st["new_extreme"],
                "raw_extreme": st["raw_extreme"], "edge": st["edge"],
                "first_beyond_at": st["first_beyond_at"],
                "departure": identity(v, &st["departure"]),
                "third": identity(v, &st["third"])})
        };
        let (re, rc) = (read(&e, se), read(&c, sc));
        if re != rc {
            bad.push(format!("cut {p}/{q}: state {re} vs {rc}"));
        }
    }
    (pairs, states, bad)
}

#[test]
fn an_extending_last_stroke_reads_as_it_completes() {
    let (mut pairs, mut states, mut bad) = (0, 0, Vec::new());
    for s in all_series()
        .into_iter()
        .filter(|s| s.config == Config::default())
    {
        let (p, st, b) = in_progress_mismatches(&s.bars);
        pairs += p;
        states += st;
        bad.extend(b.into_iter().map(|m| format!("{}: {m}", s.name)));
    }
    assert!(
        pairs >= 100 && states >= 20,
        "{pairs} pairs, {states} state pairs"
    );
    assert!(
        bad.is_empty(),
        "{} mismatches: {:#?}",
        bad.len(),
        &bad[..bad.len().min(20)]
    );
}

/// Power over `(start, end]` from the bars and the reported MACD.
fn check_power(
    bars: &[(RawBar, [Option<f64>; 3])],
    start: &Value,
    end: &Value,
    power: &Value,
    up: Option<bool>,
    dif_extreme: Option<&Value>,
    what: &str,
) {
    let (a, b) = (start.as_str().unwrap(), end.as_str().unwrap());
    let rows: Vec<_> = bars
        .iter()
        .filter(|(r, _)| fmt(r.dt).as_str() > a && fmt(r.dt).as_str() <= b)
        .collect();
    assert_eq!(power["sample_count"], json!(rows.len()), "{what}");
    if !rows.is_empty() {
        let volume: f64 = rows.iter().map(|(r, _)| r.vol).sum();
        assert_eq!(
            power["mean_volume"],
            json!(volume / rows.len() as f64),
            "{what}"
        );
    }
    let hist: Option<Vec<f64>> = rows.iter().map(|(_, m)| m[2]).collect();
    let macd = &power["macd"];
    match hist.filter(|h| !h.is_empty()) {
        Some(h) => {
            let pos: f64 = h.iter().map(|x| x.max(0.)).sum();
            let neg: f64 = h.iter().map(|x| (-x).max(0.)).sum();
            assert_eq!(macd["positive_area"], json!(pos), "{what}");
            assert_eq!(macd["negative_area"], json!(neg), "{what}");
        }
        None => {
            assert!(macd["positive_area"].is_null(), "{what}");
            assert!(macd["negative_area"].is_null(), "{what}");
        }
    }
    let (Some(up), Some(got)) = (up, dif_extreme) else {
        return;
    };
    let dif: Option<Vec<(String, f64)>> = rows
        .iter()
        .map(|(r, m)| m[0].map(|d| (fmt(r.dt), d)))
        .collect();
    let want = dif.filter(|d| !d.is_empty()).map(|d| {
        let best = d
            .into_iter()
            .reduce(|x, y| {
                let beyond = if up { y.1 > x.1 } else { y.1 < x.1 };
                if beyond { y } else { x }
            })
            .unwrap();
        json!({"value": best.1, "at": best.0})
    });
    assert_eq!(*got, want.unwrap_or(Value::Null), "{what} dif_extreme");
}

/// Every stroke, unfinished segment and path after the extreme reports the
/// power and DIF extreme recomputed from its raw bars and the reported MACD
/// over `(start_at, end_at]`; the geometry strokes too.
#[test]
fn power_is_recomputed_from_the_bars_and_the_reported_macd() {
    let mut checked = 0usize;
    for s in all_series()
        .into_iter()
        .filter(|s| s.config == Config::default())
    {
        let n = s.bars.len();
        for cut in [n / 3, 2 * n / 3, n].into_iter().filter(|&c| c > 0) {
            let bars = &s.bars[..cut];
            let macd = macd_by_dt(bars);
            let level = level_frame(bars);
            let what = |k: &str| format!("{} cut {cut} {k}", s.name);
            let up = |d: &Value| Some(d == "向上");
            for (k, st) in level["strokes"].as_array().unwrap().iter().enumerate() {
                let dif = Some(&st["dif_extreme"]);
                let w = what(&format!("stroke {k}"));
                check_power(
                    &macd,
                    &st["start_at"],
                    &st["end_at"],
                    &st["power"],
                    up(&st["direction"]),
                    dif,
                    &w,
                );
                checked += 1;
            }
            if let Some(u) = level["unfinished"].as_object() {
                let dif = Some(&u["dif_extreme"]);
                check_power(
                    &macd,
                    &u["anchor_at"],
                    &u["end_price_at"],
                    &u["power"],
                    up(&u["direction"]),
                    dif,
                    &what("unfinished"),
                );
                checked += 1;
            }
            if let Some(a) = level["after_extreme"].as_object() {
                let dif = Some(&a["dif_extreme"]);
                check_power(
                    &macd,
                    &a["start_at"],
                    &a["end_at"],
                    &a["power"],
                    up(&a["direction"]),
                    dif,
                    &what("after_extreme"),
                );
                checked += 1;
            }
            let g = serde_json::to_value(geometry(&Analyzer::from_bars(
                bars.iter().copied(),
                Config::default(),
            )))
            .unwrap();
            for (k, st) in g["strokes"].as_array().unwrap().iter().enumerate() {
                let w = what(&format!("geometry stroke {k}"));
                check_power(
                    &macd,
                    &st["start_at"],
                    &st["end_at"],
                    &st["power"],
                    None,
                    None,
                    &w,
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 1_000, "{checked}");
}

/// Appending a bar (a new time, not a revision) leaves every stroke but the
/// last two unchanged — the strokes the stroke rules may revise — with its
/// power and index, and every closed centre whose members lie before them.
#[test]
fn appending_keeps_earlier_strokes_centres_and_indices() {
    let mut reopened = 0;
    for s in all_series() {
        let mut c = Analyzer::new(s.config);
        let mut previous: Option<NaiveDateTime> = None;
        let mut before: Vec<_> = Vec::new();
        let mut centres_before = Vec::new();
        let mut snapshot: Option<chanorbit::Geometry> = None;
        for (i, bar) in s.bars.iter().enumerate() {
            c.update(*bar);
            // Every 50 inputs: a stroke with the same index and interval as
            // at the previous snapshot has the same power.
            if i % 50 == 49 || i + 1 == s.bars.len() {
                let g = geometry(&c);
                for (k, old) in snapshot.iter().flat_map(|g| g.strokes.iter().enumerate()) {
                    if let Some(new) = g.strokes.get(k)
                        && (new.start_at, new.end_at) == (old.start_at, old.end_at)
                    {
                        assert_eq!(new.power, old.power, "{}: stroke {k} power at {i}", s.name);
                    }
                }
                snapshot = Some(g);
            }
            let revision = previous == Some(bar.dt);
            previous = Some(bar.dt);
            let after: Vec<_> = c.strokes().to_vec();
            let centres = canonical_centres(c.finished_strokes());
            if !revision {
                let kept = before
                    .iter()
                    .zip(&after)
                    .take_while(|(a, b)| a == b)
                    .count();
                // Beyond the last two, only the reopening of strokes from
                // an earlier start revises strokes, and it merges them.
                if kept + 2 < before.len() {
                    reopened += 1;
                }
                assert!(
                    kept + 2 >= before.len() || after.len() < before.len(),
                    "{}: earlier strokes revised at input {i} ({})",
                    s.name,
                    fmt(bar.dt)
                );
                let closed = centres_before.len().saturating_sub(1);
                for (k, z) in centres_before[..closed].iter().enumerate() {
                    let z: &chanorbit::Centre = z;
                    if z.end < kept {
                        assert_eq!(Some(z), centres.get(k), "{}: centre {k} at {i}", s.name);
                    }
                }
            }
            before = after;
            centres_before = centres;
        }
    }
    eprintln!("{reopened} appends reopened more than the last two strokes");
}
