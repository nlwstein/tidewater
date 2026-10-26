//! Optional improvements that only change the client (`docs/design/extras.md`). Everything here is **off by default**:
//! with every option at its default the client behaves, draws and sounds exactly as without this module.
//!
//! **One table.** [`REGISTRY`] lists every option once: its name, kind, default, scope and description. The `[extras]`
//! section of `settings.txt`, the `--extra-NAME` and `--no-extra-NAME` flags, the `--help` text and the documentation
//! table are all produced from it, so a new option is one table row plus the code that reads it.
//!
//! **Where a value comes from.** Defaults, then the settings file ([`ClientExtras::read_line`]), then the command line
//! ([`ExtraFlags`]): `--extra-NAME` sets an option for this run and `--no-extra-NAME` puts it back to its default.
//! The command line is not written back to the settings file.
//!
//! **Scope.** An option is [`Scope::Client`] (**C**: it changes nothing the player could not see anyway) or
//! [`Scope::ClientLobby`] (**C+L**: client-only, but it gives an edge, so in a game with other players the lobby sets
//! it for everyone). [`effective`] picks a game's values: a local session uses the player's own, any other session
//! ([`Session::lobby_client_extras`](crate::session::Session::lobby_client_extras)) supplies its own for the C+L
//! options. The options that change the simulation are not here: they are [`tidewater_sim::Extras`].

use clap::builder::PossibleValuesParser;
use clap::{Arg, ArgAction, ArgMatches, Command, FromArgMatches};

/// An option, as an index into [`REGISTRY`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Id {
    HealthBars,
    IdleWorker,
    GameClock,
    ControlGroupBar,
    SmartAttackMove,
    VolumeVoices,
    VolumeEffects,
    VolumeMusic,
    Scaling,
    RallyPoints,
    ProductionQueue,
    SelectionLimit,
    SpeedLevels,
    MinimapPing,
    ShiftQueue,
    AutoReplay,
    Widescreen,
    MapZoom,
    TouchDrag,
    TouchSmartTap,
    TouchTwoFinger,
    TouchPinch,
    TouchHold,
    TouchButtons,
    TouchSwapBars,
    TouchMenuTap,
    PadSpeed,
    PadAcceleration,
    PadDeadZone,
    PadSwap,
    PadHideTouch,
}

/// How many options there are.
pub const COUNT: usize = 31;

/// Who decides an option in a game with other players.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// **C**: the player's own setting, always.
    Client,
    /// **C+L**: client-only but gives an edge; a non-local session sets it.
    ClientLobby,
    /// **S**: changes the simulation (`tidewater_sim::Extras`). The player's setting starts a local game; in a game
    /// with other players the lobby's value is used for everyone, and a replay uses the recording's.
    Sim,
}

/// What values an option takes. Every kind is stored as a number 0 to 100.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `on` or `off` (1 or 0).
    Flag,
    /// One of these names; the number is the name's place in the list, and the first name is the default.
    Choice(&'static [&'static str]),
    /// A percentage, 0 to 100.
    Percent,
}

/// One option's row.
#[derive(Clone, Copy, Debug)]
pub struct Spec {
    pub id: Id,
    /// The name in the settings file and after `--extra-` / `--no-extra-`.
    pub name: &'static str,
    /// A short name for the Settings screen's row (`settings`).
    pub label: &'static str,
    pub kind: Kind,
    /// The value when nothing says otherwise: off, the first choice, or (for a volume) 100, which leaves the sound as
    /// the game's own dialog sets it.
    pub default: u8,
    pub scope: Scope,
    pub description: &'static str,
}

/// The health bar modes, in the order of the choice list.
pub const HEALTH_BAR_MODES: &[&str] = &["off", "hovered", "selected", "always"];
/// The clock overlay modes.
pub const CLOCK_MODES: &[&str] = &["off", "clock", "clock-apm"];

/// The picture scaling modes (option `scaling`).
pub const SCALING_MODES: &[&str] = &["auto", "integer", "sharp"];

/// The selection limits (option `selection-limit`): the original nine, 12, 18, or no limit.
pub const SELECTION_LIMITS: &[&str] = &["9", "12", "18", "unlimited"];

/// How many speed levels to add above Fastest (option `speed-levels`).
pub const SPEED_LEVELS: &[&str] = &["off", "1", "2", "3"];

/// The values of `auto-replay`.
pub const REPLAY_SAVING: &[&str] = &["off", "menu", "always"];

/// The values of `widescreen`: the original screen, a 16:10 or 16:9 one, or one matching the window.
pub const WIDESCREEN_MODES: &[&str] = &["off", "16:10", "16:9", "auto"];

/// The values of `map-zoom`: the mouse wheel does nothing to the map view, or magnifies it.
pub const MAP_ZOOM_MODES: &[&str] = &["off", "wheel"];

/// The values of `touch-drag`: what one finger dragging on the map view does.
pub const TOUCH_DRAG_MODES: &[&str] = &["pan", "pan-no-glide", "box"];
/// The values of `touch-smart-tap`.
pub const TOUCH_SMART_TAP: &[&str] = &["on", "off"];
/// The values of `touch-two-finger`: what two fingers dragging together do.
pub const TOUCH_TWO_FINGER_MODES: &[&str] = &["pan", "box", "off"];
/// The values of `touch-pinch`: `auto` is on for phones, off for tablets (`touch::is_phone`).
pub const TOUCH_PINCH_MODES: &[&str] = &["auto", "on", "off"];
/// The values of `touch-hold`: how long a finger is held to draw the box and to right-click.
pub const TOUCH_HOLD_MODES: &[&str] = &["normal", "long", "longer"];
/// The values of `touch-buttons`: the size of the side bars' buttons.
pub const TOUCH_BUTTON_SIZES: &[&str] = &["medium", "small", "large"];
/// The values of `touch-menu-tap`.
pub const TOUCH_MENU_TAP: &[&str] = &["on", "off"];

/// The values of `pad-speed`: how fast the left stick moves the pointer.
pub const PAD_SPEEDS: &[&str] = &["normal", "slow", "fast"];
/// The values of `pad-acceleration`.
pub const PAD_ACCELERATION: &[&str] = &["on", "off"];
/// The values of `pad-dead-zone`: how far a stick moves before it counts.
pub const PAD_DEAD_ZONES: &[&str] = &["normal", "small", "large"];
/// The values of `pad-hide-touch`.
pub const PAD_HIDE_TOUCH: &[&str] = &["on", "off"];

