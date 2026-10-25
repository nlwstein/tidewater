//! The synced selection, control groups and the commands that act on them. Spec:
//! `docs/specs/ui/selecting-and-commanding.md`.
//!
//! Each player has a selection of up to 9 units and 18 control groups, all part of the game state. The interface
//! changes them with player commands ([`Command::Select`], [`Command::SetGroup`], [`Command::RecallGroup`]); the
//! right-click command and the commands from target mode, the buttons and the keys ([`Command::RightClick`],
//! [`Command::Issue`]) then act on the issuing player's synced selection. Picking units with the mouse is the client's
//! job (it needs the local view); this module holds what runs inside a cycle.
//!
//! Unit type numbers and flag bits are those of the public PUD specification (appendix A and the UDTA flags list).
//!
//! Our readings where the spec is open:
//! - Command 4 gives order 4 directly, command 10 is taken as an attack-move (order 10), commands 12 and 16 need a
//!   target unit, and command 27 without a target unit does nothing.
//! - The pause count of Enhanced multiplayer isn't limited (its starting value belongs to a later spec).
//! - Under Classic rules a speed step moves the stored 9-level setting between 0 and 6.

use tidewater_formats::udta::flags;

use crate::world::{Command, Order, PLAYERS, Unit, UnitId, World};
use crate::{slots, terrain};

/// Places in a selection and in a control group.
pub const PLACES: usize = 9;
/// Control groups per player; the keys use 0 to 9.
pub const GROUPS: usize = 18;
/// Highest speed level a speed key reaches.
pub const TOP_SPEED_ENHANCED: u8 = 8;
pub const TOP_SPEED_CLASSIC: u8 = 6;
/// Spread on an axis, in tiles, from which the formation offset no longer keeps the group's shape.
pub const FORMATION_SPREAD: i32 = 4;

/// Unit types named by the rules (public PUD specification, appendix A).
pub mod kind {
    pub const KNIGHT: u8 = 0x06;
    pub const OGRE: u8 = 0x07;
    pub const MAGE: u8 = 0x0a;
    pub const DEATH_KNIGHT: u8 = 0x0b;
    pub const PALADIN: u8 = 0x0c;
    pub const OGRE_MAGE: u8 = 0x0d;
    pub const DEMOLITION_SQUAD: u8 = 0x0e;
    pub const SAPPER: u8 = 0x0f;
    pub const TERON_GOREFIEND: u8 = 0x15;
    pub const DENTARG: u8 = 0x17;
    pub const KHADGAR: u8 = 0x18;
    pub const FLYING_MACHINE: u8 = 0x28;
    pub const ZEPPELIN: u8 = 0x29;
    pub const TURALYON: u8 = 0x2c;
    pub const EYE_OF_KILROGG: u8 = 0x2d;
    pub const CHOGALL: u8 = 0x31;
    pub const LOTHAR: u8 = 0x32;
    pub const GULDAN: u8 = 0x33;
    pub const UTHER: u8 = 0x34;
    /// The last type with a right-click action.
    pub const CRITTER: u8 = 0x39;
}

/// Right-click actions (UDTA "2nd mouse button action").
pub mod action {
    pub const NONE: u8 = 0;
    pub const ATTACK: u8 = 1;
    pub const MOVE: u8 = 2;
    pub const HARVEST: u8 = 3;
    pub const HAUL_OIL: u8 = 4;
    pub const DEMOLISH: u8 = 5;
    pub const SAIL: u8 = 6;
}

/// Flag bits 0 to 4: land, air, explode, sea, critter.
pub const MOBILE: u32 = flags::LAND | flags::AIR | flags::EXPLODE_WHEN_KILLED | flags::SEA | flags::CRITTER;

/// One player's synced selection and control groups.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct PlayerSelection {
    /// The unit slots the last select named, as sent, whether or not a slot holds a unit ("What the synced selection
    /// holds", round 107). Under Classic rules a lost unit leaves its place empty (`None`); under Enhanced rules the
    /// rest close up.
    pub places: [Option<u16>; PLACES],
    /// Per place: the unit in the slot was already lost (dying, inside something) when the select named it. The loss
    /// is an event, so such a unit stays selected (PI2a, PI10a) until it stays again and is lost anew. Not saved: a
    /// load starts with an empty selection.
    #[serde(skip)]
    pub lost: [bool; PLACES],
    pub groups: [Vec<UnitId>; GROUPS],
}

/// What a player's selection hashed as before round 107, when it held unit ids: the state hash keeps that shape, so
/// the hashes of games that never name an empty slot did not move.
#[derive(Hash)]
struct HashedSelection {
    places: [Option<UnitId>; PLACES],
    groups: [Vec<UnitId>; GROUPS],
}

impl PlayerSelection {
    /// The selected slots in place order.
    pub fn slots(&self) -> impl Iterator<Item = u16> + '_ {
        self.places.iter().flatten().copied()
    }
}

/// Whether a command number is one the game carries out from target mode, buttons or keys: 2 to 6, 8 to 17, 19, 23,
/// 24, 27, 29 and 38 to 56.
pub fn handled(command: u8) -> bool {
    matches!(command, 2..=6 | 8..=17 | 19 | 23 | 24 | 27 | 29 | 38..=56)
}

/// The formation offset ("Formation offset"): the tile each unit at `tiles` aims at for a click on `target`. `wall`
/// is whether the target tile has a wall flag; `map` the map size in tiles.
pub fn formation(tiles: &[(i32, i32)], target: (i32, i32), wall: bool, map: (i32, i32)) -> Vec<(i32, i32)> {
    let n = tiles.len() as i32;
    if wall || n == 0 {
        return vec![target; tiles.len()];
    }
    let axis = |a: fn(&(i32, i32)) -> i32| {
        let v: Vec<i32> = tiles.iter().map(a).collect();
        let (lo, hi) = (*v.iter().min().unwrap(), *v.iter().max().unwrap());
        (v.iter().sum::<i32>() / n, lo, hi)
    };
    let (mx, x0, x1) = axis(|t| t.0);
    let (my, y0, y1) = axis(|t| t.1);
    let inside = (x0..=x1).contains(&target.0) && (y0..=y1).contains(&target.1);
    if inside {
        return vec![target; tiles.len()];
    }
    let aim = |own: i32, mean: i32, lo: i32, hi: i32, to: i32, len: i32| {
        if hi - lo < FORMATION_SPREAD {
            (own + to - mean).clamp(0, (len - 1).max(0))
        } else {
            to
        }
    };
    tiles
        .iter()
        .map(|&(x, y)| (aim(x, mx, x0, x1, target.0, map.0), aim(y, my, y0, y1, target.1, map.1)))
        .collect()
}

impl World {
    fn flags_of_kind(&self, kind: u8) -> u32 {
        self.stats_of(kind).flags
    }

    /// Mobile: any of flag bits 0 to 4.
    pub fn is_mobile(&self, u: &Unit) -> bool {
        self.flags_of_kind(u.kind) & MOBILE != 0
    }

    /// Whether the target tile has a wall flag (0x04 or 0x08 of the movement word).
    pub fn wall_tile(&self, t: (i32, i32)) -> bool {
        use terrain::bits::{COMPUTER_WALL, HUMAN_WALL};
        self.terrain.contains(t.0, t.1) && self.terrain.word(t.0, t.1) & (HUMAN_WALL | COMPUTER_WALL) != 0
    }

    /// "Allied" as the right-click tables use it: the player itself, its allies, and the neutral player, who is
    /// allied to everyone.
    fn allied_to(&self, player: u8, owner: u8) -> bool {
        owner == slots::NEUTRAL || self.allied(player, owner)
    }

    /// Hashes every player's selection and groups for the state hash. A place hashes as the unit in its slot (none for
    /// an empty slot), and the marks of units named while already lost join only when one is set.
    pub(crate) fn hash_selections(&self, h: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        let all: [HashedSelection; PLAYERS] = std::array::from_fn(|p| {
            let s = &self.selections[p];
            HashedSelection {
                places: s
                    .places
                    .map(|slot| slot.and_then(|s| self.unit_in_slot(s)).map(|u| u.id)),
                groups: s.groups.clone(),
            }
        });
        all.hash(h);
        if self.selections.iter().any(|s| s.lost.contains(&true)) {
            self.selections.iter().map(|s| s.lost).collect::<Vec<_>>().hash(h);
        }
    }

    /// The unit slots of the player's synced selection in order, empty ones included: the nine places, then (option
    /// `selection-limit`) the units beyond them that the larger limit allows.
    pub fn selected_slots(&self, player: u8) -> Vec<u16> {
        self.selections.get(player as usize).map_or_else(Vec::new, |s| {
            s.slots()
                .chain(
                    self.ext.more[player as usize]
                        .iter()
                        .filter_map(|&id| self.unit(id).map(|u| u.slot)),
                )
                .collect()
        })
    }

