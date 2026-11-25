//! Order lines (option `order-lines`, `docs/design/extras.md`): while your own units are selected, a thin line runs
//! from each to where its orders take it, then on through each shift-queued order in turn, with a small marker at every
//! stop; from a selected training building a line runs to its rally point. The colour says what the order is: green
//! moves (and patrols and following), red attacks, yellow work (harvest, build, repair, carrying goods).
//!
//! Our own design, not the original's, in the spirit of other real-time strategy games' order lines. It only reads the
//! world and draws: no command, no sim state, nothing synced.
//!
//! **Why it is client-only (C) and not C+L.** It shows the player what they ordered and nothing about the enemy:
//! - only units whose owner is the local player are drawn from (an enemy or allied unit in the selection never is);
//! - a goal given to a unit is the player's own click, which they know;
//! - a *unit* an order points at (an attack target, a follow, a repair, a mine) is drawn to only when the player can see
//!   it now, or it is a building they have a remembered look of, or it is their own; an order whose target has gone into
//!   the fog is left out and the line carries on from the last point it drew, so a chase never gives away where an
//!   enemy ran to;
//! - the queue is the player's own `shift-queue`; the rally point is the player's own click.
//!
//! In a replay or as an observer there is no own unit, so nothing is drawn.
//!
//! The rules here are pure (orders in, segments out, pixels through a callback) so they are tested without a game;
//! `view` draws the segments over the map and `hud` over the minimap.

use tidewater_sim::{Order, UnitId, World};

/// The colours, palette indices of the information panel's health bar (green, yellow, red).
pub const GREEN: u8 = 149;
pub const YELLOW: u8 = 251;
pub const RED: u8 = 249;

/// The most segments one unit contributes: its current order (a patrol is two) and the queue's cap of 8.
pub const MAX_SEGMENTS: usize = 10;
/// The dash pattern along a line, in map pixels: this many drawn, then this many left out.
pub const DASH_ON: i32 = 4;
pub const DASH_OFF: i32 = 4;
/// The marker at a stop reaches this far from its centre.
pub const MARKER: i32 = 2;

/// What an order is, which sets the colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Moving, patrolling, following, boarding: green.
    Move,
    /// Attacking: red.
    Attack,
    /// Harvesting, building, repairing, carrying goods: yellow.
    Work,
    /// A move onto a unit that the command turned into a walk (a right-click on a mine, a friend or an enemy): what
    /// the unit is decides, see [`relation`]. Never in a drawn [`Segment`].
    ByTarget,
}

impl Kind {
    pub fn colour(self) -> u8 {
        match self {
            Kind::Move => GREEN,
            Kind::Attack => RED,
            Kind::Work => YELLOW,
            Kind::ByTarget => GREEN,
        }
    }
}

/// Where an order goes, before the world says where that is on the map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Aim {
    /// The middle of a tile.
    Tile((i32, i32)),
    /// A unit, wherever it is now.
    Unit(UnitId),
    /// The middle of the footprint of building `kind` whose top-left tile is the second field.
    Site(u8, (i32, i32)),
}

/// One stop of a unit's route.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Leg {
    pub kind: Kind,
    pub aim: Aim,
}

/// A line to draw, in map pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segment {
    pub kind: Kind,
    pub from: (i32, i32),
    pub to: (i32, i32),
}

