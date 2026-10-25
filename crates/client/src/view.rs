//! Drawing the map view (spec `docs/specs/ui/map-drawing.md`). No SDL types here.
//!
//! The view is drawn in palette indices into a [`Canvas`] that covers the part of the map on screen, in map pixels.
//! The terrain tiles are a separate picture underneath, so the canvas starts out transparent and holds the layers
//! above them, in the spec's order ("Layers"):
//!
//! 1. remembered buildings ("ghosts"), then buildings in draw-list order, each with its remembered look when hidden;
//! 2. the ground and sea pass over the draw list, then the air pass: shadow, box, sprite and charms of each unit, box
//!    and fire of each building;
//! 3. effects from slot 3 up, each left out when the local player sees none of the 3 × 3 tiles around it;
//! 4. the fog (`crate::fog`);
//! 5. the ground marker and the selection rectangle being dragged.
//!
//! The message lines are part of the interface picture, which is drawn over the whole view, so they cover the marker
//! and the rectangle instead of lying under them as in the spec.
//!
//! [`frame`] gathers what one picture needs from the [`World`] into a [`ViewFrame`] of plain values, and [`draw`]
//! draws a `ViewFrame`; the rules in between are small functions that the tests feed with synthetic values.
//!
//! Sprite files: unit sheets as in `tidewater_formats::sprites`; `Art\Unit\Other\shadow.grp`, `charms.grp` (stored
//! unpacked), the effect files by effect kind from the spec's "Data" table, and the carrying files of workers and
//! tankers (`carry`, spec `ui/carried-goods.md`), which replace a carrying unit's sheet.
//!
//! Defaults and placeholders where the specs stop:
//! - **Bob offset:** b is the unit's bob value (0 or 1), flipped by the sim at each bob draw of a ship or flier.
//! - **Effect frames:** the sim has no animation state for effects yet, so an effect draws frame number 0 in the
//!   facing column of its flight direction (from its start to its aim point; north for one that doesn't move).
//! - **Building fires and the ground marker** are drawn by the client: the fires' shared animation steps one frame
//!   every [`FIRE_WAIT`] cycles, and the marker ([`Marker`]) grows, holds and shrinks its X by the spec's frame
//!   table ("Overlays").
//! - **Building states in the background:** a building that is dead, removed or inside something isn't drawn; a dying
//!   one (a ruin) is (the spec's default). A remembered look is set for every building, in the view or not.

use std::collections::HashMap;

use tidewater_formats::grp::Grp;
use tidewater_formats::tileset::Palette;
use tidewater_sim::vision::{Map, bit};
use tidewater_sim::{Layer, Unit, UnitId, World};

use crate::carry::{Carried, CarryFile};
use crate::extras::HealthBars;
use crate::fog;
use crate::overlay;

/// A canvas pixel that nothing has drawn: the terrain shows through.
pub const CLEAR: u16 = 256;
/// The first index of the team-colour ramp in the sprite files.
pub const RAMP: u8 = 208;
/// Colours: the selection box of the local player's units, allied units and others ("Boxes").
pub const OWN: u8 = 250;
pub const ALLIED: u8 = 251;
pub const OTHER: u8 = 249;
/// The box of the local player's buildings while it places a building.
pub const PLACING: u8 = 246;
/// The selection rectangle being dragged.
pub const DRAG: u8 = 250;
/// The placement image's checkerboard: fine and bad tiles.
pub const GOOD_TILE: u8 = 150;
pub const BAD_TILE: u8 = 10;
/// The shadow under air units: its frame and how far below the unit's centre it lies.
pub const SHADOW_FRAME: usize = 2;
pub const SHADOW_DROP: i32 = 42;
/// The area at a charm's corner that must lie inside the view.
pub const CHARM_ROOM: i32 = 20;
/// Effect kinds of the two building fires and the ground marker, and the slots they live in.
pub const LIGHT_FIRE: u8 = 0x13;
pub const HEAVY_FIRE: u8 = 0x14;
pub const MARKER: u8 = 0x1c;
pub const PERMANENT_EFFECTS: usize = 3;
/// Placeholder: cycles per frame of the building fires' shared animation.
pub const FIRE_WAIT: u32 = 4;

/// The effect sprite files by effect kind, all under `Art\Unit\Other\` ("Data", "Effect sprites by effect kind").
pub const EFFECT_FILES: [&str; 29] = [
    "lightng", "hammer", "fireball", "fireball", "ltfire", "blizzard", "rot", "hbattle", "exorcism", "heal", "dkatak",
    "rune", "phoon", "stone", "bolt", "arrow", "axe", "torpedo", "torpedo", "ltfire", "hvfire", "cathit", "sparkle",
    "boom", "cannon", "canfire", "canboom", "fire", "blackx",
];

/// The archive name of an effect kind's sprite file.
pub fn effect_file(kind: u8) -> Option<String> {
    EFFECT_FILES
        .get(kind as usize)
        .map(|f| format!("Art\\Unit\\Other\\{f}.grp"))
}

/// The base of a colour player's team colours ("Colour"): source indices 208 to 211 are drawn as base to base + 3.
pub fn team_base(colour: u8) -> u8 {
    match colour {
        0..=6 => RAMP + 4 * colour,
        7 | 15 => 12,
        _ => RAMP,
    }
}

/// A sprite index as drawn with team-colour base `base`.
pub fn remap(i: u8, base: u8) -> u8 {
    if (RAMP..RAMP + 4).contains(&i) {
        base + (i - RAMP)
    } else {
        i
    }
}

/// The frame drawn for a facing and frame number ("Frame and mirroring"): the facing's column plus the frame number,
/// mirrored for facings 5 to 7. Buildings and ruins draw their frame number as it is. `None` at or past the sheet's
/// frame count: nothing is drawn.
pub fn sheet_frame(building: bool, facing: u8, frame: u8, frames: usize) -> Option<(usize, bool)> {
    let (col, mirrored) = if building {
        (0, false)
    } else {
        crate::scene::facing_frame(facing)
    };
    let f = col + frame as usize;
    (f < frames).then_some((f, mirrored))
}

/// The animation frame a unit is drawn with. The sim keeps the construction site's look in `script.frame` (frame 0
/// below 64/255, frame 1 from there, `economy/build-train.md`, "The construction pass", step 6), but that numbers the
/// *site picture*'s frames, and the spec doesn't say which sprite file that picture is. Until it does, a building
/// under construction draws frame 1 of its own sheet (the under-construction frame of building sheets, see
/// `tidewater_formats::sprites`) for the whole construction, so the finished frame 0 is never shown first.
pub fn drawn_frame(u: &Unit) -> u8 {
    frame_with_construction(u.building, u.construction.is_some(), u.script.frame)
}

/// `drawn_frame` on its parts: a building under construction always draws frame 1, whatever the sim's look is.
fn frame_with_construction(building: bool, under_construction: bool, frame: u8) -> u8 {
    if building && under_construction { 1 } else { frame }
}

/// Where a frame's top-left corner is drawn ("Placement"), for a sprite file of `size` W × H and a frame at offset
/// (fx, fy) of fw × fh, around centre point `centre`, raised by `lift`.
pub fn frame_origin(
    size: (i32, i32),
    offset: (i32, i32),
    fsize: (i32, i32),
    centre: (i32, i32),
    lift: i32,
    mirrored: bool,
) -> (i32, i32) {
    let x = if mirrored {
        centre.0 + size.0 / 2 - fsize.0 - offset.0
    } else {
        centre.0 - size.0 / 2 + offset.0
    };
    (x, centre.1 - lift - size.1 / 2 + offset.1)
}

