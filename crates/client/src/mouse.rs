//! Selecting and commanding with the mouse and keys (spec `docs/specs/ui/selecting-and-commanding.md`): the mouse
//! regions and the per-mode handler table, the click and box rules for selecting, the local remembered groups, the
//! hover list and the target of a click, the right-click test and its feedback, the control-group keys and the
//! in-game key table.
//!
//! The rules read the units through a [`Board`], a snapshot of what the local player may know about each unit, so
//! they can be tested on synthetic boards. What runs inside a cycle (the synced selection, the right-click command and
//! the command filter) is in the sim (`tidewater_sim::select`). No SDL types here.
//!
//! Our readings where the spec leaves room:
//! - A selection box spans `left ..= left + box width` (and the same for y), so a point on either edge is inside.
//! - The ring walk of the same-type search revisits the ring's first corner at the end of its north side; a unit is
//!   taken at most once.
//! - The double-click time is 500 ms, as is the double-press window for control groups (the system setting isn't
//!   specified).
//! - The ring search's limit uses the client's view width in tiles: the spec's 14 at the default one map pixel per
//!   screen pixel; only the `--free-zoom` development option changes it.

use std::collections::HashMap;

use tidewater_formats::udta::flags;
use tidewater_sim::select::{MOBILE, busy};
use tidewater_sim::vision::bit;
use tidewater_sim::{TILE_PX, Unit, UnitId, World};

use crate::hud::Rect;

/// Places in a selection.
#[cfg(test)]
pub const PLACES: usize = 9;
/// A drag at most this many pixels wide and high is a click.
pub const SMALL: i32 = 4;
/// Remembered groups kept by the client.
pub const REMEMBERED: usize = 16;
/// Units listed under the cursor.
pub const HOVER: usize = 9;
/// Double press of a control-group key, and our double-click time, in milliseconds.
pub const DOUBLE_MS: u64 = 500;
/// How long a right-click target flashes, in cycles.
pub const FLASH_CYCLES: u32 = 31;

/// Flash colours (palette indices).
pub mod colour {
    pub const OWN: u8 = 250;
    pub const ALLIED: u8 = 251;
    pub const OTHER: u8 = 249;
}

/// The neutral player (gold mines, critters), allied to everyone.
const NEUTRAL: u8 = 15;

// --- regions and the handler table -------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    Minimap,
    Panel,
    MapView,
    Elsewhere,
}

/// Where the regions are, in interface pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Regions {
    pub minimap: Rect,
    pub panel: Rect,
    pub view: Rect,
}

impl Regions {
    /// The original 640 x 480 screen ("Mouse regions"); the client uses its own layout (`of_layout`).
    #[cfg(test)]
    pub const ORIGINAL: Regions = Regions {
        minimap: Rect::new(24, 26, 128, 128),
        panel: Rect::new(0, 160, 176, 320),
        view: Rect::new(176, 16, 448, 448),
    };

    /// The regions of a client layout: the panel runs from the information panel's top to the canvas bottom.
    pub fn of_layout(l: &crate::hud::Layout) -> Regions {
        Regions {
            minimap: l.minimap,
            panel: Rect::new(0, l.info.y, l.info.w, l.canvas.1.saturating_sub(l.info.y.max(0) as u32)),
            view: l.viewport,
        }
    }

    /// The region holding interface pixel `p`; the minimap wins over the panel.
    pub fn at(&self, p: (f32, f32)) -> Region {
        if self.minimap.contains(p.0, p.1) {
            Region::Minimap
        } else if self.view.contains(p.0, p.1) {
            Region::MapView
        } else if self.panel.contains(p.0, p.1) {
            Region::Panel
        } else {
            Region::Elsewhere
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
}

/// The Preferences dialog's mouse style: the first has right-click commands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Style {
    #[default]
    First,
    Second,
}

/// The modes the handler table tells apart. A build menu counts as normal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModeKind {
    Normal,
    Target,
    Place,
}

/// What a button press does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Nothing,
    /// Start selecting (the drag runs to the release).
    Select,
    /// The right-click command at the map view's pointer.
    RightClick,
    /// The right-click command to the minimap tile.
    RightClickTile,
    /// Centre the view on the clicked tile.
    CentreView,
    /// Move the view to the minimap point.
    MoveView,
    /// Give the target at the map view's pointer.
    GiveTarget,
    /// Give the minimap tile as the target.
    GiveTargetTile,
    /// Placement mode's click.
    Place,
    /// End target or placement mode, sending nothing.
    EndMode,
    /// A right click on the panel or elsewhere: end the mode if one is on, else return the panel from a sub-menu.
    EndModeOrBack,
}

/// The handler table ("Mouse regions"). Nothing happens while paused or at a scrolling edge.
pub fn action(mode: ModeKind, style: Style, region: Region, button: Button, paused: bool, edge: bool) -> Action {
    use Action::*;
    if paused || edge {
        return Nothing;
    }
    match (region, button) {
        (Region::Panel | Region::Elsewhere, Button::Left) => Nothing,
        (Region::Panel | Region::Elsewhere, Button::Right) => EndModeOrBack,
        (Region::MapView, b) => match (mode, style, b) {
            (ModeKind::Normal, _, Button::Left) => Select,
            (ModeKind::Normal, Style::First, Button::Right) => RightClick,
            (ModeKind::Normal, Style::Second, Button::Right) => CentreView,
            (ModeKind::Target, _, Button::Left) => GiveTarget,
            (ModeKind::Place, _, Button::Left) => Place,
            (ModeKind::Target | ModeKind::Place, _, Button::Right) => EndMode,
        },
        (Region::Minimap, b) => match (mode, style, b) {
            (ModeKind::Target, _, Button::Left) => GiveTargetTile,
            (ModeKind::Normal, Style::First, Button::Right) => RightClickTile,
            _ => MoveView,
        },
    }
}

// --- the board -----------------------------------------------------------------------------------------------------

/// A rectangle in map pixels, edges included.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PxRect {
    pub l: i32,
    pub t: i32,
    pub r: i32,
    pub b: i32,
}

impl PxRect {
    /// The rectangle between two points, put in order.
    pub fn between(a: (i32, i32), b: (i32, i32)) -> Self {
        PxRect {
            l: a.0.min(b.0),
            t: a.1.min(b.1),
            r: a.0.max(b.0),
            b: a.1.max(b.1),
        }
    }

    /// At most 4 pixels wide and high.
    pub fn small(&self) -> bool {
        self.r - self.l <= SMALL && self.b - self.t <= SMALL
    }

    pub fn overlaps(&self, o: &PxRect) -> bool {
        self.l <= o.r && o.l <= self.r && self.t <= o.b && o.t <= self.b
    }

    pub fn contains(&self, p: (i32, i32)) -> bool {
        self.overlaps(&PxRect::between(p, p))
    }
}