/// The stops an order stands for: none for an order that goes nowhere (idle, stand ground, inside a building, a spell).
pub fn describe(order: Order) -> Vec<Leg> {
    let one = |kind, aim| vec![Leg { kind, aim }];
    let unit_or = |t: Option<UnitId>, tile| t.map_or(Aim::Tile(tile), Aim::Unit);
    match order {
        Order::Move { goal, target } => one(target.map_or(Kind::Move, |_| Kind::ByTarget), unit_or(target, goal)),
        Order::MoveAttack { goal, target } => one(Kind::Attack, unit_or(target, goal)),
        Order::AttackMove { goal } => one(Kind::Attack, Aim::Tile(goal)),
        Order::Patrol { to, back } => vec![
            Leg {
                kind: Kind::Move,
                aim: Aim::Tile(to),
            },
            Leg {
                kind: Kind::Move,
                aim: Aim::Tile(back),
            },
        ],
        Order::Follow { target } | Order::Guard { target } | Order::Board { transport: target } => {
            one(Kind::Move, Aim::Unit(target))
        }
        Order::Land { goal } => one(Kind::Move, Aim::Tile(goal)),
        Order::Attack { target }
        | Order::Chase { target }
        | Order::AttackInPlace { target }
        | Order::IdleReact { target } => one(Kind::Attack, Aim::Unit(target)),
        Order::TowerWatch { target: Some(target) } => one(Kind::Attack, Aim::Unit(target)),
        Order::AttackWall { tile }
        | Order::AttackGround { tile }
        | Order::AttackGroundMove { tile, .. }
        | Order::DemolishAt { tile } => one(Kind::Attack, Aim::Tile(tile)),
        Order::Demolish { at, tile } => one(Kind::Attack, unit_or(at, tile)),
        Order::Repair { target } => one(Kind::Work, Aim::Unit(target)),
        Order::Harvest { source, tree, .. } => one(Kind::Work, unit_or(source, tree)),
        Order::ReturnGoods { depot: Some(depot) } => one(Kind::Work, Aim::Unit(depot)),
        Order::Enter { building, .. } => one(Kind::Work, Aim::Unit(building)),
        Order::Build { kind, site, .. } => one(Kind::Work, Aim::Site(kind, site)),
        _ => Vec::new(),
    }
}

/// Where a stop is on the map, and for a unit what it is to the player (it sets the colour of a [`Kind::ByTarget`] leg).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Place {
    pub at: (i32, i32),
    pub kind: Option<Kind>,
}

impl Place {
    fn at(at: (i32, i32)) -> Self {
        Place { at, kind: None }
    }
}

/// What walking up to a unit means: work at a neutral one (a mine), an attack on an enemy, a plain move to a friend.
pub fn relation(neutral: bool, allied: bool) -> Kind {
    if neutral {
        Kind::Work
    } else if allied {
        Kind::Move
    } else {
        Kind::Attack
    }
}

/// The route of one unit: from `start` through its current order's stops, then each queued order's, as segments. A stop
/// `resolve` cannot place (a unit the player may not see) is left out and the next segment starts from the last stop that
/// was drawn.
pub fn route(
    start: (i32, i32),
    current: Order,
    queued: &[Order],
    mut resolve: impl FnMut(Aim) -> Option<Place>,
) -> Vec<Segment> {
    let mut out = Vec::new();
    let mut from = start;
    for leg in std::iter::once(current)
        .chain(queued.iter().copied())
        .flat_map(describe)
        .take(MAX_SEGMENTS)
    {
        if let Some(Place { at: to, kind }) = resolve(leg.aim) {
            let kind = if leg.kind == Kind::ByTarget {
                kind.unwrap_or(Kind::Move)
            } else {
                leg.kind
            };
            out.push(Segment { kind, from, to });
            from = to;
        }
    }
    out
}

/// What the player knows of a unit an order points at, as plain facts.
#[derive(Clone, Copy, Debug, Default)]
pub struct Facts {
    /// The unit is the local player's.
    pub own: bool,
    /// The whole map is shown (the reveal-map cheat).
    pub reveal: bool,
    /// Gone, dying out of play, or inside a building.
    pub out_of_play: bool,
    /// Hidden by the fog from the local player.
    pub fogged: bool,
    /// Not detected by the local player (a submarine, a cloaked unit).
    pub undetected: bool,
    /// A building the player holds a remembered look of.
    pub remembered_building: bool,
}

/// Whether a line may go to a unit.
pub fn known(f: Facts) -> bool {
    if f.own {
        return true;
    }
    if f.out_of_play {
        return false;
    }
    f.reveal || (!f.fogged && !f.undetected) || f.remembered_building
}