/// The most the magnifying zoom magnifies, in times the original's view (it never shows more map than the original).
pub const MAP_ZOOM_MAX: f32 = 4.0;

/// Every option. The order is the settings file's and the help text's.
pub const REGISTRY: [Spec; COUNT] = [
    Spec {
        id: Id::HealthBars,
        name: "health-bars",
        label: "Health bars",
        kind: Kind::Choice(HEALTH_BAR_MODES),
        default: 0,
        scope: Scope::ClientLobby,
        description: "Hit point bars over units: only those under the pointer, only selected ones, or all",
    },
    Spec {
        id: Id::IdleWorker,
        name: "idle-worker",
        label: "Idle worker button",
        kind: Kind::Flag,
        default: 0,
        scope: Scope::Client,
        description: "An idle-worker button at the top of the screen and the F6 key: select your next idle worker",
    },
    Spec {
        id: Id::GameClock,
        name: "game-clock",
        label: "Game clock",
        kind: Kind::Choice(CLOCK_MODES),
        default: 0,
        scope: Scope::Client,
        description: "The game clock over the map view, with or without your actions per minute",
    },
    Spec {
        id: Id::ControlGroupBar,
        name: "group-bar",
        label: "Control group bar",
        kind: Kind::Flag,
        default: 0,
        scope: Scope::Client,
        description: "A strip over the map view's top left listing your control groups with their sizes: click one to recall it",
    },
    Spec {
        id: Id::SmartAttackMove,
        name: "smart-attack-move",
        label: "Smart attack-move",
        kind: Kind::Flag,
        default: 0,
        scope: Scope::ClientLobby,
        description: "A right-click on open ground with only fighting units selected attack-moves there instead of moving",
    },
    Spec {
        id: Id::VolumeVoices,
        name: "volume-voices",
        label: "Unit voices",
        kind: Kind::Percent,
        default: 100,
        scope: Scope::Client,
        description: "Unit voices, in percent of the sound volume (100 leaves them as the game sets them)",
    },
    Spec {
        id: Id::VolumeEffects,
        name: "volume-effects",
        label: "Sound effects",
        kind: Kind::Percent,
        default: 100,
        scope: Scope::Client,
        description: "Sound effects, in percent of the sound volume",
    },
    Spec {
        id: Id::VolumeMusic,
        name: "volume-music",
        label: "Music",
        kind: Kind::Percent,
        default: 100,
        scope: Scope::Client,
        description: "Music, in percent of the music volume",
    },
    Spec {
        id: Id::Scaling,
        name: "scaling",
        label: "Scaling",
        kind: Kind::Choice(SCALING_MODES),
        default: 0,
        scope: Scope::Client,
        description: "How the picture fits the window: automatic (whole-number scales unless they leave wide borders, then any size drawn sharp bilinear), only whole-number scales, or always any size, sharp (touch platforms always use sharp)",
    },
    Spec {
        id: Id::RallyPoints,
        name: "rally-points",
        label: "Rally points",
        kind: Kind::Flag,
        default: 0,
        scope: Scope::Sim,
        description: "Right-click with a training building selected to set a rally point: the units it trains go there",
    },
    Spec {
        id: Id::ProductionQueue,
        name: "production-queue",
        label: "Production queue",
        kind: Kind::Flag,
        default: 0,
        scope: Scope::Sim,
        description: "Buildings queue up to five jobs, paid when queued; Cancel removes the last",
    },
    Spec {
        id: Id::SelectionLimit,
        name: "selection-limit",
        label: "Selection limit",
        kind: Kind::Choice(SELECTION_LIMITS),
        default: 0,
        scope: Scope::Sim,
        description: "How many units you can select and command at once: nine as in the original, 12, 18 or no limit",
    },
    Spec {
        id: Id::SpeedLevels,
        name: "speed-levels",
        label: "Extra game speeds",
        kind: Kind::Choice(SPEED_LEVELS),
        default: 0,
        scope: Scope::Sim,
        description: "Game speeds above Fastest, 1 to 3 more levels (Enhanced rules only)",
    },
    Spec {
        id: Id::MinimapPing,
        name: "minimap-ping",
        label: "Minimap ping",
        kind: Kind::Flag,
        default: 0,
        scope: Scope::Client,
        description: "Alt-click on the minimap pings that place on your allies' minimaps, and shows their pings on yours",
    },
    Spec {
        id: Id::ShiftQueue,
        name: "shift-queue",
        label: "Shift-queued orders",
        kind: Kind::Flag,
        default: 0,
        scope: Scope::Sim,
        description: "Hold Shift while giving a move, attack, attack-move, patrol, repair, harvest or build order to add it to the unit's order queue (up to 8) instead of replacing its order",
    },
    Spec {
        id: Id::AutoReplay,
        name: "auto-replay",
        label: "Record replays",
        kind: Kind::Choice(REPLAY_SAVING),
        default: 0,
        scope: Scope::Client,
        description: "Record every game: `menu` keeps the recording so the game menu can save it, `always` also writes each game's replay to the replays folder of the data directory as it is played",
    },
    Spec {
        id: Id::Widescreen,
        name: "widescreen",
        label: "Widescreen",
        kind: Kind::Choice(WIDESCREEN_MODES),
        default: 0,
        scope: Scope::ClientLobby,
        description: "A wider game screen, the extra width being map: 16:10, 16:9, or auto to match the window (ignored on touch devices; menus and videos stay 4:3)",
    },
    Spec {
        id: Id::MapZoom,
        name: "map-zoom",
        label: "Magnifying zoom",
        kind: Kind::Choice(MAP_ZOOM_MODES),
        default: 0,
        scope: Scope::Client,
        description: "`wheel`: the mouse wheel over the map view magnifies it up to 4 times around the pointer and back to the normal view, never showing more map than the original does (on touch devices the pinch is `touch-pinch`)",
    },
    Spec {
        id: Id::TouchDrag,
        name: "touch-drag",
        label: "One-finger drag",
        kind: Kind::Choice(TOUCH_DRAG_MODES),
        default: 0,
        scope: Scope::Client,
        description: "Touch only. A finger dragged on the map scrolls it (`pan` glides on after a flick, `pan-no-glide` stops where the finger lifts); hold it still a moment first to draw a selection box instead. `box`: a drag draws the box at once, as the mouse does",
    },
    Spec {
        id: Id::TouchSmartTap,
        name: "touch-smart-tap",
        label: "Tap to order",
        kind: Kind::Choice(TOUCH_SMART_TAP),
        default: 0,
        scope: Scope::Client,
        description: "Touch only. With your units selected, a tap on the ground, an enemy or a resource orders them there, as a right click does; a tap on your own unit or building selects it. `off`: every tap is a left click",
    },
    Spec {
        id: Id::TouchTwoFinger,
        name: "touch-two-finger",
        label: "Two-finger drag",
        kind: Kind::Choice(TOUCH_TWO_FINGER_MODES),
        default: 0,
        scope: Scope::Client,
        description: "Touch only. Two fingers dragged together scroll the map (`pan`), are the corners of a selection box (`box`), or do nothing (`off`; a pinch still zooms if it is on)",
    },
    Spec {
        id: Id::TouchPinch,
        name: "touch-pinch",
        label: "Pinch zoom",
        kind: Kind::Choice(TOUCH_PINCH_MODES),
        default: 0,
        scope: Scope::Client,
        description: "Touch only. Spreading two fingers magnifies the map view up to 4 times. `auto`: on for phones (a screen whose short side is under 90 mm), off for tablets",
    },
    Spec {
        id: Id::TouchHold,
        name: "touch-hold",
        label: "Hold time",
        kind: Kind::Choice(TOUCH_HOLD_MODES),
        default: 0,
        scope: Scope::Client,
        description: "Touch only. How long a finger is held still before a drag draws a box, and before it right-clicks: `normal` 0.3 and 0.45 seconds, `long` 0.4 and 0.6, `longer` 0.5 and 0.75",
    },
    Spec {
        id: Id::TouchButtons,
        name: "touch-buttons",
        label: "Button size",
        kind: Kind::Choice(TOUCH_BUTTON_SIZES),
        default: 0,
        scope: Scope::Client,
        description: "Touch only. The size of the side bars' buttons: `medium` 9 mm, `small` 7 mm, `large` 11 mm (smaller when a bar is too narrow)",
    },
    Spec {
        id: Id::TouchSwapBars,
        name: "touch-swap-bars",
        label: "Swap side bars",
        kind: Kind::Flag,
        default: 0,
        scope: Scope::Client,
        description: "Touch only. The control-group buttons go on the left and the menu, pause and modifier buttons on the right",
    },
    Spec {
        id: Id::TouchMenuTap,
        name: "touch-menu-tap",
        label: "Three-finger menu",
        kind: Kind::Choice(TOUCH_MENU_TAP),
        default: 0,
        scope: Scope::Client,
        description: "Touch only. A tap of three fingers at once opens the game menu (the Menu button always does)",
    },
    Spec {
        id: Id::PadSpeed,
        name: "pad-speed",
        label: "Pointer speed",
        kind: Kind::Choice(PAD_SPEEDS),
        default: 0,
        scope: Scope::Client,
        description: "Controller only. How fast the left stick moves the pointer (holding the left trigger doubles it)",
    },
    Spec {
        id: Id::PadAcceleration,
        name: "pad-acceleration",
        label: "Acceleration",
        kind: Kind::Choice(PAD_ACCELERATION),
        default: 0,
        scope: Scope::Client,
        description: "Controller only. A small push on the left stick moves the pointer slowly for aiming and a full push fast; it also slows over units and buttons. `off`: the speed follows the push evenly",
    },
    Spec {
        id: Id::PadDeadZone,
        name: "pad-dead-zone",
        label: "Stick dead zone",
        kind: Kind::Choice(PAD_DEAD_ZONES),
        default: 0,
        scope: Scope::Client,
        description: "Controller only. How far a stick moves before it counts: raise it if the pointer drifts on its own",
    },
    Spec {
        id: Id::PadSwap,
        name: "pad-swap",
        label: "Swap confirm and back",
        kind: Kind::Flag,
        default: 0,
        scope: Scope::Client,
        description: "Controller only. The right face button clicks and the bottom one right-clicks, for pads with Nintendo's layout",
    },
    Spec {
        id: Id::PadHideTouch,
        name: "pad-hide-touch",
        label: "Hide touch controls",
        kind: Kind::Choice(PAD_HIDE_TOUCH),
        default: 0,
        scope: Scope::Client,
        description: "Controller and touch. While a controller is connected the touch screen's side bars hide (taps still work, and the shoulder buttons are Shift and Ctrl)",
    },
];