/// The pixels drawn in map pixels.
#[derive(Clone, Debug)]
pub struct Canvas {
    /// The map pixel of the top-left corner.
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
    /// Palette indices row by row, [`CLEAR`] where nothing was drawn.
    pub px: Vec<u16>,
}

impl Canvas {
    pub fn new(left: i32, top: i32, width: i32, height: i32) -> Self {
        let (width, height) = (width.max(0), height.max(0));
        Canvas {
            left,
            top,
            width,
            height,
            px: vec![CLEAR; (width * height) as usize],
        }
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.left && y >= self.top && x < self.left + self.width && y < self.top + self.height
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        self.contains(x, y)
            .then(|| ((y - self.top) * self.width + (x - self.left)) as usize)
    }

    #[cfg(test)]
    /// The pixel at map pixel (x, y): `None` outside, [`CLEAR`] if nothing was drawn there.
    pub fn at(&self, x: i32, y: i32) -> Option<u16> {
        self.index(x, y).map(|i| self.px[i])
    }

    pub fn put(&mut self, x: i32, y: i32, c: u8) {
        if let Some(i) = self.index(x, y) {
            self.px[i] = c as u16;
        }
    }

    /// Draws `frame` of `grp` placed as in "Placement", with the team-colour base `base`. Index 0 is transparent.
    pub fn sprite(&mut self, grp: &Grp, frame: usize, centre: (i32, i32), lift: i32, mirrored: bool, base: u8) {
        let Some(f) = grp.frames.get(frame) else { return };
        let (fw, fh) = (f.width as i32, f.height as i32);
        let (x0, y0) = frame_origin(
            (grp.width as i32, grp.height as i32),
            (f.x as i32, f.y as i32),
            (fw, fh),
            centre,
            lift,
            mirrored,
        );
        self.pixels(&f.pixels, fw, fh, (x0, y0), mirrored, base);
    }

    /// Copies a fw × fh block of indices with its top-left at `at`, flipped left to right if `mirrored`.
    fn pixels(&mut self, src: &[u8], fw: i32, fh: i32, at: (i32, i32), mirrored: bool, base: u8) {
        for j in 0..fh {
            let y = at.1 + j;
            if y < self.top || y >= self.top + self.height {
                continue;
            }
            for i in 0..fw {
                let v = src[(j * fw + i) as usize];
                if v == 0 {
                    continue;
                }
                let x = at.0 + if mirrored { fw - 1 - i } else { i };
                self.put(x, y, remap(v, base));
            }
        }
    }

    /// A box outline from `a` to `b`, both corners included ("Boxes"): each edge is drawn only if its line lies inside
    /// the view, clipped to the view.
    pub fn outline(&mut self, a: (i32, i32), b: (i32, i32), c: u8) {
        let (l, r) = (a.0.min(b.0), a.0.max(b.0));
        let (t, bt) = (a.1.min(b.1), a.1.max(b.1));
        let (left, top, width, height) = (self.left, self.top, self.width, self.height);
        let in_x = |x: i32| x >= left && x < left + width;
        let in_y = |y: i32| y >= top && y < top + height;
        for y in [t, bt] {
            if in_y(y) {
                for x in l..=r {
                    self.put(x, y, c);
                }
            }
        }
        for x in [l, r] {
            if in_x(x) {
                for y in t..=bt {
                    self.put(x, y, c);
                }
            }
        }
    }

    /// The canvas as RGBA8, transparent where nothing was drawn.
    pub fn rgba(&self, pal: &Palette) -> Vec<u8> {
        let mut out = vec![0u8; self.px.len() * 4];
        for (o, &p) in out.chunks_exact_mut(4).zip(&self.px) {
            if p != CLEAR {
                let c = pal.0[p as usize & 0xff];
                o.copy_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        out
    }
}

/// A map pixel.
pub type Px = (i32, i32);

/// Which sprite file a drawing step uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sheet {
    /// A unit type's sheet.
    Unit(u8),
    /// An effect kind's file.
    Effect(u8),
    Shadow,
    /// A worker's or tanker's carrying file (`carry`).
    Carry(CarryFile),
}

/// The sprite files the view draws from.
#[derive(Default)]
pub struct Art {
    pub units: HashMap<u8, Grp>,
    pub effects: HashMap<u8, Grp>,
    pub shadow: Option<Grp>,
    pub charms: Option<Grp>,
    pub carrying: HashMap<CarryFile, Grp>,
}

impl Art {
    /// The unit sheets plus the effect, shadow and charm files from the install; a file it doesn't have is left out
    /// (nothing is drawn for it).
    pub fn load(install: &mut tidewater_formats::install::Install, units: HashMap<u8, Grp>) -> Self {
        let mut read = |name: &str, packed: bool| {
            let d = install.read(name).ok()?;
            if packed {
                Grp::parse(&d).ok()
            } else {
                Grp::parse_uncompressed(&d).ok()
            }
        };
        let effects = (0..EFFECT_FILES.len() as u8)
            .filter_map(|k| Some((k, read(&effect_file(k)?, true)?)))
            .collect();
        let carrying = CarryFile::ALL
            .iter()
            .filter_map(|&f| Some((f, read(&f.path(), true)?)))
            .collect();
        Art {
            units,
            effects,
            shadow: read("Art\\Unit\\Other\\shadow.grp", true),
            charms: read("Art\\Unit\\Other\\charms.grp", false),
            carrying,
        }
    }

    pub fn get(&self, s: Sheet) -> Option<&Grp> {
        match s {
            Sheet::Unit(k) => self.units.get(&k),
            Sheet::Effect(k) => self.effects.get(&k),
            Sheet::Shadow => self.shadow.as_ref(),
            Sheet::Carry(f) => self.carrying.get(&f),
        }
    }

    /// The number of frames in a sheet (0 when the install doesn't have it).
    pub fn frames(&self, s: Sheet) -> usize {
        self.get(s).map_or(0, |g| g.frames.len())
    }
}

/// One drawing step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// A sprite frame around a centre point.
    Sprite {
        sheet: Sheet,
        frame: usize,
        mirrored: bool,
        centre: (i32, i32),
        lift: i32,
        base: u8,
    },
    /// A box outline, both corners included.
    Outline { a: (i32, i32), b: (i32, i32), colour: u8 },
    /// A charm icon with its top-left at `at`.
    Charm { frame: usize, at: (i32, i32) },
}

/// A building fire ("Building fires").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fire {
    Light,
    Heavy,
}

impl Fire {
    pub fn kind(self) -> u8 {
        match self {
            Fire::Light => LIGHT_FIRE,
            Fire::Heavy => HEAVY_FIRE,
        }
    }

    /// Frames in the fire's file.
    pub fn frames(self) -> usize {
        match self {
            Fire::Light => 6,
            Fire::Heavy => 10,
        }
    }
}

/// The fire a complete building with `h` of `max` hit points shows.
pub fn fire(h: u16, max: u16) -> Option<Fire> {
    let (h, m) = (h as u32, max as u32);
    if h < m / 2 {
        Some(Fire::Heavy)
    } else if h < 3 * m.max(1) / 4 {
        Some(Fire::Light)
    } else {
        None
    }
}

/// How far a fire is moved up from the footprint centre, by the building's height in tiles.
pub fn fire_lift(height: u8) -> i32 {
    match height {
        2 => 24,
        3 => 20,
        4 => 26,
        _ => 0,
    }
}