    /// The player's synced selection: the units in the selected slots, in order. A slot that holds no unit takes no
    /// part ("What the synced selection holds").
    pub fn selection(&self, player: u8) -> Vec<UnitId> {
        self.selected_slots(player)
            .into_iter()
            .filter_map(|s| self.unit_in_slot(s).map(|u| u.id))
            .collect()
    }

    /// The slots of units, for a [`Command::Select`] (the interface knows the units it picked).
    pub fn slots_of(&self, units: &[UnitId]) -> Vec<u16> {
        units.iter().filter_map(|&id| self.unit(id).map(|u| u.slot)).collect()
    }

    /// The unit in the selection's first place, the one the "first unit of the selection" commands (build, job, cancel)
    /// use: none if that slot holds no unit.
    pub fn first_selected(&self, player: u8) -> Option<UnitId> {
        let slot = self.selected_slots(player).first().copied()?;
        self.unit_in_slot(slot).map(|u| u.id)
    }

    /// Replaces the player's synced selection with the slots `slots`, kept as sent: the first nine take the places,
    /// and with the option `selection-limit` on up to the limit in all are kept after them. At the original limit of
    /// nine nothing but the places is touched.
    pub(crate) fn set_selection(&mut self, player: u8, slots: &[u16]) {
        let cap = self.rules.extras.selection_cap();
        if player as usize >= PLAYERS {
            return;
        }
        let lost = |w: &World, s: u16| w.unit_in_slot(s).is_some_and(|u| !u.stays_selected());
        let mut places = [None; PLACES];
        let mut flags = [false; PLACES];
        for (n, &s) in slots.iter().take(PLACES).enumerate() {
            places[n] = Some(s);
            flags[n] = lost(self, s);
        }
        let more = if cap > PLACES {
            slots
                .iter()
                .skip(PLACES)
                .take(cap - PLACES)
                .filter_map(|&s| self.unit_in_slot(s).map(|u| u.id))
                .collect()
        } else {
            Vec::new()
        };
        let sel = &mut self.selections[player as usize];
        sel.places = places;
        sel.lost = flags;
        self.ext.more[player as usize] = more;
    }

    /// Applies the commands of this module; `false` for any other command.
    pub(crate) fn selection_command(&mut self, c: &Command) -> bool {
        match *c {
            Command::Select { player, ref units } => self.set_selection(player, units),
            Command::RightClick { player, target, tile } => self.right_click(player, target, tile),
            Command::Issue {
                player,
                command,
                target,
                tile,
            } => self.issue(player, command, target, tile),
            Command::SetGroup { player, group } => self.set_group(player, group),
            Command::RecallGroup { player, group } => self.recall_group(player, group),
            // `net/lockstep.md`, "Speed" (round 100, static study: NC1, NC2): the speed becomes the value the command
            // carries; a value above the top speed is ignored.
            Command::Speed { speed, .. } => {
                if speed <= self.rules.extras.top_speed(self.rules.enhanced) {
                    self.speed = speed;
                }
            }
            Command::Pause { player } => self.pause_command(player),
            Command::Resume { player } => self.resume_command(player),
            _ => return false,
        }
        true
    }

    /// "Pausing" (`ui/game-menu.md`): nothing if the game is paused already; in Enhanced multiplayer it uses one of the
    /// player's pauses and nothing happens for a player with none left.
    fn pause_command(&mut self, player: u8) {
        if self.paused {
            return;
        }
        let counted = self.rules.multiplayer && self.rules.enhanced;
        if counted {
            match self.pauses.get_mut(player as usize) {
                Some(n) if *n > 0 => *n -= 1,
                _ => return,
            }
        }
        self.paused = true;
        if counted {
            let left = self.pauses[player as usize];
            self.events.push(crate::economy::Event::Paused { player, left });
        }
    }

    /// "Resuming": nothing if the game is not paused; uses no pause.
    fn resume_command(&mut self, player: u8) {
        if !self.paused {
            return;
        }
        self.paused = false;
        if self.rules.multiplayer && self.rules.enhanced {
            self.events.push(crate::economy::Event::Resumed { player });
        }
    }

    /// "Losing a unit from the selection": a unit that is dying, dead, removed, inside something or entering a mine,
    /// platform or depot ([`Unit::stays_selected`]) leaves every player's synced selection. Under Enhanced rules the
    /// rest close up; under Classic rules its place stays empty. Run at the end of the cycle: no command reads the
    /// selection between order 25 being given and then.
    ///
    /// The loss is an event, not a standing test (round 107, PI2, PI10): a unit that was already lost when a select
    /// named it stays selected (`lost` marks it) until it stays again; a slot that holds no unit stays selected.
    pub(crate) fn prune_selections(&mut self) {
        let enhanced = self.rules.enhanced;
        for p in 0..PLAYERS {
            // The units beyond the nine places (option `selection-limit`) simply leave when lost.
            if !self.ext.more[p].is_empty() {
                let mut more = std::mem::take(&mut self.ext.more[p]);
                more.retain(|&id| self.unit(id).is_some_and(|u| u.stays_selected()));
                self.ext.more[p] = more;
            }
            let sel = self.selections[p].clone();
            let mut lost = sel.lost;
            let mut gone = [false; PLACES];
            for (n, slot) in sel.places.iter().enumerate() {
                let Some(u) = slot.and_then(|s| self.unit_in_slot(s)) else {
                    continue;
                };
                if u.stays_selected() {
                    lost[n] = false;
                } else {
                    gone[n] = !lost[n];
                }
            }
            if !gone.contains(&true) {
                self.selections[p].lost = lost;
                continue;
            }
            if enhanced {
                // The rest close up, and the units beyond the places move up into the room.
                let mut all: Vec<(u16, bool)> = (0..PLACES)
                    .filter(|&n| !gone[n])
                    .filter_map(|n| sel.places[n].map(|s| (s, lost[n])))
                    .collect();
                let more = std::mem::take(&mut self.ext.more[p]);
                let cap = self.rules.extras.selection_cap();
                let mut rest: Vec<UnitId> = Vec::new();
                for id in more {
                    let Some(u) = self.unit(id) else { continue };
                    if all.len() < PLACES {
                        all.push((u.slot, false));
                    } else if all.len() + rest.len() < cap {
                        rest.push(id);
                    }
                }
                let s = &mut self.selections[p];
                s.places = [None; PLACES];
                s.lost = [false; PLACES];
                for (n, (slot, l)) in all.into_iter().enumerate() {
                    s.places[n] = Some(slot);
                    s.lost[n] = l;
                }
                self.ext.more[p] = rest;
                continue;
            }
            let s = &mut self.selections[p];
            s.lost = lost;
            for (n, g) in gone.iter().enumerate() {
                if *g {
                    s.places[n] = None;
                    s.lost[n] = false;
                }
            }
        }
    }

    /// The units of the player's synced selection with the tile each aims at after the formation offset, for a
    /// command at `tile`.
    fn aims(&self, player: u8, tile: (i32, i32)) -> Vec<(usize, (i32, i32))> {
        let units: Vec<usize> = self
            .selection(player)
            .into_iter()
            .filter_map(|id| self.index(id))
            .collect();
        let tiles: Vec<(i32, i32)> = units.iter().map(|&i| self.units[i].tile).collect();
        let map = (self.terrain.width as i32, self.terrain.height as i32);
        let aims = formation(&tiles, tile, self.wall_tile(tile), map);
        units.into_iter().zip(aims).collect()
    }

    /// "What a right-click command does".
    fn right_click(&mut self, player: u8, target: Option<UnitId>, tile: (i32, i32)) {
        for (i, aim) in self.aims(player, tile) {
            let u = &self.units[i];
            if u.owner != player || !self.is_mobile(u) || Some(u.id) == target || u.kind > kind::CRITTER {
                continue;
            }
            let Some(command) = self.right_click_command(i, player, target, tile) else {
                continue;
            };
            if self.rules.enhanced {
                self.units[i].remembered = None;
            }
            // The tree row gives command 23 with no target unit.
            let target = if command == 23 && target.is_none() {
                None
            } else {
                target
            };
            // Round-56 static correction: unlike a command from target mode, the buttons or the keys, a right-click
            // keeps the unit's recorded attacker.
            self.give_command(i, command, target, aim);
        }
    }

    /// Orders spec, "A player's command forgets the attacker" (round-55 errata, `human09-fleet-3000` battleship 1539,
    /// n = 1502): each unit that takes a player's command forgets its recorded attacker once the command is given,
    /// whatever the attacker's state, so the command's own checks still saw the record.
    pub(crate) fn forget_attacker_after_command(&mut self, id: UnitId) {
        // A command that only joined the unit's queue (`waypoints.rs`) did not take over: the fight goes on.
        if self.just_queued.take() == Some(id) {
            return;
        }
        if let Some(i) = self.index(id) {
            self.units[i].attacked_by = None;
        }
    }