impl Id {
    pub fn spec(self) -> &'static Spec {
        &REGISTRY[self as usize]
    }
}

impl Spec {
    /// The long flag that sets the option.
    pub fn flag(&self) -> String {
        format!("--extra-{}", self.name)
    }

    /// The long flag that puts the option back to its default.
    pub fn no_flag(&self) -> String {
        format!("--no-extra-{}", self.name)
    }

    /// The value a text means, if it is one the kind takes.
    pub fn parse(&self, text: &str) -> Option<u8> {
        let t = text.trim();
        match self.kind {
            Kind::Flag => match t {
                "on" | "true" | "yes" | "1" => Some(1),
                "off" | "false" | "no" | "0" => Some(0),
                _ => None,
            },
            Kind::Choice(names) => names.iter().position(|n| *n == t).map(|i| i as u8),
            Kind::Percent => t.parse::<u8>().ok().filter(|&v| v <= 100),
        }
    }

    /// A value as the settings file writes it.
    pub fn text(&self, value: u8) -> String {
        match self.kind {
            Kind::Flag => if value != 0 { "on" } else { "off" }.to_string(),
            Kind::Choice(names) => names.get(value as usize).copied().unwrap_or(names[0]).to_string(),
            Kind::Percent => value.min(100).to_string(),
        }
    }

    /// The values the option takes, for help text and the docs: `on, off`, the choices, or `0 to 100`.
    pub fn values(&self) -> String {
        match self.kind {
            Kind::Flag => "on, off".to_string(),
            Kind::Choice(names) => names.join(", "),
            Kind::Percent => "0 to 100".to_string(),
        }
    }

    /// The C or C+L mark.
    pub fn scope_mark(&self) -> &'static str {
        match self.scope {
            Scope::Client => "C",
            Scope::ClientLobby => "C+L",
            Scope::Sim => "S",
        }
    }
}

/// How health bars show (option `health-bars`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HealthBars {
    Off,
    Hovered,
    Selected,
    Always,
}

/// What is done with every game's recording (option `auto-replay`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplaySaving {
    /// Nothing is recorded (unless `--record-replay` asks).
    Off,
    /// The game is recorded in memory; the game menu's Save Replay writes it.
    Menu,
    /// The game is recorded and written to the replays folder as it is played.
    Always,
}