/// What the rules need to know about one unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cand {
    pub id: UnitId,
    pub kind: u8,
    pub owner: u8,
    /// Map pixel of the footprint's top-left corner.
    pub pos: (i32, i32),
    pub tile: (i32, i32),
    /// Footprint in tiles (the type's unit size).
    pub size: (u8, u8),
    /// The type's selection box size in pixels.
    pub box_size: (i32, i32),
    pub air: bool,
    pub building: bool,
    /// Any of flag bits 0 to 4.
    pub mobile: bool,
    /// Dying or dead.
    pub dying: bool,
    /// Inside a transport, mine, platform or building.
    pub inside: bool,
    /// Hidden from the local player by the fog.
    pub hidden: bool,
    /// Detected by the local player.
    pub detected: bool,
    /// Its order or pending order is 22, 25 or 26.
    pub busy: bool,
    /// The type's "selectable via rectangle" value is 1.
    pub rect_selectable: bool,
}

impl Cand {
    /// The selection box: the type's box size centred in the footprint.
    pub fn sel_box(&self) -> PxRect {
        let l = self.pos.0 + 16 * self.size.0 as i32 - self.box_size.0 / 2;
        let t = self.pos.1 + 16 * self.size.1 as i32 - self.box_size.1 / 2;
        PxRect {
            l,
            t,
            r: l + self.box_size.0,
            b: t + self.box_size.1,
        }
    }

    fn present(&self) -> bool {
        !self.dying && !self.inside
    }

    fn area(&self) -> u32 {
        self.size.0 as u32 * self.size.1 as u32
    }
}

/// The local player's view of the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct View {
    pub local: u8,
    pub enhanced: bool,
    /// Players allied to the local player (bit p), the local player included.
    pub allies: u16,
    /// The visible map area in map pixels.
    pub view: PxRect,
    /// Its width in tiles.
    pub view_tiles: i32,
    /// The map's size in tiles.
    pub map: (i32, i32),
    /// How many units a selection holds: nine, or what the option `selection-limit` allows.
    pub limit: usize,
}

/// Whether map pixel `p` falls on a non-transparent pixel of the unit's current sprite frame.
pub type Opaque<'a> = &'a dyn Fn(&Cand, (i32, i32)) -> bool;

/// The units in slot order, with the ground and air unit of each tile.
pub struct Board<'a> {
    pub view: View,
    pub units: Vec<Cand>,
    ground: HashMap<(i32, i32), usize>,
    air: HashMap<(i32, i32), usize>,
    opaque: Opaque<'a>,
}

impl<'a> Board<'a> {
    pub fn new(view: View, units: Vec<Cand>, opaque: Opaque<'a>) -> Self {
        let (mut ground, mut air) = (HashMap::new(), HashMap::new());
        for (n, c) in units.iter().enumerate() {
            if !c.present() {
                continue;
            }
            if c.building {
                for dy in 0..c.size.1 as i32 {
                    for dx in 0..c.size.0 as i32 {
                        ground.entry((c.tile.0 + dx, c.tile.1 + dy)).or_insert(n);
                    }
                }
            } else if c.air {
                air.entry(c.tile).or_insert(n);
            } else {
                ground.entry(c.tile).or_insert(n);
            }
        }
        Board {
            view,
            units,
            ground,
            air,
            opaque,
        }
    }

    /// The board of a running game for the local player `local`.
    pub fn of_world(world: &World, local: u8, view: PxRect, view_tiles: i32, opaque: Opaque<'a>) -> Self {
        let me = bit(local) as u16;
        let allies = world.players.get(local as usize).map_or(0, |p| p.allies) | me | 1 << NEUTRAL;
        let units = world.units.iter().map(|u| cand(world, u, local)).collect();
        let v = View {
            local,
            enhanced: world.rules.enhanced,
            allies,
            view,
            view_tiles,
            map: (world.terrain.width as i32, world.terrain.height as i32),
            limit: world.rules.extras.selection_cap(),
        };
        Board::new(v, units, opaque)
    }

    pub fn get(&self, id: UnitId) -> Option<&Cand> {
        self.units.iter().find(|c| c.id == id)
    }

    fn ground_at(&self, t: (i32, i32)) -> Option<&Cand> {
        self.ground.get(&t).map(|&n| &self.units[n])
    }

    fn air_at(&self, t: (i32, i32)) -> Option<&Cand> {
        self.air.get(&t).map(|&n| &self.units[n])
    }

    /// "Who may be selected": the unit can be picked by the local player.
    pub fn can_pick(&self, c: &Cand) -> bool {
        c.present() && !c.hidden && (!self.view.enhanced || (!c.busy && c.detected))
    }

    /// Selectable via rectangle and the local player's own.
    pub fn group_selectable(&self, c: &Cand) -> bool {
        c.rect_selectable && c.owner == self.view.local
    }

    fn opaque(&self, c: &Cand, p: (i32, i32)) -> bool {
        (self.opaque)(c, p)
    }
}

/// One unit as the local player may know it.
pub fn cand(world: &World, u: &Unit, local: u8) -> Cand {
    let s = world.stats.0.get(u.kind as usize).copied().unwrap_or_default();
    let me = bit(local);
    Cand {
        id: u.id,
        kind: u.kind,
        owner: u.owner,
        pos: u.pos,
        tile: u.tile,
        size: u.size,
        box_size: (s.box_size.0 as i32, s.box_size.1 as i32),
        air: s.flags & flags::AIR != 0,
        building: u.building,
        mobile: s.flags & MOBILE != 0,
        dying: u.dying,
        inside: u.inside,
        hidden: u.owner != local && !world.reveal_view() && u.hidden_from & me != 0,
        detected: u.owner == local || u.detected_by & me != 0,
        busy: busy(u.order) || u.pending.is_some_and(busy),
        rect_selectable: s.rect_selectable,
    }
}

// --- remembered groups ---------------------------------------------------------------------------------------------

/// The client's 16 remembered groups ("Remembered groups"), not part of the game state.
#[derive(Clone, Debug, Default)]
pub struct Remembered {
    groups: [Vec<UnitId>; REMEMBERED],
    used: [u64; REMEMBERED],
    clock: u64,
}

impl Remembered {
    /// Stores a group of several units: they leave every group first, then it takes an empty group or the one used
    /// longest ago.
    pub fn store(&mut self, units: &[UnitId]) {
        for g in &mut self.groups {
            g.retain(|id| !units.contains(id));
        }
        let n = (0..REMEMBERED)
            .find(|&n| self.groups[n].is_empty())
            .unwrap_or_else(|| (0..REMEMBERED).min_by_key(|&n| self.used[n]).unwrap_or(0));
        self.groups[n] = units.to_vec();
        self.touch(n);
    }

    /// The group that holds `id`, marked as just used.
    pub fn recall(&mut self, id: UnitId) -> Option<Vec<UnitId>> {
        let n = self.groups.iter().position(|g| g.contains(&id))?;
        self.touch(n);
        Some(self.groups[n].clone())
    }

    /// Lost units leave every group: only those `keep` holds stay.
    pub fn retain(&mut self, keep: impl Fn(UnitId) -> bool) {
        for g in &mut self.groups {
            g.retain(|&u| keep(u));
        }
    }

    fn touch(&mut self, n: usize) {
        self.clock += 1;
        self.used[n] = self.clock;
    }
}

// --- selecting -----------------------------------------------------------------------------------------------------

/// Shift, Ctrl and Alt, and whether the press was a double-click.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub double: bool,
}

/// What a selecting action comes to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pick {
    /// The selection is kept and nothing is sent.
    Keep,
    /// Select these units (sent), asking for the selection sound if `sound`.
    Set { units: Vec<UnitId>, sound: bool },
}

