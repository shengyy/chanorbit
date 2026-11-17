//! Stroke endpoints: a start is the most extreme raw price of its stroke, an
//! end is moved to a more extreme fractal before any reverse stroke forms, and
//! a stroke that extends stays. Synthetic shapes of the two cases the
//! definition was revised for, and the same rules after every input of every
//! baseline.
mod common;

use chanorbit::candidate::candidate_stroke;
use chanorbit::{Analyzer, Config, Direction, Mark, RawBar, Stroke, geometry};

fn bars(ranges: &[(f64, f64)]) -> Vec<RawBar> {
    let start = chanorbit::contract::parse_dt("2026-01-05T15:00:00").unwrap();
    ranges
        .iter()
        .enumerate()
        .map(|(i, &(high, low))| RawBar {
            dt: start + chrono::Duration::days(i as i64),
            high,
            low,
            close: (high + low) / 2.0,
            vol: 100.0,
        })
        .collect()
}

/// Each stroke as direction, start price and input index, end price and index.
fn ends(c: &Analyzer) -> Vec<(Direction, f64, usize, f64, usize)> {
    let start = chanorbit::contract::parse_dt("2026-01-05T15:00:00").unwrap();
    let day = |dt: chrono::NaiveDateTime| (dt - start).num_days() as usize;
    c.strokes()
        .iter()
        .map(|s| {
            let (from, to) = (day(s.start_at()), day(s.end_at()));
            (s.direction, s.start.price, from, s.end.price, to)
        })
        .collect()
}

/// Every raw bar from a stroke's start up to its end's bar is on the stroke's
/// side of the start; the bar holding the end may reach beyond it.
fn starts_are_extreme(c: &Analyzer, s: &Stroke) -> bool {
    c.raw_bars()
        .iter()
        .filter(|b| b.dt >= s.start_at() && b.dt < s.end_at())
        .all(|b| match s.direction {
            Direction::Up => b.low >= s.start.price,
            Direction::Down => b.high <= s.start.price,
        })
}

/// No fractal of the last stroke's end mark after its end lies beyond it.
fn end_is_extreme(c: &Analyzer) -> bool {
    let Some(s) = c.strokes().last() else {
        return true;
    };
    c.pending_fractals().iter().all(|f| {
        f.dt <= s.end_at()
            || match s.direction {
                Direction::Up => f.mark != Mark::Top || f.price <= s.end.price,
                Direction::Down => f.mark != Mark::Bottom || f.price >= s.end.price,
            }
    })
}

/// Case A: an outside bar after a bottom is merged upward, which hides its low
/// from the NewBars. The low becomes the shared endpoint.
#[test]
fn an_extreme_hidden_by_inclusion_ends_the_stroke() {
    #[rustfmt::skip]
    let input = bars(&[
        (10.2, 10.0), (10.5, 10.2), (10.8, 10.5), (11.1, 10.8), (11.4, 11.1), (11.7, 11.4),
        (12.0, 11.7), // 6: top
        (11.8, 11.5), (11.5, 11.2), (11.2, 10.9), (10.9, 10.6), (10.6, 10.3),
        (10.3, 10.1), // 12: bottom fractal
        (10.5, 10.2), (10.6, 10.3),
        (10.8, 9.8), // 15: outside bar, merged upward into [10.8, 10.3]
        (11.1, 10.7), (11.4, 11.0), (11.7, 11.3),
        (11.9, 11.5), // 19: top
        (11.6, 11.2), (11.3, 10.9),
    ]);
    let c = Analyzer::from_bars(input.clone(), Config::default());
    assert_eq!(
        ends(&c),
        [
            (Direction::Down, 12.0, 6, 9.8, 15),
            (Direction::Up, 9.8, 15, 11.9, 19)
        ]
    );
    assert!(c.strokes().iter().all(|s| starts_are_extreme(&c, s)));
    // The bottom fractal identified the stroke; the endpoint is the raw low.
    assert_eq!(c.strokes()[1].start_fractal.price, 10.1);
}

