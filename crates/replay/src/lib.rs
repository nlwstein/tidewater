//! Replays (`docs/design/multiplayer-determinism.md`, "Replays"): the `.twrep` file format ([`format`]), the world a
//! replay starts from ([`start`]) and the headless run that plays one back and checks it ([`run`]). The viewer's timeline, jumping about and
//! vision are in [`seek`].
//!
//! A game is fully described by its start (map, seed, setup, lobby, rules) and the [`tidewater_sim::Command`]s each
//! cycle ran, because the simulation takes no other input. The client records and plays replays through its `Session`
//! trait; `twtool replay` and `twtool desync-diff` use this crate without the client (and so without SDL).

pub mod format;
pub mod run;
pub mod seek;
pub mod start;

pub use format::{Checkpoint, CycleCommands, Header, Replay, ReplayError, RulesTag};