    /// The command a unit's right-click action chooses ("What a right-click command does", and the harvest table),
    /// or `None` for action 0.
    pub(crate) fn right_click_command(
        &self,
        i: usize,
        player: u8,
        target: Option<UnitId>,
        tile: (i32, i32),
    ) -> Option<u8> {
        let u = &self.units[i];
        let s = self.stats_of(u.kind);
        let t = target.and_then(|t| self.unit(t));
        let allied = t.map(|t| self.allied_to(player, t.owner));
        Some(match s.right_click {
            action::MOVE | action::SAIL => 3,
            action::ATTACK => {
                if s.flags & flags::CAN_ATTACK == 0 {
                    3
                } else if allied == Some(false) {
                    8
                } else if self.wall_tile(tile) {
                    11
                } else {
                    3
                }
            }
            action::HAUL_OIL => match t {
                Some(t) if t.owner == player => {
                    let tf = self.flags_of_kind(t.kind);
                    if u.carrying && tf & flags::OIL_STORAGE != 0 {
                        24
                    } else if !u.carrying && tf & flags::OIL_SOURCE != 0 {
                        23
                    } else {
                        3
                    }
                }
                _ => 3,
            },
            action::DEMOLISH => {
                if allied == Some(false) {
                    19
                } else {
                    3
                }
            }
            action::HARVEST => self.harvest_choice(u, player, t, tile),
            _ => return None,
        })
    }

    /// The harvest action's table (first match).
    fn harvest_choice(&self, u: &Unit, player: u8, t: Option<&Unit>, tile: (i32, i32)) -> u8 {
        use crate::Gather;
        let Some(t) = t else {
            let tree = self
                .terrain
                .region(tile.0, tile.1)
                .is_some_and(terrain::region::is_forest);
            return if tree && !u.carrying { 23 } else { 3 };
        };
        if !self.allied_to(player, t.owner) {
            return 8;
        }
        if t.owner != player {
            return 3;
        }
        let tf = self.flags_of_kind(t.kind);
        if !u.carrying && tf & flags::MINE != 0 {
            23
        } else if u.carrying
            && ((u.gather == Gather::Lumber && tf & flags::WOOD_STORAGE != 0)
                || (tf & flags::GOLD_STORAGE != 0 && tf & flags::WOOD_STORAGE == 0))
        {
            // Lumber to a lumber store; anything to a store taking gold but not lumber.
            24
        } else if t.building && t.hp < self.stats_of(t.kind).hit_points {
            27
        } else {
            3
        }
    }

    /// The command filter ("Which units take part in a command"): whether unit `u` takes `command`.
    pub fn takes(&self, u: &Unit, command: u8) -> bool {
        use kind::*;
        let f = self.flags_of_kind(u.kind);
        let has = |b: u32| f & b != 0;
        let attacks = has(flags::CAN_ATTACK);
        let caster = has(flags::CASTER);
        let caster_or_mage = has(flags::CASTER) || has(flags::MAGE_LIKE);
        match command {
            23 => has(flags::PEON | flags::TANKER) && !u.carrying,
            24 => has(flags::PEON | flags::TANKER) && u.carrying,
            17 => has(flags::GROUND_ATTACK) && attacks,
            4 | 8..=16 => attacks,
            19 => attacks && has(flags::KILLED_BY_INVISIBILITY),
            5 if self.rules.enhanced
                && [DEMOLITION_SQUAD, SAPPER, FLYING_MACHINE, ZEPPELIN, EYE_OF_KILROGG].contains(&u.kind) =>
            {
                self.is_mobile(u)
            }
            5 => attacks,
            2 => true,
            3 | 6 => self.is_mobile(u),
            27 => has(flags::PEON),
            29 => has(flags::TRANSPORT),
            38..=41 => caster && [PALADIN, TURALYON, LOTHAR, UTHER, KNIGHT].contains(&u.kind),
            42..=47 => caster_or_mage && [MAGE, KHADGAR].contains(&u.kind),
            48 | 49 | 55 => caster && [OGRE_MAGE, DENTARG, CHOGALL, OGRE].contains(&u.kind),
            50..=54 | 56 => caster_or_mage && [DEATH_KNIGHT, GULDAN, TERON_GOREFIEND].contains(&u.kind),
            _ => false,
        }
    }

    /// A command from target mode, the buttons or the keys, for the units of the player's synced selection that take
    /// it.
    fn issue(&mut self, player: u8, command: u8, target: Option<UnitId>, tile: (i32, i32)) {
        if !handled(command) || !self.terrain.contains(tile.0, tile.1) {
            return;
        }
        // "Release first" (round 101, static study; the release traced: HW4): before the formation offset and the
        // filter, so a held worker is released even when no selected unit takes the command.
        self.release_workers(player);
        for (i, aim) in self.aims(player, tile) {
            let u = &self.units[i];
            if u.owner != player || Some(u.id) == target || !self.takes(u, command) {
                continue;
            }
            if self.rules.enhanced {
                self.units[i].remembered = None;
            }
            let id = self.units[i].id;
            self.give_command(i, command, target, aim);
            self.forget_attacker_after_command(id);
        }
    }

    /// Gives one unit command `command` with its target and aimed tile (orders spec, "Commands").
    pub(crate) fn give_command(&mut self, i: usize, command: u8, target: Option<UnitId>, tile: (i32, i32)) {
        let u = &self.units[i];
        if u.dying || u.building || self.stride_of(u.kind) == 0 {
            return;
        }
        self.players_command_clears_mark(i);
        let u = &self.units[i];
        let (player, units) = (u.owner, vec![u.id]);
        let c = match command {
            2 => Command::Stop { player, units },
            3 => Command::Smart {
                player,
                units,
                target,
                tile,
            },
            // Orders spec, "Commands" (round-49 errata): with a target unit, the unit walks to the target's tile and
            // records the unit as its target.
            4 => {
                let goal = target.and_then(|t| self.unit(t)).map_or(tile, |t| t.tile);
                return self.command(i, Order::MoveAttack { goal, target });
            }
            5 => Command::Patrol {
                player,
                units,
                to: tile,
            },
            6 => {
                let o = match target {
                    Some(target) => Order::Follow { target },
                    None => Order::Move {
                        goal: tile,
                        target: None,
                    },
                };
                return self.command(i, o);
            }
            8 | 10 => match target {
                Some(target) => Command::Attack { player, units, target },
                None if self.wall_tile(tile) => return self.command(i, Order::AttackWall { tile }),
                None => Command::AttackMove {
                    player,
                    units,
                    to: tile,
                },
            },
            9 | 11 => match target {
                Some(target) => Command::Attack { player, units, target },
                None if self.wall_tile(tile) => return self.command(i, Order::AttackWall { tile }),
                None => Command::Stop { player, units },
            },
            12 | 16 => {
                let Some(target) = target else { return };
                let o = if command == 12 {
                    Order::Chase { target }
                } else {
                    Order::IdleReact { target }
                };
                return self.command(i, o);
            }
            13 | 15 => Command::StandGround { player, units },
            14 => return self.command(i, Order::TowerWatch { target: None }),
            17 => Command::AttackGround { player, units, tile },
            19 => Command::Demolish {
                player,
                units,
                target,
                tile,
            },
            23 => Command::Harvest {
                player,
                units,
                target,
                tile,
            },
            // Harvest spec, "Returning goods" (round-62 errata; `orc9-orc09-return-goods-2200` n = 1150 and 1265): a
            // target unit is taken as given, with no test of its owner, kind or what it accepts.
            24 => match target {
                Some(depot) => {
                    return self.command(i, Order::ReturnGoods { depot: Some(depot) });
                }
                None => Command::ReturnGoods { player, units },
            },
            27 => {
                let Some(target) = target else { return };
                Command::Repair { player, units, target }
            }
            29 => Command::Unload {
                player,
                units,
                target,
                tile,
            },
            38..=56 => {
                let id = u.id;
                return self.cast_command(player, id, command, target, tile);
            }
            _ => return,
        };
        if let Some(o) = self.command_order(i, &c) {
            self.command(i, o);
        }
    }

    /// Set (Ctrl+digit): the group is cleared and filled from the synced selection in order, stopping at the first
    /// unit that isn't the player's own. A group is a list of unit slots ended by its first empty entry, and slot 0
    /// can't be told apart from an empty entry, so a unit in slot 0 is never held: the group ends there and the units
    /// after it are left out too ("What a group holds", V-G3; static study).
    fn set_group(&mut self, player: u8, group: u8) {
        if group as usize >= GROUPS || player as usize >= PLAYERS {
            return;
        }
        // An empty slot counts as not the player's own and ends the set (PI1d); a dying own unit is taken (PI17a).
        let mut g = Vec::new();
        for slot in self.selected_slots(player) {
            match self.unit_in_slot(slot) {
                Some(u) if u.owner == player && u.slot != 0 => g.push(u.id),
                _ => break,
            }
        }
        self.selections[player as usize].groups[group as usize] = g;
    }

