//! Tidewater's map editor: opens a `.pud` map, shows it with the installation's own tiles and sprites, paints terrain,
//! places units and saves a map the game loads. The window is `app` (and `assets`, its textures); everything else
//! here is plain data and rules, tested without a window. User guide: `docs/editor.md`.

pub mod app;
pub mod assets;
pub mod catalog;
pub mod derived;
pub mod doc;
pub mod history;
pub mod script;
pub mod terrain;
