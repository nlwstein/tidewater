//! The player's stored settings (spec `docs/specs/ui/game-menu.md`, "Stored settings"): game speed, the two scroll
//! speeds, the two volumes, the next tip number and the option bits. No SDL types here.
//!
//! Choices where the spec leaves a gap:
//! - The text form is `name=value` lines. Unknown names, bad numbers and numbers outside their range fall back to
//!   the default ("A stored number outside its range is replaced by the default").
//! - "Where they are stored is the reimplementation's choice": the file `settings.txt` in Tidewater's own per-user data
//!   directory, or in the folder given with `--save-dir` (so a run with its own save folder leaves the real settings
//!   alone), or the file given with `--settings`. [`Prefs::load`] reads it at start-up (a missing or unreadable file
//!   means first-run defaults) and [`Prefs::store`] writes it when the program exits normally (`main.rs`).
//! - Command-line options (`--mouse-scroll-speed`, `--key-scroll-speed`, `--second-mouse-style`) given on top of the
//!   file override it for the run, and are stored at exit like any other change ([`Prefs::with_args`]).
//! - The minimap terrain mode is a field here, but it is not stored and starts on ("Not stored").

/// The option bits of the preferences word ("Option bits").
#[allow(dead_code, reason = "the spec's bit table is kept whole")]
pub mod bit {
    pub const UNIT_SPEECH: u32 = 0x1;
    pub const ACKNOWLEDGEMENTS: u32 = 0x2;
    pub const BUILDING_SOUNDS: u32 = 0x4;
    pub const MUSIC: u32 = 0x8;
    pub const COLOUR_CYCLING: u32 = 0x40;
    pub const TIPS_AT_START: u32 = 0x100;
    pub const OPENING_FILM: u32 = 0x200;
    /// The first mouse style (right-click commands).
    pub const FIRST_MOUSE_STYLE: u32 = 0x800;
    pub const SYSTEM_POINTER: u32 = 0x1000;
    pub const STEREO: u32 = 0x1_0000;
    pub const SOUND_3D: u32 = 0x2_0000;
    pub const SUBTITLES: u32 = 0x4_0000;
    pub const SPEECH_RECOGNITION: u32 = 0x8_0000;
    pub const SPEAK_MESSAGES: u32 = 0x10_0000;

    /// The bits that are on by default.
    pub const DEFAULT: u32 = UNIT_SPEECH
        | ACKNOWLEDGEMENTS
        | BUILDING_SOUNDS
        | MUSIC
        | COLOUR_CYCLING
        | TIPS_AT_START
        | OPENING_FILM
        | FIRST_MOUSE_STYLE
        | STEREO
        | SOUND_3D;
}

/// The settings file's name in the user folder.
pub const FILE_NAME: &str = "settings.txt";

pub const MAX_GAME_SPEED: u8 = 8;
pub const MAX_SCROLL_SPEED: u8 = 6;
pub const MAX_VOLUME: u8 = 100;
/// The tips table has this many entries.
pub const TIPS: u32 = 60;

