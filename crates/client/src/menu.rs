//! The game menu's rules (spec `docs/specs/ui/game-menu.md`): which screen is open and where "back" goes, the keys in
//! the menus, the end-scenario choices by kind of game, the objectives entry, and the rules for the values the
//! settings dialogs edit. No SDL types here; `gamemenu` draws it and applies the results.
//!
//! Choices where the spec leaves a gap:
//! - There is no Load-less network menu and no Battle.net rules ([`bnet_quit`] holds the 120-second rule for when those
//!   games exist). The "who may restart" check of a network game is [`host_check`].
//! - A confirmation opened from the End-scenario menu goes back to it on Cancel; one opened by its key goes back to
//!   the game ("Going back").
//! - Keys inside a menu open the next screen as a child (so Previous comes back to the menu); the key table is the
//!   spec's "Key in the menu" column, plus F7, F8 and F9 in the options menu.

use crate::campaign::GameKind;
use crate::mouse::Key;

/// Confirmation dialogs of the end-scenario choices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confirm {
    Restart,
    Surrender,
    /// Battle.net before 120 s; the same outcome as surrender.
    #[allow(dead_code, reason = "Battle.net games don't exist yet")]
    QuitDraw,
    LoadCustom,
    ReturnToChat,
    QuitToMenu,
    Exit,
}

/// Which placeholder a Save or Load button opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Saving {
    Save,
    Load,
}

/// One dialog or menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Game,
    Options,
    Help,
    Sound,
    Speed,
    Prefs,
    Objectives,
    End,
    Confirm(Confirm),
    Saving(Saving),
    KeyHelp,
    /// The Touch Controls help (`touchhelp`, ours).
    TouchHelp,
    Tips,
    /// Tidewater's own Settings screen over the game (`settings`, not in the original).
    Settings,
    /// Network entry 28: only the host, the player in this slot, may make that choice ([`host_check`]).
    Refused(u8),
}

/// The open screens, from the first one opened to the current one. Only the last is shown.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Nav {
    stack: Vec<Screen>,
}

impl Nav {
    pub fn current(&self) -> Option<Screen> {
        self.stack.last().copied()
    }

    /// Opens `s` by its key or button from the game: "back" returns to the game.
    pub fn open(&mut self, s: Screen) {
        self.stack.clear();
        self.stack.push(s);
    }

    /// Opens `s` from the screen that is open: "back" returns to that screen. With nothing open it acts as [`open`].
    pub fn open_child(&mut self, s: Screen) {
        self.stack.push(s);
    }

    /// Previous or Cancel: the screen before, or the game. Returns whether the menus are all closed now.
    pub fn back(&mut self) -> bool {
        self.stack.pop();
        self.stack.is_empty()
    }

    /// OK, or "Return to the game": closes everything.
    pub fn close(&mut self) {
        self.stack.clear();
    }

    /// Replaces the shown screen (a confirmation turned into another one).
    #[allow(dead_code, reason = "used by the Battle.net 120-second swap")]
    pub fn replace(&mut self, s: Screen) {
        self.stack.pop();
        self.stack.push(s);
    }
}

/// The screen a key opens inside a menu ("Key in the menu"). Esc is handled by the caller.
pub fn menu_key(on: Screen, k: Key, can_load: bool) -> Option<Screen> {
    match (on, k) {
        (Screen::Game, Key::F(11)) => Some(Screen::Saving(Saving::Save)),
        (Screen::Game, Key::F(12)) if can_load => Some(Screen::Saving(Saving::Load)),
        (Screen::Game, Key::F(5)) => Some(Screen::Options),
        (Screen::Game, Key::F(1)) => Some(Screen::Help),
        (Screen::Game, Key::Letter('O')) => Some(Screen::Objectives),
        (Screen::Game, Key::Letter('E')) => Some(Screen::End),
        (Screen::Options, Key::F(7)) => Some(Screen::Sound),
        (Screen::Options, Key::F(8)) => Some(Screen::Speed),
        (Screen::Options, Key::F(9)) => Some(Screen::Prefs),
        _ => None,
    }
}

/// What kind of game is being played, for the end-scenario menu's columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Campaign,
    Custom,
    Multiplayer,
}

impl Kind {
    pub fn of(g: Option<GameKind>) -> Self {
        match g {
            Some(GameKind::Campaign(_)) => Kind::Campaign,
            Some(GameKind::Multiplayer) => Kind::Multiplayer,
            _ => Kind::Custom,
        }
    }
}

/// The end-scenario menu's buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndButton {
    Restart,
    ReturnToChat,
    LoadCustom,
    Surrender,
    QuitToMenu,
    Exit,
}

