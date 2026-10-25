//! The in-game interface, drawn in software into one RGBA picture that the client shows over the map: the stone
//! frame around the map view, the left panel with the minimap and the selected units, and the resource bar.
//!
//! Art comes from the installation (formats in the formats crate's `raw`, `grp` and `fnt` modules): the frame pieces
//! `Art\{h,o}_{top,btm,rgt,lft0,lft1,lft2}.bmp` and the info box `Art\{h,o}frame.grp` per race, the resource icons
//! `Art\icons.grp`, the unit icons `Art\unit\Portrait\*port*.grp` per era, the font `Art\font10x.fnt` and the
//! strings in `rez\stat_txt.tbl` and `rez\network.tbl`, and the pointers `Art\*.cur`. Pieces are placed as the 640x480 screen shows them: a 176-pixel panel on the left,
//! 16-pixel bars above, below and to the right of the map view. Larger canvases repeat the bars and extend the panel.
//! The information panel follows `docs/specs/ui/info-panel.md` (layout and rules in [`crate::info`]); the resource
//! The resource bar, the menu and pause buttons and the pointer follow `docs/specs/ui/screen-and-cursors.md` (rules in
//! [`crate::screen`]); the message area and the status bar follow `docs/specs/ui/messages.md` (rules in
//! [`crate::messages`]). Button frames, the menu button labels and the text colours of the colour codes are our own
//! choices that follow the look of the game. No SDL types here.

use anyhow::{Context, Result, bail};
use tidewater_formats::fnt::Font;
use tidewater_formats::grp::{Frame, Grp};
use tidewater_formats::install::Install;
use tidewater_formats::pud::Era;
use tidewater_formats::raw::{Cursor, RawImage};
use tidewater_formats::tbl::StringTable;
use tidewater_formats::tileset::Palette;

use crate::scene::{Picture, to_rgba};

/// A rectangle in canvas pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: u32, h: u32) -> Self {
        Self { x, y, w, h }
    }

    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x as f32
            && py >= self.y as f32
            && px < (self.x + self.w as i32) as f32
            && py < (self.y + self.h as i32) as f32
    }
}

/// Sizes of the frame pieces, read from the art.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pieces {
    pub panel_w: u32,
    /// Heights of the panel's menu strip, minimap box, info box and button area, top to bottom.
    pub panel_h: [u32; 4],
    /// Thickness of the top, bottom and right bars.
    pub bar: u32,
    /// The minimap's place inside the minimap box.
    pub hole: Rect,
}

/// Where everything goes on a canvas of a given size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub canvas: (u32, u32),
    /// The map view.
    pub viewport: Rect,
    pub minimap: Rect,
    pub info: Rect,
    pub buttons: Rect,
    pub top: Rect,
    /// The bottom bar, where messages go.
    pub bottom: Rect,
}

impl Layout {
    pub fn new(canvas: (u32, u32), p: &Pieces) -> Self {
        let (cw, ch) = canvas;
        let y_info = (p.panel_h[0] + p.panel_h[1]) as i32;
        let y_buttons = y_info + p.panel_h[2] as i32;
        let vw = cw.saturating_sub(p.panel_w + p.bar).max(1);
        let vh = ch.saturating_sub(2 * p.bar).max(1);
        Self {
            canvas,
            viewport: Rect::new(p.panel_w as i32, p.bar as i32, vw, vh),
            minimap: Rect::new(p.hole.x, p.panel_h[0] as i32 + p.hole.y, p.hole.w, p.hole.h),
            info: Rect::new(0, y_info, p.panel_w, p.panel_h[2]),
            buttons: Rect::new(0, y_buttons, p.panel_w, p.panel_h[3]),
            top: Rect::new(p.panel_w as i32, 0, vw, p.bar),
            bottom: Rect::new(p.panel_w as i32, (ch.max(p.bar) - p.bar) as i32, vw, p.bar),
        }
    }
}

/// A unit in the information panel's grid: its type, owner and health.
#[derive(Clone, Copy, Debug)]
pub struct Icon {
    pub kind: u8,
    pub owner: u8,
    pub hp: u16,
    /// The type's maximum hit points.
    pub max_hp: u16,
}

/// What the information panel shows (see [`crate::info`]).
#[derive(Clone, Copy, Debug, Default)]
pub struct InfoView<'a> {
    /// Background frame 0 to 3.
    pub frame: u8,
    pub slots: [Option<Icon>; 9],
    /// The slot whose icon is held down.
    pub pressed: Option<usize>,
    /// The text for one selected unit.
    pub content: Option<&'a crate::info::Content>,
    /// The local player, whose colour the production icon takes.
    pub local: u8,
    /// How many selected units the nine icons do not show (option `selection-limit`): drawn as "+N" on the last icon.
    pub more: usize,
}

/// The changing state the interface shows.
pub struct View<'a> {
    /// The shown gold, lumber and oil (see [`crate::screen::Counter`]).
    pub resources: [i64; 3],
    /// The food item, if drawn.
    pub food: Option<crate::screen::Food>,
    pub info: InfoView<'a>,
    pub status: &'a crate::messages::StatusBar,
    pub messages: &'a crate::messages::Messages,
    /// The game is paused: the pause button shows as a resume button.
    pub paused: bool,
    /// Whether the pause button is shown ([`crate::screen::pause_shown`]).
    pub pause_button: bool,
    /// The pointer shape and the mouse position, if the pointer is drawn.
    pub pointer: Option<(crate::screen::Pointer, (i32, i32))>,
    /// The last minimap picture drawn: 128 × 128 palette indices (see [`crate::minimap`]).
    pub minimap: &'a [u8],
    /// The command panel's shown buttons: slot and icon frame (see [`crate::panel`]).
    pub buttons: &'a [(u8, u16)],
    /// The idle-worker button, with the number of idle workers, when the option is on (`idle`).
    pub idle_button: Option<(usize, &'a str)>,
    /// The game clock line over the map view's top right corner, when the option is on (`overlay`).
    pub clock: Option<&'a str>,
    /// The control-group bar's buttons, none when the option is off (`groupbar`).
    pub group_bar: &'a [crate::groupbar::Cell],
    /// The minimap pings whose marker is on (option `minimap-ping`, `ping`).
    pub pings: &'a [PingMark],
}

