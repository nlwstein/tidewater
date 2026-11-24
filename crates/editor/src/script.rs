//! Scripted actions (`--actions`): a development aid that drives the editor without a person, for demonstrations
//! and screenshots. Pointer actions are fed in as real input events, so they go through the same code as the mouse.
//!
//! The script is a list of commands separated by `;`, each a word and its arguments. Positions are in tiles
//! (fractions allowed; `10.5 4.5` is the middle of tile (10, 4)):
//!
//! | Command | What it does |
//! |---|---|
//! | `click X Y` | Left click at the position |
//! | `drag X1 Y1 X2 Y2` | Left drag from one position to the other |
//! | `hover X Y` | Moves the pointer there |
//! | `key [cmd+][shift+]NAME` | Presses a key (egui's key names: `Z`, `Delete`, `Escape`) |
//! | `view X Y ZOOM` | Centres the view on a position at a zoom (1 is one map pixel per point) |
//! | `tool select`, `tool terrain CODE SIZE`, `tool unit KIND` | Picks a tool (`KIND` in decimal or `0x` hex) |
//! | `player N` | Picks the player the palette places units for (1 to 8, or 16 for neutral) |
//! | `window NAME` | Opens a window: `players`, `map`, `new`, `open`, `save` |
//! | `wait N` | Waits N frames |
//! | `save PATH` | Saves the map there |
//! | `shot PATH` | Writes the window's picture to a PNG file |
//! | `play` | Plays the map in Tidewater, as the button does |
//! | `quit` | Closes the editor |

use std::collections::VecDeque;
use std::path::PathBuf;

use eframe::egui::{self, Key, Modifiers, PointerButton, Pos2};

/// A command of the script.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Click(f32, f32),
    Drag(f32, f32, f32, f32),
    Hover(f32, f32),
    Key(Key, Modifiers),
    View(f32, f32, f32),
    ToolSelect,
    ToolTerrain(u8, i32),
    ToolUnit(u8),
    Player(u8),
    Window(String),
    Wait(u32),
    Save(PathBuf),
    Shot(PathBuf),
    Play,
    Quit,
}

fn num<T: std::str::FromStr>(w: Option<&str>, cmd: &str) -> Result<T, String> {
    w.and_then(|s| s.parse().ok())
        .ok_or_else(|| format!("`{cmd}`: missing or bad number"))
}

fn kind(w: Option<&str>, cmd: &str) -> Result<u8, String> {
    let s = w.ok_or_else(|| format!("`{cmd}`: missing type"))?;
    match s.strip_prefix("0x") {
        Some(h) => u8::from_str_radix(h, 16).map_err(|e| format!("`{cmd}`: {e}")),
        None => s.parse().map_err(|e| format!("`{cmd}`: {e}")),
    }
}

fn key(spec: &str) -> Result<(Key, Modifiers), String> {
    let mut m = Modifiers::NONE;
    let mut rest = spec;
    loop {
        if let Some(r) = rest.strip_prefix("cmd+") {
            m |= Modifiers::COMMAND;
            rest = r;
        } else if let Some(r) = rest.strip_prefix("shift+") {
            m |= Modifiers::SHIFT;
            rest = r;
        } else {
            break;
        }
    }
    Key::from_name(rest)
        .map(|k| (k, m))
        .ok_or_else(|| format!("unknown key `{rest}`"))
}

/// Reads a script.
pub fn parse(s: &str) -> Result<Vec<Action>, String> {
    let mut out = Vec::new();
    for cmd in s.split(';').map(str::trim).filter(|c| !c.is_empty()) {
        let mut w = cmd.split_whitespace();
        let word = w.next().unwrap_or_default();
        let mut f = || num::<f32>(w.next(), cmd);
        let a = match word {
            "click" => Action::Click(f()?, f()?),
            "hover" => Action::Hover(f()?, f()?),
            "drag" => Action::Drag(f()?, f()?, f()?, f()?),
            "view" => Action::View(f()?, f()?, f()?),
            _ => {
                let mut w = cmd.split_whitespace().skip(1);
                match word {
                    "key" => {
                        let (k, m) = key(w.next().ok_or("`key`: missing key")?)?;
                        Action::Key(k, m)
                    }
                    "tool" => match w.next() {
                        Some("select") => Action::ToolSelect,
                        Some("terrain") => Action::ToolTerrain(num(w.next(), cmd)?, num(w.next(), cmd)?),
                        Some("unit") => Action::ToolUnit(kind(w.next(), cmd)?),
                        _ => return Err(format!("`{cmd}`: tool is select, terrain or unit")),
                    },
                    "player" => Action::Player(num::<u8>(w.next(), cmd)?.saturating_sub(1)),
                    "window" => Action::Window(w.next().ok_or("`window`: missing name")?.to_string()),
                    "wait" => Action::Wait(num(w.next(), cmd)?),
                    "save" => Action::Save(w.next().ok_or("`save`: missing path")?.into()),
                    "shot" => Action::Shot(w.next().ok_or("`shot`: missing path")?.into()),
                    "play" => Action::Play,
                    "quit" => Action::Quit,
                    _ => return Err(format!("unknown command `{word}`")),
                }
            }
        };
        out.push(a);
    }
    Ok(out)
}

