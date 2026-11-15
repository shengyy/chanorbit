//! The `levels` output: every level analysed alone, then each finer level
//! placed inside the next coarser one (`docs/structure-contract.md#child-placement`).
use crate::bar::RawBar;
use crate::geometry::FractalFact;
use crate::levels::{
    AfterExtreme, Level, LevelCentre, LevelStroke, Membership, Pos, Seg, Unfinished, raw_extreme,
};
use crate::levels_compare::{Comparison, Comparisons};
use crate::stroke::Direction;
use chrono::NaiveDateTime;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PassedBy {
    pub segment: Pos,
    pub end_at: NaiveDateTime,
    pub price: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Point {
    pub at: NaiveDateTime,
    pub price: f64,
    pub child_at: Option<NaiveDateTime>,
    pub child_price: Option<f64>,
    pub passed_by: Option<PassedBy>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Points {
    pub last_stroke: Option<Point>,
    pub unfinished: Option<Point>,
    pub after_extreme: Option<Point>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Forces {
    pub last_stroke: Vec<Comparison>,
    pub unfinished: Vec<Comparison>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Boundaries {
    pub last_start: NaiveDateTime,
    pub last_end: NaiveDateTime,
    pub unfinished_extreme: Option<NaiveDateTime>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Placed {
    pub kind: &'static str,
    pub index: Option<usize>,
    pub start_at: NaiveDateTime,
    pub end_at: NaiveDateTime,
    pub zone: &'static str,
    pub crosses: Vec<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Zones {
    pub boundaries: Boundaries,
    pub objects: Vec<Placed>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Child {
    pub name: String,
    pub points: Points,
    pub forces: Forces,
    pub zones: Option<Zones>,
    pub zones_reason: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LevelOut {
    pub name: String,
    pub status: &'static str,
    pub last_input_at: NaiveDateTime,
    pub strokes: Vec<LevelStroke>,
    pub unfinished: Option<Unfinished>,
    pub after_extreme: Option<AfterExtreme>,
    pub fractals: Vec<FractalFact>,
    pub last_stroke_member: Option<Membership>,
    pub centres: Vec<LevelCentre>,
    pub comparisons: Comparisons,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub child: Option<Child>,
}

/// The finer bar inside the coarser bar at `at` (period `(previous dt, at]`)
/// holding the period's highest high (`high`) or lowest low, first occurrence.
pub fn child_time(
    parent: &[RawBar],
    child: &[RawBar],
    at: NaiveDateTime,
    high: bool,
) -> Option<(NaiveDateTime, f64)> {
    let k = parent.partition_point(|b| b.dt < at);
    let from = k.checked_sub(1).map(|p| parent[p].dt);
    let a = from.map_or(0, |f| child.partition_point(|b| b.dt <= f));
    let b = child.partition_point(|b| b.dt <= at);
    child[a..b.max(a)]
        .iter()
        .map(|c| (c.dt, if high { c.high } else { c.low }))
        .reduce(|x, y| {
            let beyond = if high { y.1 > x.1 } else { y.1 < x.1 };
            if beyond { y } else { x }
        })
}

fn up(d: Direction) -> bool {
    d == Direction::Up
}

/// Child segments: window strokes, then the unfinished reverse segment.
fn child_segments(c: &Level) -> Vec<Seg> {
    (0..c.strokes.len())
        .map(|k| c.seg(k))
        .chain(c.reverse_seg())
        .collect()
}

fn point(
    parent: &Level,
    child: &Level,
    segs: &[Seg],
    d: Direction,
    at: NaiveDateTime,
    price: f64,
) -> Point {
    let found = child_time(&parent.input, &child.input, at, up(d));
    // Any same-direction child segment ending after the child time whose
    // raw price after that time goes strictly beyond the child price.
    let passed_by = found.and_then(|(t, p)| {
        segs.iter()
            .filter(|s| s.end_at > t && s.direction == d)
            .find_map(|s| {
                let x = raw_extreme(child.raw(), d, s.start_at.max(t), s.end_at)?;
                let beyond = if up(d) { x.price > p } else { x.price < p };
                beyond.then_some(PassedBy {
                    segment: s.pos,
                    end_at: s.end_at,
                    price: x.price,
                })
            })
    });
    Point {
        at,
        price,
        child_at: found.map(|f| f.0),
        child_price: found.map(|f| f.1),
        passed_by,
    }
}

const ZONES: [&str; 6] = [
    "crosses_last_start",
    "inside_last",
    "crosses_last_end",
    "inside_unfinished",
    "crosses_unfinished_extreme",
    "after_extreme",
];

/// The zone of span `[a, b]` with `b > b1`: the first rule that applies.
fn zone_of(a: NaiveDateTime, b: NaiveDateTime, z: &Boundaries) -> &'static str {
    let (b1, b2) = (z.last_start, z.last_end);
    let crosses = |x: NaiveDateTime| a < x && x < b;
    let rules: [(bool, &'static str); 6] = match z.unfinished_extreme {
        Some(b3) => [
            (crosses(b1), ZONES[0]),
            (b1 <= a && b <= b2, ZONES[1]),
            (crosses(b2), ZONES[2]),
            (b2 <= a && b <= b3, ZONES[3]),
            (crosses(b3), ZONES[4]),
            (a >= b3, ZONES[5]),
        ],
        None => [
            (crosses(b1), ZONES[0]),
            (b1 <= a && b <= b2, ZONES[1]),
            (crosses(b2), ZONES[2]),
            (false, ZONES[3]),
            (false, ZONES[4]),
            (a >= b2, "after_last"),
        ],
    };
    let hits: Vec<&str> = rules.iter().filter(|r| r.0).map(|r| r.1).collect();
    let first = *hits
        .first()
        .unwrap_or_else(|| panic!("span [{a}, {b}] lands in no zone of {z:?}"));
    // Exclusive reading: once a rule applies, no earlier boundary is crossed
    // and the span does not start before the zone it lands in.
    let crossed_earlier = match first {
        "inside_last" | "crosses_last_end" => crosses(b1),
        "inside_unfinished" | "crosses_unfinished_extreme" | "after_extreme" | "after_last" => {
            crosses(b1) || crosses(b2) || a < b2
        }
        _ => false,
    };
    assert!(
        !crossed_earlier,
        "span [{a}, {b}] in {first} crosses an earlier boundary"
    );
    first
}

fn crossed(a: NaiveDateTime, b: NaiveDateTime, z: &Boundaries) -> Vec<&'static str> {
    [
        (Some(z.last_start), "last_start"),
        (Some(z.last_end), "last_end"),
        (z.unfinished_extreme, "unfinished_extreme"),
    ]
    .into_iter()
    .filter_map(|(x, name)| x.filter(|x| a < *x && *x < b).map(|_| name))
    .collect()
}

fn boundaries(parent: &Level, child: &Level) -> Result<Boundaries, &'static str> {
    let Some(last) = parent.strokes.last() else {
        return Err("no_parent_stroke");
    };
    let time = |at, high| child_time(&parent.input, &child.input, at, high).map(|f| f.0);
    let last_start = time(last.start_at, !up(last.direction)).ok_or("last_start_no_child_bar")?;
    let last_end = time(last.end_at, up(last.direction)).ok_or("last_end_no_child_bar")?;
    let unfinished_extreme = match parent.reverse_seg() {
        Some(u) => Some(time(u.end_at, up(u.direction)).ok_or("unfinished_extreme_no_child_bar")?),
        None => None,
    };
    Ok(Boundaries {
        last_start,
        last_end,
        unfinished_extreme,
    })
}

/// The child time of the parent's last-stroke start, when the parent has strokes.
pub fn cover_from(parent: &Level, child_input: &[RawBar]) -> Option<NaiveDateTime> {
    let last = parent.strokes.last()?;
    child_time(
        &parent.input,
        child_input,
        last.start_at,
        !up(last.direction),
    )
    .map(|f| f.0)
}

fn zones(child: &Level, z: &Boundaries) -> Vec<Placed> {
    let mut spans: Vec<(&'static str, Option<usize>, NaiveDateTime, NaiveDateTime)> = child
        .strokes
        .iter()
        .enumerate()
        .map(|(k, s)| ("stroke", Some(k), s.start_at, s.end_at))
        .collect();
    if let Some(u) = child.reverse_seg() {
        spans.push(("unfinished", None, u.start_at, u.end_at));
    }
    if let Some(p) = &child.after_extreme {
        spans.push(("after_extreme", None, p.start_at, p.end_at));
    }
    for (k, c) in child.centres.iter().enumerate() {
        spans.push(("centre", Some(k), c.start_at, c.end_at));
    }
    spans
        .into_iter()
        .filter(|s| s.3 > z.last_start)
        .map(|(kind, index, a, b)| Placed {
            kind,
            index,
            start_at: a,
            end_at: b,
            zone: zone_of(a, b, z),
            crosses: crossed(a, b, z),
        })
        .collect()
}

/// Child strokes inside `zone` sharing direction `d`, compared last against
/// first and against the previous one.
fn forces(child: &Level, placed: &[Placed], zone: &str, d: Direction) -> Vec<Comparison> {
    let inside: Vec<Seg> = placed
        .iter()
        .filter(|p| p.kind == "stroke" && p.zone == zone)
        .filter_map(|p| p.index)
        .map(|k| child.seg(k))
        .filter(|s| s.direction == d)
        .collect();
    let mut rows = Vec::new();
    if let [first, .., previous, last] = inside.as_slice() {
        rows.push(child.compare("last_vs_first", last, first));
        if inside.len() >= 3 {
            rows.push(child.compare("last_vs_previous", last, previous));
        }
    }
    rows
}

fn child_of(parent: &Level, child: &Level, name: &str) -> Child {
    let segs = child_segments(child);
    let last = parent.strokes.last();
    let points = Points {
        last_stroke: last.and_then(|s| {
            let x = s.raw_extreme?;
            Some(point(parent, child, &segs, s.direction, x.at, x.price))
        }),
        unfinished: parent.reverse_seg().and_then(|u| {
            let x = parent.unfinished.as_ref()?.raw_extreme?;
            Some(point(parent, child, &segs, u.direction, x.at, x.price))
        }),
        after_extreme: parent.after_extreme.as_ref().map(|p| {
            let x = p.raw_extreme;
            point(parent, child, &segs, p.direction, x.at, x.price)
        }),
    };
    let (zones, zones_reason, forces) = match boundaries(parent, child) {
        Err(reason) => (None, Some(reason), Forces::default()),
        Ok(b) => {
            let objects = zones(child, &b);
            let forces = Forces {
                last_stroke: last.map_or_else(Vec::new, |s| {
                    forces(child, &objects, "inside_last", s.direction)
                }),
                unfinished: parent.reverse_seg().map_or_else(Vec::new, |u| {
                    forces(child, &objects, "inside_unfinished", u.direction)
                }),
            };
            (
                Some(Zones {
                    boundaries: b,
                    objects,
                }),
                None,
                forces,
            )
        }
    };
    Child {
        name: name.into(),
        points,
        forces,
        zones,
        zones_reason,
    }
}

fn output(level: &Level, name: &str, child: Option<Child>) -> LevelOut {
    let ready = !level.strokes.is_empty();
    LevelOut {
        name: name.into(),
        status: if ready { "ready" } else { "missing_material" },
        last_input_at: level.input.last().expect("a level has bars").dt,
        strokes: level.strokes.clone(),
        unfinished: level.unfinished.clone(),
        after_extreme: level.after_extreme.clone(),
        fractals: level.fractals.clone(),
        last_stroke_member: ready.then(|| level.membership.clone()),
        centres: level.centres.clone(),
        comparisons: level.comparisons.clone(),
        child,
    }
}

/// Every level, coarsest first: analysed alone, then placed in its parent.
pub fn levels(inputs: &[(String, Vec<RawBar>)]) -> Vec<LevelOut> {
    let mut built: Vec<Level> = Vec::with_capacity(inputs.len());
    for (_, bars) in inputs {
        let mut level = Level::new(bars);
        let cover = built.last().and_then(|p| cover_from(p, &level.input));
        level.build(cover);
        built.push(level);
    }
    (0..built.len())
        .map(|i| {
            let child = built
                .get(i + 1)
                .map(|c| child_of(&built[i], c, &inputs[i + 1].0));
            output(&built[i], &inputs[i].0, child)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").unwrap()
    }

    fn bar(dt: &str, high: f64, low: f64) -> RawBar {
        RawBar {
            dt: t(dt),
            high,
            low,
            close: low,
            vol: 1.0,
        }
    }

    #[test]
    fn child_time_is_the_first_extreme_bar_inside_the_parent_period() {
        let parent = [
            bar("2026-01-05T15:00:00", 3.0, 1.0),
            bar("2026-01-06T15:00:00", 3.0, 1.0),
        ];
        let child = [
            bar("2026-01-05T14:30:00", 9.0, 0.1),
            bar("2026-01-06T10:00:00", 2.5, 1.2),
            bar("2026-01-06T11:00:00", 2.9, 1.1),
            bar("2026-01-06T14:00:00", 2.9, 1.1),
            bar("2026-01-06T15:00:00", 2.0, 1.5),
        ];
        let at = t("2026-01-06T15:00:00");
        assert_eq!(
            child_time(&parent, &child, at, true),
            Some((t("2026-01-06T11:00:00"), 2.9))
        );
        assert_eq!(
            child_time(&parent, &child, at, false),
            Some((t("2026-01-06T11:00:00"), 1.1))
        );
        // The first parent bar covers everything up to its dt.
        assert_eq!(
            child_time(&parent, &child, t("2026-01-05T15:00:00"), true),
            Some((t("2026-01-05T14:30:00"), 9.0))
        );
        assert_eq!(
            child_time(&parent, &child[1..], t("2026-01-05T15:00:00"), true),
            None
        );
    }

    #[test]
    fn every_span_after_the_last_start_lands_in_exactly_one_zone() {
        let base = t("2026-01-05T09:00:00");
        let at = |m: i64| base + chrono::Duration::minutes(m);
        for b3 in [None, Some(at(30))] {
            let z = Boundaries {
                last_start: at(10),
                last_end: at(20),
                unfinished_extreme: b3,
            };
            for a in 0..45 {
                for b in a + 1..46 {
                    if at(b) <= z.last_start {
                        continue;
                    }
                    let zone = zone_of(at(a), at(b), &z);
                    let crosses = crossed(at(a), at(b), &z);
                    let expect = match zone {
                        "crosses_last_start" => crosses.first() == Some(&"last_start"),
                        "inside_last" => a >= 10 && b <= 20 && crosses.is_empty(),
                        "crosses_last_end" => a >= 10 && crosses.first() == Some(&"last_end"),
                        "inside_unfinished" => a >= 20 && b <= 30 && crosses.is_empty(),
                        "crosses_unfinished_extreme" => {
                            a >= 20 && crosses == ["unfinished_extreme"]
                        }
                        "after_extreme" => b3.is_some() && a >= 30,
                        "after_last" => b3.is_none() && a >= 20,
                        _ => false,
                    };
                    assert!(expect, "[{a}, {b}] with {b3:?}: {zone} {crosses:?}");
                }
            }
        }
    }
}