/// A ping marker on the minimap: the pixel of its tile (0 to 127 each way) and its player's minimap colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PingMark {
    pub x: i32,
    pub y: i32,
    pub colour: u8,
}

// --- drawing primitives -------------------------------------------------------------------------------------------

/// Copies `src` to `(x, y)`, skipping fully transparent pixels and clipping to `dst`.
pub fn blit(dst: &mut Picture, x: i32, y: i32, src: &Picture) {
    for sy in 0..src.height as i32 {
        let dy = y + sy;
        if dy < 0 || dy >= dst.height as i32 {
            continue;
        }
        for sx in 0..src.width as i32 {
            let dx = x + sx;
            if dx < 0 || dx >= dst.width as i32 {
                continue;
            }
            let s = ((sy as u32 * src.width + sx as u32) * 4) as usize;
            if src.rgba[s + 3] == 0 {
                continue;
            }
            let d = ((dy as u32 * dst.width + dx as u32) * 4) as usize;
            dst.rgba[d..d + 4].copy_from_slice(&src.rgba[s..s + 4]);
        }
    }
}

/// Fills a rectangle, clipped to `dst`.
pub fn fill(dst: &mut Picture, r: Rect, c: [u8; 3]) {
    let x0 = r.x.max(0) as u32;
    let y0 = r.y.max(0) as u32;
    let x1 = ((r.x + r.w as i32).max(0) as u32).min(dst.width);
    let y1 = ((r.y + r.h as i32).max(0) as u32).min(dst.height);
    for y in y0..y1 {
        for x in x0..x1 {
            let d = ((y * dst.width + x) * 4) as usize;
            dst.rgba[d..d + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
}

/// A one-pixel outline.
pub fn outline(dst: &mut Picture, r: Rect, c: [u8; 3]) {
    if r.w == 0 || r.h == 0 {
        return;
    }
    fill(dst, Rect::new(r.x, r.y, r.w, 1), c);
    fill(dst, Rect::new(r.x, r.y + r.h as i32 - 1, r.w, 1), c);
    fill(dst, Rect::new(r.x, r.y, 1, r.h), c);
    fill(dst, Rect::new(r.x + r.w as i32 - 1, r.y, 1, r.h), c);
}

/// Text colours by font shade (see `fnt`): a light face fading to grey, and a black shadow.
pub(crate) const TEXT: [[u8; 3]; 8] = [
    [0, 0, 0],
    [252, 252, 236],
    [216, 212, 192],
    [176, 172, 152],
    [136, 132, 116],
    [0, 0, 0],
    [0, 0, 0],
    [0, 0, 0],
];

/// The font's alternate colour (colour code 4), used for bonuses: a yellow face. Our own choice; the spec names the
/// code, not its colours.
pub(crate) const TEXT_ALT: [[u8; 3]; 8] = [
    [0, 0, 0],
    [252, 224, 64],
    [220, 188, 48],
    [180, 152, 36],
    [140, 116, 24],
    [0, 0, 0],
    [0, 0, 0],
    [0, 0, 0],
];

/// Colour code 2 (chat, food in use): a pale green face. Our own choice.
const TEXT_2: [[u8; 3]; 8] = [
    [0, 0, 0],
    [200, 252, 192],
    [168, 216, 160],
    [136, 180, 128],
    [104, 140, 96],
    [0, 0, 0],
    [0, 0, 0],
    [0, 0, 0],
];

/// Colour code 3 (food over the supply): a red face. Our own choice.
const TEXT_3: [[u8; 3]; 8] = [
    [0, 0, 0],
    [252, 96, 72],
    [216, 72, 56],
    [176, 52, 40],
    [136, 36, 28],
    [0, 0, 0],
    [0, 0, 0],
    [0, 0, 0],
];

/// The shade colours of a font colour code (1 to 4).
fn colour_code(code: u8) -> &'static [[u8; 3]; 8] {
    match code {
        2 => &TEXT_2,
        3 => &TEXT_3,
        4 => &TEXT_ALT,
        _ => &TEXT,
    }
}

/// Draws `text` with its top-left at `(x, y)` and returns its width.
pub fn text(dst: &mut Picture, font: &Font, x: i32, y: i32, s: &str) -> u32 {
    text_in(dst, font, x, y, s, &TEXT)
}

/// Draws `text` in the given shade colours.
fn text_in(dst: &mut Picture, font: &Font, x: i32, y: i32, s: &str, colours: &[[u8; 3]; 8]) -> u32 {
    text_clipped(dst, font, (x, y), s, colours, i32::MAX)
}

/// Draws `text` at `at` in the given shade colours, leaving out pixels right of `clip`.
fn text_clipped(dst: &mut Picture, font: &Font, at: (i32, i32), s: &str, colours: &[[u8; 3]; 8], clip: i32) -> u32 {
    let style = TextStyle {
        colours,
        space: None,
        rows: (i32::MIN, i32::MAX),
    };
    set_text(dst, font, at, s, &style, clip)
}

/// How a line of text is set where the defaults don't do: its shade colours (by font shade, see `fnt`), the advance of
/// a space (the font's own if `None`), and the rows it may draw in (inclusive; a glyph crossing either is cut there).
#[derive(Clone, Copy, Debug)]
pub struct TextStyle<'a> {
    pub colours: &'a [[u8; 3]; 8],
    pub space: Option<u32>,
    pub rows: (i32, i32),
}

impl TextStyle<'_> {
    /// The advance of `c` in `font`.
    pub fn advance(&self, font: &Font, c: u8) -> u32 {
        match (c, self.space) {
            (b' ', Some(s)) => s,
            _ => font.advance(c),
        }
    }

    /// The width of `s` set on one line.
    pub fn width(&self, font: &Font, s: &str) -> u32 {
        s.bytes().map(|c| self.advance(font, c)).sum()
    }
}