    /// Recall (digit): the group is tidied, then its units that aren't inside anything or in orders 22, 25 or 26
    /// become the player's synced selection.
    fn recall_group(&mut self, player: u8, group: u8) {
        if group as usize >= GROUPS || player as usize >= PLAYERS {
            return;
        }
        // A group that is empty before its tidy changes nothing (PI3a); one the tidy empties empties the selection
        // (PI17b).
        if self.selections[player as usize].groups[group as usize].is_empty() {
            return;
        }
        self.tidy_group(player, group, true);
        let g = self.selections[player as usize].groups[group as usize].clone();
        let picked: Vec<u16> = g
            .iter()
            .filter_map(|&id| self.unit(id))
            .filter(|u| !u.inside && !busy(u.order))
            .map(|u| u.slot)
            .collect();
        self.set_selection(player, &picked);
    }

    /// The group tidy ("Control groups", recall): units that are dead or dying, no longer the player's own, or (in a
    /// group of more than one entry) not group-selectable are removed, each removal moving the group's last entry into
    /// the gap. The recall's tidy checks the moved entry again (`recheck`); the tidy at every kill goes on to the next
    /// entry, so the entry just moved into the gap isn't checked again in that pass (V-G1, V-G2; static study).
    fn tidy_group(&mut self, player: u8, group: u8, recheck: bool) {
        let mut g = std::mem::take(&mut self.selections[player as usize].groups[group as usize]);
        let mut n = 0;
        while n < g.len() {
            let several = g.len() > 1;
            let ok = self
                .unit(g[n])
                .is_some_and(|u| u.alive() && u.owner == player && (!several || self.stats_of(u.kind).rect_selectable));
            if ok {
                n += 1;
            } else {
                g.swap_remove(n);
                if !recheck {
                    n += 1;
                }
            }
        }
        self.selections[player as usize].groups[group as usize] = g;
    }

    /// The tidy at every kill (Enhanced only: Classic rules have no control groups): right after a unit starts dying,
    /// every player's 18 groups are tidied with the recall's removal rule, minus the recheck of the entry moved into
    /// a gap (`core/unit-slots.md`, "Death, corpses and release"; `ui/selecting-and-commanding.md`, "The tidy at every
    /// kill").
    pub(crate) fn tidy_all_groups(&mut self) {
        if !self.rules.enhanced {
            return;
        }
        for player in 0..PLAYERS as u8 {
            for group in 0..GROUPS as u8 {
                self.tidy_group(player, group, false);
            }
        }
    }
}

