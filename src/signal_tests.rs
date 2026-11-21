//! Unit tests of `signal`: the area reading and the leaf centre search.
use super::*;
use crate::analyzer::Config;
use crate::macd::raw_macd;
use crate::power::interval_power;

/// The area read here is the interval power's same-direction area, for
/// every stroke and for intervals inside the MACD warm-up.
#[test]
fn area_is_the_interval_power_area() {
    let t0 = crate::contract::parse_dt("2026-01-05T09:35:00").unwrap();
    let mut price = 100.0_f64;
    let bars: Vec<RawBar> = (0..900)
        .map(|i| {
            let from = price;
            price += if (i / 9) % 2 == 0 { 0.37 } else { -0.31 };
            RawBar {
                dt: t0 + chrono::Duration::minutes(5 * i),
                high: from.max(price) + 0.05,
                low: from.min(price) - 0.05,
                close: price,
                vol: 1.0,
            }
        })
        .collect();
    let c = Analyzer::from_bars(bars, Config::default());
    let macd = raw_macd(&c);
    let r = LevelRead::new(&c, &macd);
    assert!(r.n() > 40);
    let mut ready = 0;
    for k in 0..r.n() {
        let s = r.seg(k);
        let power = interval_power(&c, &macd, s.start_at, s.end_at).macd;
        let want = match s.direction {
            Direction::Up => power.positive_area,
            Direction::Down => power.negative_area,
        };
        assert_eq!(r.area(&s).map(f64::to_bits), want.map(f64::to_bits), "{k}");
        ready += usize::from(want.is_some());
    }
    assert!(ready > 0 && ready < r.n());
}

/// A leaf centre is found inside a span whose start is not a stroke
/// start, where the local sequence (item 0) finds nothing.
#[test]
fn a_centre_within_needs_no_aligned_first_stroke() {
    let t0 = crate::contract::parse_dt("2026-01-05T09:31:00").unwrap();
    let mut price = 100.0_f64;
    let bars: Vec<RawBar> = (0..240)
        .map(|i| {
            let from = price;
            price += if (i / 8) % 2 == 0 { -0.30 } else { 0.24 };
            RawBar {
                dt: t0 + chrono::Duration::minutes(i),
                high: from.max(price) + 0.01,
                low: from.min(price) - 0.01,
                close: price,
                vol: 1.0,
            }
        })
        .collect();
    let c = Analyzer::from_bars(bars, Config::default());
    let macd = raw_macd(&c);
    let r = LevelRead::new(&c, &macd);
    let s = c.strokes();
    let k = s
        .iter()
        .position(|x| x.direction == Direction::Down)
        .unwrap();
    let from = s[k].start_at() + chrono::Duration::minutes(1);
    let to = c.raw_bars().last().unwrap().dt;
    assert!(local(&r, s[k].start_at(), None, Direction::Down, false).is_some());
    assert!(local(&r, from, None, Direction::Down, false).is_none());
    assert!(r.centre_within(from, to));
    assert!(!r.centre_within(from, s[k + 3].end_at()));
}