/// Draws `s` with its cell's top-left at `at` in `style`; returns its width.
pub fn text_styled(dst: &mut Picture, font: &Font, at: (i32, i32), s: &str, style: &TextStyle) -> u32 {
    set_text(dst, font, at, s, style, i32::MAX)
}

fn set_text(dst: &mut Picture, font: &Font, at: (i32, i32), s: &str, style: &TextStyle, clip: i32) -> u32 {
    let (x, y) = at;
    let (top, bottom) = style.rows;
    let mut pen = x;
    for c in s.bytes() {
        if let Some(g) = font.glyph(c) {
            for gy in 0..g.height as i32 {
                for gx in 0..g.width as i32 {
                    let shade = g.shades[(gy * g.width as i32 + gx) as usize];
                    if shade == 0 {
                        continue;
                    }
                    let (dx, dy) = (pen + g.x as i32 + gx, y + g.y as i32 + gy);
                    if dx <= clip
                        && dy >= top
                        && dy <= bottom
                        && dx >= 0
                        && dy >= 0
                        && (dx as u32) < dst.width
                        && (dy as u32) < dst.height
                    {
                        let d = ((dy as u32 * dst.width + dx as u32) * 4) as usize;
                        let col = style.colours[shade as usize];
                        dst.rgba[d..d + 4].copy_from_slice(&[col[0], col[1], col[2], 255]);
                    }
                }
            }
        }
        pen += style.advance(font, c) as i32;
    }
    (pen - x) as u32
}

/// Draws `s` centred in `r`, vertically too.
fn text_centred(dst: &mut Picture, font: &Font, r: Rect, s: &str) {
    let w = font.width(s.as_bytes()) as i32;
    text(dst, font, r.x + (r.w as i32 - w) / 2, text_top(font, r), s);
}

/// The text top that centres a line of `font` in `r`.
fn text_top(font: &Font, r: Rect) -> i32 {
    r.y + (r.h as i32 - font.height as i32) / 2
}

/// The pointer `Art\{name}` in `pal`, with its hot spot, if the install has it.
pub fn load_pointer(install: &mut Install, name: &str, pal: &Palette) -> Option<(Picture, (i32, i32))> {
    pointer_picture(&install.read(&format!("Art\\{name}")).ok()?, pal)
}

/// A pointer file's picture in `pal`, with its hot spot: it has no palette of its own, so its pixels are `pal`'s
/// entries at the indices in the file (index 0 is clear).
pub fn pointer_picture(file: &[u8], pal: &Palette) -> Option<(Picture, (i32, i32))> {
    let c = Cursor::parse(file).ok()?;
    let (w, h) = (c.image.width as u32, c.image.height as u32);
    let pic = indexed(w, h, &c.image.pixels, pal, true, None);
    Some((pic, (c.hotspot.0 as i32, c.hotspot.1 as i32)))
}

fn indexed(width: u32, height: u32, idx: &[u8], pal: &Palette, transparent0: bool, owner: Option<u8>) -> Picture {
    Picture {
        width,
        height,
        rgba: to_rgba(idx, pal, transparent0, owner),
    }
}

fn raw_picture(r: &RawImage, pal: &Palette) -> Picture {
    indexed(r.width as u32, r.height as u32, &r.pixels, pal, false, None)
}

fn frame_picture(f: &Frame, pal: &Palette, owner: Option<u8>) -> Picture {
    let w = (f.pixels.len() / f.height.max(1) as usize) as u32;
    indexed(w, f.height as u32, &f.pixels, pal, true, owner)
}

// --- the art ------------------------------------------------------------------------------------------------------

pub struct HudArt {
    pal: Palette,
    top: Picture,
    bottom: Picture,
    right: Picture,
    /// Menu strip, minimap box, button area.
    left: [Picture; 3],
    /// The information panel's background frames 0 to 3 (see [`crate::info::frame`]).
    info: [Picture; 4],
    menu_button: Option<Picture>,
    /// The frames of `Art\icons.grp`: gold, lumber, oil, mana, food.
    resources: Vec<Picture>,
    /// The pointers by file name, with their hot spots.
    pointers: std::collections::HashMap<&'static str, (Picture, (i32, i32))>,
    /// `rez\network.tbl`, if the install has it.
    network: StringTable,
    icons: Grp,
    font: Font,
    /// A smaller font for the hit point text, if the install has one.
    small: Option<Font>,
    names: StringTable,
    /// The local race is orc: the race pointers' orc versions.
    orc: bool,
    pub pieces: Pieces,
}

/// Candidate names of a smaller font, tried in order.
const SMALL_FONT: [&str; 1] = ["Art\\font6.fnt"];