/// Orders 22, 25 and 26: inside a building being constructed, entering or inside a mine or platform.
pub fn busy(o: Order) -> bool {
    matches!(o, Order::InsideSite { .. } | Order::Enter { .. } | Order::Inside { .. })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Gather;
    use crate::terrain::Terrain;
    use crate::world::tests::{scripts, tileset};
    use tidewater_formats::udta::{UnitStats, UnitTable};

    const FOOTMAN: u8 = 0x00;
    const GRUNT: u8 = 0x01;
    const PEASANT: u8 = 0x02;
    const PEON: u8 = 0x03;
    const CATAPULT: u8 = 0x05;
    const TANKER: u8 = 0x1a;
    const GRYPHON: u8 = 0x2a;
    const FARM: u8 = 0x3a;
    const BARRACKS: u8 = 0x3c;
    const HALL: u8 = 0x4a;
    const MILL: u8 = 0x4c;
    const PLATFORM: u8 = 0x56;
    const MINE: u8 = 0x5c;

    fn table() -> UnitTable {
        let mut t = vec![UnitStats::default(); 110];
        let unit = |f: u32, rc: u8| UnitStats {
            hit_points: 60,
            size: (1, 1),
            box_size: (31, 31),
            flags: f,
            right_click: rc,
            rect_selectable: true,
            ..Default::default()
        };
        let fight = flags::LAND | flags::CAN_ATTACK;
        t[FOOTMAN as usize] = unit(fight, action::ATTACK);
        t[GRUNT as usize] = unit(fight, action::ATTACK);
        t[PEASANT as usize] = unit(flags::LAND | flags::PEON | flags::CAN_ATTACK, action::HARVEST);
        t[PEON as usize] = t[PEASANT as usize];
        t[CATAPULT as usize] = unit(fight | flags::GROUND_ATTACK, action::ATTACK);
        t[TANKER as usize] = unit(flags::SEA | flags::TANKER, action::HAUL_OIL);
        t[GRYPHON as usize] = unit(flags::AIR | flags::CAN_ATTACK, action::ATTACK);
        t[kind::SAPPER as usize] = unit(fight | flags::KILLED_BY_INVISIBILITY, action::DEMOLISH);
        t[kind::FLYING_MACHINE as usize] = unit(flags::AIR, action::MOVE);
        t[kind::MAGE as usize] = unit(fight | flags::CASTER, action::ATTACK);
        t[kind::KNIGHT as usize] = unit(fight, action::ATTACK);
        let building = |f: u32, size: u16| UnitStats {
            hit_points: 400,
            size: (size, size),
            box_size: (size * 32 - 1, size * 32 - 1),
            flags: flags::BUILDING | f,
            ..Default::default()
        };
        t[FARM as usize] = building(0, 2);
        t[BARRACKS as usize] = building(0, 3);
        t[HALL as usize] = building(flags::GOLD_STORAGE | flags::WOOD_STORAGE, 4);
        t[MILL as usize] = building(flags::WOOD_STORAGE, 3);
        t[PLATFORM as usize] = building(flags::OIL_SOURCE, 2);
        t[MINE as usize] = building(flags::MINE, 3);
        UnitTable(t)
    }

    fn map() -> World {
        let tiles = vec![0u16; 64 * 64];
        let mut w = World::new(
            Terrain::from_map(&tileset(), &tiles, None, None, 64, 64, false),
            table(),
            scripts(),
            0,
        );
        w.players[0].controller = crate::ai::controller::HUMAN;
        w.players[1].controller = crate::ai::controller::HUMAN;
        w
    }

    fn spawn(w: &mut World, kind: u8, owner: u8, tile: (i32, i32)) -> UnitId {
        w.spawn(kind, owner, tile, 0).unwrap()
    }

    fn slot(w: &World, id: UnitId) -> u16 {
        w.unit(id).unwrap().slot
    }

    fn select(w: &mut World, units: &[UnitId]) {
        w.step(&[Command::Select {
            player: 0,
            units: w.slots_of(units),
        }]);
    }

    fn rc(w: &World, id: UnitId, target: Option<UnitId>, tile: (i32, i32)) -> Option<u8> {
        w.right_click_command(w.index(id).unwrap(), 0, target, tile)
    }

    fn intent(w: &World, id: UnitId) -> Option<Order> {
        let u = w.unit(id).unwrap();
        u.pending.or(Some(u.order))
    }

    #[test]
    fn handled_commands() {
        let yes: Vec<u8> = (0..60).filter(|&c| handled(c)).collect();
        let mut want: Vec<u8> = (2..=6).chain(8..=17).chain([19, 23, 24, 27, 29]).collect();
        want.extend(38..=56);
        assert_eq!(yes, want);
    }

    #[test]
    fn formation_keeps_a_small_group_in_shape() {
        let tiles = [(10, 10), (11, 10), (12, 11)];
        assert_eq!(
            formation(&tiles, (30, 20), false, (64, 64)),
            vec![(29, 20), (30, 20), (31, 21)]
        );
    }

    #[test]
    fn formation_converges_a_wide_group() {
        assert_eq!(
            formation(&[(10, 5), (20, 5)], (40, 30), false, (64, 64)),
            vec![(40, 30), (40, 30)]
        );
    }

    #[test]
    fn formation_inside_the_box_or_at_a_wall_aims_at_the_tile() {
        let tiles = [(10, 10), (12, 12)];
        assert_eq!(formation(&tiles, (11, 11), false, (64, 64)), vec![(11, 11); 2]);
        assert_eq!(formation(&tiles, (30, 30), true, (64, 64)), vec![(30, 30); 2]);
    }

    #[test]
    fn formation_clamps_to_the_map() {
        // Mean (1, 0): offsets (61, 62); the first unit's aim is clamped.
        assert_eq!(
            formation(&[(0, 0), (2, 0)], (62, 62), false, (64, 64)),
            vec![(61, 62), (63, 62)]
        );
    }

    #[test]
    fn right_click_enemy_grunt_attacks_with_every_footman() {
        let mut w = map();
        let men: Vec<UnitId> = (0..3).map(|n| spawn(&mut w, FOOTMAN, 0, (10 + n, 10))).collect();
        let grunt = spawn(&mut w, GRUNT, 1, (20, 10));
        select(&mut w, &men);
        for &m in &men {
            assert_eq!(rc(&w, m, Some(grunt), (20, 10)), Some(8));
        }
        w.step(&[Command::RightClick {
            player: 0,
            target: Some(grunt),
            tile: (20, 10),
        }]);
        for &m in &men {
            assert_eq!(intent(&w, m), Some(Order::Attack { target: grunt }));
        }
    }

    #[test]
    fn harvest_table() {
        let mut w = map();
        let peasant = spawn(&mut w, PEASANT, 0, (5, 5));
        let barracks = spawn(&mut w, BARRACKS, 0, (10, 10));
        let mine = spawn(&mut w, MINE, slots::NEUTRAL, (20, 20));
        let hall = spawn(&mut w, HALL, 0, (30, 5));
        let mill = spawn(&mut w, MILL, 0, (30, 20));
        let grunt = spawn(&mut w, GRUNT, 1, (6, 5));
        // Damaged own barracks, empty-handed: repair.
        let b = w.index(barracks).unwrap();
        w.units[b].hp = 100;
        assert_eq!(rc(&w, peasant, Some(barracks), (10, 10)), Some(27));
        // A gold mine takes the "allied unit of another player" row: smart move.
        assert_eq!(rc(&w, peasant, Some(mine), (20, 20)), Some(3));
        // An enemy: attack.
        assert_eq!(rc(&w, peasant, Some(grunt), (6, 5)), Some(8));
        let p = w.index(peasant).unwrap();
        // Carrying gold to a hall at full hit points (it stores lumber too): repair check, so 3.
        w.units[p].carrying = true;
        w.units[p].gather = Gather::Gold;
        assert_eq!(rc(&w, peasant, Some(hall), (30, 5)), Some(3));
        // Carrying gold to a lumber mill at full hit points: 3.
        assert_eq!(rc(&w, peasant, Some(mill), (30, 20)), Some(3));
        // Carrying lumber to the hall: 24.
        w.units[p].gather = Gather::Lumber;
        assert_eq!(rc(&w, peasant, Some(hall), (30, 5)), Some(24));
        // A map gives a building flag bit 12 but not 18: a lumber carrier still returns there.
        w.stats.0[FARM as usize].flags |= flags::GOLD_STORAGE;
        let farm = spawn(&mut w, FARM, 0, (40, 40));
        assert_eq!(rc(&w, peasant, Some(farm), (40, 40)), Some(24));
    }

    #[test]
    fn harvest_on_a_tree_tile_has_no_target() {
        let mut w = map();
        let peon = spawn(&mut w, PEON, 0, (5, 5));
        let n = w.terrain.at(8, 5).unwrap();
        w.terrain.regions[n] = terrain::region::FOREST;
        assert_eq!(rc(&w, peon, None, (8, 5)), Some(23));
        assert_eq!(rc(&w, peon, None, (9, 5)), Some(3));
        let p = w.index(peon).unwrap();
        w.units[p].carrying = true;
        assert_eq!(rc(&w, peon, None, (8, 5)), Some(3));
    }

    #[test]
    fn tanker_and_sapper_rows() {
        let mut w = map();
        let tanker = spawn(&mut w, TANKER, 0, (4, 4));
        let platform = spawn(&mut w, PLATFORM, 0, (10, 10));
        assert_eq!(rc(&w, tanker, Some(platform), (10, 10)), Some(23));
        let sapper = spawn(&mut w, kind::SAPPER, 0, (20, 20));
        let farm = spawn(&mut w, FARM, 1, (24, 20));
        assert_eq!(rc(&w, sapper, Some(farm), (24, 20)), Some(19));
        assert_eq!(rc(&w, sapper, None, (30, 20)), Some(3));
    }

    #[test]
    fn right_click_on_a_wall_tile_attacks_it() {
        let mut w = map();
        let f = spawn(&mut w, FOOTMAN, 0, (4, 4));
        w.terrain.set_bits(8, 4, terrain::bits::HUMAN_WALL);
        assert_eq!(rc(&w, f, None, (8, 4)), Some(11));
        assert_eq!(rc(&w, f, None, (9, 4)), Some(3));
    }

    #[test]
    fn right_click_skips_enemies_and_the_target_itself() {
        let mut w = map();
        let mine = spawn(&mut w, FOOTMAN, 0, (4, 4));
        let other = spawn(&mut w, FOOTMAN, 0, (5, 4));
        let enemy = spawn(&mut w, GRUNT, 1, (8, 8));
        select(&mut w, &[enemy, mine, other]);
        let before = intent(&w, enemy);
        w.step(&[Command::RightClick {
            player: 0,
            target: Some(mine),
            tile: (4, 4),
        }]);
        assert_eq!(intent(&w, enemy), before, "not the player's own");
        assert_eq!(intent(&w, mine), Some(Order::Idle), "the target itself");
        assert_eq!(intent(&w, other), Some(Order::Follow { target: mine }));
    }

    #[test]
    fn right_click_moves_with_the_formation_offset() {
        let mut w = map();
        let men: Vec<UnitId> = [(10, 10), (11, 10), (12, 11)]
            .into_iter()
            .map(|t| spawn(&mut w, FOOTMAN, 0, t))
            .collect();
        select(&mut w, &men);
        w.step(&[Command::RightClick {
            player: 0,
            target: None,
            tile: (30, 20),
        }]);
        let goals: Vec<_> = men.iter().map(|&m| intent(&w, m)).collect();
        assert_eq!(
            goals,
            [(29, 20), (30, 20), (31, 21)].map(|goal| Some(Order::Move { goal, target: None }))
        );
    }

    /// A world with `n` footmen for player 0 in a row and the selection limit `limit` (0 for nine).
    fn crowd(n: i32, limit: u8) -> (World, Vec<UnitId>) {
        let mut w = map();
        w.rules.extras.selection_limit = limit;
        let men: Vec<UnitId> = (0..n).map(|x| spawn(&mut w, FOOTMAN, 0, (5 + x, 10))).collect();
        (w, men)
    }

    #[test]
    fn the_selection_limit_decides_how_many_units_are_kept() {
        use crate::extras::UNLIMITED;
        for (limit, kept) in [(0, 9), (12, 12), (18, 14), (UNLIMITED, 14)] {
            let (mut w, men) = crowd(14, limit);
            select(&mut w, &men);
            assert_eq!(w.selection(0), men[..kept], "limit {limit}");
        }
    }

    #[test]
    fn a_command_reaches_every_selected_unit_and_the_formation_covers_them_all() {
        let (mut w, men) = crowd(14, 18);
        select(&mut w, &men);
        w.step(&[Command::RightClick {
            player: 0,
            target: None,
            tile: (40, 30),
        }]);
        let tiles: Vec<_> = men.iter().map(|&m| w.unit(m).unwrap().tile).collect();
        let want = formation(&tiles, (40, 30), false, (64, 64));
        for (&m, goal) in men.iter().zip(want) {
            assert_eq!(intent(&w, m), Some(Order::Move { goal, target: None }));
        }
        // With the original limit the rest of the crowd does not move.
        let (mut w, men) = crowd(14, 0);
        select(&mut w, &men);
        w.step(&[Command::RightClick {
            player: 0,
            target: None,
            tile: (40, 30),
        }]);
        assert!(matches!(intent(&w, men[8]), Some(Order::Move { .. })));
        assert!(!matches!(intent(&w, men[9]), Some(Order::Move { .. })));
    }

    #[test]
    fn a_lost_unit_closes_the_selection_up_from_the_overflow() {
        let (mut w, men) = crowd(12, 12);
        select(&mut w, &men);
        let i = w.index(men[2]).unwrap();
        w.kill(i);
        w.step(&[]);
        let mut want = men.clone();
        want.remove(2);
        assert_eq!(w.selection(0), want);
        assert_eq!(
            w.selections[0].places.iter().flatten().count(),
            9,
            "the overflow moved up into the places"
        );
        assert_eq!(w.ext.more[0], want[9..]);
    }

    #[test]
    fn a_big_selection_is_a_control_group_and_comes_back() {
        let (mut w, men) = crowd(14, 18);
        select(&mut w, &men);
        w.step(&[Command::SetGroup { player: 0, group: 3 }]);
        select(&mut w, &men[..1]);
        assert_eq!(w.selection(0).len(), 1);
        w.step(&[Command::RecallGroup { player: 0, group: 3 }]);
        assert_eq!(w.selection(0), men);
    }

    #[test]
    fn two_machines_agree_with_a_big_selection() {
        let one = || {
            let (mut w, men) = crowd(14, 255);
            let mut hashes = Vec::new();
            for step in 0..60 {
                let cmds = match step {
                    0 => vec![Command::Select {
                        player: 0,
                        units: w.slots_of(&men),
                    }],
                    1 => vec![Command::RightClick {
                        player: 0,
                        target: None,
                        tile: (30, 30),
                    }],
                    _ => vec![],
                };
                w.step(&cmds);
                hashes.push(w.state_hash());
            }
            hashes
        };
        assert_eq!(one(), one());
    }

    #[test]
    fn the_overflow_is_not_saved_and_a_load_starts_with_no_selection() {
        let (mut a, men) = crowd(14, 18);
        select(&mut a, &men);
        let bytes = a.save_state();
        let (mut b, _) = crowd(14, 0);
        b.restore(&bytes, 1).unwrap();
        assert_eq!(b.rules.extras.selection_limit, 18);
        assert!(b.selection(0).is_empty());
        assert!(b.ext.more.iter().all(Vec::is_empty));
    }

    #[test]
    fn with_the_original_limit_the_hash_and_state_are_as_before() {
        let (mut a, men) = crowd(14, 0);
        select(&mut a, &men);
        assert!(a.ext.is_empty());
        assert!(a.rules.extras.is_default());
    }

    fn filter(w: &World, id: UnitId, c: u8) -> bool {
        w.takes(w.unit(id).unwrap(), c)
    }

    #[test]
    fn command_filter() {
        let mut w = map();
        let footman = spawn(&mut w, FOOTMAN, 0, (1, 1));
        let catapult = spawn(&mut w, CATAPULT, 0, (2, 1));
        let flyer = spawn(&mut w, kind::FLYING_MACHINE, 0, (4, 4));
        let peasant = spawn(&mut w, PEASANT, 0, (6, 1));
        let sapper = spawn(&mut w, kind::SAPPER, 0, (7, 1));
        let mage = spawn(&mut w, kind::MAGE, 0, (8, 1));
        let knight = spawn(&mut w, kind::KNIGHT, 0, (9, 1));
        assert!(!filter(&w, footman, 17) && filter(&w, catapult, 17));
        assert!(filter(&w, sapper, 19) && !filter(&w, footman, 19));
        assert!(filter(&w, flyer, 5) && filter(&w, footman, 5), "Enhanced patrol");
        assert!(filter(&w, peasant, 23) && !filter(&w, footman, 23));
        assert!(filter(&w, peasant, 27) && !filter(&w, footman, 27));
        assert!(!filter(&w, peasant, 24));
        assert!(filter(&w, mage, 43) && !filter(&w, footman, 43) && !filter(&w, mage, 51));
        assert!(!filter(&w, knight, 39), "the knight lacks flag bit 17 in the data");
        assert!(filter(&w, flyer, 3) && filter(&w, footman, 2));
        w.rules.enhanced = false;
        assert!(!filter(&w, flyer, 5) && filter(&w, footman, 5), "Classic patrol");
    }

    /// Orders spec, "Commands forget a stale attacker" (V272): a player's command clears the remembered order before
    /// the target check, so an attack on a target inside something leaves a plain pending idle with nothing to
    /// resume.
    #[test]
    fn a_players_command_on_a_lost_target_has_nothing_to_resume() {
        let mut w = map();
        w.rules.enhanced = true;
        let footman = spawn(&mut w, FOOTMAN, 0, (10, 10));
        let grunt = spawn(&mut w, GRUNT, 1, (14, 10));
        let gi = w.index(grunt).unwrap();
        w.units[gi].inside = true;
        let i = w.index(footman).unwrap();
        w.units[i].remembered = Some(crate::world::Remembered::AttackMove { goal: (30, 10) });
        select(&mut w, &[footman]);
        w.step(&[Command::Issue {
            player: 0,
            command: 8,
            target: Some(grunt),
            tile: (14, 10),
        }]);
        let u = w.unit(footman).unwrap();
        assert_eq!((intent(&w, footman), u.remembered), (Some(Order::Idle), None));
    }

    #[test]
    fn target_mode_attack_ground_reaches_only_the_catapult() {
        let mut w = map();
        let footman = spawn(&mut w, FOOTMAN, 0, (1, 1));
        let catapult = spawn(&mut w, CATAPULT, 0, (2, 1));
        select(&mut w, &[footman, catapult]);
        w.step(&[Command::Issue {
            player: 0,
            command: 17,
            target: None,
            tile: (3, 1),
        }]);
        assert_eq!(intent(&w, footman), Some(Order::Idle));
        assert!(matches!(intent(&w, catapult), Some(Order::AttackGround { .. })));
    }

    /// Orders spec, "A player's command forgets the attacker" (round-55 errata, `human09-fleet-3000` battleship 1539
    /// and the static-study rows): each unit that takes the command forgets its recorded attacker, whatever its state;
    /// a selected unit that fails the filter keeps its record.
    #[test]
    fn a_taken_command_forgets_the_attacker() {
        let mut w = map();
        let footman = spawn(&mut w, FOOTMAN, 0, (10, 10));
        let catapult = spawn(&mut w, CATAPULT, 0, (11, 10));
        let grunt = spawn(&mut w, GRUNT, 1, (12, 12));
        for id in [footman, catapult] {
            let i = w.index(id).unwrap();
            w.units[i].attacked_by = Some(grunt);
        }
        let record = |w: &World, id: UnitId| w.unit(id).unwrap().attacked_by;
        select(&mut w, &[grunt, footman, catapult]);
        // Attack ground: only the catapult takes it.
        w.step(&[Command::Issue {
            player: 0,
            command: 17,
            target: None,
            tile: (20, 10),
        }]);
        assert_eq!(record(&w, catapult), None, "took the command");
        assert_eq!(record(&w, footman), Some(grunt), "failed the filter");
        // Stop, with the attacker alive, detected and within reach: forgotten.
        w.step(&[Command::Issue {
            player: 0,
            command: 2,
            target: None,
            tile: (10, 10),
        }]);
        assert_eq!(record(&w, footman), None);
        assert_eq!(intent(&w, footman), Some(Order::Idle), "nothing seeds its search");
        // Round-56 static correction: a right-click keeps the record.
        let i = w.index(footman).unwrap();
        w.units[i].attacked_by = Some(grunt);
        select(&mut w, &[footman]);
        w.step(&[Command::RightClick {
            player: 0,
            target: None,
            tile: (2, 2),
        }]);
        assert_eq!(record(&w, footman), Some(grunt));
        assert!(matches!(intent(&w, footman), Some(Order::Move { .. })));
    }

    #[test]
    fn target_mode_patrol_by_rule_set() {
        for enhanced in [true, false] {
            let mut w = map();
            w.rules.enhanced = enhanced;
            let footman = spawn(&mut w, FOOTMAN, 0, (1, 1));
            let flyer = spawn(&mut w, kind::FLYING_MACHINE, 0, (4, 4));
            select(&mut w, &[footman, flyer]);
            w.step(&[Command::Issue {
                player: 0,
                command: 5,
                target: None,
                tile: (40, 40),
            }]);
            assert!(matches!(intent(&w, footman), Some(Order::Patrol { .. })));
            assert_eq!(
                matches!(intent(&w, flyer), Some(Order::Patrol { .. })),
                enhanced,
                "enhanced {enhanced}"
            );
        }
    }

    #[test]
    fn unhandled_or_off_map_commands_do_nothing() {
        let mut w = map();
        let footman = spawn(&mut w, FOOTMAN, 0, (1, 1));
        select(&mut w, &[footman]);
        w.step(&[
            Command::Issue {
                player: 0,
                command: 7,
                target: None,
                tile: (5, 5),
            },
            Command::Issue {
                player: 0,
                command: 3,
                target: None,
                tile: (64, 5),
            },
        ]);
        assert_eq!(intent(&w, footman), Some(Order::Idle));
    }

    #[test]
    fn groups_set_and_recall() {
        let mut w = map();
        let knights: Vec<UnitId> = (0..4).map(|n| spawn(&mut w, FOOTMAN, 0, (n, 1))).collect();
        let other = spawn(&mut w, FOOTMAN, 0, (10, 10));
        select(&mut w, &knights);
        w.step(&[Command::SetGroup { player: 0, group: 1 }]);
        select(&mut w, &[other]);
        w.step(&[Command::RecallGroup { player: 0, group: 1 }]);
        assert_eq!(w.selection(0), knights);
    }

    #[test]
    fn setting_a_group_stops_at_the_first_foreign_unit() {
        let mut w = map();
        let a = spawn(&mut w, FOOTMAN, 0, (1, 1));
        let foe = spawn(&mut w, GRUNT, 1, (2, 1));
        let b = spawn(&mut w, FOOTMAN, 0, (3, 1));
        select(&mut w, &[a, foe, b]);
        w.step(&[Command::SetGroup { player: 0, group: 2 }]);
        assert_eq!(w.selections[0].groups[2], vec![a]);
    }

    /// V-G3: slot 0 reads as empty, so setting a group stops at a unit in that slot.
    #[test]
    fn setting_a_group_stops_at_slot_zero() {
        let mut w = map();
        let a = spawn(&mut w, FOOTMAN, 0, (1, 1));
        let b = spawn(&mut w, FOOTMAN, 0, (3, 1));
        let c = spawn(&mut w, FOOTMAN, 0, (5, 1));
        let i = w.index(b).unwrap();
        w.units[i].slot = 0;
        select(&mut w, &[a, b, c]);
        w.step(&[Command::SetGroup { player: 0, group: 2 }]);
        assert_eq!(w.selections[0].groups[2], vec![a]);
    }

    #[test]
    fn recall_tidies_the_group() {
        let mut w = map();
        let units: Vec<UnitId> = (0..4).map(|n| spawn(&mut w, FOOTMAN, 0, (n, 1))).collect();
        w.selections[0].groups[3] = units.clone();
        // The second unit now belongs to someone else: the last entry moves into its place.
        let i = w.index(units[1]).unwrap();
        w.change_owner(i, 1);
        // The third is inside something: kept in the group, but not selected.
        let i = w.index(units[2]).unwrap();
        w.units[i].inside = true;
        w.step(&[Command::RecallGroup { player: 0, group: 3 }]);
        assert_eq!(w.selections[0].groups[3], vec![units[0], units[3], units[2]]);
        assert_eq!(w.selection(0), vec![units[0], units[3]]);
    }

    /// V-G1: every kill tidies every player's groups, so the order of the rest depends on the order of the kills; the
    /// moved entry isn't rechecked in a kill's pass. Classic has no groups, so nothing is tidied there.
    #[test]
    fn a_kill_tidies_every_players_groups() {
        let mut w = map();
        let u: Vec<UnitId> = (0..4).map(|n| spawn(&mut w, FOOTMAN, 0, (n, 1))).collect();
        w.selections[0].groups[1] = u.clone();
        let kill = |w: &mut World, id| {
            let i = w.index(id).unwrap();
            w.kill(i);
        };
        kill(&mut w, u[1]);
        assert_eq!(w.selections[0].groups[1], vec![u[0], u[3], u[2]]);
        kill(&mut w, u[0]);
        assert_eq!(w.selections[0].groups[1], vec![u[2], u[3]]);
        let mut c = map();
        c.rules.enhanced = false;
        let v: Vec<UnitId> = (0..2).map(|n| spawn(&mut c, FOOTMAN, 0, (n, 1))).collect();
        c.selections[0].groups[1] = v.clone();
        let i = c.index(v[0]).unwrap();
        c.kill(i);
        assert_eq!(c.selections[0].groups[1], v);
    }

    /// Round 107, PI1a and PI1b: a select names slots as sent; an empty one stays, and a unit created in it later is
    /// selected with nothing sent.
    #[test]
    fn an_empty_slot_stays_selected_and_a_new_unit_fills_it() {
        let mut w = map();
        let a = spawn(&mut w, FOOTMAN, 0, (1, 1));
        let sa = slot(&w, a);
        let gone = spawn(&mut w, FOOTMAN, 0, (5, 5));
        let sg = slot(&w, gone);
        let i = w.index(gone).unwrap();
        w.kill(i);
        for _ in 0..5000 {
            w.step(&[]);
        }
        assert!(w.unit(gone).is_none(), "released");
        w.step(&[Command::Select {
            player: 0,
            units: vec![sg],
        }]);
        assert_eq!(w.selected_slots(0), vec![sg]);
        assert!(w.selection(0).is_empty());
        w.step(&[]);
        assert_eq!(w.selected_slots(0), vec![sg]);
        let b = spawn(&mut w, FOOTMAN, 0, (3, 1));
        assert_eq!(slot(&w, b), sg, "the released slot is reused first");
        w.step(&[]);
        assert_eq!(w.selection(0), vec![b]);
        // The first slot empty: the first-unit commands have no unit, and a group set is empty (PI1d).
        w.step(&[Command::Select {
            player: 0,
            units: vec![sg + 50, sa],
        }]);
        assert_eq!(w.first_selected(0), None);
        w.step(&[Command::SetGroup { player: 0, group: 1 }]);
        assert!(w.selections[0].groups[1].is_empty());
    }

    /// Round 107, PI2a, PI10a: the loss is an event. A select naming a unit that is already dying keeps it, also past
    /// the release of its slot; a unit lost after it was named leaves.
    #[test]
    fn a_dying_unit_named_by_a_select_stays_selected() {
        for enhanced in [true, false] {
            let mut w = map();
            w.rules.enhanced = enhanced;
            let a = spawn(&mut w, FOOTMAN, 0, (1, 1));
            let b = spawn(&mut w, FOOTMAN, 0, (2, 1));
            let i = w.index(a).unwrap();
            w.kill(i);
            select(&mut w, &[a, b]);
            assert_eq!(w.selection(0), vec![a, b]);
            for _ in 0..3 {
                w.step(&[]);
            }
            assert_eq!(w.selection(0), vec![a, b], "the select named a unit already lost");
            let j = w.index(b).unwrap();
            w.kill(j);
            w.step(&[]);
            assert_eq!(w.selection(0), vec![a], "a unit lost after the select leaves");
            for _ in 0..5000 {
                w.step(&[]);
            }
            assert!(w.unit(a).is_none(), "released");
            assert_eq!(
                w.selected_slots(0).len(),
                1,
                "the slot stays selected after the release"
            );
        }
    }

    /// Round 107, PI3a, PI17a, PI17b: a set takes a dying own unit and stops at a unit that isn't the player's own; a
    /// recall of a group empty before its tidy changes nothing, and a tidy that empties the group empties the
    /// selection.
    #[test]
    fn groups_take_dying_units_and_recall_by_the_empty_rules() {
        let mut w = map();
        let a = spawn(&mut w, FOOTMAN, 0, (1, 1));
        let e = spawn(&mut w, FOOTMAN, 1, (2, 1));
        let c = spawn(&mut w, FOOTMAN, 0, (3, 1));
        let i = w.index(a).unwrap();
        w.kill(i);
        select(&mut w, &[a, e, c]);
        w.step(&[Command::SetGroup { player: 0, group: 1 }]);
        assert_eq!(w.selections[0].groups[1], vec![a]);
        // Recall: the tidy removes the dying unit and the selection is empty.
        w.step(&[Command::RecallGroup { player: 0, group: 1 }]);
        assert!(w.selections[0].groups[1].is_empty());
        assert!(w.selected_slots(0).is_empty());
        // An empty group: nothing changes.
        select(&mut w, &[c]);
        w.step(&[Command::RecallGroup { player: 0, group: 2 }]);
        assert_eq!(w.selection(0), vec![c]);
    }

    /// GI1a (round 108, traced): recalling a group whose units are all inside something empties the selection, and the
    /// group keeps its units; once the unit is outside the recall selects it.
    #[test]
    fn recalling_a_group_of_inside_units_empties_the_selection() {
        let mut w = map();
        let a = spawn(&mut w, FOOTMAN, 0, (1, 1));
        let c = spawn(&mut w, FOOTMAN, 0, (3, 1));
        w.selections[0].groups[1] = vec![a];
        w.selections[0].groups[2] = vec![a, c];
        let i = w.index(a).unwrap();
        w.units[i].inside = true;
        select(&mut w, &[c]);
        w.step(&[Command::RecallGroup { player: 0, group: 1 }]);
        assert!(w.selected_slots(0).is_empty());
        assert_eq!(w.selections[0].groups[1], vec![a]);
        // A group with a unit outside selects that unit.
        w.step(&[Command::RecallGroup { player: 0, group: 2 }]);
        assert_eq!(w.selection(0), vec![c]);
        w.units[i].inside = false;
        w.step(&[Command::RecallGroup { player: 0, group: 1 }]);
        assert_eq!(w.selection(0), vec![a]);
    }

    /// V-G2: after a removal the kill's pass goes on to the next entry; the recall's tidy checks the moved one.
    #[test]
    fn the_kill_tidy_does_not_recheck_the_moved_entry() {
        let mut w = map();
        let u: Vec<UnitId> = (0..3).map(|n| spawn(&mut w, FOOTMAN, 0, (n, 1))).collect();
        w.selections[0].groups[2] = u.clone();
        for &id in &u[1..] {
            let i = w.index(id).unwrap();
            w.change_owner(i, 1);
        }
        let other = spawn(&mut w, FOOTMAN, 0, (9, 9));
        let i = w.index(other).unwrap();
        w.kill(i);
        assert_eq!(w.selections[0].groups[2], vec![u[0], u[2]]);
        w.step(&[Command::RecallGroup { player: 0, group: 2 }]);
        assert_eq!(w.selections[0].groups[2], vec![u[0]]);
    }

    #[test]
    fn lost_units_leave_the_synced_selection() {
        for enhanced in [true, false] {
            let mut w = map();
            w.rules.enhanced = enhanced;
            let units: Vec<UnitId> = (0..3).map(|n| spawn(&mut w, FOOTMAN, 0, (n, 1))).collect();
            select(&mut w, &units);
            let i = w.index(units[0]).unwrap();
            w.units[i].inside = true;
            w.step(&[]);
            let places = w.selections[0].places;
            if enhanced {
                assert_eq!(
                    &places[..3],
                    &[Some(slot(&w, units[1])), Some(slot(&w, units[2])), None]
                );
            } else {
                assert_eq!(
                    &places[..3],
                    &[None, Some(slot(&w, units[1])), Some(slot(&w, units[2]))]
                );
            }
        }
    }

    /// V93: a selected peon reaching its completed great hall in order 24 leaves the selection in the update order 25
    /// is given, before it steps in.
    #[test]
    fn a_unit_leaves_the_selection_when_it_starts_entering() {
        for enhanced in [true, false] {
            let mut w = map();
            w.rules.enhanced = enhanced;
            let hall = spawn(&mut w, HALL, 0, (10, 10));
            let p = spawn(&mut w, PEON, 0, (7, 11));
            let f = spawn(&mut w, FOOTMAN, 0, (1, 1));
            select(&mut w, &[p, f]);
            let pi = w.index(p).unwrap();
            w.units[pi].carrying = true;
            w.command(pi, Order::ReturnGoods { depot: Some(hall) });
            let mut entered = false;
            for _ in 0..60 {
                w.step(&[]);
                let u = w.unit(p).unwrap();
                if let Order::Enter { stepped, .. } = u.order {
                    assert!(!stepped && !u.inside, "still outside");
                    let places = w.selections[0].places;
                    if enhanced {
                        assert_eq!(&places[..2], &[Some(slot(&w, f)), None]);
                    } else {
                        assert_eq!(&places[..2], &[None, Some(slot(&w, f))]);
                    }
                    entered = true;
                    break;
                }
                assert_eq!(w.selection(0), vec![p, f], "kept while it walks");
            }
            assert!(entered);
        }
    }

    fn set_speed(speed: u8) -> Command {
        Command::Speed { player: 0, speed }
    }

    /// `net/lockstep.md`, "Speed" (NC2): a value above the top speed is ignored, in both rule sets.
    #[test]
    fn a_speed_above_the_top_is_ignored() {
        let mut w = map();
        w.speed = TOP_SPEED_ENHANCED;
        w.step(&[set_speed(TOP_SPEED_ENHANCED + 1)]);
        assert_eq!(w.speed, TOP_SPEED_ENHANCED, "nothing changes above the top level");
        w.step(&[set_speed(3)]);
        assert_eq!(w.speed, 3);
        w.rules.enhanced = false;
        w.step(&[set_speed(TOP_SPEED_CLASSIC + 1)]);
        assert_eq!(w.speed, 3, "Classic mode: a command carrying 7 is ignored");
        w.step(&[set_speed(TOP_SPEED_CLASSIC)]);
        assert_eq!(w.speed, TOP_SPEED_CLASSIC);
        w.step(&[set_speed(0)]);
        assert_eq!(w.speed, 0);
    }

    /// NC1: two + presses before the first is carried out both carry 5, so they make one step, not two.
    #[test]
    fn two_presses_from_the_same_speed_make_one_step() {
        let mut w = map();
        w.speed = 4;
        w.step(&[set_speed(5), set_speed(5)]);
        assert_eq!(w.speed, 5);
    }

    #[test]
    fn extra_speed_levels_raise_the_top_in_enhanced_rules_only() {
        for (levels, enhanced, top) in [(0, true, 8), (1, true, 9), (3, true, 11), (3, false, 6)] {
            let mut w = map();
            w.rules.extras.extra_speeds = levels;
            w.rules.enhanced = enhanced;
            w.speed = 5;
            for _ in 0..8 {
                w.step(&[set_speed((w.speed + 1).min(top))]);
            }
            assert_eq!(w.speed, top, "{levels} levels, enhanced {enhanced}");
            w.step(&[set_speed(top + 1)]);
            assert_eq!(w.speed, top, "{levels} levels, enhanced {enhanced}: above the top");
        }
    }

    #[test]
    fn two_machines_agree_at_an_extra_speed() {
        let one = || {
            let mut w = map();
            w.rules.extras.extra_speeds = 2;
            let f = spawn(&mut w, FOOTMAN, 0, (5, 5));
            let mut hashes = Vec::new();
            for step in 0..40 {
                let cmds = match step {
                    0 => vec![Command::Select {
                        player: 0,
                        units: w.slots_of(&[f]),
                    }],
                    1..=6 => vec![set_speed(w.speed + 1)],
                    7 => vec![Command::RightClick {
                        player: 0,
                        target: None,
                        tile: (30, 30),
                    }],
                    _ => vec![],
                };
                w.step(&cmds);
                hashes.push(w.state_hash());
            }
            assert_eq!(w.speed, 10);
            hashes
        };
        assert_eq!(one(), one());
    }

    #[test]
    fn a_paused_cycle_changes_nothing_but_the_counter() {
        let mut w = map();
        let f = spawn(&mut w, FOOTMAN, 0, (1, 1));
        select(&mut w, &[f]);
        w.step(&[Command::Pause { player: 0 }]);
        let before = w.units.clone();
        let tick = w.tick;
        w.step(&[]);
        assert_eq!(w.tick, tick + 1);
        assert_eq!(w.units, before);
        // A pause while paused does nothing; a resume ends it.
        w.step(&[Command::Pause { player: 0 }]);
        assert!(w.paused);
        w.step(&[Command::Resume { player: 0 }]);
        assert!(!w.paused);
        w.step(&[Command::Resume { player: 0 }]);
        assert!(!w.paused);
        assert!(
            !w.events.iter().any(is_pause_event),
            "single player shows no pause notices"
        );
    }

    /// `ui/game-menu.md`, "Pausing": three pauses per player in Enhanced multiplayer, shown with the pauses left.
    #[test]
    fn enhanced_multiplayer_pauses_are_counted_and_announced() {
        use crate::economy::Event;
        let mut w = map();
        w.rules.multiplayer = true;
        w.rules.enhanced = true;
        let before = w.state_hash();
        for left in [2, 1, 0] {
            w.step(&[Command::Pause { player: 1 }]);
            assert!(w.paused);
            assert!(w.events.contains(&Event::Paused { player: 1, left }));
            w.events.clear();
            w.step(&[Command::Pause { player: 1 }]);
            assert!(!w.events.iter().any(is_pause_event), "already paused: nothing");
            w.step(&[Command::Resume { player: 0 }]);
            assert!(!w.paused);
            assert!(w.events.contains(&Event::Resumed { player: 0 }));
            w.events.clear();
        }
        w.step(&[Command::Pause { player: 1 }]);
        assert!(!w.paused, "a fourth pause from the same player does nothing");
        assert_eq!(w.pauses[1], 0);
        w.step(&[Command::Pause { player: 0 }]);
        assert!(w.paused, "the other player still has its own");
        assert_ne!(w.state_hash(), before);
    }

    #[test]
    fn classic_multiplayer_has_no_pause_limit_and_no_notices() {
        let mut w = map();
        w.rules.multiplayer = true;
        w.rules.enhanced = false;
        for _ in 0..5 {
            w.step(&[Command::Pause { player: 1 }]);
            assert!(w.paused);
            w.step(&[Command::Resume { player: 1 }]);
        }
        assert!(!w.events.iter().any(is_pause_event));
        assert_eq!(w.pauses[1], crate::PAUSES_PER_PLAYER);
    }

    fn is_pause_event(e: &crate::economy::Event) -> bool {
        use crate::economy::Event;
        matches!(e, Event::Paused { .. } | Event::Resumed { .. })
    }

    /// Harvest spec, round-62 vectors (`orc9-orc09-return-goods-2200` n = 1150 and 1265): command 24 with a target unit
    /// takes it as given: a carrying worker's gold to a lumber mill, a carrying tanker's oil to its own platform, and an
    /// enemy's building too. A worker that carries nothing doesn't take the command.
    #[test]
    fn return_goods_takes_its_target_as_given() {
        let mut w = map();
        let peon = spawn(&mut w, PEON, 0, (1, 1));
        let empty = spawn(&mut w, PEON, 0, (2, 1));
        let tanker = spawn(&mut w, TANKER, 0, (30, 30));
        let mill = spawn(&mut w, MILL, 0, (10, 10));
        let platform = spawn(&mut w, PLATFORM, 0, (34, 30));
        let barracks = spawn(&mut w, BARRACKS, 1, (20, 20));
        for id in [peon, tanker] {
            let i = w.index(id).unwrap();
            w.units[i].carrying = true;
            w.units[i].gather = crate::world::Gather::Gold;
        }
        for (unit, target) in [(peon, mill), (tanker, platform), (peon, barracks)] {
            select(&mut w, &[unit, empty]);
            w.step(&[Command::Issue {
                player: 0,
                command: 24,
                target: Some(target),
                tile: (0, 0),
            }]);
            assert_eq!(intent(&w, unit), Some(Order::ReturnGoods { depot: Some(target) }));
            assert_eq!(intent(&w, empty), Some(Order::Idle));
        }
    }
}