/// A building's fire frame: the fire's own frame plus the building's phase, wrapped.
pub fn fire_frame(fire: Fire, own: usize, phase: u8) -> usize {
    (own + phase as usize) % fire.frames()
}

/// The fires' own frames in game cycle `tick` (placeholder animation, see the module docs).
pub fn fire_frames(tick: u32) -> (usize, usize) {
    let n = (tick / FIRE_WAIT) as usize;
    (n % Fire::Light.frames(), n % Fire::Heavy.frames())
}

/// What decides a unit's box.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BoxCase {
    /// The flash countdown.
    pub flash: u8,
    /// Dead, dying, removed or inside something.
    pub gone: bool,
    pub detected: bool,
    pub selected: bool,
    /// The owner is the local player, or allied with it.
    pub own: bool,
    pub allied: bool,
    /// A building of the local player's, while the local player is placing a building.
    pub placing: bool,
}

/// The box colour, first matching row of "Boxes", and whether the flash stops at once.
pub fn box_colour(c: BoxCase) -> (Option<u8>, bool) {
    let colour = if c.own {
        OWN
    } else if c.allied {
        ALLIED
    } else {
        OTHER
    };
    if c.flash != 0 {
        if c.gone || !c.detected {
            return (None, true);
        }
        return ((c.flash & 8 != 0).then_some(colour), false);
    }
    if c.selected {
        (Some(colour), false)
    } else if c.placing && !c.gone {
        (Some(PLACING), false)
    } else {
        (None, false)
    }
}

/// The box outline's corners for selection-box left/top (L, T) and box size (bw, bh).
pub fn box_outline(lt: (i32, i32), size: (i32, i32)) -> ((i32, i32), (i32, i32)) {
    ((lt.0 - 1, lt.1 - 1), (lt.0 - 1 + size.0, lt.1 - 1 + size.1))
}

/// Spell timers on a unit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Spells {
    pub bloodlust: bool,
    /// Haste (positive) or slow (negative).
    pub haste: i16,
    pub invisibility: bool,
    pub unholy_armour: bool,
}

/// The charm icons of a unit as (frame, x offset), in drawing order ("Charms").
pub fn charms(s: Spells) -> Vec<(usize, i32)> {
    let mut v = Vec::new();
    if s.bloodlust {
        v.push((0, 0));
    }
    if s.haste > 0 {
        v.push((1, 16));
    } else if s.haste < 0 {
        v.push((2, 16));
    }
    if s.invisibility {
        v.push((3, 32));
    }
    if s.unholy_armour {
        v.push((4, 48));
    }
    v
}

/// Whether a charm at `at` is drawn: the 20 × 20 area at its corner must lie wholly inside the view.
pub fn charm_fits(view: &Canvas, at: (i32, i32)) -> bool {
    view.contains(at.0, at.1) && view.contains(at.0 + CHARM_ROOM - 1, at.1 + CHARM_ROOM - 1)
}

/// One unit or building as the unit passes draw it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub building: bool,
    pub air: bool,
    /// The sprite (units only).
    pub sprite: Option<Op>,
    /// The shadow (air units only).
    pub shadow: Option<Op>,
    /// The box outline's corners and colour.
    pub outline: Option<(Px, Px, u8)>,
    /// Charms as (frame, top-left).
    pub charms: Vec<(usize, (i32, i32))>,
    /// A building's fire.
    pub fire: Option<Op>,
}

/// The steps that draw one entry, in order: shadow, then a unit's box, sprite and charms, or a building's box and
/// fire. Charms that don't fit in `view` are left out.
pub fn entry_ops(e: &Entry, view: &Canvas) -> Vec<Op> {
    let mut ops = Vec::new();
    ops.extend(e.shadow);
    if let Some((a, b, colour)) = e.outline {
        ops.push(Op::Outline { a, b, colour });
    }
    if e.building {
        ops.extend(e.fire);
        return ops;
    }
    ops.extend(e.sprite);
    for &(frame, at) in &e.charms {
        if charm_fits(view, at) {
            ops.push(Op::Charm { frame, at });
        }
    }
    ops
}

/// The order of the unit passes over the draw list: entries that aren't skipped, first everything that isn't an air
/// unit, then the air units, each pass in list order. Input: (index, air, drawn).
pub fn pass_order(list: &[(usize, bool, bool)]) -> Vec<usize> {
    let pass = |air: bool| list.iter().filter(move |e| e.2 && e.1 == air).map(|e| e.0);
    pass(false).chain(pass(true)).collect()
}

/// Whether an effect is drawn: while its slot is showing, and, unless the reveal-map cheat is on, only when the local
/// player sees one of the tiles around it (`seen`, see [`effect_seen`]).
pub fn effect_drawn(showing: bool, reveal: bool, seen: impl FnOnce() -> bool) -> bool {
    showing && (reveal || seen())
}

/// Whether the 3 × 3 tiles around map pixel `pos` hold one the local player sees ("Effects"); parts off the map
/// are ignored. `unseen(x, y)` reads the local player's "seen" bit.
pub fn effect_seen(pos: (i32, i32), map: (i32, i32), unseen: impl Fn(i32, i32) -> bool) -> bool {
    let (tx, ty) = (pos.0.div_euclid(32), pos.1.div_euclid(32));
    (-1..=1).any(|dy| {
        (-1..=1).any(|dx| {
            let (x, y) = (tx + dx, ty + dy);
            x >= 0 && y >= 0 && x < map.0 && y < map.1 && !unseen(x, y)
        })
    })
}

/// The facing of a flight direction, 0 north and clockwise in eighths (north when there is no direction).
pub fn direction_facing(d: (i32, i32)) -> u8 {
    if d == (0, 0) {
        return 0;
    }
    let a = (d.0 as f64).atan2(-d.1 as f64).rem_euclid(std::f64::consts::TAU);
    ((a / std::f64::consts::FRAC_PI_4).round() as i64).rem_euclid(8) as u8
}

/// The ground marker (effect slot 2, "Overlays"): made at a map pixel by the local player's own click, then counted
/// in game cycles. With its first update in cycle U it is drawn after the updates of U + 1 to U + 13, growing the X of
/// `blackx.grp` (frames 0 to 3), holding it and shrinking it again ([`Marker::FRAMES`]). There is one marker: a new
/// click replaces it, so its sequence starts again at the new spot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Marker {
    pub at: (i32, i32),
    /// Updates since it was made.
    pub updates: u32,
}

impl Marker {
    /// Updates the marker is drawn after: the second to the fourteenth.
    pub const SHOWN: std::ops::RangeInclusive<u32> = 2..=14;
    /// The frame drawn after each of those updates (U + 1 to U + 13).
    pub const FRAMES: [u8; 13] = [0, 0, 1, 2, 3, 3, 3, 3, 3, 3, 2, 1, 0];

    /// The frame of `blackx.grp` drawn now, `None` while it isn't drawn. It has no facing: column 0, never mirrored.
    pub fn frame(&self) -> Option<u8> {
        self.showing()
            .then(|| Self::FRAMES[(self.updates - Self::SHOWN.start()) as usize])
    }

    pub fn new(at: (i32, i32)) -> Self {
        Marker { at, updates: 0 }
    }

    /// One game cycle's update; `false` once the marker has gone back to the new-effect state.
    pub fn update(&mut self) -> bool {
        self.updates += 1;
        self.updates <= *Self::SHOWN.end()
    }

    pub fn showing(&self) -> bool {
        Self::SHOWN.contains(&self.updates)
    }
}