pub const DEFAULT_GAME_SPEED: u8 = 5;
pub const DEFAULT_SCROLL_SPEED: u8 = 4;
pub const DEFAULT_MUSIC_VOLUME: u8 = 75;
pub const DEFAULT_DIGITAL_VOLUME: u8 = 100;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prefs {
    /// The game speed as a 9-level value.
    pub game_speed: u8,
    pub mouse_scroll: u8,
    pub key_scroll: u8,
    pub music_volume: u8,
    pub digital_volume: u8,
    /// The next tip to show; 0 means "start at 1".
    pub next_tip: u32,
    pub bits: u32,
    /// The minimap's terrain mode. Not stored: on when the program starts.
    pub minimap_terrain: bool,
    /// The optional improvements as the file's `[extras]` section sets them (`extras`). The command line's overrides
    /// are not in here: they last for the run and are never written back.
    pub extras: crate::extras::ClientExtras,
    /// The keys the `[keys]` section rebinds (`keymap`); empty, so the original's keys, when the file has none.
    pub keys: crate::keymap::KeyMap,
    /// What was wrong in the `[keys]` section when it was read, one message a line.
    pub key_problems: Vec<String>,
    /// The enabled mods' folder names in the `[mods]` section, one a line, lowest priority first (a later mod overrides
    /// an earlier one; `tidewater_formats::mods`, `docs/mods.md`). They apply from the next start.
    pub mods: Vec<String>,
    /// The multiplayer menus' memory (`mpmenu`): the player's name, the connection method last used (0 Battle.net,
    /// 1 Direct IP, 2 Local Network), the Direct IP Mode (Answer rather than Call), the address and the port.
    pub mp_name: String,
    pub mp_method: u8,
    pub mp_answer: bool,
    pub mp_address: String,
    pub mp_port: String,
    /// Whether the start-up check for a newer release runs (`update`); the `update_check` line, on by default.
    pub update_check: bool,
    /// The "move Tidewater to Applications" notice (`update`) has been shown once; the `move_notice_seen` line.
    pub move_notice_seen: bool,
    /// The Touch Controls help (`touchhelp`) has shown by itself once, at the first game on a phone or tablet; the
    /// `touch_help_seen` line.
    pub touch_help_seen: bool,
    /// Whether the desktop window opens (and stays) full screen; the `fullscreen` line, off by default
    /// (`settings`, the Settings screen's Window row).
    pub fullscreen: bool,
    /// Whether the mouse lock holds the pointer in the window during a game (`display`, "Mouse lock"); the `mouse_lock`
    /// line, on by default. `--no-mouse-lock` turns it off for a run whatever this says.
    pub mouse_lock: bool,
    /// Ctrl+M's remembered music volume, while the music is off by it.
    music_before: Option<u8>,
    /// Ctrl+S's remembered digital volume.
    digital_before: Option<u8>,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            game_speed: DEFAULT_GAME_SPEED,
            mouse_scroll: DEFAULT_SCROLL_SPEED,
            key_scroll: DEFAULT_SCROLL_SPEED,
            music_volume: DEFAULT_MUSIC_VOLUME,
            digital_volume: DEFAULT_DIGITAL_VOLUME,
            next_tip: 0,
            bits: bit::DEFAULT,
            minimap_terrain: true,
            extras: crate::extras::ClientExtras::default(),
            keys: crate::keymap::KeyMap::default(),
            key_problems: Vec::new(),
            mods: Vec::new(),
            mp_name: String::new(),
            mp_method: 1,
            mp_answer: false,
            mp_address: String::new(),
            mp_port: "6112".to_string(),
            update_check: true,
            move_notice_seen: false,
            touch_help_seen: false,
            fullscreen: false,
            mouse_lock: true,
            music_before: None,
            digital_before: None,
        }
    }
}

impl Prefs {
    /// These settings with the scroll speeds and the mouse style the command line gave, where it gave them.
    pub fn with_args(mut self, mouse_scroll: Option<u8>, key_scroll: Option<u8>, second_mouse_style: bool) -> Self {
        if let Some(s) = mouse_scroll {
            self.mouse_scroll = s.min(MAX_SCROLL_SPEED);
        }
        if let Some(s) = key_scroll {
            self.key_scroll = s.min(MAX_SCROLL_SPEED);
        }
        if second_mouse_style {
            self.set(bit::FIRST_MOUSE_STYLE, false);
        }
        self
    }

    /// Reads the settings file at program start ("Loading"); no file, or one that can't be read, gives the defaults.
    pub fn load(storage: &dyn crate::storage::Storage) -> Self {
        let p = storage
            .read_settings()
            .and_then(|b| String::from_utf8(b).ok())
            .map_or_else(Self::default, |s| Self::from_text(&s));
        for m in &p.key_problems {
            eprintln!("settings: [keys] {m}");
        }
        p
    }

    /// Writes the settings file ("Saving": when the program exits normally).
    pub fn store(&self, storage: &dyn crate::storage::Storage) -> std::io::Result<()> {
        storage.write_settings(self.to_text().as_bytes())
    }