/// How wide the game screen is (option `widescreen`, `crate::widescreen`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Widescreen {
    /// The original 640 x 480.
    Off,
    Ratio16x10,
    Ratio16x9,
    /// The window's shape, up to 16:9.
    Auto,
}

/// What the clock overlay shows (option `game-clock`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockMode {
    Off,
    Clock,
    ClockApm,
}

/// How the picture is fitted to the window (option `scaling`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scaling {
    /// Whole numbers when they fill the safe area well, otherwise as `Sharp` (`display::Fit::Auto`).
    Auto,
    /// Whole numbers of window pixels per screen pixel, black around.
    Integer,
    /// Any size inside the window's safe area, drawn sharp bilinear (`display`, "Fractional scale").
    Sharp,
}

/// What two fingers dragging together do (`touch-two-finger`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchTwoFinger {
    Pan,
    Corners,
    Off,
}

/// The touch options read together ([`ClientExtras::touch`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TouchSettings {
    /// One finger on the map view pans (else it draws the box at once).
    pub drag_pans: bool,
    pub glide: bool,
    pub smart_tap: bool,
    pub two_finger: TouchTwoFinger,
    /// The pinch zoom: on, off, or `None` for the device's default (on for phones).
    pub pinch: Option<bool>,
    pub arm_ms: f64,
    pub long_press_ms: f64,
    /// The side bars' button size, in millimetres.
    pub button_mm: f32,
    pub swap_bars: bool,
    pub menu_tap: bool,
}

/// The controller options read together ([`ClientExtras::pad`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PadSettings {
    /// The pointer's speed, as a factor of the normal one.
    pub speed: f32,
    /// A curve on the stick's push, and friction over units and buttons.
    pub acceleration: bool,
    /// The share of a stick's travel that counts as rest.
    pub dead_zone: f32,
    /// East confirms and south goes back.
    pub swap: bool,
    /// The touch chrome hides while a pad is connected.
    pub hide_touch: bool,
}

/// The values of every option for a run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClientExtras {
    values: [u8; COUNT],
}

impl Default for ClientExtras {
    fn default() -> Self {
        ClientExtras {
            values: REGISTRY.map(|s| s.default),
        }
    }
}

impl ClientExtras {
    pub fn get(&self, id: Id) -> u8 {
        self.values[id as usize]
    }

    pub fn set(&mut self, id: Id, value: u8) {
        self.values[id as usize] = value;
    }

    /// Whether every option is at its default (nothing differs from the original game).
    pub fn is_default(&self) -> bool {
        *self == ClientExtras::default()
    }

    pub fn health_bars(&self) -> HealthBars {
        match self.get(Id::HealthBars) {
            1 => HealthBars::Hovered,
            2 => HealthBars::Selected,
            3 => HealthBars::Always,
            _ => HealthBars::Off,
        }
    }

    pub fn scaling(&self) -> Scaling {
        match self.get(Id::Scaling) {
            1 => Scaling::Integer,
            2 => Scaling::Sharp,
            _ => Scaling::Auto,
        }
    }

    pub fn idle_worker(&self) -> bool {
        self.get(Id::IdleWorker) != 0
    }

    pub fn group_bar(&self) -> bool {
        self.get(Id::ControlGroupBar) != 0
    }

    pub fn smart_attack_move(&self) -> bool {
        self.get(Id::SmartAttackMove) != 0
    }

    pub fn minimap_ping(&self) -> bool {
        self.get(Id::MinimapPing) != 0
    }

    /// The options that change the simulation, as a new local game starts with them (`tidewater_sim::Extras`). Each
    /// sim option has a registry row of scope [`Scope::Sim`] and is read here.
    pub fn sim(&self) -> tidewater_sim::Extras {
        tidewater_sim::Extras {
            rally_points: self.get(Id::RallyPoints) != 0,
            production_queue: self.get(Id::ProductionQueue) != 0,
            selection_limit: match self.get(Id::SelectionLimit) {
                1 => 12,
                2 => 18,
                3 => tidewater_sim::extras::UNLIMITED,
                _ => 0,
            },
            extra_speeds: self.get(Id::SpeedLevels),
            order_queue: self.get(Id::ShiftQueue) != 0,
        }
    }

    /// The registry values of the sim options a running game has (`tidewater_sim::Extras`), the inverse of [`ClientExtras::sim`]:
    /// what the Settings screen shows, read-only, on its Gameplay page during a game. A selection limit the registry
    /// has no step for shows as the nearest step below it.
    pub fn of_sim(sim: &tidewater_sim::Extras) -> [(Id, u8); 5] {
        use tidewater_sim::extras::UNLIMITED;
        [
            (Id::RallyPoints, u8::from(sim.rally_points)),
            (Id::ProductionQueue, u8::from(sim.production_queue)),
            (
                Id::SelectionLimit,
                match sim.selection_limit {
                    UNLIMITED => 3,
                    18.. => 2,
                    12.. => 1,
                    _ => 0,
                },
            ),
            (Id::SpeedLevels, sim.extra_speeds.min(3)),
            (Id::ShiftQueue, u8::from(sim.order_queue)),
        ]
    }

    /// What is done with every game's recording (client-only).
    pub fn replay_saving(&self) -> ReplaySaving {
        match self.get(Id::AutoReplay) {
            1 => ReplaySaving::Menu,
            2 => ReplaySaving::Always,
            _ => ReplaySaving::Off,
        }
    }

    pub fn widescreen(&self) -> Widescreen {
        match self.get(Id::Widescreen) {
            1 => Widescreen::Ratio16x10,
            2 => Widescreen::Ratio16x9,
            3 => Widescreen::Auto,
            _ => Widescreen::Off,
        }
    }

    /// Whether the mouse wheel magnifies the map view (option `map-zoom`).
    /// The touch options read together (they do nothing without touch).
    pub fn touch(&self) -> TouchSettings {
        let pinch = match self.get(Id::TouchPinch) {
            1 => Some(true),
            2 => Some(false),
            _ => None,
        };
        let (arm_ms, long_press_ms) = match self.get(Id::TouchHold) {
            1 => (400.0, 600.0),
            2 => (500.0, 750.0),
            _ => (300.0, 450.0),
        };
        TouchSettings {
            drag_pans: self.get(Id::TouchDrag) != 2,
            glide: self.get(Id::TouchDrag) == 0,
            smart_tap: self.get(Id::TouchSmartTap) == 0,
            two_finger: match self.get(Id::TouchTwoFinger) {
                1 => TouchTwoFinger::Corners,
                2 => TouchTwoFinger::Off,
                _ => TouchTwoFinger::Pan,
            },
            pinch,
            arm_ms,
            long_press_ms,
            button_mm: match self.get(Id::TouchButtons) {
                1 => 7.0,
                2 => 11.0,
                _ => 9.0,
            },
            swap_bars: self.get(Id::TouchSwapBars) == 1,
            menu_tap: self.get(Id::TouchMenuTap) == 0,
        }
    }