/// The part of a line's steps `0..=steps` along its major axis that can fall in `bounds` (left, top, right and bottom,
/// the last two excluded), widened by a step each side. `None` when it misses the rectangle. Only a bound for the loop:
/// the pixel test still decides each pixel.
pub fn visible_steps(a: (i32, i32), b: (i32, i32), bounds: (i32, i32, i32, i32)) -> Option<(i32, i32)> {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let steps = dx.abs().max(dy.abs());
    if steps == 0 {
        return Some((0, 0));
    }
    // Liang-Barsky over the parameter t in 0..=1.
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    let edges = [
        (-(dx as f64), (a.0 - bounds.0) as f64),
        (dx as f64, (bounds.2 - 1 - a.0) as f64),
        (-(dy as f64), (a.1 - bounds.1) as f64),
        (dy as f64, (bounds.3 - 1 - a.1) as f64),
    ];
    for (p, q) in edges {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                t0 = t0.max(r);
            } else {
                t1 = t1.min(r);
            }
        }
    }
    if t0 > t1 {
        return None;
    }
    let lo = ((t0 * steps as f64).floor() as i32 - 1).max(0);
    let hi = ((t1 * steps as f64).ceil() as i32 + 1).min(steps);
    Some((lo, hi))
}

/// The pixels of a segment's line: `put(x, y)` for each. With `dashed` the line is drawn in dashes counted from
/// `from`, shifted by `phase` (a clock, so they march). The pixels are the same whatever part is in `bounds`, so a
/// line does not shimmer as the view scrolls; the loop only visits the part that can be seen.
pub fn line_pixels(
    seg: &Segment,
    bounds: (i32, i32, i32, i32),
    dashed: bool,
    phase: i32,
    mut put: impl FnMut(i32, i32),
) {
    let (a, b) = (seg.from, seg.to);
    let steps = (b.0 - a.0).abs().max((b.1 - a.1).abs());
    let Some((lo, hi)) = visible_steps(a, b, bounds) else {
        return;
    };
    for k in lo..=hi {
        if dashed && (k + phase).rem_euclid(DASH_ON + DASH_OFF) >= DASH_ON {
            continue;
        }
        let (x, y) = if steps == 0 {
            a
        } else {
            (
                a.0 + ((b.0 - a.0) as i64 * k as i64 / steps as i64) as i32,
                a.1 + ((b.1 - a.1) as i64 * k as i64 / steps as i64) as i32,
            )
        };
        if x >= bounds.0 && x < bounds.2 && y >= bounds.1 && y < bounds.3 {
            put(x, y);
        }
    }
}

/// The pixels of the marker at a stop: a small filled diamond.
pub fn marker_pixels(at: (i32, i32), mut put: impl FnMut(i32, i32)) {
    for dy in -MARKER..=MARKER {
        for dx in -MARKER..=MARKER {
            if dx.abs() + dy.abs() <= MARKER {
                put(at.0 + dx, at.1 + dy);
            }
        }
    }
}

/// The segments of every selected unit of the local player (and training building with a rally point), in map pixels.
/// Empty for an observer. Reads the world and changes nothing.
pub fn segments(world: &World, selected: &[UnitId]) -> Vec<Segment> {
    let me = world.vision.local;
    let mut out = Vec::new();
    for &id in selected {
        let Some(u) = world.unit(id) else { continue };
        if u.owner != me || u.dying || u.inside || u.removing {
            continue;
        }
        let at = u.centre_px();
        let resolve = |aim: Aim| place(world, me, aim);
        if u.building {
            // A building's only line is its rally point.
            if world.rules.extras.rally_points
                && let Some(r) = world.rally_point(id)
            {
                let (kind, aim) = rally_leg(world, r.target, r.tile);
                out.extend(place(world, me, aim).map(|p| Segment {
                    kind,
                    from: at,
                    to: p.at,
                }));
            }
            continue;
        }
        let queued: &[Order] = world.ext.orders.get(&id).map_or(&[], Vec::as_slice);
        // A command waits for the unit's current step to end before it takes over: draw the new order at once.
        out.extend(route(at, u.pending.unwrap_or(u.order), queued, resolve));
    }
    out
}

/// The kind and aim of a rally point: following a friend is a move, working a mine yellow, attacking an enemy red.
fn rally_leg(world: &World, target: Option<UnitId>, tile: (i32, i32)) -> (Kind, Aim) {
    let Some(t) = target.and_then(|t| world.unit(t)) else {
        return (Kind::Move, Aim::Tile(tile));
    };
    let kind = relation(
        t.owner == tidewater_sim::slots::NEUTRAL,
        world.allied(world.vision.local, t.owner),
    );
    (kind, Aim::Unit(t.id))
}