/// The unit icon sheet of an era.
fn portrait_sheet(era: Era) -> &'static str {
    match era {
        Era::Forest => "Art\\unit\\Portrait\\portrait.grp",
        Era::Winter => "Art\\unit\\Portrait\\s_port.grp",
        Era::Wasteland => "Art\\unit\\Portrait\\l_port.grp",
        Era::Swamp => "Art\\unit\\Portrait\\x_port.grp",
    }
}

impl HudArt {
    /// Loads the interface art for a race (0 human, otherwise orc) with an era's palette.
    pub fn load(install: &mut Install, era: Era, race: u8, pal: &Palette) -> Result<Self> {
        let p = if race == 0 { "h" } else { "o" };
        let mut raw = |name: &str| -> Result<RawImage> {
            let path = format!("Art\\{p}_{name}.bmp");
            RawImage::parse(&install.read(&path)?).with_context(|| path.clone())
        };
        let (top, bottom, right) = (raw("top")?, raw("btm")?, raw("rgt")?);
        let left = [raw("lft0")?, raw("lft1")?, raw("lft2")?];
        let frame_name = format!("Art\\{p}frame.grp");
        let frames = Grp::parse_uncompressed(&install.read(&frame_name)?).context(frame_name)?;
        if frames.frames.len() < 2 {
            bail!("info box sheet has {} frames", frames.frames.len());
        }
        let race_sheet = if race == 0 { "art\\human.grp" } else { "art\\orc.grp" };
        let menu_button = install
            .read(race_sheet)
            .ok()
            .and_then(|d| Grp::parse_uncompressed(&d).ok())
            .and_then(|g| g.frames.get(48).map(|f| frame_picture(f, pal, None)));
        let res = Grp::parse_uncompressed(&install.read("Art\\icons.grp")?).context("icons.grp")?;
        let icons = Grp::parse(&install.read(portrait_sheet(era))?).context("portrait sheet")?;
        let font = Font::parse(&install.read("Art\\font10x.fnt")?).context("font10x.fnt")?;
        let names = StringTable::parse(&install.read("rez\\stat_txt.tbl")?).context("stat_txt.tbl")?;
        let network = install
            .read("rez\\network.tbl")
            .ok()
            .and_then(|d| StringTable::parse(&d).ok())
            .unwrap_or_default();
        let mut pointers = std::collections::HashMap::new();
        for ptr in crate::screen::Pointer::all() {
            let name = ptr.file(race != 0);
            if let Some(p) = load_pointer(install, name, pal) {
                pointers.insert(name, p);
            }
        }

        // The minimap is x 24 to 151, y 26 to 153 on the screen, 2 pixels below the minimap box's top (screen spec).
        let hole = Rect::new(
            crate::minimap::ORIGIN.0,
            2,
            crate::minimap::SIZE as u32,
            crate::minimap::SIZE as u32,
        );
        let info_h = frames.height as u32;
        let pieces = Pieces {
            panel_w: left[0].width as u32,
            panel_h: [
                left[0].height as u32,
                left[1].height as u32,
                info_h,
                left[2].height as u32,
            ],
            bar: top.height as u32,
            hole,
        };
        Ok(Self {
            top: raw_picture(&top, pal),
            bottom: raw_picture(&bottom, pal),
            right: raw_picture(&right, pal),
            left: left.map(|r| raw_picture(&r, pal)),
            // A sheet with fewer than four frames repeats its last one.
            info: [0, 1, 2, 3].map(|n| frame_picture(&frames.frames[n.min(frames.frames.len() - 1)], pal, None)),
            menu_button,
            resources: res.frames.iter().take(5).map(|f| frame_picture(f, pal, None)).collect(),
            pointers,
            network,
            icons,
            font,
            small: SMALL_FONT
                .iter()
                .find_map(|n| install.read(n).ok().and_then(|d| Font::parse(&d).ok())),
            names,
            orc: race != 0,
            pal: pal.clone(),
            pieces,
        })
    }

    pub fn layout(&self, canvas: (u32, u32)) -> Layout {
        Layout::new(canvas, &self.pieces)
    }

    /// String `n` of `stat_txt.tbl`, counting from 1 as the specs do.
    pub fn string(&self, n: u16) -> String {
        self.names.text((n as usize).wrapping_sub(1)).unwrap_or_default()
    }

    /// The width of `s` set on one line in the message font (the message list wraps with it).
    pub fn text_width(&self, s: &str) -> u32 {
        self.font.width(s.as_bytes())
    }

    /// Entry `n` of `rez\network.tbl`, counting from 1.
    pub fn net_string(&self, n: u16) -> String {
        self.network.text((n as usize).wrapping_sub(1)).unwrap_or_default()
    }

    /// Pointer `p` of the local race, with its hot spot, if the install has it.
    pub fn pointer(&self, p: crate::screen::Pointer) -> Option<&(Picture, (i32, i32))> {
        self.pointers.get(p.file(self.orc))
    }

    /// Whether every pointer of the race was found, so the client can draw the pointer itself.
    pub fn has_pointers(&self) -> bool {
        self.pointers.len() == crate::screen::Pointer::all().len()
    }