    /// The controller options read together (they do nothing without a controller).
    pub fn pad(&self) -> PadSettings {
        PadSettings {
            speed: match self.get(Id::PadSpeed) {
                1 => 0.65,
                2 => 1.5,
                _ => 1.0,
            },
            acceleration: self.get(Id::PadAcceleration) == 0,
            dead_zone: match self.get(Id::PadDeadZone) {
                1 => 0.08,
                2 => 0.25,
                _ => 0.15,
            },
            swap: self.get(Id::PadSwap) == 1,
            hide_touch: self.get(Id::PadHideTouch) == 0,
        }
    }

    pub fn map_zoom_wheel(&self) -> bool {
        self.get(Id::MapZoom) == 1
    }

    pub fn game_clock(&self) -> ClockMode {
        match self.get(Id::GameClock) {
            1 => ClockMode::Clock,
            2 => ClockMode::ClockApm,
            _ => ClockMode::Off,
        }
    }

    /// The mixer's master trims for the three volumes.
    pub fn trims(&self) -> crate::audio::Trims {
        crate::audio::Trims::from_percent(
            self.get(Id::VolumeVoices),
            self.get(Id::VolumeEffects),
            self.get(Id::VolumeMusic),
        )
    }

    /// One `name=value` line of the settings file's `[extras]` section. Unknown names and values the kind doesn't take
    /// are ignored (the option keeps its value), like the rest of the settings file. Returns whether the line was used.
    pub fn read_line(&mut self, name: &str, value: &str) -> bool {
        let Some(spec) = REGISTRY.iter().find(|s| s.name == name.trim()) else {
            return false;
        };
        match spec.parse(value) {
            Some(v) => {
                self.set(spec.id, v);
                true
            }
            None => false,
        }
    }

    /// The `[extras]` section of the settings file: a comment and a line per option, from the table.
    pub fn section_text(&self) -> String {
        let mut out = String::from("[extras]\n");
        for s in &REGISTRY {
            out.push_str(&format!("# {}: {} ({})\n", s.name, s.description, s.values()));
            out.push_str(&format!("{}={}\n", s.name, s.text(self.get(s.id))));
        }
        out
    }
}

/// The C+L options, in registry order: the ones the lobby sets for everyone (`Spec::scope` is [`Scope::ClientLobby`]).
/// Read from the table, so a new C+L row is picked up with no other change.
pub fn lobby_specs() -> impl Iterator<Item = &'static Spec> {
    REGISTRY.iter().filter(|s| s.scope == Scope::ClientLobby)
}

/// The value after `value` in a lobby list that cycles an option: a flag flips, a choice takes the next name and wraps,
/// a percentage steps by 25 and wraps to 0.
pub fn next_value(spec: &Spec, value: u8) -> u8 {
    match spec.kind {
        Kind::Flag => u8::from(value == 0),
        Kind::Choice(names) => (value + 1) % names.len() as u8,
        Kind::Percent => {
            if value >= 100 {
                0
            } else {
                (value + 25).min(100)
            }
        }
    }
}

impl ClientExtras {
    /// The C+L values that differ from their default, by option name in registry order: how the lobby carries the
    /// host's choice (`net::proto`). Empty when every C+L option is at its default. Names, not positions, so a program
    /// with another table still reads what it knows.
    pub fn lobby_values(&self) -> Vec<(String, u8)> {
        lobby_specs()
            .filter(|s| self.get(s.id) != s.default)
            .map(|s| (s.name.to_string(), self.get(s.id)))
            .collect()
    }

    /// The lobby's C+L values as a set of options: every option at its default but the ones named. A name that is not a
    /// C+L option, or a value the option does not take, is ignored (a program that is not ours may send anything).
    pub fn from_lobby_values(values: &[(String, u8)]) -> ClientExtras {
        let mut out = ClientExtras::default();
        for (name, v) in values {
            if let Some(s) = lobby_specs().find(|s| s.name == name)
                && s.parse(&s.text(*v)) == Some(*v)
            {
                out.set(s.id, *v);
            }
        }
        out
    }

    /// Takes the C+L values (only those) of `other`.
    pub fn with_lobby_from(mut self, other: &ClientExtras) -> ClientExtras {
        for s in lobby_specs() {
            self.set(s.id, other.get(s.id));
        }
        self
    }
}

/// The lobby's C+L values in words, one per option that is not at its default: `health-bars always`.
pub fn describe_lobby(values: &[(String, u8)]) -> Vec<String> {
    let e = ClientExtras::from_lobby_values(values);
    lobby_specs()
        .filter(|s| e.get(s.id) != s.default)
        .map(|s| format!("{} {}", s.name, s.text(e.get(s.id))))
        .collect()
}

/// The values a game runs with. A local session uses `local`; a session that holds its own ([`Session::lobby_client_extras`])
/// gives the C+L options, and the player's own settings are not read for them.
///
/// [`Session::lobby_client_extras`]: crate::session::Session::lobby_client_extras
pub fn effective(local: &ClientExtras, lobby: Option<ClientExtras>) -> ClientExtras {
    let Some(lobby) = lobby else { return *local };
    let mut out = *local;
    for s in REGISTRY.iter().filter(|s| s.scope == Scope::ClientLobby) {
        out.set(s.id, lobby.get(s.id));
    }
    out
}

/// The sim options a new local game starts with: the player's own choice (the registry's [`Scope::Sim`] rows). A session
/// that isn't local never calls this ([`Session::sim_extras`](crate::session::Session::sim_extras)): a network game
/// takes the lobby's set and a replay the recording's.
pub fn local_sim_extras(local: &ClientExtras) -> tidewater_sim::Extras {
    local.sim()
}

/// What the command line says about the options: values to set and options to put back to their default, in the order
/// the table lists them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExtraFlags {
    changes: Vec<(Id, Option<u8>)>,
}

