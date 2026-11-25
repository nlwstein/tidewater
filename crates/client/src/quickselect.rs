//! Quick select (option `quick-select`, `extras`, `docs/design/extras.md`): selecting faster, as StarCraft, Age of
//! Empires II and OpenRA do. Our own design, not the original's. It is **C+L**: in a game with other players the
//! session sets it for everyone, because it saves clicks (an edge).
//!
//! **What the original already has.** Under Enhanced rules a Ctrl-click, or a double-click on a unit that is already
//! selected, selects the units of that type on screen (`mouse::click_rules`, rule 4), and Ctrl+digit sets a control
//! group. Those stay exactly as they are; this option adds only what is missing:
//! - **Double-click under Classic rules**: Classic has no Ctrl or double-click rule, so with the option on a
//!   double-click on one of your own selected units takes the same type on screen with the same ring search and the
//!   same selection limit as Enhanced ([`double_click`]).
//! - **Fighting units on screen (Ctrl+A) or on the map (Alt+A)**: every one of your units that can attack and is not a
//!   worker, an oil tanker or a building, which can be picked and is selectable by rectangle ([`combat`]). When there
//!   are more than the selection limit, the ones nearest the middle of the view win. Both keys are rebindable
//!   (`select-screen`, `select-map`).
//! - **Ctrl+Shift+digit adds to a control group** (Enhanced; control groups do not exist under Classic): the group's
//!   units plus the selection, up to the selection limit, become the selection and then the group ([`add_to_group`]).
//!   Plain Ctrl+digit still replaces the group.
//!
//! Every selection is the normal [`Select`](tidewater_sim::Command::Select) command, so the sim is not changed.

use tidewater_sim::UnitId;

use crate::attackmove;
use crate::mouse::{Board, Pick, find_at, same_type};

/// How far a quick selection reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach {
    /// Units whose box touches the visible map.
    Screen,
    /// Every unit.
    Map,
}

/// A double-click at map pixel `p` on one of your own selected units, under Classic rules: all of its type on
/// screen. `None` when the click is not that (the ordinary rules then apply).
pub fn double_click(board: &Board, p: (i32, i32), sel: &[UnitId]) -> Option<Pick> {
    let found = find_at(board, p)?;
    if board.view.enhanced || !board.group_selectable(found) || !sel.contains(&found.id) {
        return None;
    }
    Some(Pick::Set {
        units: same_type(board, found),
        sound: true,
    })
}

/// Whether a unit is a fighting unit of `player`: it can attack and is not a worker, an oil tanker or a building (the
/// rule smart attack-move uses).
pub fn is_combat(world: &tidewater_sim::World, player: u8, id: UnitId) -> bool {
    attackmove::applies(None, &attackmove::facts(world, player, &[id]))
}

/// The local player's fighting units within `reach`, nearest the middle of the view first, at most the selection
/// limit. `is_combat` says which units fight.
pub fn combat(board: &Board, is_combat: &dyn Fn(UnitId) -> bool, reach: Reach) -> Vec<UnitId> {
    let v = board.view.view;
    let centre = ((v.l + v.r) / 2, (v.t + v.b) / 2);
    let mut found: Vec<(i64, UnitId)> = board
        .units
        .iter()
        .filter(|c| {
            board.can_pick(c)
                && board.group_selectable(c)
                && is_combat(c.id)
                && (reach == Reach::Map || c.sel_box().overlaps(&v))
        })
        .map(|c| {
            let dx = i64::from(c.pos.0 - centre.0);
            let dy = i64::from(c.pos.1 - centre.1);
            (dx * dx + dy * dy, c.id)
        })
        .collect();
    // Stable, so units at the same distance stay in slot order.
    found.sort_by_key(|&(d, _)| d);
    found.into_iter().take(board.view.limit).map(|(_, id)| id).collect()
}

