//! A centre's state read from its raw path after the third member
//! (`docs/structure-contract.md#centre-state`), over a level's sequence.
use crate::bar::RawBar;
use crate::centre::Centre;
use crate::levels::{Pos, RetestRole, Seg, State, position, raw_extreme, raw_extreme_from, rows};
use crate::stroke::Direction;
use chrono::NaiveDateTime;

/// Side of a raw bar against the core: `above` (low over `zg`), `below`
/// (high under `zd`), otherwise `inside`.
fn side(b: &RawBar, zd: f64, zg: f64) -> &'static str {
    if b.low > zg {
        "above"
    } else if b.high < zd {
        "below"
    } else {
        "inside"
    }
}

/// The state of centre `z` (`docs/structure-contract.md#centre-state`) over a
/// level's sequence `segs`, whose first elements are the window strokes from
/// stroke `w0` on; `next` is the next listed centre, `k` this one's position.
pub(crate) fn centre_state(
    raw: &[RawBar],
    segs: &[Seg],
    w0: usize,
    z: &Centre,
    next: Option<&Centre>,
    k: usize,
) -> State {
    let (zd, zg) = (z.core_low, z.core_high);
    let members = z.member_count();
    let third = z.start + 2 - w0;
    let last = raw.last().expect("a level with strokes has bars");
    let (until, end_side) = match next {
        Some(_) => {
            let closing = &segs[z.end - w0];
            (closing.end_at, position(closing.end_price, zd, zg))
        }
        None => (last.dt, position(last.close, zd, zg)),
    };
    let containing = |t: NaiveDateTime| {
        segs.iter()
            .position(|s| s.start_at < t && t <= s.end_at)
            .unwrap_or_else(|| panic!("no segment holds {t} (centre {k})"))
    };
    if end_side == "inside" {
        assert!(
            next.is_none(),
            "centre {k}: the stroke that closed it ends inside its core [{zd}, {zg}]"
        );
        let at = if last.dt > segs[third].end_at {
            containing(last.dt)
        } else {
            third
        };
        return State::Inside {
            segment: segs[at].pos,
        };
    }
    let path = &raw[rows(raw, segs[third].end_at, until)];
    let after_third = |to: usize| segs[third + 1..to].iter().map(|s| s.pos).collect();
    // The path starts at the third member's end price, then its raw bars.
    let start_met = position(segs[third].end_price, zd, zg) != end_side;
    let above = end_side == "above";
    let exit_direction = if above {
        Direction::Up
    } else {
        Direction::Down
    };
    // The exit is the last bar touching the core when it closes beyond
    // the edge and the element holding it runs in the exit direction;
    // otherwise the bar after it (that bar itself when it is the last).
    let star = match path.iter().rposition(|b| side(b, zd, zg) != end_side) {
        Some(i) => {
            let b = &path[i];
            let closed_out = if above { b.close > zg } else { b.close < zd };
            let runs_out = segs
                .iter()
                .find(|s| s.start_at < b.dt && b.dt <= s.end_at)
                .is_some_and(|s| s.direction == exit_direction);
            if closed_out && runs_out {
                Some(b)
            } else {
                path.get(i + 1).or(Some(b))
            }
        }
        None if start_met => path.first(),
        None => None,
    };
    let Some(exit) = star else {
        assert!(
            members == 3,
            "centre {k}: {members} members but the path never meets the core [{zd}, {zg}] (ended {}, path {} bars after {}, until {until}, side {end_side})",
            next.is_some(),
            path.len(),
            segs[third].end_at
        );
        let to = match next {
            Some(_) => z.end - w0 + 1,
            None => segs.len(),
        };
        return State::FormedOut {
            third: Pos::Stroke(2 + z.start - w0),
            after: after_third(to),
        };
    };
    let j = containing(exit.dt);
    let x = segs[j];
    let departure_stroke = match x.pos {
        Pos::Stroke(p) => Some(p),
        _ => None,
    };
    let retest = match x.pos {
        Pos::Stroke(_) => segs
            .get(j + 1)
            .filter(|r| matches!(r.pos, Pos::Stroke(_) | Pos::Unfinished))
            .copied(),
        _ => None,
    };
    // Toward the core: the retest's lowest low after an upward exit,
    // its highest high after a downward one.
    let toward = if above {
        Direction::Down
    } else {
        Direction::Up
    };
    let retest_extreme = retest.and_then(|r| raw_extreme(raw, toward, r.start_at, r.end_at));
    let versus_edge = retest_extreme.map(|e| {
        let (edge, beyond) = if above {
            (zg, e.price > zg)
        } else {
            (zd, e.price < zd)
        };
        if e.price == edge {
            "at"
        } else if beyond {
            "beyond"
        } else {
            "inside"
        }
    });
    let retest_role = match (next, retest.map(|r| r.pos)) {
        (Some(n), Some(Pos::Stroke(p))) => {
            let p = p + w0;
            let role = if p + 1 == n.start {
                Some("entry")
            } else if (n.start..n.end).contains(&p) {
                Some("member")
            } else {
                None
            };
            role.map(|role| RetestRole {
                centre: k + 1,
                role,
            })
        }
        _ => None,
    };
    let toward_exit = x.direction == exit_direction;
    // Against the exit side: the extreme on the exit side from the exit bar on.
    let extreme = if toward_exit {
        raw_extreme(raw, x.direction, x.start_at, x.end_at)
    } else {
        raw_extreme_from(raw, exit_direction, exit.dt, x.end_at)
    };
    // Beyond every member but the departure itself, at the endpoints the
    // centre's `dd`/`gg` are taken from; an equal price is not new.
    let others = (z.start - w0..z.end - w0)
        .filter(|&i| Some(i) != departure_stroke)
        .map(|i| &segs[i]);
    let new_extreme = extreme.filter(|_| toward_exit).map(|e| {
        if above {
            e.price
                > others
                    .map(|s| s.start_price.max(s.end_price))
                    .fold(f64::NEG_INFINITY, f64::max)
        } else {
            e.price
                < others
                    .map(|s| s.start_price.min(s.end_price))
                    .fold(f64::INFINITY, f64::min)
        }
    });
    State::Departed {
        departure: x.pos,
        direction: x.direction,
        edge: if above { "zg" } else { "zd" },
        raw_extreme: extreme,
        new_extreme,
        member: departure_stroke.is_some_and(|p| p + w0 < z.end),
        first_beyond_at: exit.dt,
        retest: retest.map(|r| r.pos),
        retest_extreme,
        versus_edge,
        after_retest: next.is_none().then(|| match retest {
            Some(_) => segs[j + 2..].iter().map(|s| s.pos).collect(),
            None => Vec::new(),
        }),
        retest_role,
    }
}
