//! Tidewater's client core as a library (`docs/design/client-core.md`): everything but a platform shell, with no
//! engine types and no `unsafe` code. The `tidewater` binary (`shell/main.rs`) runs it in an SDL 3 window and starts
//! it with [`host::launch`].

#![forbid(unsafe_code)]

pub mod args;
mod attackmove;
pub mod audio;
mod campaign;
mod carry;
mod chat;
mod cheats;
pub mod chrome;
mod clock;
pub mod compose;
pub mod core;
mod customsetup;
mod diplomacy;
pub mod display;
pub mod extras;
pub mod firstrun;
mod fog;
mod frontend;
mod game;
mod gamemenu;
mod gesture;
mod groupbar;
pub mod host;
mod hud;
mod idle;
mod info;
pub mod input;
mod jukebox;
mod keymap;
mod layers;
mod lobbyscreen;
mod mapview;
mod menu;
mod menuscreen;
mod messages;
mod minimap;
mod mouse;
mod mpmenu;
mod mpwidgets;
#[allow(dead_code)] // the spec's track table and fade helpers are kept whole; some screens don't use them yet
mod music;
pub mod net;
mod overlay;
mod palette;
mod panel;
mod ping;
mod prefs;
mod savedlg;
mod savefile;
mod savegame;
mod saves;
mod scene;
mod screen;
mod screenshot;
pub mod script;
pub mod session;
mod settings;
mod sound;
mod states;
pub mod storage;
pub mod touch;
mod touchemu;
mod touchhelp;
pub mod update;
mod video;
mod view;
pub mod widescreen;