/// The sheet a unit is drawn from ("Terms", picture): its carrying file while it has one (`carry`) and the install has
/// it, otherwise the picture's own sheet.
fn unit_sheet(art: &Art, carrying: Option<CarryFile>, picture: u8) -> Sheet {
    match carrying {
        Some(f) if art.carrying.contains_key(&f) => Sheet::Carry(f),
        _ => Sheet::Unit(picture),
    }
}

/// The marker's drawing step: its frame of `blackx.grp` in column 0, not mirrored, in the tileset palette with no team
/// colour; `None` while it isn't drawn.
fn marker_op(art: &Art, mk: Marker) -> Option<Op> {
    let sheet = Sheet::Effect(MARKER);
    let (frame, mirrored) = sheet_frame(true, 0, mk.frame()?, art.frames(sheet))?;
    Some(Op::Sprite {
        sheet,
        frame,
        mirrored,
        centre: mk.at,
        lift: 0,
        base: RAMP,
    })
}

/// One health bar over a unit (`overlay`): its top-left map pixel, its width, the filled pixels and their colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bar {
    pub at: (i32, i32),
    pub width: i32,
    pub fill: i32,
    pub colour: u8,
}

/// A rally-point flag (option `rally-points`) of a selected training building: the map pixel of the foot of its pole
/// and the pennant's colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RallyFlag {
    pub at: (i32, i32),
    pub colour: u8,
}

/// A queued-order waypoint (option `shift-queue`) of a selected unit: the map pixel it marks, the pixel of the unit's
/// previous waypoint (or the unit itself) a dotted line comes from, and the marker's colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Waypoint {
    pub at: (i32, i32),
    pub from: (i32, i32),
    pub colour: u8,
}

/// Draws a waypoint: a dotted line from the previous point and a five-pixel square marker, hollow in the drag colour
/// around a pixel of the unit's colour.
pub fn draw_waypoint(canvas: &mut Canvas, w: &Waypoint) {
    let (dx, dy) = (w.at.0 - w.from.0, w.at.1 - w.from.1);
    let steps = dx.abs().max(dy.abs());
    for k in (0..steps).step_by(4) {
        canvas.put(w.from.0 + dx * k / steps, w.from.1 + dy * k / steps, DRAG);
    }
    for d in -2..=2 {
        for (x, y) in [(d, -2), (d, 2), (-2, d), (2, d)] {
            canvas.put(w.at.0 + x, w.at.1 + y, DRAG);
        }
    }
    canvas.put(w.at.0, w.at.1, w.colour);
}

/// The pole's height and the pennant's size, in pixels.
pub const FLAG_POLE: i32 = 14;
pub const FLAG_PENNANT: i32 = 6;

/// Draws a rally flag: a one-pixel pole in the drag colour and a triangular pennant at its top.
pub fn draw_flag(canvas: &mut Canvas, f: &RallyFlag) {
    for y in 0..=FLAG_POLE {
        canvas.put(f.at.0, f.at.1 - y, DRAG);
    }
    for row in 0..FLAG_PENNANT {
        for x in 1..=(FLAG_PENNANT + 1 - row) {
            canvas.put(f.at.0 + x, f.at.1 - FLAG_POLE + row, f.colour);
        }
    }
}

/// Draws a health bar: the filled part, then the empty part, [`overlay::BAR_HEIGHT`] rows high.
pub fn draw_bar(canvas: &mut Canvas, b: &Bar) {
    for row in 0..overlay::BAR_HEIGHT {
        for x in 0..b.width {
            canvas.put(
                b.at.0 + x,
                b.at.1 + row,
                if x < b.fill { b.colour } else { overlay::EMPTY },
            );
        }
    }
}

/// Everything one picture of the view needs, as plain values.
#[derive(Clone, Debug, Default)]
pub struct ViewFrame {
    /// Layer 2: remembered buildings, then buildings.
    pub background: Vec<Op>,
    /// Layers 3 and 4, in pass order.
    pub entries: Vec<Entry>,
    /// Layer 5.
    pub effects: Vec<Op>,
    /// The optional health bars (`overlay`), over the units and effects; empty unless the option is on.
    pub bars: Vec<Bar>,
    /// The rally flags of the selected training buildings, over the fog; empty unless the option is on.
    pub flags: Vec<RallyFlag>,
    /// The queued-order waypoints of the selected units, over the fog; empty unless the option is on.
    pub waypoints: Vec<Waypoint>,
    /// Layer 6: per map tile the visible and explored codes (row by row, `fog.0` tiles wide); `None` with the
    /// reveal-map cheat.
    pub fog: Option<(usize, Vec<(u8, u8)>)>,
    /// Layer 8.
    pub marker: Option<Op>,
    /// Layer 9: the press point and the pointer.
    pub drag: Option<((i32, i32), (i32, i32))>,
}

/// Draws a frame of the view in layer order.
pub fn draw(canvas: &mut Canvas, f: &ViewFrame, art: &Art, shapes: &fog::Shapes) {
    let run = |canvas: &mut Canvas, op: &Op| match *op {
        Op::Sprite {
            sheet,
            frame,
            mirrored,
            centre,
            lift,
            base,
        } => {
            if let Some(g) = art.get(sheet) {
                canvas.sprite(g, frame, centre, lift, mirrored, base);
            }
        }
        Op::Outline { a, b, colour } => canvas.outline(a, b, colour),
        Op::Charm { frame, at } => {
            if let Some(f) = art.charms.as_ref().and_then(|g| g.frames.get(frame)) {
                canvas.pixels(&f.pixels, f.width as i32, f.height as i32, at, false, RAMP);
            }
        }
    };
    for op in &f.background {
        run(canvas, op);
    }
    for e in &f.entries {
        for op in entry_ops(e, canvas) {
            run(canvas, &op);
        }
    }
    for op in &f.effects {
        run(canvas, op);
    }
    for b in &f.bars {
        draw_bar(canvas, b);
    }
    if let Some((width, codes)) = &f.fog {
        fog::draw(canvas, *width, codes, shapes);
    }
    for flag in &f.flags {
        draw_flag(canvas, flag);
    }
    for w in &f.waypoints {
        draw_waypoint(canvas, w);
    }
    if let Some(op) = &f.marker {
        run(canvas, op);
    }
    if let Some((a, b)) = f.drag {
        let (l, r) = (a.0.min(b.0), a.0.max(b.0));
        let (t, bt) = (a.1.min(b.1), a.1.max(b.1));
        for x in l..=r {
            canvas.put(x, t, DRAG);
            canvas.put(x, bt, DRAG);
        }
        for y in t..=bt {
            canvas.put(l, y, DRAG);
            canvas.put(r, y, DRAG);
        }
    }
}

/// The placement image ("Placement image"): a footprint-sized picture of the building's frame 0 in team-colour base
/// `base`, with the pixels where (x + y) is odd set to [`GOOD_TILE`] or [`BAD_TILE`] by tile. `bad` lists the
/// footprint's tiles row by row.
pub fn placement_image(grp: Option<&Grp>, base: u8, size: (u8, u8), bad: &[bool]) -> Canvas {
    let (w, h) = (size.0 as i32 * 32, size.1 as i32 * 32);
    let mut c = Canvas::new(0, 0, w, h);
    if let Some(g) = grp {
        c.sprite(g, 0, (w / 2, h / 2), 0, false, base);
    }
    for y in 0..h {
        for x in 0..w {
            if (x + y) % 2 == 1 {
                let t = (y / 32 * size.0 as i32 + x / 32) as usize;
                let colour = if bad.get(t).copied().unwrap_or(true) {
                    BAD_TILE
                } else {
                    GOOD_TILE
                };
                c.put(x, y, colour);
            }
        }
    }
    c
}