/// The left button's release: the rectangle from the press point to `to`, in map pixels. `press` holds what the press
/// remembered (Shift; under Enhanced rules Ctrl and the double-click) and Alt as it is now. `sel` is the current
/// selection in place order.
pub fn release(
    board: &Board,
    from: (i32, i32),
    to: (i32, i32),
    press: Mods,
    sel: &[UnitId],
    rem: &mut Remembered,
) -> Pick {
    let r = PxRect::between(from, to);
    if board.view.enhanced
        && r.small()
        && let Some(p) = click_rules(board, (r.l, r.t), press, sel, rem)
    {
        return p;
    }
    box_rules(board, r, press.shift, press.alt, sel, rem)
}

/// The tap tolerance of a touchscreen (`docs/design/touch.md`): where a tap at map pixel `p` should select. A finger
/// is wider than a unit, so a tap that lands on no unit's selection box but within `radius` map pixels of one acts as
/// a click on the nearest such unit's centre; a tap on a box, or farther than `radius` from every unit, stays where it
/// was. Only units the local player can pick count, and the closest box wins (the first in slot order on a tie). This
/// changes which point the ordinary release rules ([`release`]) are given, never the rules.
pub fn tap_snap(board: &Board, p: (i32, i32), radius: i32) -> (i32, i32) {
    let mut best: Option<(i64, (i32, i32))> = None;
    for c in board.units.iter().filter(|c| board.can_pick(c)) {
        let b = c.sel_box();
        if b.contains(p) {
            return p;
        }
        let dx = (b.l - p.0).max(p.0 - b.r).max(0) as i64;
        let dy = (b.t - p.1).max(p.1 - b.b).max(0) as i64;
        let d = dx * dx + dy * dy;
        if d <= (radius as i64).pow(2) && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, ((b.l + b.r) / 2, (b.t + b.b) / 2)));
        }
    }
    best.map_or(p, |(_, centre)| centre)
}

/// The smart tap of a touchscreen (ours, the `touch-smart-tap` option, `docs/design/touch.md`): whether a tap at map
/// pixel `p` should be a right click rather than a left one. It is when the selection holds a unit of the local
/// player's that can move and the tap lands on no unit of the local player's, within `radius` map pixels of its box (the
/// tap tolerance, [`tap_snap`]): ground, an enemy or a resource then takes the order, and a tap on one of your own units
/// still selects it.
pub fn smart_tap(board: &Board, p: (i32, i32), radius: i32, sel: &[UnitId]) -> bool {
    let local = board.view.local;
    let commanding = board
        .units
        .iter()
        .any(|c| c.owner == local && c.mobile && !c.dying && sel.contains(&c.id));
    let near = |c: &Cand| {
        let b = c.sel_box();
        let dx = i64::from((b.l - p.0).max(p.0 - b.r).max(0));
        let dy = i64::from((b.t - p.1).max(p.1 - b.b).max(0));
        dx * dx + dy * dy <= i64::from(radius.max(0)).pow(2)
    };
    let on_own = board
        .units
        .iter()
        .any(|c| c.owner == local && board.can_pick(c) && near(c));
    commanding && !on_own
}

/// The click rules (Enhanced only); `None` hands the click to the box rules.
fn click_rules(board: &Board, p: (i32, i32), m: Mods, sel: &[UnitId], rem: &mut Remembered) -> Option<Pick> {
    // 1. Find the unit.
    let mut best: Option<&Cand> = None;
    for c in board
        .units
        .iter()
        .filter(|c| board.can_pick(c) && c.sel_box().contains(p))
    {
        best = Some(match best {
            None => c,
            Some(b) if !b.air && (board.opaque(c, p) || c.area() < b.area()) => c,
            Some(b) if b.air && !board.opaque(b, p) && c.area() < b.area() => c,
            Some(b) => b,
        });
    }
    let Some(found) = best else {
        return Some(Pick::Keep);
    };
    // 2 and 3.
    let double = m.double && sel.contains(&found.id);
    let many = m.ctrl || double;
    if m.shift && !sel.is_empty() && !many {
        return None;
    }
    if !many {
        // 5. Plain click.
        return Some(if m.alt {
            Pick::Set {
                units: rem.recall(found.id).unwrap_or_else(|| vec![found.id]),
                sound: false,
            }
        } else {
            Pick::Set {
                units: vec![found.id],
                sound: true,
            }
        });
    }
    // 4. All of this type on screen.
    let ring = same_type(board, found);
    let mut units = if m.shift {
        let primary_ok = sel
            .first()
            .and_then(|&id| board.get(id))
            .is_some_and(|p| p.owner == board.view.local && p.rect_selectable);
        if !primary_ok || !found.rect_selectable {
            return Some(Pick::Keep);
        }
        let mut merged = sel.to_vec();
        for id in ring {
            if merged.len() < board.view.limit && !merged.contains(&id) {
                merged.push(id);
            }
        }
        merged
    } else {
        ring
    };
    if units.len() == 1 && m.alt {
        if let Some(g) = rem.recall(units[0]) {
            units = g;
        }
    } else if units.len() > 1 {
        rem.store(&units);
    }
    Some(Pick::Set { units, sound: true })
}

/// The tiles of the square ring of side `side` around `c`: east along the top, then south, west and north, each side
/// after the first skipping its first tile.
fn ring(c: (i32, i32), side: i32) -> Vec<(i32, i32)> {
    let h = side / 2;
    let (x0, y0, x1, y1) = (c.0 - h, c.1 - h, c.0 + h, c.1 + h);
    let mut out: Vec<(i32, i32)> = (x0..=x1).map(|x| (x, y0)).collect();
    out.extend((y0 + 1..=y1).map(|y| (x1, y)));
    out.extend((x0..x1).rev().map(|x| (x, y1)));
    out.extend((y0..y1).rev().map(|y| (x0, y)));
    out
}

/// The same-type search of click rule 4: the found unit first, then the rings outward.
fn same_type(board: &Board, found: &Cand) -> Vec<UnitId> {
    let mut taken = vec![found.id];
    let limit = 2 * board.view.view_tiles.max(1);
    let mut side = 3;
    while side < limit && taken.len() < board.view.limit {
        let mut touched = false;
        for t in ring(found.tile, side) {
            if t.0 < 0 || t.1 < 0 || t.0 >= board.view.map.0 || t.1 >= board.view.map.1 {
                continue;
            }
            touched = true;
            let at = if found.air { board.air_at(t) } else { board.ground_at(t) };
            let Some(c) = at else { continue };
            if taken.len() < board.view.limit
                && !taken.contains(&c.id)
                && board.can_pick(c)
                && board.group_selectable(c)
                && c.owner == found.owner
                && c.kind == found.kind
                && c.sel_box().overlaps(&board.view.view)
            {
                taken.push(c.id);
            }
        }
        if !touched {
            break;
        }
        side += 2;
    }
    taken
}