/// Where an aim is on the map, or `None` when the player may not know.
fn place(world: &World, me: u8, aim: Aim) -> Option<Place> {
    match aim {
        Aim::Tile(t) => Some(Place::at((t.0 * 32 + 16, t.1 * 32 + 16))),
        Aim::Site(kind, t) => {
            let size = world
                .stats
                .0
                .get(kind as usize)
                .map_or((1, 1), |s| (s.size.0 as i32, s.size.1 as i32));
            Some(Place::at((t.0 * 32 + 16 * size.0, t.1 * 32 + 16 * size.1)))
        }
        Aim::Unit(id) => {
            let u = world.unit(id)?;
            let facts = Facts {
                own: u.owner == me,
                reveal: world.reveal_view(),
                out_of_play: u.removing || u.inside || u.dying,
                fogged: u.hidden_from & tidewater_sim::vision::bit(me) != 0,
                undetected: u.detected_by & tidewater_sim::vision::bit(me) == 0,
                remembered_building: u.building && world.remembered.copies().any(|g| g.tile == u.tile),
            };
            known(facts).then(|| Place {
                at: u.centre_px(),
                kind: Some(relation(
                    u.owner == tidewater_sim::slots::NEUTRAL,
                    world.allied(me, u.owner),
                )),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOUNDS: (i32, i32, i32, i32) = (100, 100, 200, 180);

    fn tile(x: i32, y: i32) -> Aim {
        Aim::Tile((x, y))
    }

    #[test]
    fn each_order_kind_has_its_colour_and_stops() {
        let g = (4, 5);
        let u = UnitId(7);
        let kinds = |o| describe(o).iter().map(|l| (l.kind, l.aim)).collect::<Vec<_>>();
        assert_eq!(kinds(Order::Move { goal: g, target: None }), [(Kind::Move, tile(4, 5))]);
        assert_eq!(
            kinds(Order::Move {
                goal: g,
                target: Some(u)
            }),
            [(Kind::ByTarget, Aim::Unit(u))],
            "a smart move onto a unit goes to the unit, not the tile it was on, colored by what the unit is"
        );
        assert_eq!(kinds(Order::AttackMove { goal: g }), [(Kind::Attack, tile(4, 5))]);
        assert_eq!(kinds(Order::Attack { target: u }), [(Kind::Attack, Aim::Unit(u))]);
        assert_eq!(kinds(Order::Chase { target: u }), [(Kind::Attack, Aim::Unit(u))]);
        assert_eq!(kinds(Order::Repair { target: u }), [(Kind::Work, Aim::Unit(u))]);
        assert_eq!(kinds(Order::Follow { target: u }), [(Kind::Move, Aim::Unit(u))]);
        assert_eq!(
            kinds(Order::Harvest {
                source: Some(u),
                tree: g,
                chopping: false
            }),
            [(Kind::Work, Aim::Unit(u))]
        );
        assert_eq!(
            kinds(Order::Harvest {
                source: None,
                tree: g,
                chopping: true
            }),
            [(Kind::Work, tile(4, 5))],
            "a woodcutter goes to its tree"
        );
        assert_eq!(
            kinds(Order::Build {
                kind: 0x3a,
                site: g,
                goal: None
            }),
            [(Kind::Work, Aim::Site(0x3a, g))]
        );
        assert_eq!(
            kinds(Order::Patrol { to: g, back: (1, 1) }),
            [(Kind::Move, tile(4, 5)), (Kind::Move, tile(1, 1))],
            "a patrol is there and back"
        );
        assert_eq!(
            kinds(Order::ReturnGoods { depot: Some(u) }),
            [(Kind::Work, Aim::Unit(u))]
        );
    }

    #[test]
    fn orders_that_go_nowhere_draw_nothing() {
        for o in [
            Order::Idle,
            Order::StandGround,
            Order::Inside { building: None },
            Order::InsideSite { building: None },
            Order::ReturnGoods { depot: None },
            Order::TowerWatch { target: None },
            Order::Unload,
            Order::Docked,
        ] {
            assert!(describe(o).is_empty(), "{o:?}");
        }
    }

    #[test]
    fn colours_are_the_health_bar_green_yellow_and_red() {
        assert_eq!(Kind::Move.colour(), crate::info::health_bar(100, 100).1);
        assert_eq!(Kind::Work.colour(), crate::info::health_bar(60, 100).1);
        assert_eq!(Kind::Attack.colour(), crate::info::health_bar(10, 100).1);
    }

    #[test]
    fn the_route_runs_through_the_current_order_then_each_queued_one() {
        let at = |a: Aim| match a {
            Aim::Tile(t) => Some(Place::at((t.0 * 32 + 16, t.1 * 32 + 16))),
            _ => Some(Place::at((0, 0))),
        };
        let segs = route(
            (10, 10),
            Order::Move {
                goal: (2, 2),
                target: None,
            },
            &[
                Order::AttackMove { goal: (5, 2) },
                Order::Move {
                    goal: (5, 6),
                    target: None,
                },
            ],
            at,
        );
        assert_eq!(
            segs,
            [
                Segment {
                    kind: Kind::Move,
                    from: (10, 10),
                    to: (80, 80)
                },
                Segment {
                    kind: Kind::Attack,
                    from: (80, 80),
                    to: (176, 80)
                },
                Segment {
                    kind: Kind::Move,
                    from: (176, 80),
                    to: (176, 208)
                },
            ]
        );
        assert!(route((0, 0), Order::Idle, &[], at).is_empty());
    }

    #[test]
    fn a_stop_the_player_may_not_see_is_skipped_and_the_line_carries_on() {
        let u = UnitId(9);
        let resolve = |a: Aim| match a {
            Aim::Unit(_) => None,
            Aim::Tile(t) => Some(Place::at((t.0, t.1))),
            Aim::Site(..) => None,
        };
        let segs = route(
            (0, 0),
            Order::Attack { target: u },
            &[Order::Move {
                goal: (7, 7),
                target: None,
            }],
            resolve,
        );
        assert_eq!(
            segs,
            [Segment {
                kind: Kind::Move,
                from: (0, 0),
                to: (7, 7)
            }],
            "nothing is drawn to the hidden target, and nothing reveals where it is"
        );
    }

    #[test]
    fn walking_up_to_a_unit_is_colored_by_what_it_is() {
        assert_eq!(relation(true, false), Kind::Work, "a gold mine");
        assert_eq!(relation(false, true), Kind::Move, "a friend");
        assert_eq!(relation(false, false), Kind::Attack, "an enemy");
        let u = UnitId(3);
        let at = |k| {
            move |_: Aim| {
                Some(Place {
                    at: (5, 5),
                    kind: Some(k),
                })
            }
        };
        let go = Order::Move {
            goal: (1, 1),
            target: Some(u),
        };
        assert_eq!(route((0, 0), go, &[], at(Kind::Work))[0].kind, Kind::Work);
        assert_eq!(route((0, 0), go, &[], at(Kind::Attack))[0].kind, Kind::Attack);
        // A plain move is a move whatever stands on the spot.
        let plain = Order::Move {
            goal: (1, 1),
            target: None,
        };
        assert_eq!(route((0, 0), plain, &[], at(Kind::Attack))[0].kind, Kind::Move);
    }

    #[test]
    fn a_route_is_capped() {
        let queued = vec![
            Order::Move {
                goal: (1, 1),
                target: None
            };
            20
        ];
        let segs = route((0, 0), Order::Idle, &queued, |_| Some(Place::at((1, 1))));
        assert_eq!(segs.len(), MAX_SEGMENTS);
    }

    #[test]
    fn the_player_knows_their_own_and_what_they_can_see() {
        let f = Facts::default();
        assert!(known(f), "in plain sight");
        assert!(
            known(Facts {
                own: true,
                fogged: true,
                ..f
            }),
            "their own, always"
        );
        assert!(!known(Facts { fogged: true, ..f }), "an enemy in the fog");
        assert!(!known(Facts { undetected: true, ..f }), "a cloaked or submerged one");
        assert!(!known(Facts { out_of_play: true, ..f }), "one that is gone");
        assert!(
            known(Facts {
                fogged: true,
                remembered_building: true,
                ..f
            }),
            "a building they remember"
        );
        assert!(
            known(Facts {
                fogged: true,
                reveal: true,
                ..f
            }),
            "with the whole map shown"
        );
        assert!(
            !known(Facts {
                out_of_play: true,
                reveal: true,
                ..f
            }),
            "even then, not one that is gone"
        );
    }

    /// Every pixel the line puts, for a segment, with dashes on or off.
    fn drawn(seg: Segment, bounds: (i32, i32, i32, i32), dashed: bool, phase: i32) -> Vec<(i32, i32)> {
        let mut v = Vec::new();
        line_pixels(&seg, bounds, dashed, phase, |x, y| v.push((x, y)));
        v
    }

    fn seg(from: (i32, i32), to: (i32, i32)) -> Segment {
        Segment {
            kind: Kind::Move,
            from,
            to,
        }
    }

    #[test]
    fn a_line_inside_the_view_is_drawn_whole_and_touches_both_ends() {
        let all = drawn(seg((110, 110), (150, 130)), BOUNDS, false, 0);
        assert_eq!(
            all.len(),
            41,
            "one pixel for each step along the long axis, ends included"
        );
        assert_eq!(all.first(), Some(&(110, 110)));
        assert_eq!(all.last(), Some(&(150, 130)));
    }

    #[test]
    fn a_line_leaving_the_view_is_cut_at_its_edge_and_a_distant_one_costs_nothing() {
        let all = drawn(seg((150, 150), (150, 100_000)), BOUNDS, false, 0);
        assert_eq!(all.first(), Some(&(150, 150)));
        assert_eq!(all.last(), Some(&(150, 179)), "stops at the bottom row");
        assert!(drawn(seg((0, 0), (0, 100_000)), BOUNDS, false, 0).is_empty());
        assert!(drawn(seg((-500, -500), (-100, 99)), BOUNDS, false, 0).is_empty());
        // The bound on the loop is tight: far fewer steps than the line has.
        let (lo, hi) = visible_steps((150, 150), (150, 100_000), BOUNDS).unwrap();
        assert!(lo <= 1 && hi < 40, "{lo}..{hi}");
    }

    #[test]
    fn clipping_never_loses_a_pixel_the_whole_line_would_draw_in_view() {
        let inside = |&(x, y): &(i32, i32)| x >= BOUNDS.0 && x < BOUNDS.2 && y >= BOUNDS.1 && y < BOUNDS.3;
        for (a, b) in [
            ((0, 0), (300, 250)),
            ((300, 20), (-50, 260)),
            ((90, 170), (210, 105)),
            ((150, 0), (150, 400)),
            ((0, 140), (400, 141)),
            ((199, 179), (100, 100)),
        ] {
            let s = seg(a, b);
            // The reference: the same pixels with a view so large nothing is cut.
            let wide = (-1000, -1000, 1000, 1000);
            let want: Vec<_> = drawn(s, wide, false, 0).into_iter().filter(inside).collect();
            assert_eq!(drawn(s, BOUNDS, false, 0), want, "{a:?} to {b:?}");
        }
    }

    #[test]
    fn dashes_count_from_the_unit_so_scrolling_does_not_move_them() {
        let s = seg((0, 150), (400, 150));
        let wide = drawn(s, (-10, 0, 500, 300), true, 0);
        let part = drawn(s, (100, 0, 200, 300), true, 0);
        let want: Vec<_> = wide.iter().copied().filter(|&(x, _)| (100..200).contains(&x)).collect();
        assert_eq!(part, want);
        // A period is DASH_ON drawn and DASH_OFF left out; the phase marches them.
        assert_eq!(
            wide[..DASH_ON as usize].iter().map(|p| p.0).collect::<Vec<_>>(),
            [0, 1, 2, 3]
        );
        assert_eq!(wide[DASH_ON as usize].0, DASH_ON + DASH_OFF);
        let shifted = drawn(s, (-10, 0, 500, 300), true, 1);
        assert_eq!(
            shifted[..3].iter().map(|p| p.0).collect::<Vec<_>>(),
            [0, 1, 2],
            "marched by one"
        );
        assert_eq!(shifted[3].0, DASH_ON + DASH_OFF - 1);
    }

    #[test]
    fn a_marker_is_a_small_diamond() {
        let mut v = Vec::new();
        marker_pixels((10, 10), |x, y| v.push((x, y)));
        assert_eq!(v.len(), 13);
        assert!(v.contains(&(10, 10)) && v.contains(&(12, 10)) && v.contains(&(10, 8)));
        assert!(!v.contains(&(12, 12)));
    }

    #[test]
    fn a_zero_length_line_is_one_pixel() {
        assert_eq!(drawn(seg((120, 120), (120, 120)), BOUNDS, false, 0), [(120, 120)]);
    }
}