/// The selection after Ctrl+Shift+digit: the group's units, then the selected ones that are not in it, at most
/// `limit`. Setting the group from this selection adds to it.
pub fn add_to_group(group: &[UnitId], selection: &[UnitId], limit: usize) -> Vec<UnitId> {
    let mut out: Vec<UnitId> = Vec::new();
    for &id in group.iter().chain(selection) {
        if out.len() < limit && !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mouse::{Cand, PxRect, View};

    const ME: u8 = 0;
    const FOE: u8 = 1;
    const FOOTMAN: u8 = 0;
    const ARCHER: u8 = 8;
    const PEASANT: u8 = 2;

    fn unit(id: u32, kind: u8, owner: u8, tile: (i32, i32)) -> Cand {
        Cand {
            id: UnitId(id),
            kind,
            owner,
            pos: (tile.0 * 32, tile.1 * 32),
            tile,
            size: (1, 1),
            box_size: (31, 31),
            air: false,
            building: false,
            mobile: true,
            dying: false,
            inside: false,
            hidden: false,
            detected: true,
            busy: false,
            rect_selectable: true,
        }
    }

    fn board(enhanced: bool, limit: usize, units: Vec<Cand>) -> Board<'static> {
        let view = View {
            local: ME,
            enhanced,
            allies: 1,
            view: PxRect {
                l: 0,
                t: 0,
                r: 14 * 32 - 1,
                b: 14 * 32 - 1,
            },
            view_tiles: 14,
            map: (64, 64),
            limit,
        };
        Board::new(view, units, &|_, _| false)
    }

    fn at(tile: (i32, i32)) -> (i32, i32) {
        (tile.0 * 32 + 16, tile.1 * 32 + 16)
    }

    /// Footmen and archers fight; peasants do not.
    fn fights(units: &[Cand]) -> impl Fn(UnitId) -> bool + '_ {
        move |id| units.iter().any(|c| c.id == id && c.kind != PEASANT)
    }

    fn ids(v: &[u32]) -> Vec<UnitId> {
        v.iter().map(|&n| UnitId(n)).collect()
    }

    #[test]
    fn a_classic_double_click_takes_the_same_type_on_screen() {
        let units = vec![
            unit(1, FOOTMAN, ME, (3, 3)),
            unit(2, FOOTMAN, ME, (5, 3)),
            unit(3, ARCHER, ME, (4, 4)),
            unit(4, FOOTMAN, FOE, (4, 3)),
            unit(5, FOOTMAN, ME, (40, 40)),
        ];
        let b = board(false, 9, units);
        let Some(Pick::Set { mut units, sound }) = double_click(&b, at((3, 3)), &ids(&[1])) else {
            panic!("a pick");
        };
        units.sort();
        assert_eq!(units, ids(&[1, 2]), "own footmen on screen only");
        assert!(sound);
    }

    #[test]
    fn the_double_click_leaves_the_other_cases_to_the_ordinary_rules() {
        let units = vec![unit(1, FOOTMAN, ME, (3, 3)), unit(4, FOOTMAN, FOE, (6, 3))];
        let b = board(false, 9, units.clone());
        assert!(double_click(&b, at((3, 3)), &[]).is_none(), "not selected yet");
        assert!(double_click(&b, at((6, 3)), &ids(&[4])).is_none(), "an enemy");
        assert!(double_click(&b, at((9, 9)), &ids(&[1])).is_none(), "open ground");
        let enhanced = board(true, 9, units);
        assert!(
            double_click(&enhanced, at((3, 3)), &ids(&[1])).is_none(),
            "Enhanced has its own rule"
        );
    }

    #[test]
    fn the_double_click_respects_the_selection_limit() {
        let units: Vec<Cand> = (0..12)
            .map(|n| unit(n + 1, FOOTMAN, ME, (n as i32 % 6, n as i32 / 6)))
            .collect();
        let b = board(false, 9, units);
        let Some(Pick::Set { units, .. }) = double_click(&b, at((0, 0)), &ids(&[1])) else {
            panic!("a pick");
        };
        assert_eq!(units.len(), 9);
        let wide = board(
            false,
            12,
            (0..12)
                .map(|n| unit(n + 1, FOOTMAN, ME, (n as i32 % 6, n as i32 / 6)))
                .collect(),
        );
        let Some(Pick::Set { units, .. }) = double_click(&wide, at((0, 0)), &ids(&[1])) else {
            panic!("a pick");
        };
        assert_eq!(units.len(), 12);
    }

    #[test]
    fn combat_takes_own_fighting_units_on_screen() {
        let mut dead = unit(6, FOOTMAN, ME, (4, 4));
        dead.dying = true;
        let mut hidden = unit(7, FOOTMAN, ME, (5, 5));
        hidden.hidden = true;
        let units = vec![
            unit(1, FOOTMAN, ME, (2, 2)),
            unit(2, ARCHER, ME, (3, 2)),
            unit(3, PEASANT, ME, (4, 2)),
            unit(4, FOOTMAN, FOE, (5, 2)),
            unit(5, FOOTMAN, ME, (40, 40)),
            dead,
            hidden,
        ];
        let b = board(true, 9, units.clone());
        let mut got = combat(&b, &fights(&units), Reach::Screen);
        got.sort();
        assert_eq!(got, ids(&[1, 2]));
        let mut all = combat(&b, &fights(&units), Reach::Map);
        all.sort();
        assert_eq!(all, ids(&[1, 2, 5]), "the map reaches the far footman too");
    }

    #[test]
    fn combat_keeps_the_ones_nearest_the_middle_when_over_the_limit() {
        let units: Vec<Cand> = (0..5).map(|n| unit(n + 1, FOOTMAN, ME, (n as i32 * 3, 7))).collect();
        let b = board(true, 2, units.clone());
        // The view's middle is at tile 7: the footmen at tiles 6 and 9 are the nearest.
        assert_eq!(combat(&b, &fights(&units), Reach::Screen), ids(&[3, 4]));
    }

    #[test]
    fn adding_to_a_group_keeps_it_first_and_stops_at_the_limit() {
        assert_eq!(add_to_group(&ids(&[1, 2]), &ids(&[2, 3]), 9), ids(&[1, 2, 3]));
        assert_eq!(add_to_group(&[], &ids(&[4, 5]), 9), ids(&[4, 5]));
        assert_eq!(add_to_group(&ids(&[1, 2]), &ids(&[3, 4, 5]), 4), ids(&[1, 2, 3, 4]));
    }
}