/// The box rules (both rule sets).
fn box_rules(board: &Board, r: PxRect, shift: bool, alt: bool, sel: &[UnitId], rem: &mut Remembered) -> Pick {
    let local = board.view.local;
    // 1.
    let mut kept: Vec<UnitId> = if shift {
        sel.iter()
            .copied()
            .filter(|&id| board.get(id).is_some_and(|c| c.owner == local))
            .collect()
    } else {
        Vec::new()
    };
    let mut toggled = None;
    if shift && r.small() {
        toggled = kept
            .iter()
            .copied()
            .find(|&id| board.get(id).is_some_and(|c| c.sel_box().overlaps(&r)));
        kept.retain(|&id| Some(id) != toggled);
    }
    if kept.len() >= board.view.limit {
        return Pick::Keep;
    }
    // 2 and 3.
    let mut found: Vec<&Cand> = Vec::new();
    let mut fallback = None;
    for c in &board.units {
        if !board.can_pick(c) || Some(c.id) == toggled || !c.sel_box().overlaps(&r) || (shift && sel.contains(&c.id)) {
            continue;
        }
        if !board.group_selectable(c) {
            fallback = Some(c.id);
        } else if kept.len() + found.len() < board.view.limit {
            found.push(c);
        }
    }
    // 4.
    if r.small() && found.len() > 1 && !shift {
        let one = found.iter().find(|c| c.air).copied().unwrap_or(found[0]);
        found = vec![one];
    }
    // 5.
    let mut units = kept;
    if found.is_empty() && toggled.is_none() {
        match fallback {
            Some(f) => units = vec![f],
            None => return Pick::Keep,
        }
    } else {
        units.extend(found.iter().map(|c| c.id));
    }
    // 6 and 7.
    if units.len() == 1 && alt {
        if let Some(g) = rem.recall(units[0]) {
            units = g;
        }
        return Pick::Set { units, sound: false };
    }
    if units.len() > 1 {
        rem.store(&units);
    }
    Pick::Set {
        units,
        sound: toggled.is_none(),
    }
}

/// Whether the selection sound is asked for ("Setting the selection", step 1): Classic rules only for one unit.
pub fn sound_wanted(enhanced: bool, sound: bool, count: usize) -> bool {
    sound && count > 0 && (enhanced || count == 1)
}

// --- hovering and the target of a click ---------------------------------------------------------------------------

/// The units under the cursor ("Hovering"), the hovered unit first.
pub fn hover(board: &Board, cursor: (i32, i32)) -> Vec<UnitId> {
    let me = (cursor.0.div_euclid(TILE_PX), cursor.1.div_euclid(TILE_PX));
    let mut out: Vec<UnitId> = Vec::new();
    const OFFSETS: [i32; 5] = [0, -1, 1, -2, 2];
    for dx in OFFSETS {
        for dy in OFFSETS {
            let t = (me.0 + dx, me.1 + dy);
            for c in [board.ground_at(t), board.air_at(t)].into_iter().flatten() {
                if out.len() < HOVER
                    && !out.contains(&c.id)
                    && c.present()
                    && !c.hidden
                    && c.detected
                    && c.sel_box().contains(cursor)
                    && c.sel_box().overlaps(&board.view.view)
                {
                    out.push(c.id);
                }
            }
        }
    }
    out
}

/// The target of a right-click or a target-mode click on `tile` ("Choosing the target of a click"). `reveal` is the
/// reveal-map cheat; `explored` whether the tile is explored.
pub fn target(
    board: &Board,
    hovered: Option<UnitId>,
    tile: (i32, i32),
    reveal: bool,
    explored: bool,
) -> Option<UnitId> {
    let c = match hovered {
        Some(id) => board.get(id)?,
        None => board.ground_at(tile).filter(|c| c.building)?,
    };
    if c.dying {
        return None;
    }
    if reveal {
        return Some(c.id);
    }
    if !explored {
        return None;
    }
    (!c.hidden || c.building).then_some(c.id)
}

// --- right-click ---------------------------------------------------------------------------------------------------

/// Whether a right-click on the map sends the command ("Right-click on the map"). `blocked` is a pause or an open
/// menu; it only counts under Enhanced rules.
pub fn right_click_sends(board: &Board, sel: &[UnitId], target: Option<UnitId>, blocked: bool) -> bool {
    if board.view.enhanced && blocked {
        return false;
    }
    let Some(p) = sel.first().and_then(|&id| board.get(id)) else {
        return false;
    };
    if p.owner != board.view.local || !p.mobile {
        return false;
    }
    !(sel.len() == 1 && target == Some(p.id))
}

/// The flash colour of a right-click target: own, allied (the neutral player counts as allied) or other.
pub fn flash_colour(board: &Board, owner: u8) -> u8 {
    if owner == board.view.local {
        colour::OWN
    } else if owner < 16 && board.view.allies & (1 << owner) != 0 {
        colour::ALLIED
    } else {
        colour::OTHER
    }
}

// --- control groups ------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupKey {
    /// Ctrl+digit.
    Set,
    /// Digit.
    Recall,
    /// Alt+digit.
    AltRecall,
}

/// The command a control-group key sends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupSend {
    Set(u8),
    Recall(u8),
}

/// What a control-group key does.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GroupPlan {
    /// Select these units locally, with sound, without sending the selection.
    pub select: Option<Vec<UnitId>>,
    pub send: Option<GroupSend>,
    /// Move the view so this tile is its top-left tile.
    pub view: Option<(i32, i32)>,
}

/// The double-press memory: the last group set or recalled (with the recall sent), and when.
#[derive(Clone, Copy, Debug, Default)]
pub struct GroupKeys {
    last: Option<(u8, u64)>,
}

impl GroupKeys {
    /// Any other change of selection that is sent forgets the last group.
    pub fn forget(&mut self) {
        self.last = None;
    }

    /// A control-group key for group `g`, whose synced contents are `group`. `mode` is whether target or placement
    /// mode is on; `now` is in milliseconds.
    pub fn press(&mut self, board: &Board, key: GroupKey, g: u8, group: &[UnitId], mode: bool, now: u64) -> GroupPlan {
        let mut plan = GroupPlan::default();
        if !board.view.enhanced || (mode && key != GroupKey::AltRecall) {
            return plan;
        }
        if key == GroupKey::Set {
            plan.send = Some(GroupSend::Set(g));
            self.last = Some((g, now));
            return plan;
        }
        let few = group.len() < 2;
        let units: Vec<UnitId> = group
            .iter()
            .copied()
            .filter(|&id| {
                board.get(id).is_some_and(|c| {
                    c.present() && !c.busy && (few || (c.owner == board.view.local && board.group_selectable(c)))
                })
            })
            .collect();
        let centre = |units: &[UnitId]| group_view(board, units);
        if key == GroupKey::AltRecall {
            let present: Vec<UnitId> = group
                .iter()
                .copied()
                .filter(|&id| board.get(id).is_some_and(|c| c.present()))
                .collect();
            plan.view = centre(&present);
        }
        if units.is_empty() {
            return plan;
        }
        let double = self
            .last
            .is_some_and(|(lg, t)| lg == g && now.saturating_sub(t) <= DOUBLE_MS);
        if double {
            plan.view = centre(&units);
        } else {
            plan.send = Some(GroupSend::Recall(g));
            self.last = Some((g, now));
        }
        plan.select = Some(units);
        plan
    }
}