    /// The parts that only change with the canvas size; the map view is left transparent.
    pub fn frame(&self, l: &Layout) -> Picture {
        let (cw, ch) = l.canvas;
        let mut pic = Picture {
            width: cw,
            height: ch,
            rgba: vec![0; (cw * ch * 4) as usize],
        };
        let px = l.viewport.x;
        let right_x = l.viewport.x + l.viewport.w as i32;
        // Bars: repeat each piece along its length.
        let mut x = px;
        while x < right_x {
            blit(&mut pic, x, 0, &self.top);
            blit(&mut pic, x, ch as i32 - self.bottom.height as i32, &self.bottom);
            x += self.top.width.max(1) as i32;
        }
        let mut y = 0;
        while y < ch as i32 {
            blit(&mut pic, right_x, y, &self.right);
            y += self.right.height.max(1) as i32;
        }
        // The panel, then plain info-box stone below it on tall canvases.
        blit(&mut pic, 0, 0, &self.left[0]);
        blit(&mut pic, 0, self.pieces.panel_h[0] as i32, &self.left[1]);
        blit(&mut pic, l.info.x, l.info.y, &self.info[1]);
        blit(&mut pic, l.buttons.x, l.buttons.y, &self.left[2]);
        let mut y = l.buttons.y + l.buttons.h as i32;
        while y < ch as i32 {
            blit(&mut pic, 0, y, &self.info[0]);
            y += self.info[0].height.max(1) as i32;
        }
        pic
    }

    /// The menu and pause buttons (screen spec, "Menu buttons"); the labels are our own wording.
    fn draw_menu_buttons(&self, pic: &mut Picture, paused: bool, pause_button: bool) {
        use crate::screen::MenuButton;
        for (b, label) in [
            (MenuButton::Menu, "Menu (F10)"),
            (MenuButton::Pause, if paused { "Resume" } else { "Pause" }),
        ] {
            if b == MenuButton::Pause && !pause_button {
                continue;
            }
            let r = b.rect();
            match &self.menu_button {
                Some(p) => blit(
                    pic,
                    r.x + (r.w as i32 - p.width as i32) / 2,
                    r.y + (r.h as i32 - p.height as i32) / 2,
                    p,
                ),
                None => {
                    fill(pic, Rect::new(r.x, r.y + 2, r.w, r.h - 4), [40, 36, 28]);
                    outline(pic, Rect::new(r.x, r.y + 2, r.w, r.h - 4), [140, 120, 60]);
                }
            }
            text_centred(pic, &self.font, r, label);
        }
    }

    /// The idle-worker button (`idle`): a plain button in the resource bar, dark when no worker is idle. `key` is the
    /// key that does the same (F6, or the player's own, `keymap`).
    fn draw_idle_button(&self, pic: &mut Picture, idle: usize, key: &str) {
        let r = crate::screen::IDLE_BUTTON;
        fill(pic, r, if idle > 0 { [40, 36, 28] } else { [24, 24, 24] });
        outline(pic, r, [140, 120, 60]);
        text_centred(pic, &self.font, r, &format!("Idle {idle} ({key})"));
    }

    /// The control-group bar (`groupbar`): a button for each group with units, in the map view's top left corner.
    fn draw_group_bar(&self, pic: &mut Picture, view: Rect, cells: &[crate::groupbar::Cell]) {
        for (n, &cell) in cells.iter().enumerate() {
            let r = crate::groupbar::cell_rect(view, n);
            fill(pic, r, [40, 36, 28]);
            outline(pic, r, [140, 120, 60]);
            text_centred(pic, &self.font, r, &crate::groupbar::label(cell));
        }
    }

    /// The clock line (`overlay`): right-aligned at the top of the map view.
    fn draw_clock(&self, pic: &mut Picture, view: Rect, line: &str) {
        let w = self.font.width(line.as_bytes()) as i32;
        text(pic, &self.font, view.x + view.w as i32 - w - 6, view.y + 4, line);
    }

    /// One item of the resource bar or the status bar: the icon at the item's left edge `x` in `bar`, then the text
    /// [`crate::screen::TEXT_DX`] to the right, vertically centred, in spans of colour codes.
    fn item(&self, pic: &mut Picture, bar: Rect, x: i32, icon: usize, spans: &[(&str, u8)]) {
        if let Some(i) = self.resources.get(icon) {
            blit(pic, x, bar.y + (bar.h as i32 - i.height as i32) / 2, i);
        }
        let mut pen = x + crate::screen::TEXT_DX;
        for &(s, code) in spans {
            pen += text_in(pic, &self.font, pen, text_top(&self.font, bar), s, colour_code(code)) as i32;
        }
    }

    /// The resource bar (screen spec, "Resource bar"): single-player positions.
    fn draw_resources(&self, pic: &mut Picture, bar: Rect, v: &View) {
        use crate::screen::{Item, amount_text};
        for (item, amount) in Item::ALL.iter().zip(v.resources) {
            let x = bar.x + item.area(false).0;
            self.item(pic, bar, x, item.icon(), &[(&amount_text(amount), 1)]);
        }
        if let Some(f) = v.food {
            let (used, rest) = f.colours();
            let x = bar.x + Item::Food.area(false).0;
            let (u, s) = (f.used.to_string(), format!("/{}", f.supply));
            self.item(pic, bar, x, Item::Food.icon(), &[(&u, used), (&s, rest)]);
        }
    }

    /// The status bar (messages spec, "The status bar").
    fn draw_status(&self, pic: &mut Picture, bar: Rect, s: &crate::messages::StatusBar) {
        let at = (bar.x + crate::messages::STATUS_TEXT_X, text_top(&self.font, bar));
        if let Some(line) = &s.chat {
            // Messages spec, "Chat line": a label and the text field instead of the text and the costs.
            let clip = bar.x + bar.w as i32 - crate::messages::STATUS_TEXT_X;
            text_clipped(pic, &self.font, at, &crate::chat::shown(line), &TEXT, clip);
            return;
        }
        let clip = bar.x + s.text_right();
        text_clipped(pic, &self.font, at, &s.text, &TEXT, clip);
        for (cost, value) in s.shown() {
            let ((x0, _), icon) = cost.place();
            self.item(pic, bar, bar.x + x0, icon, &[(&value.to_string(), 1)]);
        }
    }

