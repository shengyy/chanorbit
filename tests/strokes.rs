//! Stroke endpoints: both ends are the most extreme raw prices of the stroke's
//! raw span, an end is moved to a more extreme fractal before any reverse
//! stroke forms, a stroke that extends stays, and the candidate ends at the
//! most extreme raw price after the last stroke. Synthetic shapes of the cases
//! the definition was revised for, and the same rules after every input of
//! every baseline.
mod common;

use chanorbit::candidate::candidate_stroke;
use chanorbit::stroke::extreme_at;
use chanorbit::{Analyzer, Config, Direction, Mark, RawBar, Stroke, geometry};

fn input_start() -> chrono::NaiveDateTime {
    chanorbit::contract::parse_dt("2026-01-05T15:00:00").unwrap()
}

fn bars(ranges: &[(f64, f64)]) -> Vec<RawBar> {
    let start = input_start();
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
    let start = input_start();
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

/// No raw bar after a stroke's start up to its end is beyond its end.
fn ends_are_extreme(c: &Analyzer, s: &Stroke) -> bool {
    c.raw_bars()
        .iter()
        .filter(|b| b.dt > s.start_at() && b.dt <= s.end_at())
        .all(|b| match s.direction {
            Direction::Up => b.high <= s.end.price,
            Direction::Down => b.low >= s.end.price,
        })
}

/// No raw bar after the last stroke's end is beyond it unless the candidate
/// extends it, and none after the candidate's end is beyond that end.
fn candidate_is_extreme(c: &Analyzer) -> bool {
    let (Some(s), k) = (c.strokes().last(), candidate_stroke(c)) else {
        return true;
    };
    let past = |dt, up: bool, price: f64| {
        c.raw_bars()
            .iter()
            .any(|b| b.dt > dt && if up { b.high > price } else { b.low < price })
    };
    let up = s.direction == Direction::Up;
    let extension = k.kind.as_deref() == Some("extension");
    let end_stands = extension || !past(s.end_at(), up, s.end.price);
    let candidate_stands = match (k.end_price_at, k.end_price, k.direction) {
        (Some(at), Some(price), Some(d)) => !past(at, d == Direction::Up, price),
        _ => true,
    };
    end_stands && candidate_stands
}

/// Raw bars strictly between the extreme raw bars of a stroke's two fractals.
fn raw_gap(c: &Analyzer, s: &Stroke) -> usize {
    let (from, to) = (extreme_at(&s.start_fractal), extreme_at(&s.end_fractal));
    c.raw_bars()
        .iter()
        .filter(|b| b.dt > from && b.dt < to)
        .count()
}

/// Every stroke end, and the candidate end, is held by the first raw bar
/// after its start that reaches it: no earlier bar has the same price.
fn ends_are_first(c: &Analyzer) -> bool {
    let earlier = |from, to, up: bool, price: f64| {
        c.raw_bars()
            .iter()
            .any(|b| b.dt > from && b.dt < to && (if up { b.high } else { b.low }) == price)
    };
    let strokes = c.strokes().iter().all(|s| {
        !earlier(
            s.start_at(),
            s.end_at(),
            s.direction == Direction::Up,
            s.end.price,
        )
    });
    let k = candidate_stroke(c);
    let candidate = match (k.anchor_at, k.end_price_at, k.end_price, k.direction) {
        (Some(a), Some(at), Some(price), Some(d)) => !earlier(a, at, d == Direction::Up, price),
        _ => true,
    };
    strokes && candidate
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

/// A wide bar holds a stroke's end and a high beyond its start (the end bar
/// note); the next stroke starts at a low that inclusion hid, two bars later.
/// Moving the end there would put that high inside the stroke, so the stroke
/// starts again at it and the stroke before ends there.
#[test]
fn a_moved_end_restarts_the_stroke_across_its_end_bar() {
    #[rustfmt::skip]
    let input = bars(&[
        (13.76, 13.42), (13.60, 13.40), (13.78, 13.51), (13.96, 13.75), (14.10, 13.90),
        (14.45, 14.37), (14.24, 14.23), (14.20, 14.07), (13.80, 13.64),
        (14.51, 12.51), // 9: end of the down stroke and a high beyond its start
        (12.76, 12.64), (12.80, 12.66),
        (13.84, 11.84), // 12: a low merged upward out of sight
        (12.94, 12.61), (14.02, 13.76), (13.83, 13.66),
    ]);
    let config = Config::default();
    let at = |n: usize| Analyzer::from_bars(input[..n].iter().copied(), config);
    let (down, up) = (Direction::Down, Direction::Up);
    // Before the next stroke: the end bar holds both prices.
    assert_eq!(
        ends(&at(15)),
        [(up, 13.40, 1, 14.45, 5), (down, 14.45, 5, 12.51, 9)]
    );
    let c = at(16);
    assert_eq!(
        ends(&c),
        [
            (up, 13.40, 1, 14.51, 9),
            (down, 14.51, 9, 11.84, 12),
            (up, 11.84, 12, 14.02, 14)
        ]
    );
    for n in 1..=input.len() {
        let c = at(n);
        assert!(
            c.strokes().iter().all(|s| starts_are_extreme(&c, s)),
            "input {n}"
        );
    }
    // Earlier versions of every input leave no trace of the restart.
    let revised: Vec<RawBar> = input
        .iter()
        .flat_map(|b| {
            [
                RawBar {
                    high: b.high + 0.3,
                    low: b.low - 0.3,
                    ..*b
                },
                *b,
            ]
        })
        .collect();
    let all = Analyzer::from_bars(revised, config);
    assert_eq!(geometry(&all), geometry(&c));
}

/// An outside bar merged upward hides a low in the bottom fractal's first
/// NewBar, or in the NewBar before it; the low ends the stroke either way.
#[test]
fn an_extreme_hidden_before_the_end_fractal_ends_the_stroke() {
    #[rustfmt::skip]
    let fall = [
        (10.2, 10.0), (10.5, 10.2), (10.8, 10.5), (11.1, 10.8), (11.4, 11.1), (11.7, 11.4),
        (12.0, 11.7), // 6: top
        (11.8, 11.5), (11.5, 11.2), (11.2, 10.9), (10.9, 10.6), (10.6, 10.3),
        (10.4, 10.2), (10.5, 10.3),
        (10.9, 9.9), // 14: outside bar, merged upward into [10.9, 10.3]
    ];
    #[rustfmt::skip]
    let rise = [(10.6, 10.2), (10.9, 10.5), (11.2, 10.8), (11.5, 11.1), (11.8, 11.4), (12.1, 11.7), (11.8, 11.4), (11.5, 11.1)];
    let (down, up) = (Direction::Down, Direction::Up);
    for (between, fractal, top) in [(&[][..], 15, 21), (&[(10.7, 10.25)][..], 16, 22)] {
        let input: Vec<_> = fall
            .iter()
            .chain(between)
            .chain(&[(10.4, 10.0)])
            .chain(&rise)
            .copied()
            .collect();
        let c = Analyzer::from_bars(bars(&input), Config::default());
        assert_eq!(
            ends(&c),
            [(down, 12.0, 6, 9.9, 14), (up, 9.9, 14, 12.1, top)]
        );
        assert_eq!(c.strokes()[1].start_fractal.price, 10.0);
        let day = |s: &Stroke| (s.start_fractal.dt - input_start()).num_days();
        assert_eq!(day(&c.strokes()[1]), fractal);
    }
}

/// An outside bar merged downward hides the high of the rise after the last
/// stroke; the candidate ends there.
#[test]
fn a_candidate_ends_at_an_extreme_hidden_by_inclusion() {
    #[rustfmt::skip]
    let input = bars(&[
        (10.2, 10.0), (10.5, 10.2), (10.8, 10.5), (11.1, 10.8), (11.4, 11.1), (11.7, 11.4),
        (12.0, 11.7), (11.8, 11.5), (11.5, 11.2), (11.2, 10.9), (10.9, 10.6), (10.6, 10.3),
        (10.3, 10.0), // 12: bottom
        (10.6, 10.2), (10.9, 10.5), (10.8, 10.4),
        (11.0, 10.1), // 16: outside bar, merged downward into [10.8, 10.1]
    ]);
    let c = Analyzer::from_bars(input.clone(), Config::default());
    assert_eq!(ends(&c), [(Direction::Down, 12.0, 6, 10.0, 12)]);
    let candidate = candidate_stroke(&c);
    assert_eq!(candidate.kind.as_deref(), Some("reverse"));
    assert_eq!(
        (candidate.end_price, candidate.end_price_at),
        (Some(11.0), Some(input[16].dt))
    );
}

/// A wide bar ends a rebound and reaches below both the rebound's start and
/// the next stroke's end: no order of its high and low gives both strokes
/// their extremes, so the decline before them ends at its low.
#[test]
fn a_bar_beyond_both_ends_cannot_end_the_stroke() {
    #[rustfmt::skip]
    let input = bars(&[
        (10.2, 10.0), (10.5, 10.2), (10.8, 10.5), (11.1, 10.8), (11.4, 11.1), (11.7, 11.4),
        (12.0, 11.7), (11.8, 11.5), (11.5, 11.2), (11.2, 10.9), (10.9, 10.6), (10.6, 10.3),
        (10.3, 10.0), // 12: bottom
        (10.5, 10.2), (10.7, 10.4), (10.9, 10.6), (11.1, 10.8),
        (11.3, 11.0), // 17: top
        (11.1, 10.8), (10.9, 10.6), (10.7, 10.4), (10.5, 10.2),
        (10.35, 10.15), // 22: bottom above 10.0
        (10.5, 10.3), (10.6, 10.4),
        (11.5, 9.8), // 25: merged upward, its low hidden
        (10.9, 10.3),
        (10.6, 9.95), // 27: bottom between 9.8 and 10.0
        (10.8, 10.2), (11.0, 10.5), (11.3, 10.8), (11.6, 11.1), (11.9, 11.4),
        (12.2, 11.7), // 33: top
        (11.9, 11.4), (11.6, 11.1),
    ]);
    let c = Analyzer::from_bars(input, Config::default());
    assert_eq!(
        ends(&c),
        [
            (Direction::Down, 12.0, 6, 9.8, 25),
            (Direction::Up, 9.8, 25, 12.2, 33)
        ]
    );
}

/// Two raw bars between the bottom's and the top's extreme bars form no
/// stroke, even with six NewBars; an equal later top does not unlock it, a
/// higher one does. Three raw bars between form the stroke at once.
#[test]
fn a_stroke_needs_three_raw_bars_between_its_fractals() {
    #[rustfmt::skip]
    let input = bars(&[
        (10.5, 10.2), (10.3, 10.0), // 1: bottom
        (10.6, 10.3), (10.9, 10.6),
        (11.2, 10.9), // 4: top, two raw bars after the bottom
        (11.0, 10.7), (10.8, 10.5), (10.9, 10.6), (11.1, 10.8),
        (11.2, 10.95), // 9: an equal top
        (11.0, 10.7), (10.8, 10.5), (11.1, 10.8),
        (11.4, 11.1), // 13: a higher top
        (11.2, 10.9),
    ]);
    let config = Config::default();
    let at = |n: usize| Analyzer::from_bars(input[..n].iter().copied(), config);
    for n in [6, 11, 14] {
        assert_eq!(ends(&at(n)), [], "input {n}");
    }
    let c = at(15);
    assert_eq!(ends(&c), [(Direction::Up, 10.0, 1, 11.4, 13)]);
    assert_eq!(raw_gap(&c, &c.strokes()[0]), 11);

    #[rustfmt::skip]
    let input = bars(&[
        (10.5, 10.2), (10.3, 10.0), (10.6, 10.3), (10.8, 10.5), (11.0, 10.7),
        (11.2, 10.9), // 5: top, three raw bars after the bottom
        (11.0, 10.7),
    ]);
    let c = Analyzer::from_bars(input, config);
    assert_eq!(ends(&c), [(Direction::Up, 10.0, 1, 11.2, 5)]);
    assert_eq!(raw_gap(&c, &c.strokes()[0]), 3);
}

/// Equal lows: the stroke ends at the first raw bar holding its low, though
/// inclusion dates the bottom fractal at the later one, and the candidate of
/// an extending stroke stops at the first bar reaching the new low; once the
/// extension is a stroke it ends there too.
#[test]
fn an_equal_extreme_belongs_to_the_first_bar_reaching_it() {
    #[rustfmt::skip]
    let input = bars(&[
        (10.6, 10.3), (11.0, 10.7),
        (11.4, 11.1), // 2: top
        (11.2, 10.9), (11.0, 10.7), (10.8, 10.5), (10.6, 10.3),
        (10.4, 10.0), // 7: first 10.0
        (10.3, 10.0), // 8: inside 7, the merged bottom's time
        (10.5, 10.2),
        (10.3, 9.8), // 10: first 9.8
        (10.2, 9.9),
        (10.0, 9.8), // 12: 9.8 again
        (10.4, 10.1), (10.6, 10.3),
    ]);
    let config = Config::default();
    let at = |n: usize| Analyzer::from_bars(input[..n].iter().copied(), config);
    let start = input_start();
    let day = |dt: Option<chrono::NaiveDateTime>| dt.map(|t| (t - start).num_days());
    let c = at(10);
    assert_eq!(c.strokes()[0].end_fractal.dt, input[8].dt);
    assert_eq!(ends(&c), [(Direction::Down, 11.4, 2, 10.0, 7)]);
    for n in 11..=13 {
        let k = candidate_stroke(&at(n));
        assert_eq!(k.kind.as_deref(), Some("extension"), "input {n}");
        assert_eq!(
            (k.end_price, day(k.end_price_at)),
            (Some(9.8), Some(10)),
            "input {n}"
        );
    }
    assert_eq!(ends(&at(15)), [(Direction::Down, 11.4, 2, 9.8, 10)]);
    for n in 1..=input.len() {
        assert!(ends_are_first(&at(n)), "input {n}");
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

/// After every input of every baseline: starts and ends are extreme over the
/// stroke's raw span, every stroke has three raw bars between its fractals'
/// extremes, ends are held by the first bar reaching them, the last end is
/// extreme among the unfinished fractals,
/// the candidate leaves no raw price beyond the last end or its own, and the
/// last stroke never returns to the stroke before it (an extending stroke is
/// kept, not withdrawn). The first stroke is checked too.
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
            for s in c.strokes() {
                assert!(starts_are_extreme(&c, s), "{name} input {i}: {s:?}");
                assert!(ends_are_extreme(&c, s), "{name} input {i}: {s:?}");
                assert!(raw_gap(&c, s) >= 3, "{name} input {i}: {s:?}");
            }
            assert!(ends_are_first(&c), "{name} input {i}");
            assert!(end_is_extreme(&c), "{name} input {i}");
            assert!(candidate_is_extreme(&c), "{name} input {i}");
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