// --- from the world --------------------------------------------------------------------------------------------------

/// What the client adds to the world for one picture.
pub struct Local<'a> {
    pub selected: &'a [UnitId],
    pub placing: bool,
    pub marker: Option<Marker>,
    pub drag: Option<((i32, i32), (i32, i32))>,
    /// The carrying files the workers and tankers are drawn from (`carry`).
    pub carried: &'a Carried,
    /// The optional health bars (`overlay`): which units show one, and the unit under the pointer.
    pub bars: HealthBars,
    pub hovered: Option<UnitId>,
}

/// Dead (slot released) or removed: not drawn at all.
fn out_of_play(u: &Unit) -> bool {
    u.removing || u.script.anim == tidewater_sim::RELEASED
}

/// Detected by the local player; in an Enhanced game with a local player of 8 or more, by anyone.
fn detected(world: &World, u: &Unit) -> bool {
    let me = world.vision.local;
    if me >= 8 {
        return !world.rules.enhanced || u.detected_by != 0;
    }
    u.detected_by & bit(me) != 0
}

fn hidden(world: &World, u: &Unit) -> bool {
    !world.reveal_view() && u.hidden_from & bit(world.vision.local) != 0
}

pub fn base_of(world: &World, colour: u8) -> u8 {
    team_base(world.players.get(colour as usize).map_or(colour, |p| p.colour))
}

fn allied(world: &World, owner: u8) -> bool {
    let me = world.vision.local;
    world
        .players
        .get(me as usize)
        .is_some_and(|p| owner < 16 && p.allies & (1 << owner) != 0)
}

fn centre(u: &Unit) -> (i32, i32) {
    u.centre_px()
}

/// The picture of a building or ghost look, drawn into the background.
fn look_op(art: &Art, look: tidewater_sim::minimap::Picture, centre: (i32, i32), base: u8) -> Option<Op> {
    let sheet = Sheet::Unit(look.kind);
    let (frame, mirrored) = sheet_frame(true, look.facing, look.frame, art.frames(sheet))?;
    Some(Op::Sprite {
        sheet,
        frame,
        mirrored,
        centre,
        lift: 0,
        base,
    })
}