    /// The message area (messages spec): the list and the notice line, kept at the same distance from the map
    /// view's bottom-left as on the 640 x 480 screen.
    fn draw_messages(&self, pic: &mut Picture, view: Rect, m: &crate::messages::Messages) {
        use crate::messages::{CLIP_RIGHT, LEFT};
        let bottom = view.y + view.h as i32;
        let right = view.x + view.w as i32;
        for (y, line) in m.lines() {
            let at = (view.x + (LEFT - 176), bottom - (464 - y));
            let clip = right - (624 - CLIP_RIGHT);
            text_clipped(pic, &self.font, at, &line.text, colour_code(line.colour), clip);
        }
    }

    /// The pointer, its hot spot on the mouse position.
    fn draw_pointer(&self, pic: &mut Picture, p: crate::screen::Pointer, at: (i32, i32)) {
        if let Some((img, hot)) = self.pointer(p) {
            blit(pic, at.0 - hot.0, at.1 - hot.1, img);
        }
    }

    /// Draws icon frame `n` of the era's icon set in `owner`'s colour inside a button frame, with its top-left at
    /// `(x, y)`. The frame is our own drawing (a dark box with a gold edge), as the command buttons use.
    fn button(&self, pic: &mut Picture, x: i32, y: i32, n: u16, owner: u8) {
        let Some(f) = self.icons.frames.get(n as usize) else {
            return;
        };
        let p = frame_picture(f, &self.pal, Some(owner));
        let r = Rect::new(x - 2, y - 2, p.width + 4, p.height + 4);
        fill(pic, r, [24, 24, 24]);
        blit(pic, x, y, &p);
        outline(pic, r, [140, 120, 60]);
    }

    /// Draws icon frame `n` at a quarter of its size (every fourth pixel) with its top-left at `(x, y)`, in a thin
    /// frame, for the production queue.
    fn small_icon(&self, pic: &mut Picture, x: i32, y: i32, n: u16, owner: u8) {
        let Some(f) = self.icons.frames.get(n as usize) else {
            return;
        };
        let p = frame_picture(f, &self.pal, Some(owner));
        let (w, h) = (p.width as i32 / 4, p.height as i32 / 4);
        outline(pic, Rect::new(x - 1, y - 1, w as u32 + 2, h as u32 + 2), [140, 120, 60]);
        fill(pic, Rect::new(x, y, w as u32, h as u32), [24, 24, 24]);
        for sy in 0..h {
            for sx in 0..w {
                let from = ((sy * 4 + 2) as usize * p.width as usize + (sx * 4 + 2) as usize) * 4;
                let Some(px) = p.rgba.get(from..from + 4) else { continue };
                if px[3] == 0 {
                    continue;
                }
                let (dx, dy) = (x + sx, y + sy);
                if dx < 0 || dy < 0 || dx as u32 >= pic.width || dy as u32 >= pic.height {
                    continue;
                }
                let to = (dy as usize * pic.width as usize + dx as usize) * 4;
                pic.rgba[to..to + 4].copy_from_slice(px);
            }
        }
    }

    /// Uses the cycled palette (`palette`) from now on: the minimap and the icons drawn later show its colours.
    pub fn set_palette(&mut self, pal: &Palette) {
        self.pal.clone_from(pal);
    }

    /// A palette colour of the era.
    fn colour(&self, n: u8) -> [u8; 3] {
        self.pal.0[n as usize]
    }

