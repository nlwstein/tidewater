//! The editor window: menus, the tool palette, the map canvas, the minimap and the property windows.
//!
//! The canvas draws the visible tiles as one mesh over the tileset atlas (`assets`), then the units. Left button:
//! the current tool (paint terrain, place a unit, or select and drag one). Right or middle drag pans; the wheel zooms
//! around the pointer; arrow keys scroll. Every edit records an undo step first (`history`); a paint stroke is one
//! step however long it runs.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use eframe::egui::{self, Color32, Key, KeyboardShortcut, Modifiers, Pos2, Rect, Sense, Stroke, Vec2};
use tidewater_formats::install::Install;
use tidewater_formats::pud::{Era, PlacedUnit};
use tidewater_formats::tileset::TILE;

use crate::assets::{self, Assets};
use crate::catalog::{self, Group, Layer, Race};
use crate::doc::{self, Map, NEUTRAL, owner};
use crate::history::History;
use crate::script::{self, Action, Runner, Step};
use crate::terrain;

/// What the command line gives the editor.
pub struct Options {
    /// The game installation the art is read from.
    pub install: PathBuf,
    pub map: Option<PathBuf>,
    /// The game program "Play in Tidewater" starts; without one, `tidewater` next to the editor's own program.
    pub game: Option<PathBuf>,
    /// The data directory the play test runs with (its settings, saves and the map copy).
    pub play_data_dir: PathBuf,
    pub actions: Vec<Action>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Select,
    Terrain,
    Unit(u8),
}

/// The terrain brushes: code and name.
const BRUSHES: [(u8, &str); 8] = [
    (1, "Deep water"),
    (2, "Water"),
    (3, "Dark coast"),
    (4, "Coast"),
    (5, "Dark ground"),
    (6, "Ground"),
    (7, "Forest"),
    (8, "Rock"),
];

/// Gold mine and oil patch amounts are stored in units of this many.
const AMOUNT_UNIT: u32 = 2500;
const ZOOMS: (f32, f32) = (0.0625, 4.0);

struct Open {
    map: Map,
    assets: Assets,
    path: Option<PathBuf>,
}

/// A unit being dragged with the select tool: its index and the tile under the pointer when it was grabbed, relative
/// to the unit's top left.
#[derive(Clone, Copy)]
struct Drag {
    unit: usize,
    grab: (i32, i32),
    target: (i32, i32),
}

#[derive(Default)]
struct Windows {
    players: bool,
    map: bool,
    new: bool,
    open: bool,
    save_as: bool,
}

pub struct Editor {
    inst: Install,
    opts: Options,
    doc: Option<Open>,
    tool: Tool,
    code: u8,
    brush: i32,
    /// The player new units go to: 0 to 7, or 15 for neutral.
    player: u8,
    history: History,
    stroke: Option<(i32, i32)>,
    status: String,
    /// The map pixel at the canvas's top left, and points per map pixel.
    offset: Vec2,
    zoom: f32,
    canvas: Rect,
    hover: Option<(i32, i32)>,
    selected: Option<usize>,
    drag: Option<Drag>,
    minimap: Option<egui::TextureHandle>,
    minimap_dirty: bool,
    windows: Windows,
    new_size: u16,
    new_era: Era,
    path_text: String,
    script: Runner,
    pending: Option<Step>,
}

fn era_name(e: Era) -> &'static str {
    match e {
        Era::Forest => "Forest",
        Era::Winter => "Winter",
        Era::Wasteland => "Wasteland",
        Era::Swamp => "Swamp",
    }
}

fn controller_name(c: u8) -> &'static str {
    match c {
        owner::PERSON => "Human player",
        owner::COMPUTER => "Computer",
        owner::PASSIVE_COMPUTER => "Passive computer",
        owner::RESCUE_PASSIVE => "Rescue (passive)",
        owner::RESCUE_ACTIVE => "Rescue (active)",
        owner::NOBODY => "Nobody",
        _ => "Other",
    }
}

impl Editor {
    pub fn new(ctx: &egui::Context, opts: Options) -> Result<Self> {
        let inst = Install::open(&opts.install)
            .with_context(|| format!("opening the game installation {}", opts.install.display()))?;
        let script = Runner::new(opts.actions.clone());
        let mut e = Self {
            inst,
            doc: None,
            tool: Tool::Select,
            code: 7,
            brush: 1,
            player: 0,
            history: History::default(),
            stroke: None,
            status: String::new(),
            offset: Vec2::ZERO,
            zoom: 0.5,
            canvas: Rect::NOTHING,
            hover: None,
            selected: None,
            drag: None,
            minimap: None,
            minimap_dirty: true,
            windows: Windows::default(),
            new_size: 64,
            new_era: Era::Forest,
            path_text: String::new(),
            script,
            pending: None,
            opts,
        };
        match e.opts.map.clone() {
            Some(p) => e.open_path(ctx, &p)?,
            None => e.new_map(ctx, 64, Era::Forest)?,
        }
        Ok(e)
    }

    fn set_doc(&mut self, ctx: &egui::Context, map: Map, path: Option<PathBuf>) -> Result<()> {
        let assets = Assets::load(ctx, &mut self.inst, map.era, map.section(b"UDTA"))?;
        self.doc = Some(Open { map, assets, path });
        self.history = History::default();
        self.selected = None;
        self.drag = None;
        self.minimap_dirty = true;
        self.zoom = 0.5;
        self.offset = Vec2::ZERO;
        Ok(())
    }