impl ExtraFlags {
    /// Flags from `(name, value)` pairs, as a scripted scene asks for them; a value `"default"` resets the option.
    /// Panics on a name or value the table doesn't have (a scene is code, so that is a bug).
    pub fn from_pairs(pairs: &[(&str, &str)]) -> ExtraFlags {
        let changes = pairs
            .iter()
            .map(|&(name, value)| {
                let spec = REGISTRY
                    .iter()
                    .find(|s| s.name == name)
                    .unwrap_or_else(|| panic!("no extra named {name}"));
                if value == "default" {
                    (spec.id, None)
                } else {
                    let v = spec
                        .parse(value)
                        .unwrap_or_else(|| panic!("{value} is not a value of {name}"));
                    (spec.id, Some(v))
                }
            })
            .collect();
        ExtraFlags { changes }
    }

    /// Puts the command line's changes over `extras`.
    pub fn apply(&self, extras: &mut ClientExtras) {
        for &(id, v) in &self.changes {
            extras.set(id, v.unwrap_or(id.spec().default));
        }
    }

    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

fn leak(s: String) -> &'static str {
    // Built once while the command line is built; the program ends soon after.
    Box::leak(s.into_boxed_str())
}

fn on_id(s: &Spec) -> &'static str {
    leak(format!("extra-{}", s.name))
}

fn off_id(s: &Spec) -> &'static str {
    leak(format!("no-extra-{}", s.name))
}

impl clap::Args for ExtraFlags {
    fn augment_args(mut cmd: Command) -> Command {
        const HEADING: &str = "Optional improvements (all off by default)";
        for s in &REGISTRY {
            let (on, off) = (on_id(s), off_id(s));
            let help = format!("{} [{}]", s.description, s.scope_mark());
            let mut arg = Arg::new(on)
                .long(on)
                .help(help)
                .help_heading(HEADING)
                .overrides_with(off);
            arg = match s.kind {
                Kind::Flag => arg.action(ArgAction::SetTrue),
                Kind::Choice(names) => arg
                    .value_name("MODE")
                    .value_parser(PossibleValuesParser::new(names.iter().copied())),
                Kind::Percent => arg
                    .value_name("PERCENT")
                    .value_parser(clap::value_parser!(u8).range(0..=100)),
            };
            cmd = cmd.arg(arg);
            cmd = cmd.arg(
                Arg::new(off)
                    .long(off)
                    .action(ArgAction::SetTrue)
                    .hide(true)
                    .overrides_with(on),
            );
        }
        cmd.after_help(
            "Each --extra-NAME option has a matching --no-extra-NAME that puts it back to its default for this run. \
             The settings file's [extras] section sets them for good. C: client only. C+L: client only, but in a game \
             with other players the lobby sets it. S: changes the simulation, so in a game with other players the \
             host's lobby sets it for everyone.",
        )
    }

    fn augment_args_for_update(cmd: Command) -> Command {
        Self::augment_args(cmd)
    }
}

impl FromArgMatches for ExtraFlags {
    fn from_arg_matches(m: &ArgMatches) -> Result<Self, clap::Error> {
        let mut changes = Vec::new();
        for s in &REGISTRY {
            let (on, off) = (on_id(s), off_id(s));
            if m.get_flag(off) {
                changes.push((s.id, None));
                continue;
            }
            let v = match s.kind {
                Kind::Flag => m.get_flag(on).then_some(1),
                Kind::Choice(_) => m.get_one::<String>(on).and_then(|t| s.parse(t)),
                Kind::Percent => m.get_one::<u8>(on).copied(),
            };
            if let Some(v) = v {
                changes.push((s.id, Some(v)));
            }
        }
        Ok(ExtraFlags { changes })
    }

    fn update_from_arg_matches(&mut self, m: &ArgMatches) -> Result<(), clap::Error> {
        *self = Self::from_arg_matches(m)?;
        Ok(())
    }
}