    /// Reads the text form: every setting takes its default, then the stored value when it is in range.
    pub fn from_text(s: &str) -> Self {
        let mut p = Self::default();
        // A `[extras]` or `[keys]` line starts that section, which goes to the next section line or the file's end.
        let mut in_extras = false;
        let mut in_keys = false;
        let mut in_mods = false;
        let mut key_lines: Vec<String> = Vec::new();
        for line in s.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                in_extras = line == "[extras]";
                in_keys = line == "[keys]";
                in_mods = line == "[mods]";
                continue;
            }
            if in_mods {
                // A name per line; a comment, an empty line or a name that could not be a mod's is skipped.
                if tidewater_formats::mods::valid_name(line) && !p.mods.iter().any(|m| m.eq_ignore_ascii_case(line)) {
                    p.mods.push(line.to_string());
                }
                continue;
            }
            if in_keys {
                key_lines.push(line.to_string());
                continue;
            }
            let Some((name, value)) = line.split_once('=') else {
                continue;
            };
            if in_extras {
                p.extras.read_line(name, value);
                continue;
            }
            // The multiplayer menus' text settings; a number that is out of range or text that is too long is ignored.
            let text = value.trim();
            match name.trim() {
                "mp_name" if text.chars().count() <= 11 => p.mp_name = text.to_string(),
                "mp_address" if text.chars().count() <= 63 && !text.contains(' ') => p.mp_address = text.to_string(),
                "mp_port" if text.parse::<u16>().is_ok() => p.mp_port = text.to_string(),
                "mp_method" => p.mp_method = text.parse::<u8>().ok().filter(|&m| m <= 2).unwrap_or(p.mp_method),
                "mp_answer" => p.mp_answer = text == "1",
                "update_check" => p.update_check = text != "0",
                "move_notice_seen" => p.move_notice_seen = text == "1",
                "touch_help_seen" => p.touch_help_seen = text == "1",
                "fullscreen" => p.fullscreen = text == "1",
                "mouse_lock" => p.mouse_lock = text != "0",
                _ => {}
            }
            let Ok(n) = value.trim().parse::<u32>() else {
                continue;
            };
            let within = |max: u8| u8::try_from(n).ok().filter(|&v| v <= max);
            match name.trim() {
                "game_speed" => p.game_speed = within(MAX_GAME_SPEED).unwrap_or(p.game_speed),
                "mouse_scroll" => p.mouse_scroll = within(MAX_SCROLL_SPEED).unwrap_or(p.mouse_scroll),
                "key_scroll" => p.key_scroll = within(MAX_SCROLL_SPEED).unwrap_or(p.key_scroll),
                "music_volume" => p.music_volume = within(MAX_VOLUME).unwrap_or(p.music_volume),
                "digital_volume" => p.digital_volume = within(MAX_VOLUME).unwrap_or(p.digital_volume),
                "next_tip" => p.next_tip = n,
                "bits" => p.bits = n,
                _ => {}
            }
        }
        (p.keys, p.key_problems) = crate::keymap::KeyMap::parse(&key_lines);
        p
    }

    /// The text form of everything stored, ending with the `[extras]` section.
    pub fn to_text(&self) -> String {
        format!(
            "game_speed={}\nmouse_scroll={}\nkey_scroll={}\nmusic_volume={}\ndigital_volume={}\nnext_tip={}\nbits={}\nmp_name={}\nmp_method={}\nmp_answer={}\nmp_address={}\nmp_port={}\nupdate_check={}\nmove_notice_seen={}\ntouch_help_seen={}\nfullscreen={}\nmouse_lock={}\n{}{}",
            self.game_speed,
            self.mouse_scroll,
            self.key_scroll,
            self.music_volume,
            self.digital_volume,
            self.next_tip,
            self.bits,
            self.mp_name,
            self.mp_method,
            u8::from(self.mp_answer),
            self.mp_address,
            self.mp_port,
            u8::from(self.update_check),
            u8::from(self.move_notice_seen),
            u8::from(self.touch_help_seen),
            u8::from(self.fullscreen),
            u8::from(self.mouse_lock),
            self.extras.section_text(),
            self.mods_section_text() + &self.keys.section_text()
        )
    }

    /// The `[mods]` section, empty when no mod is enabled.
    fn mods_section_text(&self) -> String {
        if self.mods.is_empty() {
            return String::new();
        }
        format!("[mods]\n{}\n", self.mods.join("\n"))
    }

    pub fn has(&self, bit: u32) -> bool {
        self.bits & bit != 0
    }

    pub fn set(&mut self, bit: u32, on: bool) {
        if on {
            self.bits |= bit;
        } else {
            self.bits &= !bit;
        }
    }

    /// Whether palette cycling is on (`ui/palette-effects.md`).
    // TODO: palette.rs reads this once it is on main.
    #[allow(dead_code, reason = "palette.rs reads it once it is on main")]
    pub fn colour_cycling(&self) -> bool {
        self.has(bit::COLOUR_CYCLING)
    }

    /// Ctrl+M: music volume to 0 with the old value remembered, or the remembered value back. Returns whether the
    /// music is now on. No bit changes.
    #[allow(dead_code, reason = "music.rs calls it from its Ctrl+M key arm")]
    pub fn toggle_music(&mut self) -> bool {
        toggle(&mut self.music_volume, &mut self.music_before)
    }

    /// Ctrl+S: the same for the digital volume.
    pub fn toggle_digital(&mut self) -> bool {
        toggle(&mut self.digital_volume, &mut self.digital_before)
    }

    /// Takes the tip to show now and moves the stored number on ("Tips at the start of a game").
    pub fn take_tip(&mut self) -> u32 {
        let n = if self.next_tip == 0 || self.next_tip > TIPS {
            1
        } else {
            self.next_tip
        };
        self.next_tip = if n + 1 > TIPS { 1 } else { n + 1 };
        n
    }

    /// The digital volume on the sound module's 0 to 255 scale (our range).
    pub fn digital_level(&self) -> u8 {
        (self.digital_volume as u32 * 255).div_ceil(100) as u8
    }
}