    fn open_path(&mut self, ctx: &egui::Context, path: &Path) -> Result<()> {
        let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let map = Map::from_bytes(&bytes).with_context(|| format!("opening {}", path.display()))?;
        self.set_doc(ctx, map, Some(path.to_path_buf()))?;
        self.status = format!("Opened {}", path.display());
        Ok(())
    }

    fn new_map(&mut self, ctx: &egui::Context, size: u16, era: Era) -> Result<()> {
        // Light ground, the tileset's first plain grass group.
        let map = Map::new(size, era, 0x50)?;
        self.set_doc(ctx, map, None)?;
        self.status = format!("New {size} × {size} {} map", era_name(era));
        Ok(())
    }

    fn save_to(&mut self, path: &Path) -> Result<()> {
        let Some(d) = self.doc.as_mut() else { return Ok(()) };
        let bytes = d.map.to_bytes(&d.assets.tileset);
        if let Some(dir) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))?;
        d.map.saved(&d.assets.tileset);
        d.path = Some(path.to_path_buf());
        self.status = format!("Saved {}", path.display());
        Ok(())
    }

    /// Saves a copy in the play-test data directory and starts the game on it.
    fn play(&mut self) -> Result<()> {
        let Some(d) = self.doc.as_ref() else { return Ok(()) };
        let dir = &self.opts.play_data_dir;
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        let map = dir.join("editor-playtest.pud");
        std::fs::write(&map, d.map.to_bytes(&d.assets.tileset))?;
        let game = match &self.opts.game {
            Some(g) => g.clone(),
            None => {
                let exe = std::env::current_exe()?;
                exe.with_file_name(format!("tidewater{}", std::env::consts::EXE_SUFFIX))
            }
        };
        std::process::Command::new(&game)
            .arg("--install")
            .arg(&self.opts.install)
            .arg("--data-dir")
            .arg(dir)
            .arg(&map)
            .spawn()
            .with_context(|| format!("starting {}", game.display()))?;
        self.status = format!("Started {} on the map", game.display());
        Ok(())
    }

    fn report(&mut self, r: Result<()>) {
        if let Err(e) = r {
            self.status = format!("Error: {e:#}");
        }
    }

    // --- editing ---------------------------------------------------------------------------------------------

    fn begin_edit(&mut self) {
        if let Some(d) = &self.doc {
            self.history.push(d.map.state.clone());
        }
    }

    fn undo(&mut self) {
        if let Some(d) = self.doc.as_mut()
            && let Some(s) = self.history.undo(&d.map.state)
        {
            d.map.state = s;
            self.selected = None;
            self.minimap_dirty = true;
        }
    }

    fn redo(&mut self) {
        if let Some(d) = self.doc.as_mut()
            && let Some(s) = self.history.redo(&d.map.state)
        {
            d.map.state = s;
            self.selected = None;
            self.minimap_dirty = true;
        }
    }

    /// Paints the current brush centred on a tile.
    fn paint(&mut self, (x, y): (i32, i32)) {
        let Some(d) = self.doc.as_mut() else { return };
        let n = d.map.size;
        let half = (self.brush - 1) / 2;
        terrain::paint(
            &d.assets.rules,
            &d.assets.tileset,
            &mut d.map.state.tiles,
            n,
            n,
            terrain::Stroke {
                at: (x - half, y - half),
                size: self.brush,
                code: self.code,
            },
        );
        self.minimap_dirty = true;
    }

    /// Whether a unit of `kind` for `owner` may stand at `tile`, leaving out unit `ignore` (the one being moved).
    fn placement_ok(&self, kind: u8, tile: (i32, i32), ignore: Option<usize>) -> bool {
        let Some(d) = &self.doc else { return false };
        let stats = &d.assets.stats;
        let flags = |x: i32, y: i32| d.map.tile(x, y).map(|t| d.assets.tileset.flags_for(t));
        if !catalog::terrain_allows(stats, kind, tile, flags) {
            return false;
        }
        let layer = catalog::layer(stats, kind);
        if layer == Layer::Marker {
            return true;
        }
        let (w, h) = catalog::footprint(stats, kind);
        !d.map.state.units.iter().enumerate().any(|(i, u)| {
            let (uw, uh) = catalog::footprint(stats, u.kind);
            Some(i) != ignore
                && catalog::layer(stats, u.kind) == layer
                && tile.0 < u.x as i32 + uw
                && (u.x as i32) < tile.0 + w
                && tile.1 < u.y as i32 + uh
                && (u.y as i32) < tile.1 + h
        })
    }

    /// The owner a new unit of `kind` gets: the neutral player for neutral types unless a player is picked for them.
    fn owner_for(&self, kind: u8) -> u8 {
        if catalog::is_neutral(kind) && !catalog::STARTS.contains(&kind) {
            NEUTRAL
        } else {
            self.player.min(7)
        }
    }

    fn place(&mut self, kind: u8, tile: (i32, i32)) {
        if !self.placement_ok(kind, tile, None) {
            self.status = format!("{} can't stand there", catalog::name(kind));
            return;
        }
        let owner = self.owner_for(kind);
        self.begin_edit();
        let Some(d) = self.doc.as_mut() else { return };
        let units = &mut d.map.state.units;
        // One start location per player: a new one replaces the old.
        if catalog::STARTS.contains(&kind) {
            units.retain(|u| !(catalog::STARTS.contains(&u.kind) && u.owner == owner));
        }
        let data = match kind {
            catalog::GOLD_MINE | catalog::OIL_PATCH => (50_000 / AMOUNT_UNIT) as u16,
            k if catalog::STARTS.contains(&k) => 0,
            _ => 1,
        };
        units.push(PlacedUnit {
            x: tile.0 as u16,
            y: tile.1 as u16,
            kind,
            owner,
            data,
        });
        self.selected = Some(units.len() - 1);
        self.minimap_dirty = true;
        self.status = format!("Placed {} for {}", catalog::name(kind), player_name(owner));
    }

    /// The topmost unit covering a tile: air over ground, later over earlier.
    fn unit_at(&self, (x, y): (i32, i32)) -> Option<usize> {
        let d = self.doc.as_ref()?;
        let stats = &d.assets.stats;
        let covers = |u: &PlacedUnit| {
            let (w, h) = catalog::footprint(stats, u.kind);
            (u.x as i32..u.x as i32 + w).contains(&x) && (u.y as i32..u.y as i32 + h).contains(&y)
        };
        let units = &d.map.state.units;
        let rank = |l: Layer| match l {
            Layer::Air => 2,
            Layer::Marker => 1,
            Layer::Ground => 0,
        };
        (0..units.len())
            .filter(|&i| covers(&units[i]))
            .max_by_key(|&i| (rank(catalog::layer(stats, units[i].kind)), i))
    }

    fn delete_selected(&mut self) {
        let Some(i) = self.selected else { return };
        self.begin_edit();
        if let Some(d) = self.doc.as_mut()
            && i < d.map.state.units.len()
        {
            let u = d.map.state.units.remove(i);
            self.status = format!("Deleted {}", catalog::name(u.kind));
        }
        self.selected = None;
        self.minimap_dirty = true;
    }

    // --- view ------------------------------------------------------------------------------------------------

    fn to_screen(&self, map_px: Vec2) -> Pos2 {
        self.canvas.min + (map_px - self.offset) * self.zoom
    }

    fn to_map(&self, p: Pos2) -> Vec2 {
        (p - self.canvas.min) / self.zoom + self.offset
    }

    fn tile_at(&self, p: Pos2) -> Option<(i32, i32)> {
        let d = self.doc.as_ref()?;
        let m = self.to_map(p) / TILE as f32;
        let (x, y) = (m.x.floor() as i32, m.y.floor() as i32);
        let n = d.map.size as i32;
        (x >= 0 && y >= 0 && x < n && y < n).then_some((x, y))
    }

    fn centre_on(&mut self, map_px: Vec2) {
        self.offset = map_px - self.canvas.size() / (2.0 * self.zoom);
        self.clamp_view();
    }

    fn clamp_view(&mut self) {
        let Some(d) = &self.doc else { return };
        let full = (d.map.size * TILE) as f32;
        let view = self.canvas.size() / self.zoom;
        let lim = |o: f32, v: f32| {
            if v >= full {
                (full - v) / 2.0
            } else {
                o.clamp(-v * 0.25, full - v * 0.75)
            }
        };
        self.offset = Vec2::new(lim(self.offset.x, view.x), lim(self.offset.y, view.y));
    }

    fn zoom_at(&mut self, factor: f32, around: Pos2) {
        let before = self.to_map(around);
        self.zoom = (self.zoom * factor).clamp(ZOOMS.0, ZOOMS.1);
        self.offset = before - (around - self.canvas.min) / self.zoom;
        self.clamp_view();
    }

    // --- script ----------------------------------------------------------------------------------------------

    fn run_app_action(&mut self, ctx: &egui::Context, a: Action) {
        match a {
            Action::View(x, y, z) => {
                self.zoom = z.clamp(ZOOMS.0, ZOOMS.1);
                self.centre_on(Vec2::new(x, y) * TILE as f32);
            }
            Action::ToolSelect => self.tool = Tool::Select,
            Action::ToolTerrain(c, s) => {
                self.tool = Tool::Terrain;
                self.code = c;
                self.brush = s;
            }
            Action::ToolUnit(k) => self.tool = Tool::Unit(k),
            Action::Player(p) => self.player = if p >= 8 { NEUTRAL } else { p },
            Action::Window(w) => match w.as_str() {
                "players" => self.windows.players = true,
                "map" => self.windows.map = true,
                "new" => self.windows.new = true,
                "open" => self.windows.open = true,
                "save" => self.windows.save_as = true,
                _ => self.status = format!("no window {w}"),
            },
            Action::Save(p) => {
                let r = self.save_to(&p);
                self.report(r);
            }
            Action::Shot(p) => ctx.request_screenshot(move |img| {
                if let Err(e) = script::write_png(&p, &img) {
                    eprintln!("screenshot: {e:#}");
                }
            }),
            Action::Play => {
                let r = self.play();
                self.report(r);
            }
            Action::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            _ => {}
        }
    }

    // --- drawing ---------------------------------------------------------------------------------------------

    fn draw_canvas(&mut self, ui: &mut egui::Ui) {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let first = self.canvas == Rect::NOTHING;
        self.canvas = rect;
        if first && let Some(d) = &self.doc {
            let full = (d.map.size * TILE) as f32;
            self.zoom = (rect.width().min(rect.height()) / full).clamp(ZOOMS.0, 1.0);
            self.centre_on(Vec2::splat(full / 2.0));
        }
        self.handle_canvas_input(ui, &response);
        let ctx = ui.ctx().clone();
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::from_gray(24));
        let Some(d) = self.doc.as_mut() else { return };
        let n = d.map.size as i32;
        // Visible tiles.
        let tl = (self.offset / TILE as f32).floor();
        let br = ((self.offset + rect.size() / self.zoom) / TILE as f32).ceil();
        let (x0, y0) = ((tl.x as i32).max(0), (tl.y as i32).max(0));
        let (x1, y1) = ((br.x as i32).min(n), (br.y as i32).min(n));
        let (aw, ah) = d.assets.atlas_size();
        let mut mesh = egui::Mesh::with_texture(d.assets.atlas.id());
        let t = TILE as f32;
        let to_screen = |mp: Vec2| rect.min + (mp - self.offset) * self.zoom;
        for y in y0..y1 {
            for x in x0..x1 {
                let m = d.assets.megatile(d.map.state.tiles[(y * n + x) as usize]);
                let (cx, cy) = assets::atlas_cell(m);
                let uv = Rect::from_min_size(
                    Pos2::new(cx as f32 * t / aw, cy as f32 * t / ah),
                    Vec2::new(t / aw, t / ah),
                );
                let r = Rect::from_min_max(
                    to_screen(Vec2::new(x as f32, y as f32) * t),
                    to_screen(Vec2::new(x as f32 + 1.0, y as f32 + 1.0) * t),
                );
                mesh.add_rect_with_uv(r, uv, Color32::WHITE);
            }
        }
        painter.add(egui::Shape::mesh(mesh));
        // Units: ground first, then start locations, then air.
        let stats = d.assets.stats.clone();
        let mut order: Vec<usize> = (0..d.map.state.units.len()).collect();
        order.sort_by_key(|&i| match catalog::layer(&stats, d.map.state.units[i].kind) {
            Layer::Ground => 0,
            Layer::Marker => 1,
            Layer::Air => 2,
        });
        for i in order {
            let mut u = d.map.state.units[i];
            if let Some(dr) = self.drag.filter(|dr| dr.unit == i) {
                u.x = dr.target.0.max(0) as u16;
                u.y = dr.target.1.max(0) as u16;
            }
            let (w, h) = catalog::footprint(&stats, u.kind);
            let foot = Rect::from_min_max(
                to_screen(Vec2::new(u.x as f32, u.y as f32) * t),
                to_screen(Vec2::new((u.x as i32 + w) as f32, (u.y as i32 + h) as f32) * t),
            );
            if !foot.intersects(rect) {
                continue;
            }
            match d.assets.sprite(&ctx, u.kind, u.owner) {
                Some(s) => {
                    let centre = foot.center();
                    let min =
                        centre + (Vec2::new(s.offset.0, s.offset.1) - Vec2::new(s.cell.0, s.cell.1) / 2.0) * self.zoom;
                    let r = Rect::from_min_size(min, Vec2::new(s.size.0, s.size.1) * self.zoom);
                    painter.image(
                        s.texture.id(),
                        r,
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
                None => {
                    painter.rect_filled(
                        foot.shrink(2.0),
                        2.0,
                        player_colour(&d.assets, u.owner).gamma_multiply(0.8),
                    );
                }
            }
            if catalog::STARTS.contains(&u.kind) {
                painter.rect_stroke(
                    foot,
                    0.0,
                    Stroke::new(2.0, player_colour(&d.assets, u.owner)),
                    egui::StrokeKind::Inside,
                );
            }
            if self.selected == Some(i) {
                painter.rect_stroke(
                    foot,
                    0.0,
                    Stroke::new(2.0, Color32::from_rgb(80, 255, 80)),
                    egui::StrokeKind::Outside,
                );
            }
        }
        // The tool's cursor.
        if let Some(h) = self.hover {
            match self.tool {
                Tool::Terrain => {
                    let half = (self.brush - 1) / 2;
                    let a = Vec2::new((h.0 - half) as f32, (h.1 - half) as f32) * t;
                    let r = Rect::from_min_max(to_screen(a), to_screen(a + Vec2::splat(self.brush as f32 * t)));
                    painter.rect_stroke(r, 0.0, Stroke::new(1.5, Color32::WHITE), egui::StrokeKind::Inside);
                }
                Tool::Unit(k) => {
                    let (w, hh) = catalog::footprint(&stats, k);
                    let ok = self.placement_ok(k, h, None);
                    let owner = self.owner_for(k);
                    let Some(d) = self.doc.as_mut() else { return };
                    let a = Vec2::new(h.0 as f32, h.1 as f32) * t;
                    let r = Rect::from_min_max(to_screen(a), to_screen(a + Vec2::new(w as f32 * t, hh as f32 * t)));
                    if let Some(s) = d.assets.sprite(&ctx, k, owner) {
                        let min = r.center()
                            + (Vec2::new(s.offset.0, s.offset.1) - Vec2::new(s.cell.0, s.cell.1) / 2.0) * self.zoom;
                        let sr = Rect::from_min_size(min, Vec2::new(s.size.0, s.size.1) * self.zoom);
                        painter.image(
                            s.texture.id(),
                            sr,
                            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                            Color32::from_white_alpha(170),
                        );
                    }
                    let c = if ok { Color32::WHITE } else { Color32::RED };
                    painter.rect_stroke(r, 0.0, Stroke::new(2.0, c), egui::StrokeKind::Inside);
                }
                Tool::Select => {
                    if let Some(dr) = self.drag {
                        let Some(d) = self.doc.as_ref() else { return };
                        let k = d.map.state.units[dr.unit].kind;
                        let (w, hh) = catalog::footprint(&stats, k);
                        let ok = self.placement_ok(k, dr.target, Some(dr.unit));
                        let a = Vec2::new(dr.target.0 as f32, dr.target.1 as f32) * t;
                        let r = Rect::from_min_max(to_screen(a), to_screen(a + Vec2::new(w as f32 * t, hh as f32 * t)));
                        let c = if ok { Color32::WHITE } else { Color32::RED };
                        painter.rect_stroke(r, 0.0, Stroke::new(2.0, c), egui::StrokeKind::Inside);
                    }
                }
            }
        }
    }

    fn handle_canvas_input(&mut self, ui: &egui::Ui, response: &egui::Response) {
        // Pan with the right or middle button, zoom with the wheel, scroll with the arrow keys.
        if response.dragged_by(egui::PointerButton::Secondary) || response.dragged_by(egui::PointerButton::Middle) {
            self.offset -= response.drag_delta() / self.zoom;
            self.clamp_view();
        }
        if let Some(p) = response.hover_pos() {
            let (scroll, zoom) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta()));
            if zoom != 1.0 {
                self.zoom_at(zoom, p);
            } else if scroll.y != 0.0 {
                self.zoom_at((scroll.y / 200.0).exp(), p);
            }
        }
        if !ui.ctx().egui_wants_keyboard_input() {
            let step = 400.0 * ui.input(|i| i.stable_dt).min(0.1) / self.zoom;
            let k = |key| ui.input(|i| i.key_down(key));
            let d = Vec2::new(
                f32::from(k(Key::ArrowRight)) - f32::from(k(Key::ArrowLeft)),
                f32::from(k(Key::ArrowDown)) - f32::from(k(Key::ArrowUp)),
            );
            if d != Vec2::ZERO {
                self.offset += d * step;
                self.clamp_view();
                ui.ctx().request_repaint();
            }
        }
        self.hover = response
            .hover_pos()
            .or(response.interact_pointer_pos())
            .and_then(|p| self.tile_at(p));
        let (pressed, down, released) = ui.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.pointer.primary_released(),
            )
        });
        let on_canvas = response.is_pointer_button_down_on() || response.contains_pointer();
        match self.tool {
            Tool::Terrain => {
                let painting = down && on_canvas && self.stroke.is_some() || pressed && response.contains_pointer();
                if painting
                    && let Some(t) = self.hover
                    && self.stroke != Some(t)
                {
                    if self.stroke.is_none() {
                        self.begin_edit();
                    }
                    self.paint(t);
                    self.stroke = Some(t);
                }
                if released || !down {
                    self.stroke = None;
                }
            }
            Tool::Unit(k) => {
                if pressed
                    && response.contains_pointer()
                    && let Some(t) = self.hover
                {
                    self.place(k, t);
                }
            }
            Tool::Select => {
                if pressed && response.contains_pointer() {
                    self.selected = self.hover.and_then(|t| self.unit_at(t));
                    if let (Some(i), Some(t)) = (self.selected, self.hover)
                        && let Some(d) = &self.doc
                    {
                        let u = d.map.state.units[i];
                        self.drag = Some(Drag {
                            unit: i,
                            grab: (t.0 - u.x as i32, t.1 - u.y as i32),
                            target: (u.x as i32, u.y as i32),
                        });
                    }
                }
                if let (Some(mut dr), Some(t)) = (self.drag, self.hover)
                    && down
                {
                    dr.target = (t.0 - dr.grab.0, t.1 - dr.grab.1);
                    self.drag = Some(dr);
                }
                if released && let Some(dr) = self.drag.take() {
                    self.finish_move(dr);
                }
            }
        }
    }

    fn finish_move(&mut self, dr: Drag) {
        let Some(d) = &self.doc else { return };
        let u = d.map.state.units[dr.unit];
        if (u.x as i32, u.y as i32) == dr.target {
            return;
        }
        if !self.placement_ok(u.kind, dr.target, Some(dr.unit)) {
            self.status = format!("{} can't stand there", catalog::name(u.kind));
            return;
        }
        self.begin_edit();
        if let Some(d) = self.doc.as_mut() {
            let u = &mut d.map.state.units[dr.unit];
            u.x = dr.target.0 as u16;
            u.y = dr.target.1 as u16;
        }
        self.minimap_dirty = true;
    }

    fn minimap(&mut self, ui: &mut egui::Ui) {
        let Some(d) = self.doc.as_ref() else { return };
        let n = d.map.size;
        if self.minimap_dirty || self.minimap.is_none() {
            let mut img = egui::ColorImage::filled([n, n], Color32::BLACK);
            for (i, &t) in d.map.state.tiles.iter().enumerate() {
                img.pixels[i] = d
                    .assets
                    .colours
                    .get(d.assets.megatile(t))
                    .copied()
                    .unwrap_or(Color32::BLACK);
            }
            for u in &d.map.state.units {
                let (w, h) = catalog::footprint(&d.assets.stats, u.kind);
                let c = match u.kind {
                    catalog::GOLD_MINE => Color32::from_rgb(252, 252, 0),
                    catalog::OIL_PATCH => Color32::BLACK,
                    _ => player_colour(&d.assets, u.owner),
                };
                for y in u.y as usize..(u.y as usize + h as usize).min(n) {
                    for x in u.x as usize..(u.x as usize + w as usize).min(n) {
                        img.pixels[y * n + x] = c;
                    }
                }
            }
            self.minimap = Some(ui.ctx().load_texture("minimap", img, egui::TextureOptions::NEAREST));
            self.minimap_dirty = false;
        }
        let side = ui.available_width().min(256.0);
        let (rect, resp) = ui.allocate_exact_size(Vec2::splat(side), Sense::click_and_drag());
        if let Some(tex) = &self.minimap {
            ui.painter().image(
                tex.id(),
                rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        let full = (n * TILE) as f32;
        let scale = side / full;
        let view =
            Rect::from_min_size(rect.min + self.offset * scale, self.canvas.size() / self.zoom * scale).intersect(rect);
        ui.painter()
            .rect_stroke(view, 0.0, Stroke::new(1.0, Color32::WHITE), egui::StrokeKind::Inside);
        if let Some(p) = resp.interact_pointer_pos()
            && (resp.clicked() || resp.dragged())
        {
            self.centre_on((p - rect.min) / scale);
        }
    }

    fn palette(&mut self, ui: &mut egui::Ui) {
        ui.heading("Tools");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tool, Tool::Select, "Select");
            ui.selectable_value(&mut self.tool, Tool::Terrain, "Terrain");
        });
        ui.separator();
        ui.label("Terrain");
        let solid = self
            .doc
            .as_ref()
            .map(|d| d.assets.rules.solid.clone())
            .unwrap_or_default();
        egui::Grid::new("brushes").num_columns(2).show(ui, |ui| {
            for (i, (code, name)) in BRUSHES.iter().enumerate() {
                if !solid.contains(code) {
                    continue;
                }
                let on = self.tool == Tool::Terrain && self.code == *code;
                let swatch = self.brush_colour(*code);
                let b = egui::Button::new(egui::RichText::new(*name).color(Color32::WHITE))
                    .fill(swatch)
                    .selected(on);
                if ui.add_sized([110.0, 22.0], b).clicked() {
                    self.tool = Tool::Terrain;
                    self.code = *code;
                }
                if i % 2 == 1 {
                    ui.end_row();
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("Brush");
            for s in [1, 3, 5] {
                ui.selectable_value(&mut self.brush, s, format!("{s}×{s}"));
            }
        });
        ui.separator();
        ui.label("Units for");
        egui::ComboBox::from_id_salt("player")
            .selected_text(player_name(self.player))
            .show_ui(ui, |ui| {
                for p in (0..8).chain([NEUTRAL]) {
                    ui.selectable_value(&mut self.player, p, player_name(p));
                }
            });
        let race = self
            .doc
            .as_ref()
            .map(|d| d.map.state.sides[self.player as usize & 15])
            .unwrap_or(0);
        let race = match race {
            0 => Race::Human,
            1 => Race::Orc,
            _ => Race::Any,
        };
        egui::ScrollArea::vertical().show(ui, |ui| {
            for g in Group::ALL {
                egui::CollapsingHeader::new(g.name())
                    .default_open(matches!(g, Group::Land | Group::Buildings | Group::Neutral))
                    .show(ui, |ui| {
                        for &(k, name, r, group) in catalog::TYPES {
                            let shown = group == g && (r == Race::Any || race == Race::Any || r == race);
                            if shown && ui.selectable_label(self.tool == Tool::Unit(k), name).clicked() {
                                self.tool = Tool::Unit(k);
                            }
                        }
                    });
            }
        });
    }

    fn brush_colour(&self, code: u8) -> Color32 {
        let Some(d) = &self.doc else { return Color32::GRAY };
        d.assets
            .rules
            .group([code; 4])
            .and_then(|g| d.assets.rules.variants(g).first().copied())
            .map_or(Color32::GRAY, |t| d.assets.colours[d.assets.megatile(t)])
            .gamma_multiply(0.8)
    }

    fn inspector(&mut self, ui: &mut egui::Ui) {
        ui.heading("Minimap");
        self.minimap(ui);
        ui.separator();
        let Some(i) = self.selected else {
            ui.label("Click a unit with Select to edit it.");
            return;
        };
        let Some(d) = self.doc.as_ref() else { return };
        let Some(u) = d.map.state.units.get(i).copied() else {
            return;
        };
        ui.heading(catalog::name(u.kind));
        ui.label(format!("Tile {}, {}", u.x, u.y));
        let mut edited = u;
        egui::ComboBox::from_id_salt("owner")
            .selected_text(player_name(u.owner))
            .show_ui(ui, |ui| {
                for p in (0..8).chain([NEUTRAL]) {
                    ui.selectable_value(&mut edited.owner, p, player_name(p));
                }
            });
        if catalog::is_resource(u.kind) {
            let mut amount = u32::from(u.data) * AMOUNT_UNIT;
            ui.horizontal(|ui| {
                ui.label(if u.kind == catalog::GOLD_MINE { "Gold" } else { "Oil" });
                ui.add(
                    egui::DragValue::new(&mut amount)
                        .range(0..=(u32::from(u16::MAX) * AMOUNT_UNIT))
                        .speed(AMOUNT_UNIT),
                );
            });
            edited.data = (amount / AMOUNT_UNIT).min(u32::from(u16::MAX)) as u16;
        } else if !catalog::STARTS.contains(&u.kind) {
            let mut active = u.data != 0;
            ui.checkbox(&mut active, "Active (computer players attack with it)");
            edited.data = u16::from(active);
        }
        if ui.button("Delete").clicked() {
            self.delete_selected();
            return;
        }
        if edited != u {
            self.begin_edit();
            if let Some(d) = self.doc.as_mut() {
                d.map.state.units[i] = edited;
            }
            self.minimap_dirty = true;
        }
    }

    fn players_window(&mut self, ctx: &egui::Context) {
        let mut open = self.windows.players;
        let Some(d) = self.doc.as_ref() else { return };
        let mut s = d.map.state.clone();
        egui::Window::new("Players")
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(self.canvas.center())
            .open(&mut open)
            .show(ctx, |ui| {
                egui::Grid::new("players").striped(true).show(ui, |ui| {
                    for h in ["Player", "Race", "Controller", "Gold", "Lumber", "Oil"] {
                        ui.strong(h);
                    }
                    ui.end_row();
                    for p in 0..8usize {
                        ui.label(player_name(p as u8));
                        egui::ComboBox::from_id_salt(("race", p))
                            .selected_text(["Human", "Orc", "Neutral"][s.sides[p].min(2) as usize])
                            .show_ui(ui, |ui| {
                                for (v, n) in [(0u8, "Human"), (1, "Orc")] {
                                    ui.selectable_value(&mut s.sides[p], v, n);
                                }
                            });
                        egui::ComboBox::from_id_salt(("ctl", p))
                            .selected_text(controller_name(s.owners[p]))
                            .show_ui(ui, |ui| {
                                for c in [
                                    owner::PERSON,
                                    owner::COMPUTER,
                                    owner::PASSIVE_COMPUTER,
                                    owner::RESCUE_PASSIVE,
                                    owner::RESCUE_ACTIVE,
                                    owner::NOBODY,
                                ] {
                                    ui.selectable_value(&mut s.owners[p], c, controller_name(c));
                                }
                            });
                        ui.add(egui::DragValue::new(&mut s.gold[p]).speed(100));
                        ui.add(egui::DragValue::new(&mut s.lumber[p]).speed(100));
                        ui.add(egui::DragValue::new(&mut s.oil[p]).speed(100));
                        ui.end_row();
                    }
                });
                if ui.button("Copy player 1's resources to all").clicked() {
                    for p in 1..8 {
                        s.gold[p] = s.gold[0];
                        s.lumber[p] = s.lumber[0];
                        s.oil[p] = s.oil[0];
                    }
                }
            });
        self.windows.players = open;
        self.apply_state(s);
    }

    /// Replaces the state with an edited copy, recording an undo step when it differs. Start locations follow a
    /// race change.
    fn apply_state(&mut self, mut s: doc::State) {
        let Some(d) = self.doc.as_ref() else { return };
        if s == d.map.state {
            return;
        }
        for u in s.units.iter_mut().filter(|u| catalog::STARTS.contains(&u.kind)) {
            u.kind = catalog::STARTS[(s.sides.get(u.owner as usize).copied().unwrap_or(0) as usize).min(1)];
        }
        self.begin_edit();
        if let Some(d) = self.doc.as_mut() {
            d.map.state = s;
        }
        self.minimap_dirty = true;
    }

    fn map_window(&mut self, ctx: &egui::Context) {
        let mut open = self.windows.map;
        let Some(d) = self.doc.as_ref() else { return };
        let mut s = d.map.state.clone();
        let (size, era) = (d.map.size, d.map.era);
        egui::Window::new("Map properties")
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(self.canvas.center())
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(format!("{size} × {size} tiles, {}", era_name(era)));
                ui.horizontal(|ui| {
                    ui.label("Description");
                    ui.add(egui::TextEdit::singleline(&mut s.description).char_limit(31));
                });
            });
        self.windows.map = open;
        self.apply_state(s);
    }

    fn file_windows(&mut self, ctx: &egui::Context) {
        if self.windows.new {
            let mut open = true;
            let mut go = false;
            egui::Window::new("New map")
                .pivot(egui::Align2::CENTER_CENTER)
                .default_pos(self.canvas.center())
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Size");
                        for s in [32u16, 64, 96, 128] {
                            ui.selectable_value(&mut self.new_size, s, s.to_string());
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("Terrain");
                        for e in [Era::Forest, Era::Winter, Era::Wasteland, Era::Swamp] {
                            ui.selectable_value(&mut self.new_era, e, era_name(e));
                        }
                    });
                    go = ui.button("Create").clicked();
                });
            if go {
                let r = self.new_map(ctx, self.new_size, self.new_era);
                self.canvas = Rect::NOTHING;
                self.report(r);
                open = false;
            }
            self.windows.new = open;
        }
        if self.windows.open {
            let mut open = true;
            let mut chosen = None;
            let maps = list_maps(&self.opts.install.join("Maps"));
            egui::Window::new("Open map")
                .pivot(egui::Align2::CENTER_CENTER)
                .default_pos(self.canvas.center())
                .open(&mut open)
                .default_width(420.0)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Path");
                        ui.text_edit_singleline(&mut self.path_text);
                        if ui.button("Open").clicked() {
                            chosen = Some(PathBuf::from(self.path_text.trim()));
                        }
                    });
                    ui.label("Maps in the installation:");
                    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                        for p in &maps {
                            let name = p.strip_prefix(&self.opts.install).unwrap_or(p).display().to_string();
                            if ui.selectable_label(false, name).clicked() {
                                chosen = Some(p.clone());
                            }
                        }
                    });
                });
            if let Some(p) = chosen {
                let r = self.open_path(ctx, &p);
                self.canvas = Rect::NOTHING;
                if r.is_ok() {
                    open = false;
                }
                self.report(r);
            }
            self.windows.open = open;
        }
        if self.windows.save_as {
            let mut open = true;
            let mut go = false;
            egui::Window::new("Save map as")
                .pivot(egui::Align2::CENTER_CENTER)
                .default_pos(self.canvas.center())
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Path");
                        ui.text_edit_singleline(&mut self.path_text);
                    });
                    go = ui.button("Save").clicked();
                });
            if go {
                let p = PathBuf::from(self.path_text.trim());
                let r = self.save_to(&p);
                if r.is_ok() {
                    open = false;
                }
                self.report(r);
            }
            self.windows.save_as = open;
        }
    }

    fn menu(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New…").clicked() {
                    self.windows.new = true;
                }
                if ui.button("Open…").clicked() {
                    self.windows.open = true;
                }
                if ui.button("Save").clicked() {
                    self.save();
                }
                if ui.button("Save as…").clicked() {
                    self.windows.save_as = true;
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Edit", |ui| {
                if ui
                    .add_enabled(self.history.can_undo(), egui::Button::new("Undo"))
                    .clicked()
                {
                    self.undo();
                }
                if ui
                    .add_enabled(self.history.can_redo(), egui::Button::new("Redo"))
                    .clicked()
                {
                    self.redo();
                }
                if ui
                    .add_enabled(self.selected.is_some(), egui::Button::new("Delete unit"))
                    .clicked()
                {
                    self.delete_selected();
                }
            });
            ui.menu_button("Map", |ui| {
                if ui.button("Properties…").clicked() {
                    self.windows.map = true;
                }
                if ui.button("Players…").clicked() {
                    self.windows.players = true;
                }
            });
            ui.separator();
            if ui.button("Play in Tidewater").clicked() {
                let r = self.play();
                self.report(r);
            }
        });
    }

    fn save(&mut self) {
        match self.doc.as_ref().and_then(|d| d.path.clone()) {
            Some(p) => {
                let r = self.save_to(&p);
                self.report(r);
            }
            None => self.windows.save_as = true,
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let cmd = |k| KeyboardShortcut::new(Modifiers::COMMAND, k);
        let shift = |k| KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, k);
        if ctx.input_mut(|i| i.consume_shortcut(&shift(Key::Z))) || ctx.input_mut(|i| i.consume_shortcut(&cmd(Key::Y)))
        {
            self.redo();
        } else if ctx.input_mut(|i| i.consume_shortcut(&cmd(Key::Z))) {
            self.undo();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&cmd(Key::S))) {
            self.save();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&cmd(Key::O))) {
            self.windows.open = true;
        }
        if !ctx.egui_wants_keyboard_input() {
            if ctx.input(|i| i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace)) {
                self.delete_selected();
            }
            if ctx.input(|i| i.key_pressed(Key::Escape)) {
                self.tool = Tool::Select;
                self.drag = None;
            }
        }
    }

    fn title(&self) -> String {
        let Some(d) = &self.doc else {
            return "Tidewater map editor".into();
        };
        let name = d
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map_or("Untitled".into(), |n| n.to_string_lossy().into_owned());
        format!(
            "{name}{} - Tidewater map editor",
            if d.map.modified() { " *" } else { "" }
        )
    }
}