/// The documentation table of the options, as `docs/design/extras.md` and the README keep it (a test checks they do).
pub fn doc_table() -> String {
    let mut out =
        String::from("| Option | Flag | Values | Scope | Default | What it does |\n|---|---|---|---|---|---|\n");
    for s in &REGISTRY {
        let flag = match s.kind {
            Kind::Flag => format!("`--extra-{}`", s.name),
            _ => format!("`--extra-{} VALUE`", s.name),
        };
        out.push_str(&format!(
            "| `{}` | {} | {} | {} | {} | {} |\n",
            s.name,
            flag,
            s.values(),
            s.scope_mark(),
            s.text(s.default),
            s.description
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::args::Args;
    use clap::Parser;

    #[test]
    fn scaling_is_stored_by_name_so_older_settings_files_keep_their_meaning() {
        let mut e = ClientExtras::default();
        assert_eq!(e.scaling(), Scaling::Auto, "auto is the default");
        assert!(e.read_line("scaling", "integer"));
        assert_eq!(e.scaling(), Scaling::Integer);
        assert!(e.read_line("scaling", "sharp"));
        assert_eq!(e.scaling(), Scaling::Sharp);
        assert!(e.section_text().contains("\nscaling=sharp\n"));
        assert!(e.read_line("scaling", "auto"));
        assert_eq!(e.scaling(), Scaling::Auto);
        assert_eq!(
            lobby_specs().filter(|s| s.name == "scaling").count(),
            0,
            "scaling is not a lobby option"
        );
    }

    #[test]
    fn the_lobby_carries_only_the_c_plus_l_values_that_differ_and_reads_back_what_it_knows() {
        let mut e = ClientExtras::default();
        assert!(e.lobby_values().is_empty());
        e.set(Id::HealthBars, 3);
        e.set(Id::Widescreen, 2);
        e.set(Id::IdleWorker, 1); // a C option: not the lobby's
        e.set(Id::RallyPoints, 1); // a sim option: not here
        let v = e.lobby_values();
        assert_eq!(v, [("health-bars".to_string(), 3), ("widescreen".to_string(), 2)]);
        let back = ClientExtras::from_lobby_values(&v);
        assert_eq!(back.lobby_values(), v);
        assert!(!back.idle_worker());
        // Unknown names, C options and values out of range are ignored.
        let junk = [
            ("idle-worker".to_string(), 1),
            ("health-bars".to_string(), 9),
            ("nonsense".to_string(), 1),
            ("smart-attack-move".to_string(), 1),
        ];
        let got = ClientExtras::from_lobby_values(&junk);
        assert_eq!(got.lobby_values(), [("smart-attack-move".to_string(), 1)]);
        assert_eq!(describe_lobby(&v), ["health-bars always", "widescreen 16:9"]);
        // Every C+L row of the table cycles through all its values.
        for s in lobby_specs() {
            let mut at = s.default;
            let mut seen = vec![at];
            loop {
                at = next_value(s, at);
                if at == s.default {
                    break;
                }
                seen.push(at);
                assert!(seen.len() < 200, "{} never comes back", s.name);
            }
            assert!(seen.iter().all(|&v| s.parse(&s.text(v)) == Some(v)), "{}", s.name);
        }
    }

    #[test]
    fn the_registry_is_in_id_order_with_unique_names() {
        for (i, s) in REGISTRY.iter().enumerate() {
            assert_eq!(s.id as usize, i, "{} is out of place", s.name);
            assert_eq!(REGISTRY.iter().filter(|o| o.name == s.name).count(), 1);
            assert_eq!(
                s.parse(&s.text(s.default)),
                Some(s.default),
                "{} default round trip",
                s.name
            );
        }
        assert_eq!(REGISTRY.len(), COUNT);
    }

    #[test]
    fn every_extra_is_off_by_default() {
        let e = ClientExtras::default();
        assert!(e.is_default());
        assert_eq!(e.health_bars(), HealthBars::Off);
        assert!(!e.idle_worker());
        assert!(!e.group_bar());
        assert!(!e.smart_attack_move());
        assert_eq!(e.game_clock(), ClockMode::Off);
        // A volume's "off" leaves the sound alone.
        assert_eq!(e.trims(), crate::audio::Trims::default());
    }

    #[test]
    fn the_magnifying_zoom_is_a_client_option_that_is_off_by_default() {
        let mut e = ClientExtras::default();
        assert!(!e.map_zoom_wheel());
        assert_eq!(Id::MapZoom.spec().scope_mark(), "C");
        assert_eq!(Id::MapZoom.spec().name, "map-zoom");
        assert_eq!(Id::MapZoom.spec().values(), "off, wheel");
        Args::parse_from(["tidewater", "--extra-map-zoom", "wheel"])
            .extras
            .apply(&mut e);
        assert!(e.map_zoom_wheel() && e.sim().is_default());
        assert!(Args::try_parse_from(["tidewater", "--extra-map-zoom", "pinch"]).is_err());
        assert!(e.read_line("map-zoom", "off") && !e.map_zoom_wheel());
        assert!(e.section_text().contains("map-zoom=off"));
    }

    #[test]
    fn rally_points_are_off_by_default_and_the_flag_turns_them_on() {
        let mut e = ClientExtras::default();
        assert!(e.sim().is_default());
        Args::parse_from(["tidewater", "--extra-rally-points"])
            .extras
            .apply(&mut e);
        assert!(e.sim().rally_points);
        assert_eq!(local_sim_extras(&e), e.sim());
        assert_eq!(Id::RallyPoints.spec().scope_mark(), "S");
        Args::parse_from(["tidewater", "--no-extra-rally-points"])
            .extras
            .apply(&mut e);
        assert!(e.sim().is_default());
    }

    #[test]
    fn the_sim_options_map_to_the_sims_set_and_back() {
        use tidewater_sim::extras::UNLIMITED;
        let mut e = ClientExtras::default();
        assert!(e.sim().is_default() && !e.minimap_ping());
        e.set(Id::RallyPoints, 1);
        e.set(Id::ProductionQueue, 1);
        e.set(Id::SelectionLimit, 3);
        e.set(Id::SpeedLevels, 2);
        e.set(Id::ShiftQueue, 1);
        let sim = e.sim();
        assert_eq!(
            sim,
            tidewater_sim::Extras {
                rally_points: true,
                production_queue: true,
                selection_limit: UNLIMITED,
                extra_speeds: 2,
                order_queue: true,
            }
        );
        // The other limits.
        for (v, limit) in [(0, 0), (1, 12), (2, 18)] {
            e.set(Id::SelectionLimit, v);
            assert_eq!(e.sim().selection_limit, limit);
        }
        assert_eq!(local_sim_extras(&e), e.sim());
    }

    #[test]
    fn a_games_sim_options_show_as_registry_values_and_back() {
        let mut e = ClientExtras::default();
        for (id, v) in ClientExtras::of_sim(&e.sim()) {
            assert_eq!(v, id.spec().default, "{}", id.spec().name);
        }
        e.set(Id::RallyPoints, 1);
        e.set(Id::SelectionLimit, 2);
        e.set(Id::SpeedLevels, 3);
        e.set(Id::ShiftQueue, 1);
        let mut back = ClientExtras::default();
        for (id, v) in ClientExtras::of_sim(&e.sim()) {
            back.set(id, v);
        }
        assert_eq!(back.sim(), e.sim());
        for v in 0..4 {
            e.set(Id::SelectionLimit, v);
            let shown = ClientExtras::of_sim(&e.sim());
            assert_eq!(shown[2], (Id::SelectionLimit, v));
        }
    }

    #[test]
    fn the_sim_flags_parse() {
        let mut e = ClientExtras::default();
        Args::parse_from([
            "tidewater",
            "--extra-rally-points",
            "--extra-selection-limit",
            "18",
            "--extra-speed-levels",
            "3",
            "--extra-minimap-ping",
        ])
        .extras
        .apply(&mut e);
        assert!(e.sim().rally_points && e.minimap_ping());
        assert_eq!((e.sim().selection_limit, e.sim().extra_speeds), (18, 3));
        assert!(Args::try_parse_from(["tidewater", "--extra-selection-limit", "10"]).is_err());
    }

    #[test]
    fn auto_replay_is_off_by_default_and_a_client_option() {
        let mut e = ClientExtras::default();
        assert_eq!(e.replay_saving(), ReplaySaving::Off);
        Args::parse_from(["tidewater", "--extra-auto-replay", "always"])
            .extras
            .apply(&mut e);
        assert_eq!(e.replay_saving(), ReplaySaving::Always);
        assert!(e.sim().is_default(), "recording is not a rule of the game");
        Args::parse_from(["tidewater", "--extra-auto-replay", "menu"])
            .extras
            .apply(&mut e);
        assert_eq!(e.replay_saving(), ReplaySaving::Menu);
        assert_eq!(Id::AutoReplay.spec().scope_mark(), "C");
        assert_eq!(Id::ShiftQueue.spec().scope_mark(), "S");
        assert!(Args::try_parse_from(["tidewater", "--extra-auto-replay", "sometimes"]).is_err());
    }

    #[test]
    fn values_parse_by_kind() {
        let s = Id::IdleWorker.spec();
        assert_eq!(
            (s.parse("on"), s.parse("off"), s.parse("maybe")),
            (Some(1), Some(0), None)
        );
        let s = Id::HealthBars.spec();
        assert_eq!((s.parse("selected"), s.parse("sometimes")), (Some(2), None));
        let s = Id::VolumeMusic.spec();
        assert_eq!((s.parse("40"), s.parse("101"), s.parse("-1")), (Some(40), None, None));
    }

    #[test]
    fn the_settings_section_round_trips_and_ignores_bad_lines() {
        let mut e = ClientExtras::default();
        e.set(Id::HealthBars, 3);
        e.set(Id::VolumeMusic, 25);
        e.set(Id::IdleWorker, 1);
        let text = e.section_text();
        assert!(text.starts_with("[extras]\n"));
        let mut back = ClientExtras::default();
        for line in text.lines().filter(|l| !l.starts_with('#') && l.contains('=')) {
            let (n, v) = line.split_once('=').unwrap();
            assert!(back.read_line(n, v));
        }
        assert_eq!(back, e);
        // A bad value, an unknown name: the option keeps its value.
        assert!(!back.read_line("health-bars", "loud"));
        assert!(!back.read_line("no-such-extra", "on"));
        assert_eq!(back, e);
    }

    #[test]
    fn flags_set_and_reset_and_the_last_one_wins() {
        let parse = |argv: &[&str]| {
            let mut v = vec!["tidewater"];
            v.extend(argv);
            Args::parse_from(v).extras
        };
        let mut base = ClientExtras::default();
        base.set(Id::VolumeMusic, 30);
        base.set(Id::IdleWorker, 1);
        // Nothing given: nothing changes.
        let mut e = base;
        parse(&[]).apply(&mut e);
        assert_eq!(e, base);
        // Set a flag, a choice and a percentage; reset another.
        let mut e = base;
        parse(&[
            "--extra-health-bars",
            "always",
            "--extra-game-clock=clock-apm",
            "--extra-volume-voices",
            "60",
            "--no-extra-idle-worker",
            "--no-extra-volume-music",
        ])
        .apply(&mut e);
        assert_eq!(e.health_bars(), HealthBars::Always);
        assert_eq!(e.game_clock(), ClockMode::ClockApm);
        assert_eq!(e.get(Id::VolumeVoices), 60);
        assert!(!e.idle_worker(), "reset to the default");
        assert_eq!(e.get(Id::VolumeMusic), 100, "reset to the default");
        // The last of a pair wins.
        let mut e = ClientExtras::default();
        parse(&["--extra-idle-worker", "--no-extra-idle-worker"]).apply(&mut e);
        assert!(!e.idle_worker());
        let mut e = ClientExtras::default();
        parse(&["--no-extra-idle-worker", "--extra-idle-worker"]).apply(&mut e);
        assert!(e.idle_worker());
    }

    #[test]
    fn bad_flag_values_are_refused() {
        assert!(Args::try_parse_from(["tidewater", "--extra-health-bars", "loud"]).is_err());
        assert!(Args::try_parse_from(["tidewater", "--extra-volume-music", "101"]).is_err());
    }

    #[test]
    fn the_help_lists_every_option_from_the_table() {
        use clap::CommandFactory;
        let help = Args::command().render_long_help().to_string();
        for s in &REGISTRY {
            assert!(help.contains(&s.flag()), "--help lacks {}", s.flag());
            assert!(help.contains(s.description), "--help lacks the text of {}", s.name);
        }
        assert!(help.contains("all off by default"));
    }

    /// Scope C+L options follow the session's setting in a game with other players, not the local settings. (A stub
    /// until the lobby exists, M3: the sessions hand over what the lobby will.)
    #[test]
    fn a_non_local_session_sets_the_c_plus_l_options_and_leaves_the_c_ones() {
        let mut local = ClientExtras::default();
        local.set(Id::HealthBars, 3);
        local.set(Id::IdleWorker, 1);
        local.set(Id::VolumeMusic, 10);
        // A local session: the player's own.
        assert_eq!(effective(&local, None), local);
        // A session that holds its own (lobby setting: no health bars, and it shouts about nothing else).
        let mut lobby = ClientExtras::default();
        lobby.set(Id::IdleWorker, 0);
        lobby.set(Id::VolumeMusic, 99);
        let e = effective(&local, Some(lobby));
        assert_eq!(e.health_bars(), HealthBars::Off, "C+L follows the session");
        assert!(e.idle_worker(), "C stays the player's");
        assert_eq!(e.get(Id::VolumeMusic), 10, "C stays the player's");
        // And the session's own value wins when it allows them.
        lobby.set(Id::HealthBars, 2);
        assert_eq!(effective(&local, Some(lobby)).health_bars(), HealthBars::Selected);
    }

    #[test]
    fn a_replay_follows_its_recording_and_a_local_game_the_player() {
        use crate::session::{LocalSession, ReplaySession, Session};
        use tidewater_replay::Replay;
        let mut local = ClientExtras::default();
        local.set(Id::HealthBars, 3);
        assert_eq!(LocalSession::new().lobby_client_extras(), None);
        let header = tidewater_replay::Header {
            sim_build: "test".into(),
            map_name: "m.pud".into(),
            map_hash: 1,
            seed: 2,
            setup: Default::default(),
            lobby: None,
            rules: tidewater_replay::RulesTag {
                enhanced: true,
                multiplayer: false,
                cheats_disabled: false,
                extras: Default::default(),
            },
            start_cheats: 0,
            local: 0,
        };
        let replay = ReplaySession::new(Replay::new(header));
        let lobby = replay.lobby_client_extras();
        assert_eq!(
            lobby,
            Some(ClientExtras::default()),
            "a recording has no health bars of its own"
        );
        assert_eq!(effective(&local, lobby).health_bars(), HealthBars::Off);
    }

    /// Prints the table for the docs: `cargo test -p tidewater-client print_the_doc_table -- --ignored --nocapture`.
    #[test]
    #[ignore = "prints the table to paste into docs/design/extras.md"]
    fn print_the_doc_table() {
        println!("{}", doc_table());
    }

    /// The documentation keeps the generated table, so the docs and the code can't drift apart.
    #[test]
    fn the_docs_carry_the_generated_table() {
        let doc = include_str!("../../../docs/design/extras.md");
        let table = doc_table();
        assert!(
            doc.contains(&table),
            "docs/design/extras.md must hold this table exactly (copy it from `extras::doc_table`):\n{table}"
        );
        let readme = include_str!("../../../README.md");
        for s in &REGISTRY {
            assert!(readme.contains(&s.flag()), "README.md lacks {}", s.flag());
        }
    }
}