/// The view's top-left tile that centres it on `units`: their average tile minus half the view size.
pub fn group_view(board: &Board, units: &[UnitId]) -> Option<(i32, i32)> {
    let tiles: Vec<(i32, i32)> = units.iter().filter_map(|&id| board.get(id)).map(|c| c.tile).collect();
    let n = tiles.len() as i32;
    if n == 0 {
        return None;
    }
    let (sx, sy) = tiles.iter().fold((0, 0), |a, t| (a.0 + t.0, a.1 + t.1));
    let half = board.view.view_tiles / 2;
    Some((sx / n - half, sy / n - half))
}

// --- keys ----------------------------------------------------------------------------------------------------------

/// A key as the in-game table names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    /// A letter, upper case.
    Letter(char),
    Digit(u8),
    /// + or =.
    Plus,
    /// − or _.
    Minus,
    /// F1 to F12.
    F(u8),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct KeyMods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

/// What a key does ("Keys").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyAction {
    /// Game speed one step up (true) or down.
    Speed(bool),
    Group(GroupKey, u8),
    CentrePrimary,
    SoundDialog,
    HelpMenu,
    KeyboardHelp,
    LoadGame,
    ToggleMusic,
    GameMenu,
    Options,
    Pause,
    /// Multiplayer: surrender or quit dialog; otherwise quit to the main menu.
    Quit,
    Restart,
    ToggleSounds,
    SaveGame,
    Tips,
    /// Move the view to remembered view position 0 to 2 (F2 to F4).
    GotoView(u8),
    /// Remember the view position 0 to 2 (Shift+F2 to F4).
    RememberView(u8),
    SpeedDialog,
    Preferences,
    QuitDialog,
}