impl EndButton {
    pub fn confirm(self) -> Confirm {
        match self {
            EndButton::Restart => Confirm::Restart,
            EndButton::ReturnToChat => Confirm::ReturnToChat,
            EndButton::LoadCustom => Confirm::LoadCustom,
            EndButton::Surrender => Confirm::Surrender,
            EndButton::QuitToMenu => Confirm::QuitToMenu,
            EndButton::Exit => Confirm::Exit,
        }
    }
}

/// The buttons shown, in the spec's order ("Ending the scenario"; Previous is always there besides).
pub fn end_buttons(kind: Kind) -> Vec<EndButton> {
    use EndButton::*;
    match kind {
        Kind::Campaign => vec![Restart, Surrender, QuitToMenu, Exit],
        Kind::Custom => vec![LoadCustom, Surrender, QuitToMenu, Exit],
        Kind::Multiplayer => vec![ReturnToChat, Surrender, QuitToMenu, Exit],
    }
}

/// `ui/game-menu.md`, "Who may restart, return or load", and `game/save-load.md`, "Who may save or load": in a network
/// game only the host may open restart, return to chat room, load another scenario, save or load; anyone else gets
/// [`Screen::Refused`] naming the host (network entry 28). `host` is the session's host: `None` when there is none at
/// the moment, and then the check passes. An observer's choices touch only its own machine, so it is never refused.
pub fn host_check(screen: Screen, network: bool, observer: bool, host: Option<u8>, me: u8) -> Screen {
    let gated = matches!(
        screen,
        Screen::Confirm(Confirm::Restart | Confirm::LoadCustom | Confirm::ReturnToChat) | Screen::Saving(_)
    );
    match host {
        Some(h) if network && gated && !observer && h != me => Screen::Refused(h),
        _ => screen,
    }
}

/// Battle.net's rule: after more than this many seconds of game time the quit button is a surrender.
#[allow(dead_code, reason = "Battle.net games don't exist yet")]
pub const BNET_SURRENDER_SECS: u32 = 120;

/// What Battle.net's button 1 is at `secs` of game time.
#[allow(dead_code, reason = "Battle.net games don't exist yet")]
pub fn bnet_quit(secs: u32) -> Confirm {
    if secs > BNET_SURRENDER_SECS {
        Confirm::Surrender
    } else {
        Confirm::QuitDraw
    }
}

/// The `objctivs.tbl` entry (from 1) the objectives screen shows ("The objectives screen").
pub fn objectives_entry(game: Option<GameKind>) -> u16 {
    match game {
        Some(GameKind::Campaign(e)) if e < 52 => e as u16 + 1,
        _ => 59,
    }
}

/// The ranges of the three speed sliders for the rules in force.
pub fn game_speed_max(enhanced: bool, extra_speeds: u8) -> u8 {
    tidewater_sim::Extras {
        extra_speeds,
        ..Default::default()
    }
    .top_speed(enhanced)
}

/// A slider's value for a pointer at `x` over a track starting at `left` and `width` wide, 0 to `max`.
pub fn slider_value(x: f32, left: f32, width: f32, max: u8) -> u8 {
    if width <= 0.0 {
        return 0;
    }
    let f = ((x - left) / width).clamp(0.0, 1.0);
    (f * max as f32).round() as u8
}

/// The sound dialog's change delay: 5 dialog ticks from the first change, one tick a frame (open question default).
pub const SOUND_DELAY_TICKS: u8 = 5;

/// The countdown for applying the sound dialog's changes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Delay(Option<u8>);

impl Delay {
    /// A slider or checkbox changed: starts the countdown unless one is running.
    pub fn changed(&mut self) {
        self.0.get_or_insert(SOUND_DELAY_TICKS);
    }

    /// One dialog tick. True when the countdown ran out and the changes are to be applied.
    pub fn tick(&mut self) -> bool {
        match self.0 {
            Some(0) | Some(1) => {
                self.0 = None;
                true
            }
            Some(n) => {
                self.0 = Some(n - 1);
                false
            }
            None => false,
        }
    }