/// One frame's worth of a script.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// The pointer moves to a tile position, then optionally a button goes down or up.
    Pointer(f32, f32, Option<bool>),
    Key(Key, Modifiers),
    /// An action the editor carries out itself.
    App(Action),
    Wait(u32),
}

/// Runs a script a step per frame.
#[derive(Clone, Debug, Default)]
pub struct Runner {
    steps: VecDeque<Step>,
}

impl Runner {
    pub fn new(actions: Vec<Action>) -> Self {
        let mut steps = VecDeque::new();
        for a in actions {
            match a {
                Action::Click(x, y) => {
                    steps.extend([
                        Step::Pointer(x, y, None),
                        Step::Pointer(x, y, Some(true)),
                        Step::Pointer(x, y, Some(false)),
                    ]);
                }
                Action::Hover(x, y) => steps.push_back(Step::Pointer(x, y, None)),
                Action::Drag(x1, y1, x2, y2) => {
                    steps.extend([Step::Pointer(x1, y1, None), Step::Pointer(x1, y1, Some(true))]);
                    let n = 12;
                    for i in 1..=n {
                        let t = i as f32 / n as f32;
                        steps.push_back(Step::Pointer(x1 + (x2 - x1) * t, y1 + (y2 - y1) * t, None));
                    }
                    steps.push_back(Step::Pointer(x2, y2, Some(false)));
                }
                Action::Key(k, m) => steps.push_back(Step::Key(k, m)),
                Action::Wait(n) => steps.push_back(Step::Wait(n)),
                // A shot waits for the frame to settle first.
                Action::Shot(p) => steps.extend([Step::Wait(3), Step::App(Action::Shot(p)), Step::Wait(5)]),
                a => steps.push_back(Step::App(a)),
            }
        }
        Self { steps }
    }

    pub fn is_running(&self) -> bool {
        !self.steps.is_empty()
    }

    /// This frame's step.
    pub fn step(&mut self) -> Option<Step> {
        if let Some(Step::Wait(n)) = self.steps.front_mut() {
            if *n > 0 {
                *n -= 1;
                return None;
            }
            self.steps.pop_front();
        }
        self.steps.pop_front()
    }
}

/// The input events a pointer or key step adds, at screen position `pos`.
pub fn events(step: &Step, pos: Pos2) -> Vec<egui::Event> {
    match *step {
        Step::Pointer(_, _, button) => {
            let mut v = vec![egui::Event::PointerMoved(pos)];
            if let Some(pressed) = button {
                v.push(egui::Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed,
                    modifiers: Modifiers::NONE,
                });
            }
            v
        }
        Step::Key(key, modifiers) => [true, false]
            .map(|pressed| egui::Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers,
            })
            .to_vec(),
        _ => Vec::new(),
    }
}

/// Writes a screenshot to a PNG file.
pub fn write_png(path: &std::path::Path, img: &egui::ColorImage) -> anyhow::Result<()> {
    use anyhow::Context;
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let f = std::fs::File::create(path).with_context(|| format!("creating {}", path.display()))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), img.size[0] as u32, img.size[1] as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let data: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
    enc.write_header()?.write_image_data(&data)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_commands() {
        let a = parse("click 1.5 2; tool terrain 7 3; tool unit 0x5c; player 2; key cmd+Z; shot a.png; quit").unwrap();
        assert_eq!(
            a,
            [
                Action::Click(1.5, 2.0),
                Action::ToolTerrain(7, 3),
                Action::ToolUnit(0x5c),
                Action::Player(1),
                Action::Key(Key::Z, Modifiers::COMMAND),
                Action::Shot("a.png".into()),
                Action::Quit
            ]
        );
        assert!(parse("jump 1").is_err());
        assert!(parse("click 1").is_err());
    }

    #[test]
    fn a_click_is_three_frames_and_waits_count_down() {
        let mut r = Runner::new(vec![Action::Click(1.0, 1.0), Action::Wait(2), Action::Quit]);
        assert_eq!(r.step(), Some(Step::Pointer(1.0, 1.0, None)));
        assert_eq!(r.step(), Some(Step::Pointer(1.0, 1.0, Some(true))));
        assert_eq!(r.step(), Some(Step::Pointer(1.0, 1.0, Some(false))));
        assert_eq!((r.step(), r.step()), (None, None));
        assert_eq!(r.step(), Some(Step::App(Action::Quit)));
        assert!(!r.is_running());
    }
}