/// Case B, as the series grows: a rebound stroke whose start is broken before
/// its end is passed; the passing bar moves the down stroke's end to the true
/// low; the rise from there becomes a stroke that extends and stays.
#[test]
fn a_broken_start_hands_the_end_back_and_an_extending_stroke_stays() {
    #[rustfmt::skip]
    let input = bars(&[
        (10.0, 9.8), (10.3, 10.0), (10.6, 10.3), (10.9, 10.6), (11.2, 10.9),
        (11.5, 11.2), // 5: top S
        (11.3, 11.0), (11.1, 10.8), (10.9, 10.6), (10.7, 10.4), (10.5, 10.2),
        (10.3, 10.0), // 11: bottom P
        (10.5, 10.2), (10.7, 10.4), (10.9, 10.6), (11.1, 10.8),
        (11.3, 11.0), // 16: top Q
        (10.9, 10.3),
        (10.2, 9.6), // 18: bottom H below P, too close to Q for a down stroke
        (10.4, 9.8), (10.6, 10.0), (10.8, 10.2), (11.0, 10.4), (11.2, 10.6),
        (11.4, 10.8), // 24: passes Q
        (11.6, 11.0),
        (11.8, 11.2), // 26: top T
        (11.6, 11.0),
        (12.0, 11.4), // 28: passes T
        (12.2, 11.6), // 29: top
        (12.0, 11.5),
    ]);
    let config = Config::default();
    let at = |n: usize| Analyzer::from_bars(input[..n].iter().copied(), config);
    let (down, up) = (Direction::Down, Direction::Up);

    // H stands below the rebound's start: the decline may still become a stroke.
    let c = at(20);
    assert_eq!(
        ends(&c),
        [(down, 11.5, 5, 10.0, 11), (up, 10.0, 11, 11.3, 16)]
    );
    assert_eq!(candidate_stroke(&c).end_price, Some(9.6));

    // Passing Q cannot extend the rebound across H: the down stroke ends at H.
    let c = at(25);
    assert_eq!(ends(&c), [(down, 11.5, 5, 9.6, 18)]);
    let candidate = candidate_stroke(&c);
    assert_eq!(candidate.kind.as_deref(), Some("reverse"));
    assert_eq!(candidate.anchor_price, Some(9.6));

    // The rise from H is a stroke once T is a fractal, and stays while it extends.
    let c = at(28);
    assert_eq!(
        ends(&c),
        [(down, 11.5, 5, 9.6, 18), (up, 9.6, 18, 11.8, 26)]
    );
    for n in 29..=30 {
        let c = at(n);
        assert_eq!(ends(&c)[1..2], [(up, 9.6, 18, 11.8, 26)], "input {n}");
        assert_eq!(candidate_stroke(&c).kind.as_deref(), Some("extension"));
    }
    let c = at(31);
    assert_eq!(ends(&c)[1..], [(up, 9.6, 18, 12.2, 29)]);

    for n in 1..=input.len() {
        let c = at(n);
        assert!(
            c.strokes().iter().all(|s| starts_are_extreme(&c, s)),
            "input {n}"
        );
        assert!(end_is_extreme(&c), "input {n}");
    }
}

/// Revisions undo extensions, reopened strokes and moved endpoints exactly.
#[test]
fn revisions_undo_endpoint_changes() {
    #[rustfmt::skip]
    let input = bars(&[
        (10.0, 9.8), (10.3, 10.0), (10.6, 10.3), (10.9, 10.6), (11.2, 10.9), (11.5, 11.2),
        (11.3, 11.0), (11.1, 10.8), (10.9, 10.6), (10.7, 10.4), (10.5, 10.2), (10.3, 10.0),
        (10.5, 10.2), (10.7, 10.4), (10.9, 10.6), (11.1, 10.8), (11.3, 11.0), (10.9, 10.3),
        (10.2, 9.6), (10.4, 9.8), (10.8, 9.5), (11.0, 10.4), (11.2, 10.6), (11.4, 10.8),
        (11.6, 11.0), (11.8, 11.2), (11.6, 11.0), (12.0, 11.4), (12.2, 11.6), (12.0, 11.5),
    ]);
    let mut revised = Vec::new();
    for b in &input {
        for k in [1.04, 0.96] {
            revised.push(RawBar {
                high: b.high * k,
                low: b.low * k,
                close: b.close * k,
                ..*b
            });
        }
        revised.push(*b);
    }
    let config = Config::default();
    let clean = Analyzer::from_bars(input, config);
    let all = Analyzer::from_bars(revised, config);
    assert!(clean.strokes().len() >= 2);
    assert_eq!(geometry(&all), geometry(&clean));
    assert_eq!(all.raw_bars(), clean.raw_bars());
}

/// After every input of every baseline: starts are extreme, the last end is
/// extreme among the unfinished fractals, and the last stroke never returns
/// to the stroke before it (an extending stroke is kept, not withdrawn). The
/// first retained stroke has no stroke before it to reopen, so a later start
/// linked onto its end may carry it across its own start (`f5_corrections`).
#[test]
fn baselines_keep_endpoints_extreme_after_every_input() {
    let mut checked = 0;
    for path in common::golden_files() {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let (input, config) = common::input(&common::load(&path));
        let mut c = Analyzer::new(config);
        let mut previous: Option<Stroke> = None;
        for (i, bar) in input.iter().enumerate() {
            c.update(*bar);
            if input.get(i + 1).is_some_and(|b| b.dt == bar.dt) {
                continue;
            }
            for s in c.strokes().iter().skip(1) {
                assert!(starts_are_extreme(&c, s), "{name} input {i}: {s:?}");
            }
            assert!(end_is_extreme(&c), "{name} input {i}");
            let last = c.strokes().last().cloned();
            if let (Some(u), Some(d)) = (&previous, &last) {
                let returned = d.direction != u.direction
                    && d.start_at() < u.start_at()
                    && d.end_at() == u.start_at();
                assert!(!returned, "{name} input {i}: returned to {d:?}");
            }
            previous = last;
            checked += 1;
        }
    }
    assert!(checked > 6_000, "{checked}");
}