fn toggle(volume: &mut u8, before: &mut Option<u8>) -> bool {
    match before.take() {
        Some(v) => {
            *volume = v;
            true
        }
        None => {
            *before = Some(*volume);
            *volume = 0;
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_run_defaults() {
        let p = Prefs::from_text("");
        assert_eq!(
            (
                p.game_speed,
                p.mouse_scroll,
                p.key_scroll,
                p.music_volume,
                p.digital_volume
            ),
            (5, 4, 4, 75, 100)
        );
        assert_eq!(p.bits, bit::DEFAULT);
        assert!(p.has(bit::UNIT_SPEECH | bit::MUSIC | bit::COLOUR_CYCLING | bit::FIRST_MOUSE_STYLE));
        assert!(!p.has(bit::SUBTITLES) && !p.has(bit::SYSTEM_POINTER));
        assert!(!p.has(bit::SPEECH_RECOGNITION) && !p.has(bit::SPEAK_MESSAGES));
        assert!(p.minimap_terrain);
    }

    #[test]
    fn the_window_and_mouse_lock_settings_round_trip() {
        let p = Prefs::from_text("");
        assert!(
            !p.fullscreen && p.mouse_lock,
            "windowed with the mouse lock on, by default"
        );
        let set = Prefs {
            fullscreen: true,
            mouse_lock: false,
            ..Prefs::default()
        };
        let text = set.to_text();
        assert!(text.contains("fullscreen=1\n") && text.contains("mouse_lock=0\n"));
        let back = Prefs::from_text(&text);
        assert!(back.fullscreen && !back.mouse_lock);
        // Anything but the stored words falls back to the defaults' meaning.
        let odd = Prefs::from_text("fullscreen=yes\nmouse_lock=maybe\n");
        assert!(!odd.fullscreen && odd.mouse_lock);
    }

    #[test]
    fn the_move_notice_is_remembered() {
        assert!(!Prefs::from_text("").move_notice_seen);
        let seen = Prefs {
            move_notice_seen: true,
            ..Prefs::default()
        };
        assert!(Prefs::from_text(&seen.to_text()).move_notice_seen);
    }

    #[test]
    fn the_touch_help_is_remembered() {
        assert!(!Prefs::from_text("").touch_help_seen);
        let seen = Prefs {
            touch_help_seen: true,
            ..Prefs::default()
        };
        assert!(seen.to_text().contains("touch_help_seen=1\n"));
        assert!(Prefs::from_text(&seen.to_text()).touch_help_seen);
    }

    #[test]
    fn the_update_check_is_on_unless_the_file_turns_it_off() {
        assert!(Prefs::from_text("").update_check);
        assert!(!Prefs::from_text("update_check=0").update_check);
        let off = Prefs {
            update_check: false,
            ..Prefs::default()
        };
        assert!(!Prefs::from_text(&off.to_text()).update_check);
    }

    #[test]
    fn out_of_range_values_take_the_default() {
        let p = Prefs::from_text("mouse_scroll=9\ngame_speed=8\nmusic_volume=101\nkey_scroll=x\nnext_tip=60");
        assert_eq!(p.mouse_scroll, 4);
        assert_eq!(p.game_speed, 8);
        assert_eq!(p.music_volume, 75);
        assert_eq!(p.key_scroll, 4);
        assert_eq!(p.next_tip, 60);
    }

    #[test]
    fn text_round_trip() {
        let mut p = Prefs {
            mouse_scroll: 6,
            music_volume: 0,
            next_tip: 17,
            ..Prefs::default()
        };
        p.set(bit::SUBTITLES, true);
        assert_eq!(Prefs::from_text(&p.to_text()), p);
    }

    #[test]
    fn the_extras_section_is_read_and_written_and_the_rest_is_unaffected() {
        use crate::extras::{HealthBars, Id};
        // An old file without the section: every extra at its default.
        assert!(Prefs::from_text("game_speed=7\nbits=3\n").extras.is_default());
        let p = Prefs::from_text(
            "game_speed=7\n[extras]\n# a comment\nhealth-bars=selected\nvolume-music=20\nbogus=1\ngame_speed=2\n",
        );
        assert_eq!(p.game_speed, 7, "a line in the section is not a main setting");
        assert_eq!(p.extras.health_bars(), HealthBars::Selected);
        assert_eq!(p.extras.get(Id::VolumeMusic), 20);
        assert_eq!(Prefs::from_text(&p.to_text()), p);
        // The main settings come first, so an older build reads them from a newer file.
        let text = Prefs::default().to_text();
        assert!(text.starts_with("game_speed=5\n"));
        assert!(text.contains("\n[extras]\n"));
        assert!(Prefs::from_text(&text).extras.is_default());
    }

    #[test]
    fn the_mods_section_keeps_the_order_and_drops_what_cannot_be_a_mod() {
        assert!(Prefs::default().mods.is_empty());
        assert!(!Prefs::default().to_text().contains("[mods]"));
        let p = Prefs::from_text(
            "game_speed=7\n[extras]\nhealth-bars=always\n[mods]\nbase pack\n# off\n..\n../x\nsounds\nBase Pack\n[keys]\npause=ctrl+u\n",
        );
        assert_eq!(
            p.mods,
            ["base pack", "sounds"],
            "order kept, a repeat and path names dropped"
        );
        assert_eq!(p.extras.health_bars(), crate::extras::HealthBars::Always);
        assert!(!p.keys.is_default(), "a section after [mods] still reads");
        assert_eq!(p.game_speed, 7);
        let again = Prefs::from_text(&p.to_text());
        assert_eq!(again, p);
        assert!(p.to_text().contains("[mods]\nbase pack\nsounds\n"));
    }

    #[test]
    fn the_keys_section_is_read_kept_and_written_back_as_it_was() {
        // No section: the original's keys, and none is written.
        let p = Prefs::from_text("game_speed=7\n[extras]\nhealth-bars=always\n");
        assert!(p.keys.is_default() && p.key_problems.is_empty());
        assert!(!p.to_text().contains("[keys]"));
        // A section after the extras one, with a mistake in it: the good line works, the bad one is reported and
        // kept for the next write, and the extras before it are unaffected.
        let text = "game_speed=7\n[extras]\nhealth-bars=always\n[keys]\n# mine\npause=ctrl+u\nspeed-up=f5\n";
        let p = Prefs::from_text(text);
        assert_eq!(p.extras.health_bars(), crate::extras::HealthBars::Always);
        assert!(!p.keys.is_default());
        assert_eq!(p.key_problems.len(), 1, "{:?}", p.key_problems);
        assert!(p.key_problems[0].contains("speed-up") && p.key_problems[0].contains("options"));
        let written = p.to_text();
        assert!(
            written.ends_with("[keys]\n# mine\npause=ctrl+u\nspeed-up=f5\n"),
            "{written}"
        );
        assert_eq!(Prefs::from_text(&written), p);
        // A [keys] section before [extras] still leaves the extras section readable.
        let p = Prefs::from_text("[keys]\npause=ctrl+u\n[extras]\nvolume-music=20\n");
        assert_eq!(p.extras.get(crate::extras::Id::VolumeMusic), 20);
        assert!(!p.keys.is_default());
    }

    #[test]
    fn tip_numbers() {
        let mut p = Prefs::from_text("next_tip=60");
        assert_eq!((p.take_tip(), p.next_tip), (60, 1));
        let mut p = Prefs::default();
        assert_eq!((p.take_tip(), p.next_tip), (1, 2));
        p.next_tip = 61;
        assert_eq!((p.take_tip(), p.next_tip), (1, 2));
    }

    #[test]
    fn ctrl_m_swaps_the_volume_and_keeps_the_bits() {
        let mut p = Prefs::default();
        let bits = p.bits;
        assert!(!p.toggle_music());
        assert_eq!(p.music_volume, 0);
        // A program that exits now stores 0.
        assert_eq!(Prefs::from_text(&p.to_text()).music_volume, 0);
        assert!(p.toggle_music());
        assert_eq!(p.music_volume, 75);
        assert_eq!(p.bits, bits);
        assert!(!p.toggle_digital());
        assert_eq!(p.digital_volume, 0);
        assert!(p.toggle_digital());
        assert_eq!(p.digital_volume, 100);
    }

    #[test]
    fn the_command_line_picks_scroll_and_mouse_style() {
        let p = Prefs::default().with_args(Some(2), Some(9), true);
        assert_eq!((p.mouse_scroll, p.key_scroll), (2, 6));
        assert!(!p.has(bit::FIRST_MOUSE_STYLE));
        assert!(
            Prefs::default()
                .with_args(None, None, false)
                .has(bit::FIRST_MOUSE_STYLE)
        );
        // Options not given leave the stored values.
        let stored = Prefs::from_text("mouse_scroll=1\nkey_scroll=2");
        let p = stored.clone().with_args(None, Some(5), false);
        assert_eq!((p.mouse_scroll, p.key_scroll), (1, 5));
    }

    #[test]
    fn the_multiplayer_choices_round_trip_and_bad_values_are_ignored() {
        let p = Prefs {
            mp_name: "Fen".into(),
            mp_method: 2,
            mp_answer: true,
            mp_address: "10.1.2.3".into(),
            mp_port: "7000".into(),
            ..Prefs::default()
        };
        let q = Prefs::from_text(&p.to_text());
        assert_eq!((q.mp_name.as_str(), q.mp_method, q.mp_answer), ("Fen", 2, true));
        assert_eq!((q.mp_address.as_str(), q.mp_port.as_str()), ("10.1.2.3", "7000"));
        let bad = Prefs::from_text("mp_port=99999\nmp_method=9\nmp_name=abcdefghijklm\n");
        assert_eq!((bad.mp_port.as_str(), bad.mp_method), ("6112", 1));
        assert_eq!(bad.mp_name, "");
    }

    #[test]
    fn the_settings_file_round_trips_and_a_missing_one_gives_defaults() {
        let dir = std::env::temp_dir().join(format!("tidewater-prefs-{}", std::process::id()));
        let path = dir.join("sub").join("settings.txt");
        let storage = crate::storage::Desktop::new(dir.join("install"), dir.join("Save"), path, dir.clone());
        assert_eq!(Prefs::load(&storage), Prefs::default());
        let mut p = Prefs {
            game_speed: 7,
            digital_volume: 30,
            ..Prefs::default()
        };
        p.set(bit::COLOUR_CYCLING, false);
        p.store(&storage).unwrap();
        assert_eq!(Prefs::load(&storage), p);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn digital_level_spans_the_sound_range() {
        let mut p = Prefs::default();
        assert_eq!(p.digital_level(), 255);
        p.digital_volume = 0;
        assert_eq!(p.digital_level(), 0);
    }
}