/// Gathers one picture of the view from the world. Returns it with the units whose flash stops at once.
pub fn frame(world: &World, art: &Art, view: &Canvas, local: &Local) -> (ViewFrame, Vec<UnitId>) {
    let me = world.vision.local;
    let mut f = ViewFrame::default();
    let mut stop = Vec::new();
    // Layer 2: ghosts, then buildings.
    // An observer sees the buildings themselves, so it keeps no remembered looks.
    for g in world.remembered.copies().filter(|_| !world.observing) {
        let c = (
            g.tile.0 * 32 + 16 * g.size.0 as i32,
            g.tile.1 * 32 + 16 * g.size.1 as i32,
        );
        f.background.extend(look_op(art, g.look, c, base_of(world, g.colour)));
    }
    let list: Vec<&Unit> = world.draw_list().iter().filter_map(|&id| world.unit(id)).collect();
    for u in list.iter().filter(|u| u.building && !out_of_play(u) && !u.inside) {
        let look = if hidden(world, u) {
            u.look.0
        } else {
            Some(tidewater_sim::minimap::Picture {
                kind: u.picture(),
                frame: drawn_frame(u),
                facing: u.facing,
            })
        };
        if let Some(look) = look {
            f.background
                .extend(look_op(art, look, centre(u), base_of(world, u.colour)));
        }
    }
    // Layers 3 and 4.
    let drawn: Vec<(usize, bool, bool)> = list
        .iter()
        .enumerate()
        .map(|(n, u)| {
            let skip = out_of_play(u) || u.inside || hidden(world, u) || !detected(world, u);
            (n, u.layer == Layer::Air, !skip)
        })
        .collect();
    let (light, heavy) = fire_frames(world.tick);
    for n in pass_order(&drawn) {
        let u = list[n];
        let s = world.stats.0.get(u.kind as usize).copied().unwrap_or_default();
        let base = base_of(world, u.colour);
        let c = centre(u);
        let bsize = (s.box_size.0 as i32, s.box_size.1 as i32);
        let lt = (c.0 - bsize.0 / 2, c.1 - bsize.1 / 2);
        let gone = u.dying || out_of_play(u) || u.inside;
        let (colour, stops) = box_colour(BoxCase {
            flash: u.flash.0,
            gone,
            detected: detected(world, u),
            selected: local.selected.contains(&u.id),
            own: u.owner == me,
            allied: u.owner != me && allied(world, u.owner),
            placing: local.placing && u.building && u.owner == me,
        });
        if stops {
            stop.push(u.id);
        }
        let outline = colour.map(|col| {
            let (a, b) = box_outline(lt, bsize);
            (a, b, col)
        });
        let air = u.layer == Layer::Air;
        let sheet = unit_sheet(art, local.carried.file(world, u), u.picture());
        let bob = u.bob.value as i32;
        let sprite =
            sheet_frame(u.building, u.facing, drawn_frame(u), art.frames(sheet)).map(|(frame, mirrored)| Op::Sprite {
                sheet,
                frame,
                mirrored,
                centre: c,
                lift: bob,
                base,
            });
        let shadow = air.then_some(Op::Sprite {
            sheet: Sheet::Shadow,
            frame: SHADOW_FRAME,
            mirrored: false,
            centre: c,
            lift: -(SHADOW_DROP + bob),
            base,
        });
        let corner = (lt.0 - 1, lt.1 - 1);
        let charms = if u.building {
            Vec::new()
        } else {
            charms(Spells {
                bloodlust: u.bloodlust > 0,
                haste: u.haste,
                invisibility: u.invisibility > 0,
                unholy_armour: u.unholy_armor > 0,
            })
            .into_iter()
            .map(|(frame, dx)| (frame, (corner.0 + dx, corner.1)))
            .collect()
        };
        let burning = u.building && !u.dying && u.construction.is_none();
        let fire = burning.then(|| fire(u.hp, s.hit_points)).flatten().map(|fi| {
            let own = if fi == Fire::Light { light } else { heavy };
            Op::Sprite {
                sheet: Sheet::Effect(fi.kind()),
                frame: fire_frame(fi, own, u.variation),
                mirrored: false,
                centre: (c.0, c.1 - fire_lift(u.size.1)),
                lift: 0,
                base,
            }
        });
        if overlay::bar_shown(local.bars, local.selected.contains(&u.id), local.hovered == Some(u.id))
            && !gone
            && let Some((fill, colour)) = overlay::bar_fill(u.hp, s.hit_points, bsize.0)
        {
            f.bars.push(Bar {
                at: (lt.0, lt.1 - overlay::BAR_GAP - overlay::BAR_HEIGHT),
                width: bsize.0,
                fill,
                colour,
            });
        }
        f.entries.push(Entry {
            building: u.building,
            air,
            sprite,
            shadow,
            outline,
            charms,
            fire,
        });
    }
    // Layer 5: effects from slot 3 up.
    let reveal = world.reveal_view();
    let map = (world.vision.width as i32, world.vision.height as i32);
    for m in world.missiles.iter().skip(PERMANENT_EFFECTS).flatten() {
        if !effect_drawn(m.showing(), reveal, || {
            effect_seen(m.pos, map, |x, y| world.vision.unseen_by(me, x, y))
        }) {
            continue;
        }
        let sheet = Sheet::Effect(m.kind);
        let facing = direction_facing((m.to.0 - m.from.0, m.to.1 - m.from.1));
        if let Some((frame, mirrored)) = sheet_frame(false, facing, 0, art.frames(sheet)) {
            f.effects.push(Op::Sprite {
                sheet,
                frame,
                mirrored,
                centre: m.pos,
                lift: 0,
                base: RAMP,
            });
        }
    }
    // Layer 6: the fog over the tiles in view.
    if !reveal {
        let v = &world.vision;
        let codes = (0..v.height as i32)
            .flat_map(|y| (0..v.width as i32).map(move |x| (x, y)))
            .map(|(x, y)| (v.code(Map::Visible, x, y), v.code(Map::Explored, x, y)))
            .collect();
        f.fog = Some((v.width, codes));
    }
    // The rally flags of the selected buildings (option `rally-points`), at the tile or on the unit they were set on.
    if world.rules.extras.rally_points {
        for &id in local.selected {
            let Some(r) = world.rally_point(id) else { continue };
            let spot = r
                .target
                .and_then(|t| world.unit(t))
                .map_or((r.tile.0 * 32 + 16, r.tile.1 * 32 + 24), |t| {
                    let c = centre(t);
                    (c.0, c.1 + 8)
                });
            f.flags.push(RallyFlag {
                at: spot,
                colour: base_of(world, world.unit(id).map_or(0, |u| u.owner)),
            });
        }
    }
    // The queued orders of the selected units (option `shift-queue`): a marker at each waypoint, joined in order.
    if world.rules.extras.order_queue {
        for &id in local.selected {
            let Some(u) = world.unit(id) else { continue };
            let colour = base_of(world, u.owner);
            let mut from = centre(u);
            for tile in world.waypoints(id) {
                let at = (tile.0 * 32 + 16, tile.1 * 32 + 16);
                f.waypoints.push(Waypoint { at, from, colour });
                from = at;
            }
        }
    }
    // Layers 8 and 9.
    f.marker = local.marker.and_then(|mk| marker_op(art, mk));
    f.drag = local.drag;
    let _ = view;
    (f, stop)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tidewater_formats::grp::Frame;

    /// A sheet of `w` × `h` with frames that are solid blocks of `colour` at `offset`, `size`.
    /// A frame: offset, size and colour.
    type Block = ((u8, u8), (u8, u8), u8);

    fn sheet(w: u16, h: u16, frames: &[Block]) -> Grp {
        Grp {
            width: w,
            height: h,
            frames: frames
                .iter()
                .map(|&((x, y), (fw, fh), c)| Frame {
                    x,
                    y,
                    width: fw,
                    height: fh,
                    pixels: vec![c; fw as usize * fh as usize],
                })
                .collect(),
        }
    }

    #[test]
    fn a_building_under_construction_never_draws_the_finished_frame() {
        // Regression: the sim's construction look is 0 at placement, which indexed the finished frame 0 of the
        // building's own sheet, so a new site flashed its final form before the construction frames.
        for look in [0, 1] {
            assert_eq!(frame_with_construction(true, true, look), 1);
        }
        assert_eq!(
            frame_with_construction(true, false, 0),
            0,
            "a completed building keeps its frame"
        );
        assert_eq!(frame_with_construction(false, true, 3), 3, "units are untouched");
    }

    #[test]
    fn a_bar_draws_its_fill_then_the_empty_part_in_three_rows() {
        let mut c = Canvas::new(0, 0, 20, 10);
        draw_bar(
            &mut c,
            &Bar {
                at: (2, 4),
                width: 10,
                fill: 6,
                colour: 149,
            },
        );
        for row in 4..7 {
            assert_eq!(c.at(2, row), Some(149));
            assert_eq!(c.at(7, row), Some(149));
            assert_eq!(c.at(8, row), Some(overlay::EMPTY as u16));
            assert_eq!(c.at(11, row), Some(overlay::EMPTY as u16));
            assert_eq!(c.at(12, row), Some(CLEAR), "just past the width");
        }
        assert_eq!(c.at(2, 3), Some(CLEAR));
        assert_eq!(c.at(2, 7), Some(CLEAR));
    }

    #[test]
    fn a_rally_flag_is_a_pole_with_a_pennant() {
        let mut c = Canvas::new(0, 0, 30, 30);
        draw_flag(
            &mut c,
            &RallyFlag {
                at: (5, 25),
                colour: 77,
            },
        );
        assert_eq!(c.at(5, 25), Some(DRAG as u16));
        assert_eq!(c.at(5, 25 - FLAG_POLE), Some(DRAG as u16));
        assert_eq!(c.at(5, 25 - FLAG_POLE - 1), Some(CLEAR));
        assert_eq!(c.at(6, 25 - FLAG_POLE), Some(77));
        assert_eq!(c.at(6 + FLAG_PENNANT, 25 - FLAG_POLE), Some(77));
        assert_eq!(c.at(8, 25 - FLAG_POLE + FLAG_PENNANT), Some(CLEAR), "below the pennant");
    }

    #[test]
    fn a_waypoint_is_a_dotted_line_and_a_hollow_square() {
        let mut c = Canvas::new(0, 0, 40, 40);
        draw_waypoint(
            &mut c,
            &Waypoint {
                at: (20, 20),
                from: (4, 20),
                colour: 77,
            },
        );
        assert_eq!(c.at(20, 20), Some(77), "the unit's colour at the centre");
        assert_eq!(c.at(18, 18), Some(DRAG as u16), "a corner of the square");
        assert_eq!(c.at(19, 19), Some(CLEAR), "hollow");
        assert_eq!(c.at(4, 20), Some(DRAG as u16), "the line starts at the previous point");
        assert_eq!(c.at(5, 20), Some(CLEAR), "dotted: every fourth pixel");
        assert_eq!(c.at(8, 20), Some(DRAG as u16));
    }

    #[test]
    fn team_colour_bases() {
        let want = [
            208, 212, 216, 220, 224, 228, 232, 12, 208, 208, 208, 208, 208, 208, 208, 12,
        ];
        for (p, &b) in want.iter().enumerate() {
            assert_eq!(team_base(p as u8), b, "colour player {p}");
        }
        assert_eq!([208, 209, 211, 212, 0].map(|i| remap(i, 216)), [216, 217, 219, 212, 0]);
    }

    #[test]
    fn facing_six_frame_ten_is_frame_twelve_mirrored() {
        assert_eq!(sheet_frame(false, 6, 10, 100), Some((12, true)));
        assert_eq!(sheet_frame(false, 2, 0, 100), Some((2, false)));
        // A frame number at or past the frame count draws nothing.
        assert_eq!(sheet_frame(false, 4, 1, 5), None);
        // Buildings draw their frame as it is.
        assert_eq!(sheet_frame(true, 4, 1, 2), Some((1, false)));
    }

    #[test]
    fn sprite_placement_from_the_centre() {
        // Sheet 72 × 72, frame at (20, 18) of 30 × 36; unit at (320, 160), 1 × 1, bob 0: centre (336, 176).
        assert_eq!(
            frame_origin((72, 72), (20, 18), (30, 36), (336, 176), 0, false),
            (320, 158)
        );
        assert_eq!(
            frame_origin((72, 72), (20, 18), (30, 36), (336, 176), 0, true),
            (322, 158)
        );
        // Drawn: the frame's first column lands at x 320, mirrored its last at 322 + 29.
        let g = sheet(72, 72, &[((20, 18), (30, 36), 7)]);
        let mut c = Canvas::new(300, 140, 100, 100);
        c.sprite(&g, 0, (336, 176), 0, false, RAMP);
        assert_eq!(
            (c.at(319, 158), c.at(320, 158), c.at(349, 193)),
            (Some(CLEAR), Some(7), Some(7))
        );
        assert_eq!(c.at(320, 157), Some(CLEAR));
    }

    #[test]
    fn mirrored_sprites_flip_their_pixels() {
        let g = Grp {
            width: 4,
            height: 1,
            frames: vec![Frame {
                x: 0,
                y: 0,
                width: 2,
                height: 1,
                pixels: vec![1, 2],
            }],
        };
        let mut c = Canvas::new(0, 0, 8, 2);
        c.sprite(&g, 0, (4, 0), 0, true, RAMP);
        // Mirrored: x = 4 + 2 − 2 − 0 = 4, pixels reversed.
        assert_eq!((c.at(4, 0), c.at(5, 0)), (Some(2), Some(1)));
    }

    #[test]
    fn shadows_hang_below_air_units() {
        // Lift −(42 + b): the shadow's centre is 42 + b below the unit's centre.
        assert_eq!(
            frame_origin((24, 24), (0, 0), (23, 23), (100, 100), -42, false),
            (88, 130)
        );
    }

    #[test]
    fn gryphon_after_footman() {
        // The gryphon rider (air) is earlier in the list, but drawn in the second pass.
        assert_eq!(
            pass_order(&[(0, true, true), (1, false, true), (2, false, false)]),
            [1, 0]
        );
    }

    #[test]
    fn selected_own_footman_box() {
        // Box 31 × 31, selection box left/top (321, 161): outline (320, 160) to (351, 191), in colour 250.
        assert_eq!(box_outline((321, 161), (31, 31)), ((320, 160), (351, 191)));
        let own = BoxCase {
            selected: true,
            own: true,
            detected: true,
            ..Default::default()
        };
        assert_eq!(box_colour(own), (Some(OWN), false));
        // The box is drawn before the sprite.
        let sprite = Op::Sprite {
            sheet: Sheet::Unit(0),
            frame: 0,
            mirrored: false,
            centre: (336, 176),
            lift: 0,
            base: RAMP,
        };
        let e = Entry {
            building: false,
            air: false,
            sprite: Some(sprite),
            shadow: None,
            outline: Some(((320, 160), (351, 191), OWN)),
            charms: vec![],
            fire: None,
        };
        let ops = entry_ops(&e, &Canvas::new(0, 0, 1000, 1000));
        assert!(matches!(ops[0], Op::Outline { colour: OWN, .. }));
        assert_eq!(ops[1], sprite);
    }

    #[test]
    fn box_colours() {
        let sel = BoxCase {
            selected: true,
            detected: true,
            ..Default::default()
        };
        assert_eq!(box_colour(sel).0, Some(OTHER), "selected enemy, not allied");
        assert_eq!(box_colour(BoxCase { allied: true, ..sel }).0, Some(ALLIED));
        // A rescued unit flashing: countdown 100 has bit 3 clear, so no box even though it is selected; 104 has it.
        assert_eq!(box_colour(BoxCase { flash: 100, ..sel }), (None, false));
        assert_eq!(box_colour(BoxCase { flash: 104, ..sel }), (Some(OTHER), false));
        // A flashing unit that isn't detected, or is gone: no box and the flash stops.
        assert_eq!(
            box_colour(BoxCase {
                flash: 104,
                detected: false,
                ..sel
            }),
            (None, true)
        );
        assert_eq!(
            box_colour(BoxCase {
                flash: 9,
                gone: true,
                ..sel
            }),
            (None, true)
        );
        // Placing a farm: the local player's town hall and a barracks under construction are both outlined.
        let placing = BoxCase {
            placing: true,
            own: true,
            detected: true,
            ..Default::default()
        };
        assert_eq!(box_colour(placing).0, Some(PLACING));
        assert_eq!(box_colour(BoxCase { gone: true, ..placing }).0, None);
    }

    #[test]
    fn box_edges_outside_the_view_are_left_out() {
        let mut c = Canvas::new(0, 0, 10, 10);
        // The left edge (x = −2) lies outside; the top edge is clipped to the view.
        c.outline((-2, 2), (5, 6), 9);
        assert_eq!(
            (c.at(0, 2), c.at(5, 2), c.at(5, 4), c.at(0, 4)),
            (Some(9), Some(9), Some(9), Some(CLEAR))
        );
    }

    #[test]
    fn charms_in_a_row_at_the_box_corner() {
        let s = Spells {
            bloodlust: true,
            haste: -5,
            ..Default::default()
        };
        assert_eq!(charms(s), [(0, 0), (2, 16)]);
        assert_eq!(
            charms(Spells {
                haste: 3,
                invisibility: true,
                unholy_armour: true,
                ..Default::default()
            }),
            [(1, 16), (3, 32), (4, 48)]
        );
        // A box corner 10 pixels from the view's right edge: the 20 × 20 area doesn't fit, so the icon is left out.
        let view = Canvas::new(0, 0, 448, 448);
        assert!(!charm_fits(&view, (438, 100)));
        assert!(charm_fits(&view, (428, 100)));
        let e = Entry {
            building: false,
            air: false,
            sprite: None,
            shadow: None,
            outline: None,
            charms: vec![(0, (438, 100))],
            fire: None,
        };
        assert!(entry_ops(&e, &view).is_empty());
    }

    #[test]
    fn fires_by_hit_points() {
        // Town hall, M = 1200.
        assert_eq!(fire(700, 1200), Some(Fire::Light));
        assert_eq!(fire(599, 1200), Some(Fire::Heavy));
        assert_eq!(fire(600, 1200), Some(Fire::Light));
        assert_eq!(fire(900, 1200), None);
        assert_eq!(fire(0, 0), None, "no fire at max HP 0");
        // Two 4 × 4 halls with phases 0 and 3, heavy fire on frame 8.
        assert_eq!((fire_frame(Fire::Heavy, 8, 0), fire_frame(Fire::Heavy, 8, 3)), (8, 1));
        assert_eq!([1, 2, 3, 4].map(fire_lift), [0, 24, 20, 26]);
        assert_eq!((LIGHT_FIRE, HEAVY_FIRE), (0x13, 0x14));
    }

    #[test]
    fn effects_drawn_while_showing_and_seen() {
        assert!(!effect_drawn(true, false, || false), "fogged");
        assert!(
            effect_drawn(true, true, || false),
            "the reveal-map cheat draws every showing effect"
        );
        assert!(!effect_drawn(false, true, || true), "not showing yet");
        assert!(effect_drawn(true, false, || true));
    }

    #[test]
    fn a_fogged_missile_is_left_out() {
        // None of the 9 tiles around it is seen.
        assert!(!effect_seen((100, 100), (64, 64), |_, _| true));
        // One neighbour seen is enough; tiles off the map are ignored.
        assert!(effect_seen((100, 100), (64, 64), |x, y| (x, y) != (4, 4)));
        assert!(!effect_seen((10, 10), (64, 64), |x, y| x >= 0 && y >= 0));
    }

    #[test]
    fn effect_files() {
        assert_eq!(
            effect_file(0x15).as_deref(),
            Some("Art\\Unit\\Other\\cathit.grp"),
            "ballista impact"
        );
        assert_eq!(effect_file(0x13).as_deref(), Some("Art\\Unit\\Other\\ltfire.grp"));
        assert_eq!(effect_file(0x14).as_deref(), Some("Art\\Unit\\Other\\hvfire.grp"));
        assert_eq!(effect_file(0x1c).as_deref(), Some("Art\\Unit\\Other\\blackx.grp"));
        assert_eq!(effect_file(0x1d), None);
    }

    #[test]
    fn directions() {
        assert_eq!(
            [
                (0, -5),
                (5, -5),
                (5, 0),
                (3, 3),
                (0, 1),
                (-2, 2),
                (-9, 0),
                (-1, -1),
                (0, 0)
            ]
            .map(direction_facing),
            [0, 1, 2, 3, 4, 5, 6, 7, 0]
        );
    }

    #[test]
    fn the_ground_marker_shows_for_thirteen_updates() {
        let mut m = Marker::new((40, 40));
        // First update in cycle U: not drawn after it.
        assert!(m.update() && !m.showing());
        for _ in 1..=13 {
            assert!(m.update() && m.showing());
        }
        // Hidden from the update in U + 14.
        assert!(!m.update() && !m.showing());
    }

    /// The marker's frames after the next 13 updates.
    fn marker_frames(m: &mut Marker) -> Vec<Option<u8>> {
        (0..13)
            .map(|_| {
                m.update();
                m.frame()
            })
            .collect()
    }

    const MARKER_SEQUENCE: [u8; 13] = [0, 0, 1, 2, 3, 3, 3, 3, 3, 3, 2, 1, 0];

    #[test]
    fn the_ground_marker_grows_holds_and_shrinks() {
        // Map-drawing spec, test vector: frames 0, 0, 1, 2, 3, 3, 3, 3, 3, 3, 2, 1, 0 after U + 1 to U + 13.
        let mut m = Marker::new((40, 40));
        m.update();
        assert_eq!(m.frame(), None, "not drawn after its first update");
        assert_eq!(marker_frames(&mut m), MARKER_SEQUENCE.map(Some));
        m.update();
        assert_eq!(m.frame(), None, "hidden from the update in U + 14");
    }

    #[test]
    fn a_new_click_restarts_the_marker_at_the_new_spot() {
        // Shown at frame 3 (cycle U + 6), then a right-click elsewhere with its first update in cycle V.
        let mut m = Marker::new((40, 40));
        for _ in 0..7 {
            m.update();
        }
        assert_eq!(m.frame(), Some(3));
        m = Marker::new((200, 120));
        m.update();
        assert_eq!((m.at, m.frame()), ((200, 120), None), "not drawn after the update of V");
        assert_eq!(marker_frames(&mut m), MARKER_SEQUENCE.map(Some));
    }

    #[test]
    fn the_marker_draws_its_frame_in_column_0_unmirrored() {
        let art = Art {
            effects: HashMap::from([(MARKER, sheet(8, 8, &[((0, 0), (8, 8), 77); 4]))]),
            ..Default::default()
        };
        let drawn = |updates| match marker_op(&art, Marker { at: (16, 16), updates }) {
            Some(Op::Sprite {
                frame, mirrored, base, ..
            }) => Some((frame, mirrored, base)),
            _ => None,
        };
        assert_eq!(
            drawn(7),
            Some((3, false, RAMP)),
            "cycle U + 6: the whole X, no team colour"
        );
        assert_eq!(drawn(13), Some((1, false, RAMP)));
        assert_eq!(drawn(15), None);
    }

    fn black_tile_shapes() -> fog::Shapes {
        fog::Shapes::from_vr4(&vec![0u8; 16 * 1024])
    }

    #[test]
    fn the_marker_and_drag_rectangle_lie_over_the_fog() {
        let art = Art {
            effects: HashMap::from([(MARKER, sheet(8, 8, &[((0, 0), (8, 8), 77)]))]),
            ..Default::default()
        };
        let mut c = Canvas::new(0, 0, 32, 32);
        let f = ViewFrame {
            effects: vec![Op::Sprite {
                sheet: Sheet::Effect(MARKER),
                frame: 0,
                mirrored: false,
                centre: (20, 20),
                lift: 0,
                base: RAMP,
            }],
            fog: Some((1, vec![(16, 16)])),
            marker: Some(Op::Sprite {
                sheet: Sheet::Effect(MARKER),
                frame: 0,
                mirrored: false,
                centre: (8, 8),
                lift: 0,
                base: RAMP,
            }),
            drag: Some(((30, 30), (31, 31))),
            ..Default::default()
        };
        draw(&mut c, &f, &art, &black_tile_shapes());
        assert_eq!(c.at(8, 8), Some(77), "the marker is over the fog");
        assert_eq!(c.at(20, 20), Some(0), "an effect is under it");
        assert_eq!(c.at(30, 30), Some(DRAG as u16));
        assert_eq!(c.at(31, 31), Some(DRAG as u16), "both corners included");
    }

    #[test]
    fn layers_draw_bottom_to_top() {
        let art = Art {
            units: HashMap::from([
                (1, sheet(8, 8, &[((0, 0), (8, 8), 1)])),
                (2, sheet(8, 8, &[((0, 0), (8, 8), 2)])),
            ]),
            ..Default::default()
        };
        let op = |k: u8| Op::Sprite {
            sheet: Sheet::Unit(k),
            frame: 0,
            mirrored: false,
            centre: (8, 8),
            lift: 0,
            base: RAMP,
        };
        let unit = |k: u8, air: bool| Entry {
            building: false,
            air,
            sprite: Some(op(k)),
            shadow: None,
            outline: None,
            charms: vec![],
            fire: None,
        };
        // A building in the background is covered by a unit; with no fog drawn (reveal), the unit shows.
        let f = ViewFrame {
            background: vec![op(1)],
            entries: vec![unit(2, false)],
            ..Default::default()
        };
        let mut c = Canvas::new(0, 0, 16, 16);
        draw(&mut c, &f, &art, &black_tile_shapes());
        assert_eq!(c.at(8, 8), Some(2));
        assert_eq!(c.at(0, 0), Some(CLEAR), "nothing drawn there; the terrain shows");
    }

    #[test]
    fn placement_checkerboard() {
        // A farm (2 × 2) with a tree on its second tile: that tile's odd pixels in colour 10, the others in 150.
        let g = sheet(64, 64, &[((0, 0), (64, 64), 5)]);
        let c = placement_image(Some(&g), RAMP, (2, 2), &[false, true, false, false]);
        assert_eq!((c.width, c.height), (64, 64));
        assert_eq!(c.at(1, 0), Some(GOOD_TILE as u16));
        assert_eq!(c.at(33, 0), Some(BAD_TILE as u16));
        assert_eq!(c.at(0, 0), Some(5), "even pixels keep the building");
        // A shipyard with no shore: all 9 tiles bad.
        let c = placement_image(None, RAMP, (3, 3), &[true; 9]);
        assert!((0..96).all(|y| (0..96).all(|x| c.at(x, y) == Some(if (x + y) % 2 == 1 { 10 } else { CLEAR }))));
    }

    #[test]
    fn rgba_is_transparent_where_nothing_was_drawn() {
        let mut pal = Palette([[0; 3]; 256]);
        pal.0[9] = [1, 2, 3];
        let mut c = Canvas::new(0, 0, 2, 1);
        c.put(1, 0, 9);
        assert_eq!(c.rgba(&pal), [0, 0, 0, 0, 1, 2, 3, 255]);
    }
}