fn player_name(p: u8) -> String {
    if p == NEUTRAL {
        "Neutral".into()
    } else {
        format!("Player {}", p + 1)
    }
}

fn player_colour(a: &Assets, owner: u8) -> Color32 {
    let [r, g, b] = a.tileset.palette.0[assets::team_base(owner) as usize];
    Color32::from_rgb(r, g, b)
}

/// The `.pud` files under a folder, sorted.
fn list_maps(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("pud")) {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

impl eframe::App for Editor {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        if !self.script.is_running() {
            return;
        }
        ctx.request_repaint();
        let Some(step) = self.script.step() else { return };
        let pos = match step {
            Step::Pointer(x, y, _) => self.to_screen(Vec2::new(x, y) * TILE as f32),
            _ => Pos2::ZERO,
        };
        raw.events.extend(script::events(&step, pos));
        if let Step::App(_) = step {
            self.pending = Some(step);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if let Some(Step::App(a)) = self.pending.take() {
            self.run_app_action(&ctx, a);
        }
        self.shortcuts(&ctx);
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(self.title()));
        egui::Panel::top("menu").show(ui, |ui| self.menu(ui));
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                if let (Some(t), Some(d)) = (self.hover, self.doc.as_ref()) {
                    let tile = d.map.tile(t.0, t.1).unwrap_or(0);
                    ui.label(format!("Tile {}, {}  ({tile:#06x})", t.0, t.1));
                    if let Some(i) = self.unit_at(t) {
                        let u = d.map.state.units[i];
                        ui.label(format!("{} ({})", catalog::name(u.kind), player_name(u.owner)));
                    }
                    ui.separator();
                }
                ui.label(&self.status);
            });
        });
        egui::Panel::left("palette")
            .default_size(250.0)
            .show(ui, |ui| self.palette(ui));
        egui::Panel::right("inspector")
            .default_size(270.0)
            .show(ui, |ui| self.inspector(ui));
        egui::CentralPanel::no_frame().show(ui, |ui| self.draw_canvas(ui));
        self.players_window(&ctx);
        self.map_window(&ctx);
        self.file_windows(&ctx);
    }
}