    /// Draws the information panel with its top-left corner at `o` (spec `ui/info-panel.md`).
    fn draw_info(&self, pic: &mut Picture, o: (i32, i32), v: &InfoView) {
        use crate::info;
        let at = |r: Rect| Rect::new(o.0 + r.x, o.1 + r.y, r.w, r.h);
        blit(pic, o.0, o.1, &self.info[(v.frame as usize).min(3)]);
        // Open question: the background colour of the health bar strip and the mana bar's inner outline is assumed
        // black.
        let background = [0, 0, 0];
        for (n, slot) in v.slots.iter().enumerate() {
            let Some(s) = slot else { continue };
            let r = at(info::slot_rect(n));
            let d = if v.pressed == Some(n) { 5 } else { 4 };
            if let Some(i) = info::icon(s.kind) {
                self.button(pic, r.x + d, r.y + d, i, s.owner);
            }
            fill(pic, Rect::new(r.x, r.y + 46, 54, 7), background);
            let (filled, c) = info::health_bar(s.hp, s.max_hp);
            fill(pic, Rect::new(r.x + 2, r.y + 46, 50, 5), self.colour(99));
            fill(pic, Rect::new(r.x + 2, r.y + 46, filled, 5), self.colour(c));
        }
        if v.more > 0 {
            // Option `selection-limit`: "+N" in the corner of the last icon for the units without an icon of their own.
            let label = format!("+{}", v.more);
            let r = at(info::slot_rect(info::SLOTS - 1));
            let w = self.font.width(label.as_bytes());
            let (x, y) = (r.x + r.w as i32 - w as i32 - 4, r.y + 28);
            fill(pic, Rect::new(x - 2, y - 1, w + 4, 14), [24, 24, 24]);
            text(pic, &self.font, x, y, &label);
        }
        let Some(c) = v.content else { return };
        let font = &self.font;
        if let Some(n) = c.name {
            let (l1, l2) = info::name_lines(&self.string(n), &|b| font.width(b));
            text_centred(pic, font, at(info::NAME[0]), &l1);
            text_centred(pic, font, at(info::NAME[1]), &l2);
        }
        if let Some(n) = c.level {
            text_centred(pic, font, at(info::LEVEL), &format!("{} {n}", self.string(411)));
        }
        if let Some((hp, max)) = c.hp {
            // Open question: the hit point text's style probably selects a smaller font; the small font is used
            // when the install has it.
            let small = self.small.as_ref().unwrap_or(font);
            // Ours: "1200/1200" is wider than the area, so the centred text is kept inside the panel's frame.
            let (r, s) = (at(info::HP), format!("{hp}/{max}"));
            let w = small.width(s.as_bytes()) as i32;
            let left = (r.x + (r.w as i32 - w) / 2).max(o.0 + 3);
            text(pic, small, left, text_top(small, r), &s);
        }
        for (n, row) in c.rows.iter().enumerate() {
            let Some(row) = row else { continue };
            // Open question: labels are right-aligned in their area; values start at its left edge.
            let lr = at(info::label_rect(n));
            let label = self.string(row.label);
            let w = font.width(label.as_bytes()) as i32;
            text(pic, font, lr.x + lr.w as i32 - w, text_top(font, lr), &label);
            if c.production.is_some() {
                continue;
            }
            let vr = at(info::value_rect(n));
            match &row.value {
                info::Value::Empty => {}
                info::Value::Text(spans) => {
                    let mut x = vr.x;
                    for s in spans {
                        let colours = if s.alt { &TEXT_ALT } else { &TEXT };
                        x += text_in(pic, font, x, text_top(font, vr), &s.text, colours) as i32;
                    }
                }
                info::Value::Mana(m) => {
                    outline(pic, vr, self.colour(246));
                    outline(pic, Rect::new(vr.x + 1, vr.y + 1, vr.w - 2, vr.h - 2), background);
                    let bar = at(info::MANA_BAR);
                    fill(pic, bar, self.colour(99));
                    fill(
                        pic,
                        Rect::new(bar.x, bar.y, info::mana_fill(*m), bar.h),
                        self.colour(186),
                    );
                    text_centred(pic, font, bar, &m.to_string());
                }
            }
        }
        if let Some(i) = c.production {
            let r = at(info::PRODUCTION);
            self.button(pic, r.x + 4, r.y + 4, i, v.local);
            // Production queue (`docs/design/extras.md`): the waiting jobs as quarter-size icons under the running one.
            for (n, icon) in c.waiting.iter().flatten().enumerate() {
                let q = info::QUEUE_ICON;
                let (x, y) = (r.x + q.x + n as i32 * (q.w as i32 + info::QUEUE_GAP), r.y + q.y);
                self.small_icon(pic, x, y, *icon, v.local);
            }
        }
        if let Some(p) = c.progress {
            let r = at(info::PROGRESS);
            fill(pic, Rect::new(r.x, r.y, info::progress_fill(p), r.h), self.colour(149));
            text_centred(pic, font, r, &self.string(434));
        }
    }