    /// Cancels a pending application (the dialog closed).
    pub fn clear(&mut self) {
        self.0 = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previous_returns_to_the_menu_it_came_from() {
        let mut n = Nav::default();
        n.open(Screen::Game);
        n.open_child(Screen::Options);
        n.open_child(Screen::Sound);
        assert!(!n.back());
        assert_eq!(n.current(), Some(Screen::Options));
        assert!(!n.back());
        assert_eq!(n.current(), Some(Screen::Game));
        assert!(n.back());
        assert_eq!(n.current(), None);
    }

    #[test]
    fn a_dialog_opened_by_its_key_returns_to_the_game() {
        let mut n = Nav::default();
        n.open(Screen::Sound);
        assert!(n.back());
        assert_eq!(n.current(), None);
    }

    #[test]
    fn ok_closes_all_menus() {
        let mut n = Nav::default();
        n.open(Screen::Game);
        n.open_child(Screen::Options);
        n.open_child(Screen::Speed);
        n.close();
        assert_eq!(n.current(), None);
    }

    #[test]
    fn opening_by_key_replaces_what_was_open() {
        let mut n = Nav::default();
        n.open(Screen::Game);
        n.open_child(Screen::Options);
        n.open(Screen::Sound);
        assert!(n.back());
    }

    #[test]
    fn keys_in_the_menus() {
        assert_eq!(menu_key(Screen::Game, Key::F(5), true), Some(Screen::Options));
        assert_eq!(menu_key(Screen::Game, Key::Letter('E'), true), Some(Screen::End));
        assert_eq!(menu_key(Screen::Game, Key::Letter('O'), true), Some(Screen::Objectives));
        assert_eq!(menu_key(Screen::Game, Key::F(12), false), None);
        assert_eq!(
            menu_key(Screen::Game, Key::F(12), true),
            Some(Screen::Saving(Saving::Load))
        );
        assert_eq!(menu_key(Screen::Options, Key::F(8), true), Some(Screen::Speed));
        assert_eq!(menu_key(Screen::Options, Key::F(5), true), None);
    }

    #[test]
    fn end_scenario_buttons_by_kind() {
        use EndButton::*;
        assert_eq!(
            end_buttons(Kind::Campaign),
            [Restart, Surrender, QuitToMenu, Exit],
            "campaign: restart, surrender, quit to menu, exit"
        );
        assert_eq!(end_buttons(Kind::Custom), [LoadCustom, Surrender, QuitToMenu, Exit]);
        assert_eq!(
            end_buttons(Kind::Multiplayer),
            [ReturnToChat, Surrender, QuitToMenu, Exit]
        );
        assert_eq!(Kind::of(Some(GameKind::Campaign(3))), Kind::Campaign);
        assert_eq!(Kind::of(None), Kind::Custom);
    }

    #[test]
    fn only_the_host_may_return_save_or_load_in_a_network_game() {
        let ret = Screen::Confirm(Confirm::ReturnToChat);
        let save = Screen::Saving(Saving::Save);
        // Player 2 while player 1 hosts: entry 28 names player 1 (LV13: after a hand-over, the new host).
        assert_eq!(host_check(ret, true, false, Some(1), 2), Screen::Refused(1));
        assert_eq!(host_check(save, true, false, Some(1), 2), Screen::Refused(1));
        assert_eq!(
            host_check(Screen::Saving(Saving::Load), true, false, Some(1), 2),
            Screen::Refused(1)
        );
        // The host itself, single player, no host at the moment, an observer, and an ungated choice all pass.
        assert_eq!(host_check(ret, true, false, Some(1), 1), ret);
        assert_eq!(host_check(save, false, false, None, 0), save);
        assert_eq!(host_check(ret, true, false, None, 2), ret);
        assert_eq!(host_check(ret, true, true, Some(0), 0), ret);
        let quit = Screen::Confirm(Confirm::QuitToMenu);
        assert_eq!(host_check(quit, true, false, Some(1), 2), quit);
    }

    #[test]
    fn battle_net_quit_button_by_game_time() {
        assert_eq!(bnet_quit(60), Confirm::QuitDraw);
        assert_eq!(bnet_quit(120), Confirm::QuitDraw);
        assert_eq!(bnet_quit(121), Confirm::Surrender);
    }

    #[test]
    fn objectives_entries() {
        assert_eq!(objectives_entry(Some(GameKind::Campaign(0))), 1);
        assert_eq!(objectives_entry(Some(GameKind::Campaign(51))), 52);
        assert_eq!(objectives_entry(Some(GameKind::Campaign(52))), 59);
        assert_eq!(objectives_entry(Some(GameKind::Custom)), 59);
        assert_eq!(objectives_entry(None), 59);
    }

    #[test]
    fn speed_slider_ranges_and_steps() {
        assert_eq!(game_speed_max(true, 0), 8);
        assert_eq!(game_speed_max(false, 0), 6);
        assert_eq!(game_speed_max(true, 2), 10);
        assert_eq!(game_speed_max(false, 2), 6);
    }

    #[test]
    fn slider_positions() {
        assert_eq!(slider_value(0.0, 10.0, 100.0, 6), 0);
        assert_eq!(slider_value(60.0, 10.0, 100.0, 6), 3);
        assert_eq!(slider_value(500.0, 10.0, 100.0, 6), 6);
        assert_eq!(slider_value(60.0, 10.0, 100.0, 100), 50);
    }

    #[test]
    fn a_quick_drag_is_applied_once_five_ticks_after_the_first_change() {
        let mut d = Delay::default();
        d.changed();
        let mut applied = 0;
        let mut at = 0;
        for t in 1..=10 {
            if t == 3 {
                d.changed();
            }
            if d.tick() {
                applied += 1;
                at = t;
            }
        }
        assert_eq!((applied, at), (1, 5));
    }
}