/// The in-game key table. `chat` is whether a chat line is being typed; `multiplayer` whether the game has more than
/// one human player.
pub fn key_action(key: Key, m: KeyMods, chat: bool, enhanced: bool, multiplayer: bool) -> Option<KeyAction> {
    use KeyAction::*;
    let either = m.ctrl || m.alt;
    Some(match key {
        Key::Plus | Key::Minus | Key::Digit(_) if chat => return None,
        Key::Plus | Key::Minus if enhanced && multiplayer => return None,
        Key::Plus => Speed(true),
        Key::Minus => Speed(false),
        Key::Digit(_) if !enhanced => return None,
        Key::Digit(d) if m.ctrl => Group(GroupKey::Set, d),
        Key::Digit(d) if m.alt => Group(GroupKey::AltRecall, d),
        Key::Digit(d) => Group(GroupKey::Recall, d),
        Key::F(1) => HelpMenu,
        Key::F(n @ 2..=4) if m.shift => RememberView(n - 2),
        Key::F(n @ 2..=4) => GotoView(n - 2),
        Key::F(5) => Options,
        Key::F(7) => SoundDialog,
        Key::F(8) => SpeedDialog,
        Key::F(9) => Preferences,
        Key::F(11) => SaveGame,
        Key::F(12) => LoadGame,
        Key::F(_) => return None,
        Key::Letter(c) if either => match (c, m.ctrl) {
            ('C', _) => CentrePrimary,
            ('D', true) => SoundDialog,
            ('H', _) => HelpMenu,
            ('K', _) => KeyboardHelp,
            ('L', false) => LoadGame,
            ('M', true) => ToggleMusic,
            ('M', false) => GameMenu,
            ('O', _) => Options,
            ('P', _) => Pause,
            ('Q', _) => Quit,
            ('R', _) if !multiplayer => Restart,
            ('S', true) => ToggleSounds,
            ('S', false) => SaveGame,
            ('T', _) => Tips,
            ('X', _) => QuitDialog,
            _ => return None,
        },
        Key::Letter(_) => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FOOTMAN: u8 = 0x00;
    const GRUNT: u8 = 0x01;
    const ARCHER: u8 = 0x08;
    const KNIGHT: u8 = 0x06;
    const PEASANT: u8 = 0x02;
    const GRYPHON: u8 = 0x2a;
    const FARM: u8 = 0x3a;

    fn unit(id: u32, kind: u8, owner: u8, tile: (i32, i32)) -> Cand {
        let building = kind == FARM;
        let air = kind == GRYPHON;
        Cand {
            id: UnitId(id),
            kind,
            owner,
            pos: (tile.0 * 32, tile.1 * 32),
            tile,
            size: if building { (2, 2) } else { (1, 1) },
            box_size: if building { (63, 63) } else { (31, 31) },
            air,
            building,
            mobile: !building,
            dying: false,
            inside: false,
            hidden: false,
            detected: true,
            busy: false,
            rect_selectable: !building,
        }
    }

    fn view(enhanced: bool) -> View {
        View {
            local: 0,
            enhanced,
            allies: 1 | 1 << NEUTRAL,
            view: PxRect {
                l: 0,
                t: 0,
                r: 14 * 32 - 1,
                b: 14 * 32 - 1,
            },
            view_tiles: 14,
            map: (64, 64),
            limit: PLACES,
        }
    }

    const NO_PIXELS: &dyn Fn(&Cand, (i32, i32)) -> bool = &|_, _| false;
    const ALL_PIXELS: &dyn Fn(&Cand, (i32, i32)) -> bool = &|_, _| true;

    fn board(enhanced: bool, units: Vec<Cand>) -> Board<'static> {
        Board::new(view(enhanced), units, NO_PIXELS)
    }

    /// The centre of a unit at `tile`, in map pixels.
    fn at(tile: (i32, i32)) -> (i32, i32) {
        (tile.0 * 32 + 16, tile.1 * 32 + 16)
    }

    fn click(b: &Board, p: (i32, i32), m: Mods, sel: &[UnitId]) -> Pick {
        release(b, p, p, m, sel, &mut Remembered::default())
    }

    fn ids(n: impl IntoIterator<Item = u32>) -> Vec<UnitId> {
        n.into_iter().map(UnitId).collect()
    }

    fn set(units: Vec<UnitId>, sound: bool) -> Pick {
        Pick::Set { units, sound }
    }

    const SHIFT: Mods = Mods {
        shift: true,
        ctrl: false,
        alt: false,
        double: false,
    };
    const CTRL: Mods = Mods {
        shift: false,
        ctrl: true,
        alt: false,
        double: false,
    };

    #[test]
    fn handler_table() {
        use Action::*;
        let a = |mode, style, region, button| action(mode, style, region, button, false, false);
        let (n, t, p) = (ModeKind::Normal, ModeKind::Target, ModeKind::Place);
        let (f, s) = (Style::First, Style::Second);
        let (map, mini) = (Region::MapView, Region::Minimap);
        let (l, r) = (Button::Left, Button::Right);
        assert_eq!(
            [a(n, f, map, l), a(n, f, map, r), a(n, f, mini, l), a(n, f, mini, r)],
            [Select, RightClick, MoveView, RightClickTile]
        );
        assert_eq!(
            [a(n, s, map, l), a(n, s, map, r), a(n, s, mini, l), a(n, s, mini, r)],
            [Select, CentreView, MoveView, MoveView]
        );
        assert_eq!(
            [a(t, f, map, l), a(t, f, map, r), a(t, f, mini, l), a(t, f, mini, r)],
            [GiveTarget, EndMode, GiveTargetTile, MoveView]
        );
        assert_eq!(
            [a(p, f, map, l), a(p, f, map, r), a(p, f, mini, l), a(p, f, mini, r)],
            [Place, EndMode, MoveView, MoveView]
        );
        assert_eq!(
            a(p, f, Region::Panel, r),
            EndModeOrBack,
            "placement mode; right click on the panel"
        );
        assert_eq!(
            a(n, f, Region::Panel, r),
            EndModeOrBack,
            "the panel returns from a sub-menu"
        );
        assert_eq!(a(n, f, Region::Elsewhere, l), Nothing);
        assert_eq!(action(n, f, map, l, true, false), Nothing, "paused");
        assert_eq!(action(n, f, map, r, false, true), Nothing, "at a scrolling edge");
    }

    #[test]
    fn regions_of_the_original_screen() {
        let r = Regions::ORIGINAL;
        assert_eq!(r.at((24.0, 26.0)), Region::Minimap);
        assert_eq!(r.at((151.0, 153.0)), Region::Minimap);
        assert_eq!(r.at((100.0, 200.0)), Region::Panel);
        assert_eq!(r.at((176.0, 16.0)), Region::MapView);
        assert_eq!(r.at((623.0, 463.0)), Region::MapView);
        assert_eq!(r.at((300.0, 470.0)), Region::Elsewhere);
        assert_eq!(r.at((10.0, 10.0)), Region::Elsewhere);
    }

    #[test]
    fn selection_box_is_centred_in_the_footprint() {
        let farm = unit(1, FARM, 0, (10, 10));
        assert_eq!(
            farm.sel_box(),
            PxRect {
                l: 320 + 32 - 31,
                t: 320 + 32 - 31,
                r: 320 + 32 - 31 + 63,
                b: 320 + 32 - 31 + 63
            }
        );
        assert!(
            farm.sel_box().contains((farm.sel_box().r, farm.sel_box().b)),
            "edges count"
        );
    }

    #[test]
    fn enhanced_click_on_an_own_footman() {
        let b = board(true, vec![unit(1, FOOTMAN, 0, (3, 3))]);
        assert_eq!(click(&b, at((3, 3)), Mods::default(), &[]), set(ids([1]), true));
    }

    #[test]
    fn click_on_empty_ground_keeps_the_selection() {
        for enhanced in [true, false] {
            let b = board(enhanced, vec![unit(1, FOOTMAN, 0, (3, 3))]);
            assert_eq!(click(&b, at((8, 8)), Mods::default(), &ids([1])), Pick::Keep);
        }
    }

    fn twelve(kind: u8) -> Vec<Cand> {
        (0..12).map(|n| unit(n + 1, kind, 0, (1 + n as i32, 2))).collect()
    }

    #[test]
    fn box_over_twelve_footmen_takes_the_first_nine() {
        for (enhanced, sound) in [(true, true), (false, true)] {
            let b = board(enhanced, twelve(FOOTMAN));
            let p = release(
                &b,
                (0, 0),
                (20 * 32, 4 * 32),
                Mods::default(),
                &[],
                &mut Remembered::default(),
            );
            assert_eq!(p, set(ids(1..=9), sound));
            // Classic asks for the sound only with one unit.
            assert_eq!(sound_wanted(enhanced, sound, 9), enhanced);
        }
    }

    #[test]
    fn a_larger_selection_limit_lets_a_box_take_more() {
        for (limit, want) in [(12, 12), (5, 5)] {
            let mut b = board(true, twelve(FOOTMAN));
            b.view.limit = limit;
            let p = release(
                &b,
                (0, 0),
                (20 * 32, 4 * 32),
                Mods::default(),
                &[],
                &mut Remembered::default(),
            );
            assert_eq!(p, set(ids(1..=want), true));
        }
    }

    #[test]
    fn box_mixtures() {
        // 3 footmen and an enemy grunt: the footmen.
        let mut u: Vec<Cand> = (0..3).map(|n| unit(n + 1, FOOTMAN, 0, (2 + n as i32, 2))).collect();
        u.push(unit(4, GRUNT, 1, (6, 2)));
        let b = board(true, u);
        let mut rem = Remembered::default();
        let p = release(&b, (0, 0), (300, 120), Mods::default(), &[], &mut rem);
        assert_eq!(p, set(ids(1..=3), true));
        // 2 enemy grunts only: the one found last.
        let b = board(true, vec![unit(1, GRUNT, 1, (2, 2)), unit(2, GRUNT, 1, (3, 2))]);
        assert_eq!(
            release(&b, (0, 0), (300, 120), Mods::default(), &[], &mut rem),
            set(ids([2]), true)
        );
        // Footmen and an own farm: only the footmen.
        let b = board(
            true,
            vec![
                unit(1, FARM, 0, (2, 2)),
                unit(2, FOOTMAN, 0, (5, 2)),
                unit(3, FOOTMAN, 0, (6, 2)),
            ],
        );
        assert_eq!(
            release(&b, (0, 0), (300, 120), Mods::default(), &[], &mut rem),
            set(ids([2, 3]), true)
        );
    }

    #[test]
    fn classic_small_click_prefers_the_air_unit() {
        let b = board(false, vec![unit(1, FOOTMAN, 0, (3, 3)), unit(2, GRYPHON, 0, (3, 3))]);
        assert_eq!(click(&b, at((3, 3)), Mods::default(), &[]), set(ids([2]), true));
    }

    #[test]
    fn shift_click_toggles_and_adds() {
        let mut u: Vec<Cand> = (0..3).map(|n| unit(n + 1, FOOTMAN, 0, (2 + 2 * n as i32, 2))).collect();
        u.push(unit(4, FOOTMAN, 0, (9, 9)));
        for enhanced in [true, false] {
            let b = board(enhanced, u.clone());
            let sel = ids(1..=3);
            // On a selected footman: removed, no sound.
            assert_eq!(click(&b, at((4, 2)), SHIFT, &sel), set(ids([1, 3]), false));
            // On an unselected one: 4 selected, with sound (Classic asks only for one unit).
            let p = click(&b, at((9, 9)), SHIFT, &sel);
            assert_eq!(p, set(ids(1..=4), true));
            assert_eq!(sound_wanted(enhanced, true, 4), enhanced);
        }
    }

    #[test]
    fn shift_ctrl_click_with_an_enemy_selected_does_nothing() {
        let b = board(true, vec![unit(1, GRUNT, 1, (2, 2)), unit(2, ARCHER, 0, (5, 5))]);
        let m = Mods {
            shift: true,
            ctrl: true,
            ..Default::default()
        };
        assert_eq!(click(&b, at((5, 5)), m, &ids([1])), Pick::Keep);
    }

    #[test]
    fn ctrl_click_finds_nine_archers_nearest_rings_first() {
        // 12 archers in a row from column 1; the click is on the one at column 6.
        let b = board(true, twelve(ARCHER));
        let mut rem = Remembered::default();
        let p = release(&b, at((6, 2)), at((6, 2)), CTRL, &[], &mut rem);
        // The found unit, then ring 3 (columns 5 to 7, east side first), ring 5 and so on.
        let want = ids([6, 7, 5, 8, 4, 9, 3, 10, 2]);
        assert_eq!(p, set(want.clone(), true));
        assert_eq!(rem.recall(UnitId(9)), Some(want), "stored as a remembered group");
    }

    #[test]
    fn ring_walk_order() {
        assert_eq!(
            ring((5, 5), 3),
            vec![(4, 4), (5, 4), (6, 4), (6, 5), (6, 6), (5, 6), (4, 6), (4, 5), (4, 4)]
        );
    }

    #[test]
    fn double_click_on_an_unselected_archer_is_a_plain_click() {
        let b = board(true, twelve(ARCHER));
        let m = Mods {
            double: true,
            ..Default::default()
        };
        assert_eq!(click(&b, at((3, 2)), m, &[]), set(ids([3]), true));
        // Already selected: all of the type on screen.
        let Pick::Set { units, .. } = click(&b, at((3, 2)), m, &ids([3])) else {
            panic!()
        };
        assert_eq!(units.len(), 9);
    }

    #[test]
    fn alt_click_recalls_the_remembered_group() {
        let b = board(true, twelve(FOOTMAN));
        let mut rem = Remembered::default();
        rem.store(&ids([2, 3, 4]));
        let m = Mods {
            alt: true,
            ..Default::default()
        };
        assert_eq!(
            release(&b, at((3, 2)), at((3, 2)), m, &[], &mut rem),
            set(ids([2, 3, 4]), false)
        );
    }

    #[test]
    fn remembered_groups_replace_the_one_used_longest_ago() {
        let mut rem = Remembered::default();
        for g in 0..REMEMBERED as u32 {
            rem.store(&ids([100 + 2 * g, 101 + 2 * g]));
        }
        rem.recall(UnitId(100));
        rem.store(&ids([1, 2]));
        assert!(rem.recall(UnitId(102)).is_none(), "the oldest unused group went");
        assert!(rem.recall(UnitId(100)).is_some());
        // A new group takes its units out of older ones.
        rem.store(&ids([100, 5]));
        assert_eq!(rem.recall(UnitId(101)), Some(ids([101])));
    }

    #[test]
    fn busy_units_are_not_picked_under_enhanced_rules() {
        let mut peasant = unit(1, PEASANT, 0, (3, 3));
        peasant.busy = true;
        let b = board(true, vec![peasant]);
        assert_eq!(
            release(&b, (0, 0), (300, 300), Mods::default(), &[], &mut Remembered::default()),
            Pick::Keep
        );
        let b = board(false, vec![peasant]);
        assert_eq!(
            release(&b, (0, 0), (300, 300), Mods::default(), &[], &mut Remembered::default()),
            set(ids([1]), true),
            "Classic rules skip the order test"
        );
    }

    #[test]
    fn pixel_and_footprint_tie_break() {
        // A farm under a footman's box: the later footman wins by its smaller footprint.
        let farm = unit(1, FARM, 0, (3, 3));
        let man = unit(2, FOOTMAN, 0, (3, 3));
        let p = at((3, 3));
        let b = board(true, vec![farm, man]);
        assert_eq!(click(&b, p, Mods::default(), &[]), set(ids([2]), true));
        // An air unit found first keeps the click while the point is on one of its pixels.
        let gryphon = unit(1, GRYPHON, 0, (3, 3));
        let b = Board::new(view(true), vec![gryphon, man], ALL_PIXELS);
        assert_eq!(click(&b, p, Mods::default(), &[]), set(ids([1]), true));
        let b = Board::new(view(true), vec![gryphon, man], NO_PIXELS);
        assert_eq!(
            click(&b, p, Mods::default(), &[]),
            set(ids([1]), true),
            "same footprint: kept"
        );
    }

    #[test]
    fn hover_and_target() {
        let mut fogged = unit(2, GRUNT, 1, (5, 5));
        fogged.hidden = true;
        let b = board(
            true,
            vec![unit(1, FOOTMAN, 0, (3, 3)), fogged, unit(3, FARM, 1, (8, 8))],
        );
        assert_eq!(hover(&b, at((3, 3))), ids([1]));
        assert!(hover(&b, at((5, 5))).is_empty(), "hidden");
        assert_eq!(target(&b, Some(UnitId(1)), (3, 3), false, true), Some(UnitId(1)));
        // A building on the clicked tile is the target without hovering.
        assert_eq!(target(&b, None, (9, 9), false, true), Some(UnitId(3)));
        assert_eq!(target(&b, None, (9, 9), false, false), None, "unexplored");
        assert_eq!(target(&b, Some(UnitId(2)), (5, 5), false, true), None, "hidden");
        assert_eq!(
            target(&b, Some(UnitId(2)), (5, 5), true, false),
            Some(UnitId(2)),
            "reveal-map cheat"
        );
    }

    #[test]
    fn right_click_tests() {
        let b = board(
            true,
            vec![
                unit(1, FOOTMAN, 0, (3, 3)),
                unit(2, GRUNT, 1, (5, 5)),
                unit(3, FARM, 0, (8, 8)),
            ],
        );
        assert!(right_click_sends(&b, &ids([1]), Some(UnitId(2)), false));
        assert!(
            !right_click_sends(&b, &ids([2]), None, false),
            "only an enemy unit selected"
        );
        assert!(
            !right_click_sends(&b, &ids([1]), Some(UnitId(1)), false),
            "the single unit itself"
        );
        assert!(!right_click_sends(&b, &ids([3]), None, false), "not mobile");
        assert!(!right_click_sends(&b, &ids([1]), None, true), "paused, Enhanced");
        assert!(right_click_sends(&board(false, b.units.clone()), &ids([1]), None, true));
        assert_eq!(flash_colour(&b, 1), colour::OTHER);
        assert_eq!(flash_colour(&b, 0), colour::OWN);
        assert_eq!(flash_colour(&b, NEUTRAL), colour::ALLIED);
    }

    #[test]
    fn control_group_keys() {
        let knights: Vec<Cand> = (0..4).map(|n| unit(n + 1, KNIGHT, 0, (2 + n as i32, 2))).collect();
        let b = board(true, knights);
        let group = ids(1..=4);
        let mut keys = GroupKeys::default();
        let p = keys.press(&b, GroupKey::Set, 1, &[], false, 0);
        assert_eq!(p.send, Some(GroupSend::Set(1)));
        // Later, press 1: the knights, recall sent.
        let p = keys.press(&b, GroupKey::Recall, 1, &group, false, 2000);
        assert_eq!(
            (p.select.clone(), p.send, p.view),
            (Some(group.clone()), Some(GroupSend::Recall(1)), None)
        );
        // Again within 500 ms: the view is centred and nothing is sent.
        let p = keys.press(&b, GroupKey::Recall, 1, &group, false, 2400);
        assert_eq!(p.send, None);
        assert_eq!(p.select, Some(group.clone()));
        // Average tile (3, 2), minus half the view (7).
        assert_eq!(p.view, Some((-4, -5)));
        // Any other sent change forgets the last group.
        keys.forget();
        assert!(keys.press(&b, GroupKey::Recall, 1, &group, false, 2500).send.is_some());
        // Target mode: set and recall do nothing, Alt+digit centres and recalls.
        let mut keys = GroupKeys::default();
        assert_eq!(
            keys.press(&b, GroupKey::Recall, 1, &group, true, 0),
            GroupPlan::default()
        );
        let p = keys.press(&b, GroupKey::AltRecall, 1, &group, true, 0);
        assert_eq!((p.send, p.view.is_some()), (Some(GroupSend::Recall(1)), true));
        // Classic: nothing.
        let b = board(false, b.units.clone());
        assert_eq!(
            keys.press(&b, GroupKey::Recall, 1, &group, false, 9999),
            GroupPlan::default()
        );
    }

    #[test]
    fn recalled_groups_take_only_pickable_units() {
        let mut inside = unit(2, KNIGHT, 0, (3, 3));
        inside.inside = true;
        let b = board(
            true,
            vec![unit(1, KNIGHT, 0, (2, 2)), inside, unit(3, GRUNT, 1, (4, 4))],
        );
        let p = GroupKeys::default().press(&b, GroupKey::Recall, 0, &ids([1, 2, 3]), false, 0);
        assert_eq!(p.select, Some(ids([1])));
        // A one-entry group needn't be own.
        let p = GroupKeys::default().press(&b, GroupKey::Recall, 0, &ids([3]), false, 0);
        assert_eq!(p.select, Some(ids([3])));
        let p = GroupKeys::default().press(&b, GroupKey::Recall, 0, &ids([2]), false, 0);
        assert_eq!(p, GroupPlan::default(), "nothing qualifies: nothing happens");
    }

    #[test]
    fn key_table() {
        let none = KeyMods::default();
        let ctrl = KeyMods { ctrl: true, ..none };
        let alt = KeyMods { alt: true, ..none };
        let shift = KeyMods { shift: true, ..none };
        let k = |key, m| key_action(key, m, false, true, false);
        assert_eq!(k(Key::Plus, none), Some(KeyAction::Speed(true)));
        assert_eq!(
            key_action(Key::Plus, none, false, true, true),
            None,
            "Enhanced multiplayer"
        );
        assert_eq!(
            key_action(Key::Minus, none, false, false, true),
            Some(KeyAction::Speed(false))
        );
        assert_eq!(key_action(Key::Digit(1), none, true, true, false), None, "chat");
        assert_eq!(key_action(Key::Digit(1), none, false, false, false), None, "Classic");
        assert_eq!(k(Key::Digit(1), ctrl), Some(KeyAction::Group(GroupKey::Set, 1)));
        assert_eq!(k(Key::Digit(1), alt), Some(KeyAction::Group(GroupKey::AltRecall, 1)));
        assert_eq!(k(Key::Digit(1), none), Some(KeyAction::Group(GroupKey::Recall, 1)));
        assert_eq!(k(Key::Letter('C'), alt), Some(KeyAction::CentrePrimary));
        assert_eq!(k(Key::Letter('M'), ctrl), Some(KeyAction::ToggleMusic));
        assert_eq!(k(Key::Letter('M'), alt), Some(KeyAction::GameMenu));
        assert_eq!(k(Key::Letter('S'), ctrl), Some(KeyAction::ToggleSounds));
        assert_eq!(k(Key::Letter('S'), alt), Some(KeyAction::SaveGame));
        assert_eq!(k(Key::Letter('D'), alt), None);
        assert_eq!(k(Key::Letter('P'), ctrl), Some(KeyAction::Pause));
        assert_eq!(
            key_action(Key::Letter('R'), ctrl, false, true, true),
            None,
            "no restart in multiplayer"
        );
        assert_eq!(k(Key::Letter('S'), none), None, "plain letters are the panel's");
        assert_eq!(k(Key::F(3), none), Some(KeyAction::GotoView(1)));
        assert_eq!(k(Key::F(3), shift), Some(KeyAction::RememberView(1)));
        assert_eq!(k(Key::F(10), none), None, "open question: not in the table");
    }

    #[test]
    fn a_tap_beside_a_unit_snaps_to_it() {
        let b = board(true, vec![unit(1, FOOTMAN, 0, (5, 5))]);
        let c = at((5, 5));
        // 20 px right of the box's edge (the box is 31 wide around the centre).
        let near = (c.0 + 15 + 20, c.1);
        assert_eq!(tap_snap(&b, near, 24), c);
        assert_eq!(tap_snap(&b, near, 18), near, "too far for the tolerance");
        assert_eq!(tap_snap(&b, c, 24), c);
        // On a box already: stays put, wherever on it.
        let corner = (c.0 + 10, c.1 - 12);
        assert_eq!(tap_snap(&b, corner, 24), corner);
    }

    #[test]
    fn a_tap_picks_the_nearest_of_two_units_and_ignores_unpickable_ones() {
        let mut hidden = unit(3, FOOTMAN, 0, (6, 5));
        hidden.hidden = true;
        let b = board(
            true,
            vec![unit(1, FOOTMAN, 0, (2, 5)), unit(2, GRUNT, 1, (8, 5)), hidden],
        );
        // Between tile 2 and tile 8, closer to 8 (a hidden unit at tile 6 does not count).
        let p = (7 * 32 + 8, 5 * 32 + 16);
        assert_eq!(tap_snap(&b, p, 64), at((8, 5)));
        // The first of two equally near wins.
        let b = board(true, vec![unit(1, FOOTMAN, 0, (2, 5)), unit(2, FOOTMAN, 0, (4, 5))]);
        assert_eq!(tap_snap(&b, (3 * 32 + 16, 5 * 32 + 16), 64), at((2, 5)));
    }

    #[test]
    fn a_smart_tap_orders_with_your_units_selected_and_selects_your_own() {
        let b = board(
            true,
            vec![
                unit(1, FOOTMAN, 0, (5, 5)),
                unit(2, GRUNT, 1, (9, 5)),
                unit(3, FARM, 0, (2, 2)),
            ],
        );
        let ground = at((5, 9));
        // Nothing of yours selected: a tap is a left click everywhere.
        assert!(!smart_tap(&b, ground, 24, &[]));
        // An enemy selected (to look at it) does not take orders either; nor does your farm.
        assert!(!smart_tap(&b, ground, 24, &ids([2])));
        assert!(!smart_tap(&b, ground, 24, &ids([3])));
        // Your footman selected: ground and the enemy take the order.
        assert!(smart_tap(&b, ground, 24, &ids([1])));
        assert!(smart_tap(&b, at((9, 5)), 24, &ids([1])));
        // A tap on, or within the tolerance of, your own unit or building still selects.
        let beside = (at((5, 5)).0 + 15 + 20, at((5, 5)).1);
        assert!(!smart_tap(&b, at((5, 5)), 24, &ids([1])));
        assert!(!smart_tap(&b, beside, 24, &ids([1])));
        assert!(smart_tap(&b, beside, 18, &ids([1])));
        assert!(!smart_tap(&b, at((2, 2)), 24, &ids([1])));
    }

    #[test]
    fn a_snapped_tap_selects_through_the_ordinary_rules() {
        let b = board(true, vec![unit(1, FOOTMAN, 0, (5, 5))]);
        let c = at((5, 5));
        let p = tap_snap(&b, (c.0 + 30, c.1 + 2), 24);
        let m = Mods::default();
        assert_eq!(click(&b, p, m, &[]), set(ids([1]), true));
        // Without the tolerance the same tap selects nothing.
        assert_eq!(click(&b, (c.0 + 30, c.1 + 2), m, &[]), Pick::Keep);
    }
}