    /// The whole interface: `frame` plus the minimap, resources and selection.
    pub fn draw(&self, frame: &Picture, l: &Layout, v: &View) -> Picture {
        let mut pic = Picture {
            width: frame.width,
            height: frame.height,
            rgba: frame.rgba.clone(),
        };
        let mini = Picture {
            width: crate::minimap::SIZE as u32,
            height: crate::minimap::SIZE as u32,
            rgba: to_rgba(v.minimap, &self.pal, false, None),
        };
        if v.minimap.len() == crate::minimap::SIZE * crate::minimap::SIZE {
            blit(&mut pic, l.minimap.x, l.minimap.y, &mini);
        }

        for m in v.pings {
            // A ring in the sender's colour round a white centre, over the minimap's pixels.
            let c = to_rgba(&[m.colour], &self.pal, false, None);
            let (cx, cy) = (l.minimap.x + m.x, l.minimap.y + m.y);
            outline(&mut pic, Rect::new(cx - 4, cy - 4, 9, 9), [c[0], c[1], c[2]]);
            outline(&mut pic, Rect::new(cx - 3, cy - 3, 7, 7), [c[0], c[1], c[2]]);
            fill(&mut pic, Rect::new(cx - 1, cy - 1, 3, 3), [255, 255, 255]);
        }
        self.draw_resources(&mut pic, l.top, v);
        self.draw_status(&mut pic, l.bottom, v.status);
        self.draw_menu_buttons(&mut pic, v.paused, v.pause_button);
        self.draw_messages(&mut pic, l.viewport, v.messages);
        if let Some((n, key)) = v.idle_button {
            self.draw_idle_button(&mut pic, n, key);
        }
        self.draw_group_bar(&mut pic, l.viewport, v.group_bar);
        if let Some(line) = v.clock {
            self.draw_clock(&mut pic, l.viewport, line);
        }

        // The command panel: frames of the icon sheet (the command icons share the unit icon sheet).
        for &(slot, icon) in v.buttons {
            let r = crate::panel::slot_rect((l.buttons.x, l.buttons.y), slot as usize);
            fill(&mut pic, r, [24, 24, 24]);
            if let Some(f) = self.icons.frames.get(icon as usize) {
                let p = frame_picture(f, &self.pal, None);
                blit(
                    &mut pic,
                    r.x + (r.w as i32 - p.width as i32) / 2,
                    r.y + (r.h as i32 - p.height as i32) / 2,
                    &p,
                );
            }
            outline(&mut pic, r, [140, 120, 60]);
        }

        self.draw_info(&mut pic, (l.info.x, l.info.y), &v.info);
        if let Some((p, at)) = v.pointer {
            self.draw_pointer(&mut pic, p, at);
        }
        pic
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pieces() -> Pieces {
        Pieces {
            panel_w: 176,
            panel_h: [24, 136, 176, 144],
            bar: 16,
            hole: Rect::new(24, 2, 128, 128),
        }
    }

    #[test]
    fn layout_of_the_original_screen() {
        let l = Layout::new((640, 480), &pieces());
        assert_eq!(l.viewport, Rect::new(176, 16, 448, 448));
        assert_eq!(l.minimap, Rect::new(24, 26, 128, 128));
        assert_eq!(l.info, Rect::new(0, 160, 176, 176));
        assert_eq!(l.buttons, Rect::new(0, 336, 176, 144));
        assert_eq!(l.top, Rect::new(176, 0, 448, 16));
        assert_eq!(l.bottom, Rect::new(176, 464, 448, 16));
        assert!(l.viewport.contains(176.0, 16.0));
        assert!(!l.viewport.contains(175.9, 100.0));
        assert!(!l.viewport.contains(624.0, 100.0));
    }

    /// Widescreen (`crate::widescreen`): a wider canvas gives the view the extra width, whole tiles of it, and leaves the
    /// interface where it was against the left edge.
    #[test]
    fn layout_of_a_wide_screen_only_widens_the_view_and_its_bars() {
        let original = Layout::new((640, 480), &pieces());
        for (width, tiles) in [(768, 18), (864, 21)] {
            let l = Layout::new((width, 480), &pieces());
            assert_eq!(l.viewport, Rect::new(176, 16, tiles * 32, 448), "{width}");
            assert_eq!(l.viewport.w % 32, 0, "whole tiles");
            assert_eq!(l.minimap, original.minimap);
            assert_eq!(l.info, original.info);
            assert_eq!(l.buttons, original.buttons);
            assert_eq!(l.top, Rect::new(176, 0, tiles * 32, 16));
            assert_eq!(l.bottom, Rect::new(176, 464, tiles * 32, 16));
            assert_eq!(l.canvas, (width, 480));
        }
    }

    #[test]
    fn blit_skips_transparent_and_clips() {
        let mut dst = Picture {
            width: 2,
            height: 1,
            rgba: vec![9; 8],
        };
        let src = Picture {
            width: 2,
            height: 1,
            rgba: vec![1, 1, 1, 255, 2, 2, 2, 0],
        };
        blit(&mut dst, 1, 0, &src);
        assert_eq!(dst.rgba, vec![9, 9, 9, 9, 1, 1, 1, 255]);
        blit(&mut dst, -1, 0, &src);
        assert_eq!(dst.rgba, vec![9, 9, 9, 9, 1, 1, 1, 255]);
    }

    #[test]
    fn panel_names_split_in_a_real_font() {
        // A font where every character is 8 pixels wide (7 + 1 spacing).
        let mut d = b"FONT".to_vec();
        d.extend([b'a', b'z', 7, 1]);
        let first = 8 + 26 * 4;
        for i in 0..26u32 {
            d.extend((first as u32 + i * 11).to_le_bytes());
        }
        for _ in 0..26 {
            d.extend([7, 1, 0, 0, 1, 1, 1, 1, 1, 1, 1]);
        }
        let font = Font::parse(&d).unwrap();
        let width = |b: &[u8]| font.width(b);
        // 13 characters, 104 px: one line.
        let lines = crate::info::name_lines("abcdefghijklm", &width);
        assert_eq!(lines, (String::new(), "abcdefghijklm".to_string()));
        let lines = crate::info::name_lines("abcdef ghijklmnop", &width);
        assert_eq!(lines, ("abcdef".to_string(), "ghijklmnop".to_string()));
    }

    #[test]
    fn styled_text_takes_its_space_and_is_cut_at_its_rows() {
        // One 2 × 4 glyph of shade 1 at offset (0, 0) for every character from '!'; the space has none.
        let g = tidewater_formats::fnt::Glyph {
            width: 2,
            height: 4,
            x: 0,
            y: 0,
            shades: vec![1; 8],
        };
        let mut glyphs = vec![None];
        glyphs.extend((33..=126).map(|_| Some(g.clone())));
        let font = Font {
            first: 32,
            max_width: 2,
            height: 6,
            glyphs,
        };
        let mut colours = [[0u8; 3]; 8];
        colours[1] = [9, 9, 9];
        let style = TextStyle {
            colours: &colours,
            space: Some(9),
            rows: (11, 12),
        };
        // 'a' advances 3 (2 + 1), the space 9: a 1 + 9 = 10-pixel gap before 'b'.
        assert_eq!(style.width(&font, "a b"), 3 + 9 + 3);
        let mut p = Picture {
            width: 20,
            height: 20,
            rgba: vec![0; 20 * 20 * 4],
        };
        assert_eq!(text_styled(&mut p, &font, (0, 10), "a b", &style), 15);
        let inked = |x: u32, y: u32| p.rgba[((y * 20 + x) * 4 + 3) as usize] != 0;
        assert!(
            !inked(0, 10) && inked(0, 11) && inked(0, 12) && !inked(0, 13),
            "only rows 11 and 12"
        );
        assert!(
            inked(12, 11) && !inked(11, 11) && !inked(3, 11),
            "'b' 10 pixels after 'a'"
        );
    }
}
