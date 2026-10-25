//! Which music track plays when, how it ends, fades and how loud it is. Spec: `docs/specs/ui/music.md`. No SDL types
//! here; `jukebox.rs` plays the result in the core's mixer.
//!
//! [`Music`] is the state machine: the track table ("Data"), the start rules (same track, the *Tides of Darkness*
//! bump, volume 0), the two "start if silent" tests, the fades, the chain gap, the volume table and the armed
//! restart-on-raise rule ("Volume"). The client calls [`Music::update`] every frame with the playback position and
//! applies the [`Action`]s it returns. The track draw for a non-campaign game uses the setup-shuffle generator
//! ([`crate::sound::Shuffle`], the one the sound variants use); music never touches the synced game generator.
//!
//! Choices of our own where the spec leaves a gap: fades are not blocking, they run over the following `update` calls
//! (the first step at once when the fade begins, the next ones 20 ms apart, as specified), and a start that must
//! follow a fade waits for it, as must a screen that the spec has wait for a fade (the act screen's close); the position of an open track is
//! reported by the caller in bytes of audio, and a voice that has played out counts as the end; a loop track loops in
//! the core's mixer from its loop point, without a gap (`audio`), and the [`Action::SeekLoop`] asked for when the
//! position wraps is left for a player that can't loop from a point (the mixer ignores it); the chain gap counts from the next
//! `update` after the one that saw the end (the spec's "first check after the end"); a track that reached its end
//! stays open and "started" until something closes it; the music draw is made by the caller right after game setup
//! (open question "Order of the music draw"); the whole file is read into memory at load (the "streaming" is the
//! decoder's), a chain's next track ahead of time on a thread of its own (`jukebox`); fades of all sound only ask the caller to cut the effects ([`Action::CutEffects`]); the music volume
//! starts at 75, the first-run default of `ui/game-menu.md`, as nothing stores settings yet.
//!
//! Not yet here: the fade when the program exits, the attract demo start, a custom game starting from the
//! custom-scenario screen and entering Battle.net (their screens do not exist yet); the sound dialog (it calls
//! `Jukebox::set_volume`); blocking fades (see above).
//!
//! Our choice for the campaign flow: the briefing's track is closed at once when the game screen opens, so the game's
//! own track plays rather than the briefing's carrying on (the spec's "only if not started" would let it).

use crate::sound::Shuffle;

/// Bytes of audio per second of the music files: 22,050 Hz, 16-bit, stereo ("Files").
pub const BYTES_PER_SECOND: u32 = 88_200;
/// Bytes per sample frame.
pub const BYTES_PER_FRAME: u32 = 4;
/// The silence between chained tracks, in milliseconds ("Constants").
pub const GAP_MS: u32 = 2_000;
/// The end fade starts when fewer than this many bytes of audio remain.
pub const END_FADE_BYTES: u32 = 616_000;
/// Milliseconds per fade step.
pub const STEP_MS: u32 = 20;
/// Silent, in hundredths of a decibel.
pub const SILENT: i32 = -10_000;
/// A fade stop ends at the first step below this.
pub const FADE_STOP_FLOOR: i32 = -4_000;
/// An end fade ends at the first step below this.
pub const END_FADE_FLOOR: i32 = -3_396;
/// The volume Ctrl+M restores when none was remembered.
pub const REMEMBERED_AT_START: u8 = 25;
/// The music volume of a first run (`ui/game-menu.md`).
pub const DEFAULT_VOLUME: u8 = 75;
/// A campaign entry below this is *Tides of Darkness*.
pub const TIDES_ENTRIES: u8 = 28;
/// The disco track.
pub const DISCO: u8 = 25;

/// How a track ends ("Tracks").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    /// Plays to its end, then silence.
    Once,
    /// Loops without a gap from sample frame `point` (0 is the start of the file).
    Loop { point: u32 },
    /// After the file, 2 s of silence, then track `next`.
    Chain { next: u8 },
}

/// One row of the track table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Track {
    /// The file name in the install's `Music` folder.
    pub file: &'static str,
    pub end: End,
}

const fn chain(file: &'static str, next: u8) -> Track {
    Track {
        file,
        end: End::Chain { next },
    }
}

const fn once(file: &'static str) -> Track {
    Track { file, end: End::Once }
}

const fn looped(file: &'static str, point: u32) -> Track {
    Track {
        file,
        end: End::Loop { point },
    }
}

/// The 26 tracks ("Tracks"). Tracks 18 and 19 are never started.
pub const TRACKS: [Track; 26] = [
    chain("HUMAN1.WAV", 1),
    chain("HUMAN2.WAV", 2),
    chain("HUMAN3.WAV", 3),
    chain("HUMAN4.WAV", 4),
    chain("HUMAN5.WAV", 5),
    chain("HUMAN6.WAV", 0),
    chain("ORC1.WAV", 7),
    chain("ORC2.WAV", 8),
    chain("ORC3.WAV", 9),
    chain("ORC4.WAV", 10),
    chain("ORC5.WAV", 11),
    chain("ORC6.WAV", 6),
    once("HDEFEAT.WAV"),
    once("ODEFEAT.WAV"),
    once("HVICTORY.WAV"),
    once("OVICTORY.WAV"),
    chain("HUMAN5.WAV", 17),
    chain("HUMAN6.WAV", 16),
    chain("ORC5.WAV", 19),
    chain("ORC6.WAV", 18),
    looped("HWARROOM.WAV", 66_596),
    looped("OWARROOM.WAV", 0),
    looped("OWARROOM.WAV", 0),
    chain("HUMAN4.WAV", 24),
    chain("HUMAN5.WAV", 23),
    looped("DISCOWC.WAV", 0),
];

/// The archive name of a track's file.
pub fn path(track: u8) -> String {
    format!("Music\\{}", TRACKS[track as usize].file)
}

/// Attenuation in hundredths of a decibel for volume 1 to 100 ("Volume table").
pub const VOLUME_TABLE: [i16; 100] = [
    -4000, -3396, -3044, -2794, -2602, -2442, -2308, -2192, -2090, -2000, //
    -1917, -1840, -1772, -1706, -1646, -1590, -1539, -1489, -1442, -1396, //
    -1354, -1315, -1275, -1238, -1204, -1170, -1137, -1104, -1075, -1044, //
    -1017, -988, -961, -937, -910, -887, -862, -840, -816, -794, //
    -774, -752, -733, -713, -692, -674, -654, -636, -618, -602, //
    -583, -566, -551, -535, -519, -502, -488, -473, -458, -442, //
    -429, -415, -401, -386, -374, -359, -346, -333, -322, -308, //
    -297, -285, -273, -260, -248, -238, -227, -214, -203, -192, //
    -183, -172, -160, -151, -141, -131, -119, -111, -101, -90, //
    -80, -72, -63, -52, -43, -35, -26, -20, -20, -20,
];

/// The attenuation of music volume `v` (0 to 100): silent at 0, else the table entry `v`.
pub fn attenuation(v: u8) -> i32 {
    match v {
        0 => SILENT,
        v => VOLUME_TABLE[v.min(100) as usize - 1] as i32,
    }
}

/// Hundredths of a decibel to decibels, for the player.
pub fn decibels(centi: i32) -> f32 {
    centi as f32 / 100.0
}

/// The attenuation after step `n` (from 1) of a fade stop: it drops by 0.10 dB times `n`.
pub fn fade_stop_step(atten: i32, n: u32) -> i32 {
    atten - 10 * n as i32
}

/// Every attenuation a fade stop passes through from `start`, the last being the first below the floor.
pub fn fade_stop_steps(start: i32) -> Vec<i32> {
    let mut out = Vec::new();
    let mut a = start;
    let mut n = 1;
    while a >= FADE_STOP_FLOOR {
        a = fade_stop_step(a, n);
        out.push(a);
        n += 1;
    }
    out
}

/// Every attenuation an end fade passes through from `start`: 0.10 dB per step until below the floor.
pub fn end_fade_steps(start: i32) -> Vec<i32> {
    let mut out = Vec::new();
    let mut a = start;
    while a >= END_FADE_FLOOR {
        a -= 10;
        out.push(a);
    }
    out
}

/// The track of a campaign mission ("The game's track"): 6 x race + (entry / 2 mod 6).
pub fn campaign_track(orc: bool, entry: u8) -> u8 {
    6 * orc as u8 + (entry / 2) % 6
}

/// The track of any other game from one draw `d` (0 to 32767) of the setup-shuffle generator.
pub fn drawn_track(orc: bool, d: u32) -> u8 {
    6 * orc as u8 + (d % 6) as u8
}

/// The track a new game gets: the disco track if the cheat is on, else the campaign formula or one draw. The caller
/// turns the cheat off.
pub fn game_track(orc: bool, campaign: Option<u8>, disco: bool, shuffle: &mut Shuffle) -> u8 {
    if disco {
        DISCO
    } else if let Some(entry) = campaign {
        campaign_track(orc, entry)
    } else {
        drawn_track(orc, shuffle.draw())
    }
}

/// The score screen's track ("Where tracks start"): victory 14 or 15, defeat 12 or 13.
pub fn score_track(victory: bool, orc: bool) -> u8 {
    (if victory { 14 } else { 12 }) + orc as u8
}

/// The briefing's track: 20 for the human, 21 for the orc.
pub fn briefing_track(orc: bool) -> u8 {
    20 + orc as u8
}

/// The campaign finale's track: 16 or 17.
pub fn finale_track(orc: bool) -> u8 {
    16 + orc as u8
}

/// The title screen's and the main menu's track.
pub const TITLE_TRACK: u8 = 22;
/// The credits' track.
pub const CREDITS_TRACK: u8 = 23;

/// The bytes of audio in a WAV file (its `data` chunk), or `None` if it isn't RIFF/WAVE.
pub fn wav_data_len(bytes: &[u8]) -> Option<u32> {
    if bytes.get(..4)? != b"RIFF" || bytes.get(8..12)? != b"WAVE" {
        return None;
    }
    let mut at = 12;
    while let Some(head) = bytes.get(at..at + 8) {
        let size = u32::from_le_bytes(head[4..8].try_into().ok()?);
        if &head[..4] == b"data" {
            return Some(size);
        }
        at += 8 + size as usize + (size as usize & 1);
    }
    None
}

/// What the client must do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Close the open track at once.
    Stop,
    /// Close any open track, load this file and play it from its start at this attenuation.
    Load {
        track: u8,
        file: &'static str,
        looped: bool,
        attenuation: i32,
    },
    /// Set the open track's attenuation (hundredths of a decibel).
    SetAttenuation(i32),
    /// Jump to this sample frame (a loop track's loop point).
    SeekLoop { frame: u32 },
    /// A fade of all sound began: cut the sound effects.
    CutEffects,
}

/// Where the open track is, in bytes of audio, as the caller measures it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Playback {
    pub position: u32,
    pub length: u32,
}

/// The track that is open (loaded) and what it is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Open {
    track: u8,
    /// The position reached the end; the track is silent but still counts as started.
    ended: bool,
    /// The end fade is running.
    end_fading: bool,
    /// Milliseconds since the first check after the end (chain tracks).
    gap: Option<u32>,
}

/// A fade stop in progress ("Where music fades out").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Fade {
    steps: u32,
    /// The track to start (if not started) when the fade is done.
    then: Option<u8>,
}

/// The music state.
#[derive(Clone, Debug)]
pub struct Music {
    volume: u8,
    remembered: u8,
    /// A restart was done since the last volume 0 or program start.
    restart_done: bool,
    /// The current track, after the bump.
    current: Option<u8>,
    /// A single-player *Tides of Darkness* campaign game: tracks 0 and 6 become 1 and 7.
    bump: bool,
    open: Option<Open>,
    atten: i32,
    fade: Option<Fade>,
    /// Milliseconds not yet spent on 20 ms steps.
    carry: u32,
    last_position: u32,
}

impl Default for Music {
    fn default() -> Self {
        Music::new(DEFAULT_VOLUME)
    }
}

impl Music {
    /// Music at program start with this volume. Nothing plays: the current track is none.
    pub fn new(volume: u8) -> Self {
        Music {
            volume: volume.min(100),
            remembered: REMEMBERED_AT_START,
            // The first apply at program start arms the restart (nothing is open to restart).
            restart_done: volume > 0,
            current: None,
            bump: false,
            open: None,
            atten: attenuation(volume),
            fade: None,
            carry: 0,
            last_position: 0,
        }
    }

    /// The music volume, 0 to 100.
    pub fn volume(&self) -> u8 {
        self.volume
    }

    /// The attenuation now, in hundredths of a decibel.
    pub fn attenuation(&self) -> i32 {
        self.atten
    }

    /// The current track (after the bump), if any.
    pub fn current(&self) -> Option<u8> {
        self.current
    }

    /// The loaded track, if any.
    pub fn open_track(&self) -> Option<u8> {
        self.open.map(|o| o.track)
    }

    /// The "playing" test: a track is open and its position is before its end.
    pub fn playing(&self) -> bool {
        self.open.is_some_and(|o| !o.ended)
    }

    /// The "started" test: a track is open, even one that reached its end.
    pub fn started(&self) -> bool {
        self.open.is_some()
    }

    /// Whether a fade stop is running.
    pub fn fading(&self) -> bool {
        self.fade.is_some()
    }

    /// Sets the game kind for the bump: `Some(entry)` for a single-player campaign game, `None` otherwise.
    pub fn set_campaign(&mut self, entry: Option<u8>) {
        self.bump = entry.is_some_and(|e| e < TIDES_ENTRIES);
    }

    /// The track after the campaign bump.
    fn bumped(&self, t: u8) -> u8 {
        if self.bump && (t == 0 || t == 6) { t + 1 } else { t }
    }

    fn close(&mut self, out: &mut Vec<Action>) {
        if self.open.take().is_some() {
            out.push(Action::Stop);
        }
        self.atten = SILENT;
    }

    fn begin(&mut self, track: Option<u8>, carry_on: bool, out: &mut Vec<Action>) {
        // A start ends a running fade (the fade's own `then` has already taken it).
        self.fade = None;
        let Some(t) = track else {
            self.current = None;
            self.close(out);
            return;
        };
        let t = self.bumped(t);
        self.current = Some(t);
        if carry_on && self.open_track() == Some(t) {
            return;
        }
        self.close(out);
        if self.volume == 0 {
            return;
        }
        self.atten = attenuation(self.volume);
        self.last_position = 0;
        self.open = Some(Open {
            track: t,
            ended: false,
            end_fading: false,
            gap: None,
        });
        let row = TRACKS[t as usize];
        out.push(Action::Load {
            track: t,
            file: row.file,
            looped: matches!(row.end, End::Loop { .. }),
            attenuation: self.atten,
        });
    }

    /// Starts a track from its start whatever is playing (`None` is track -1, no music). The old track stops without a
    /// fade. At volume 0 the track is chosen but not played.
    pub fn start(&mut self, track: Option<u8>) -> Vec<Action> {
        let mut out = Vec::new();
        self.begin(track, false, &mut out);
        out
    }

    /// Starts a track only if none is playing (main menu, briefing).
    pub fn start_unless_playing(&mut self, track: u8) -> Vec<Action> {
        let mut out = Vec::new();
        if !self.playing() {
            self.begin(Some(track), true, &mut out);
        }
        out
    }

    /// Starts a track only if none has started (credits, a game start).
    pub fn start_unless_started(&mut self, track: u8) -> Vec<Action> {
        let mut out = Vec::new();
        if !self.started() {
            self.begin(Some(track), true, &mut out);
        }
        out
    }

    /// Begins a fade stop, or a fade of all sound, after which `then` starts (if not started). The first step is taken
    /// at once; the rest run over the following [`Music::update`] calls, 20 ms apart.
    pub fn fade_stop(&mut self, all_sound: bool, then: Option<u8>) -> Vec<Action> {
        let mut out = Vec::new();
        if all_sound {
            out.push(Action::CutEffects);
        }
        if self.open.is_some() {
            // The first step comes at once; the next ones 20 ms apart, counted from now.
            self.carry = 0;
            let mut fade = Fade { steps: 0, then };
            if self.fade_step(&mut fade, &mut out) {
                self.fade = Some(fade);
                out.push(Action::SetAttenuation(self.atten));
            }
        } else if let Some(t) = then {
            self.begin(Some(t), true, &mut out);
        }
        out
    }

    /// One step of a fade stop. At the first step below the floor the fade ends: the track closes and the fade's
    /// track starts (if given and not started). Returns whether the fade goes on.
    fn fade_step(&mut self, fade: &mut Fade, out: &mut Vec<Action>) -> bool {
        fade.steps += 1;
        self.atten = fade_stop_step(self.atten, fade.steps);
        if self.atten >= FADE_STOP_FLOOR {
            return true;
        }
        self.fade = None;
        self.close(out);
        if let Some(t) = fade.then {
            self.begin(Some(t), true, out);
        }
        false
    }

    /// Applies the music volume `v` ("Volume"): 0 closes the track and forgets the restart; a nonzero volume sets the
    /// loudness and, once since the last 0, starts the current track if none is started.
    pub fn apply_volume(&mut self, v: u8) -> Vec<Action> {
        let mut out = Vec::new();
        self.volume = v.min(100);
        if self.volume == 0 {
            self.fade = None;
            self.close(&mut out);
            self.restart_done = false;
            return out;
        }
        if self.open.is_some() {
            self.atten = attenuation(self.volume);
            out.push(Action::SetAttenuation(self.atten));
        }
        if !self.restart_done {
            self.restart_done = true;
            if self.open.is_none() {
                self.begin(self.current, false, &mut out);
            }
        }
        out
    }

    /// Ctrl+M: volume 0 (remembering the old one), or the remembered volume back.
    pub fn toggle_mute(&mut self) -> Vec<Action> {
        if self.volume > 0 {
            self.remembered = self.volume;
            self.apply_volume(0)
        } else {
            self.apply_volume(self.remembered)
        }
    }

    /// A game starts with this track: starts it if none has started.
    pub fn start_game(&mut self, track: u8) -> Vec<Action> {
        self.start_unless_started(track)
    }

    /// The disco cheat toggled ("Disco cheat"). `normal` is the game's track without the cheat (the campaign formula
    /// or a new draw). Returns the game's new track and the actions: if music is playing, all sound fades and the new
    /// track starts (if not started); else only the game's track changes.
    pub fn disco(&mut self, on: bool, normal: u8) -> (u8, Vec<Action>) {
        let track = if on { DISCO } else { normal };
        let actions = if self.playing() {
            self.fade_stop(true, Some(track))
        } else {
            // Only the game's track changes; it is heard the next time the music starts (a volume change).
            self.current = Some(self.bumped(track));
            Vec::new()
        };
        (track, actions)
    }

    /// Runs `ms` milliseconds: the running fade, the end checks and the chain gap. `playback` is where the open track
    /// is, if there is one and it can be measured.
    pub fn update(&mut self, ms: u32, playback: Option<Playback>) -> Vec<Action> {
        let mut out = Vec::new();
        self.carry += ms;
        let steps = self.carry / STEP_MS;
        self.carry %= STEP_MS;
        if let Some(mut fade) = self.fade {
            for _ in 0..steps {
                if !self.fade_step(&mut fade, &mut out) {
                    return out;
                }
            }
            self.fade = Some(fade);
            if steps > 0 {
                out.push(Action::SetAttenuation(self.atten));
            }
            return out;
        }
        let Some(mut open) = self.open else {
            return out;
        };
        let end = TRACKS[open.track as usize].end;
        if let (Some(p), false) = (playback, open.ended) {
            match end {
                End::Loop { point } => {
                    if p.position < self.last_position && point > 0 {
                        out.push(Action::SeekLoop { frame: point });
                    }
                }
                End::Once | End::Chain { .. } => {
                    if p.position >= p.length {
                        open.ended = true;
                        open.end_fading = false;
                        self.atten = SILENT;
                        out.push(Action::SetAttenuation(SILENT));
                    } else if p.length > END_FADE_BYTES && p.length - p.position < END_FADE_BYTES {
                        open.end_fading = true;
                    }
                }
            }
            self.last_position = p.position;
        } else if open.ended
            && let End::Chain { next } = end
        {
            let gap = open.gap.map_or(0, |g| g + ms);
            open.gap = Some(gap);
            if gap >= GAP_MS {
                self.begin(Some(next), false, &mut out);
                return out;
            }
        }
        if open.end_fading && !open.ended {
            let before = self.atten;
            for _ in 0..steps {
                self.atten -= 10;
                if self.atten < END_FADE_FLOOR {
                    self.atten = SILENT;
                    open.end_fading = false;
                    break;
                }
            }
            if self.atten != before {
                out.push(Action::SetAttenuation(self.atten));
            }
        }
        self.open = Some(open);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEN: u32 = 20_000_000;

    fn at(position: u32) -> Option<Playback> {
        Some(Playback { position, length: LEN })
    }

    fn loaded(actions: &[Action]) -> Option<u8> {
        actions.iter().rev().find_map(|a| match a {
            Action::Load { track, .. } => Some(*track),
            _ => None,
        })
    }

    #[test]
    fn the_table_matches_the_spec() {
        assert_eq!(TRACKS[0].file, "HUMAN1.WAV");
        assert_eq!(TRACKS[5].end, End::Chain { next: 0 });
        assert_eq!(TRACKS[11].end, End::Chain { next: 6 });
        assert_eq!(TRACKS[14].end, End::Once);
        assert_eq!(TRACKS[20].end, End::Loop { point: 66_596 });
        assert_eq!(TRACKS[21].file, TRACKS[22].file);
        assert_eq!(TRACKS[25].file, "DISCOWC.WAV");
        assert_eq!(path(3), "Music\\HUMAN4.WAV");
        for (t, row) in TRACKS.iter().enumerate().take(12) {
            let End::Chain { next } = row.end else { panic!("{t}") };
            assert_eq!(next as usize / 6, t / 6, "rings stay within a race");
        }
        // 3.020 s at 22,050 Hz.
        assert_eq!(66_596 * BYTES_PER_FRAME / BYTES_PER_SECOND, 3);
    }

    #[test]
    fn the_volume_table_spot_checks() {
        assert_eq!(attenuation(0), SILENT);
        assert_eq!(attenuation(1), -4000);
        assert_eq!(attenuation(2), -3396);
        assert_eq!(attenuation(10), -2000);
        assert_eq!(attenuation(25), -1204);
        assert_eq!(attenuation(50), -602, "-6.02 dB");
        assert_eq!(attenuation(75), -248);
        assert_eq!(attenuation(97), -26);
        assert_eq!(attenuation(98), -20);
        assert_eq!(attenuation(100), -20);
        assert!(
            VOLUME_TABLE.windows(2).all(|w| w[0] <= w[1]),
            "never gets quieter as v rises"
        );
    }

    #[test]
    fn fade_stop_vectors() {
        let full = fade_stop_steps(attenuation(100));
        assert_eq!(full.len(), 28, "560 ms");
        assert_eq!(*full.last().unwrap(), -4080);
        assert!(full[26] >= FADE_STOP_FLOOR);
        let quiet = fade_stop_steps(attenuation(25));
        assert_eq!(quiet.len(), 24, "480 ms");
        assert_eq!(*quiet.last().unwrap(), -4204);
    }

    #[test]
    fn end_fade_takes_338_steps_from_full_volume() {
        let steps = end_fade_steps(attenuation(100));
        assert_eq!(steps.len(), 338, "6.76 s");
        assert_eq!(steps[336], -3390);
        assert_eq!(steps[337], -3400);
    }

    #[test]
    fn game_track_vectors() {
        // Tides of Darkness human mission 1 (entry 0), orc mission 5 (entry 9), human mission 7 (entry 12).
        assert_eq!(campaign_track(false, 0), 0);
        assert_eq!(campaign_track(true, 9), 10);
        assert_eq!(campaign_track(false, 12), 0);
        // Beyond the Dark Portal.
        assert_eq!(campaign_track(false, 32), 4);
        assert_eq!(campaign_track(true, 29), 8);
        // Custom scenario as orc, d = 15.
        assert_eq!(drawn_track(true, 15), 9);
        let mut s = Shuffle { state: 0 };
        assert_eq!(game_track(false, None, true, &mut s), DISCO);
        assert_eq!(s, Shuffle { state: 0 }, "disco makes no draw");
        assert_eq!(game_track(true, Some(9), false, &mut s), 10);
        assert_eq!(s, Shuffle { state: 0 }, "a campaign makes no draw");
        let mut t = s;
        let d = t.draw();
        assert_eq!(game_track(false, None, false, &mut s), drawn_track(false, d));
        assert_eq!(s, t, "a custom game makes one draw");
    }

    #[test]
    fn screen_tracks() {
        assert_eq!(score_track(true, false), 14);
        assert_eq!(score_track(true, true), 15);
        assert_eq!(score_track(false, false), 12);
        assert_eq!(score_track(false, true), 13);
        assert_eq!((briefing_track(false), briefing_track(true)), (20, 21));
        assert_eq!((finale_track(false), finale_track(true)), (16, 17));
    }

    #[test]
    fn the_tides_bump() {
        let mut m = Music::new(100);
        m.set_campaign(Some(0));
        assert_eq!(loaded(&m.start(Some(0))), Some(1));
        assert_eq!(m.current(), Some(1));
        assert_eq!(loaded(&m.start(Some(6))), Some(7));
        assert_eq!(loaded(&m.start(Some(2))), Some(2));
        m.set_campaign(Some(27));
        assert_eq!(loaded(&m.start(Some(0))), Some(1));
        m.set_campaign(Some(28));
        assert_eq!(loaded(&m.start(Some(0))), Some(0), "no bump from entry 28");
        m.set_campaign(None);
        assert_eq!(loaded(&m.start(Some(6))), Some(6), "no bump in a custom game");
    }

    fn run_to_end(m: &mut Music) -> Vec<Action> {
        // Reaches the end, then runs 20 ms frames until something loads.
        let mut all = m.update(20, at(LEN));
        for _ in 0..200 {
            let a = m.update(20, at(LEN));
            let done = loaded(&a).is_some();
            all.extend(a);
            if done {
                break;
            }
        }
        all
    }

    #[test]
    fn a_chain_waits_two_seconds_then_moves_on() {
        let mut m = Music::new(100);
        m.set_campaign(Some(30));
        m.start(Some(5));
        let first = m.update(20, at(LEN));
        assert!(first.contains(&Action::SetAttenuation(SILENT)), "silent at the end");
        assert!(m.started() && !m.playing());
        // The check after the end starts the gap; 100 more frames of 20 ms end it.
        let mut frames = 0;
        let mut next = None;
        while next.is_none() && frames < 500 {
            next = loaded(&m.update(20, at(LEN)));
            frames += 1;
        }
        assert_eq!(next, Some(0), "no bump from entry 30");
        assert_eq!(frames, 101, "2 s after the first check after the end");
        assert_eq!(m.attenuation(), -20, "volume restored");
        assert!(m.playing());
    }

    #[test]
    fn a_tides_chain_skips_track_zero() {
        let mut m = Music::new(100);
        m.set_campaign(Some(0));
        m.start(Some(5));
        let a = run_to_end(&mut m);
        assert_eq!(loaded(&a), Some(1));
    }

    #[test]
    fn a_chain_ring_goes_round() {
        let mut m = Music::new(100);
        m.set_campaign(Some(40));
        m.start(Some(10));
        assert_eq!(loaded(&run_to_end(&mut m)), Some(11));
        assert_eq!(loaded(&run_to_end(&mut m)), Some(6));
    }

    #[test]
    fn the_end_fade_starts_with_616000_bytes_left() {
        let mut m = Music::new(100);
        m.start(Some(3));
        assert!(
            m.update(20, at(LEN - END_FADE_BYTES)).is_empty(),
            "exactly 616,000 left is not fewer"
        );
        let mut n = 0;
        while m.attenuation() != SILENT {
            m.update(20, at(LEN - END_FADE_BYTES + 1));
            n += 1;
            assert!(n <= 338, "338 steps");
            if n < 338 {
                assert!(m.attenuation() > END_FADE_FLOOR);
            }
        }
        assert_eq!(n, 338);
        // Silent, but the track isn't over until the end.
        assert!(m.playing());
        assert!(m.update(20, at(LEN)).contains(&Action::SetAttenuation(SILENT)));
        assert!(!m.playing() && m.started());
    }

    #[test]
    fn loop_tracks_never_fade_and_seek_to_the_loop_point() {
        let mut m = Music::new(100);
        m.start(Some(20));
        assert!(m.update(20, at(LEN - 10)).is_empty());
        assert_eq!(m.attenuation(), -20);
        assert_eq!(m.update(20, at(100)), vec![Action::SeekLoop { frame: 66_596 }]);
        let mut o = Music::new(100);
        o.start(Some(22));
        o.update(20, at(LEN - 10));
        assert!(o.update(20, at(100)).is_empty(), "a loop to the start needs no seek");
        let a = o.start(Some(25));
        assert!(matches!(a.last(), Some(Action::Load { looped: true, .. })));
    }

    #[test]
    fn a_short_file_does_not_fade() {
        let mut m = Music::new(100);
        m.start(Some(14));
        let short = Some(Playback {
            position: 100_000,
            length: 500_000,
        });
        assert!(m.update(20, short).is_empty());
    }

    #[test]
    fn a_fade_stop_runs_in_20_ms_steps() {
        let mut m = Music::new(100);
        m.start(Some(3));
        let first = m.fade_stop(false, None);
        assert_eq!(first, vec![Action::SetAttenuation(-30)], "the first step comes at once");
        assert!(m.fading());
        let mut frames = 0;
        let mut stopped = false;
        while !stopped {
            let a = m.update(20, at(0));
            frames += 1;
            stopped = a.contains(&Action::Stop);
        }
        assert_eq!(frames, 27, "28 steps, the last at 540 ms");
        assert!(!m.started() && !m.fading());
        assert_eq!(m.attenuation(), SILENT);
        // At volume 25: 24 steps.
        let mut q = Music::new(25);
        q.start(Some(3));
        q.fade_stop(false, None);
        let frames = (1..).find(|_| q.update(20, at(0)).contains(&Action::Stop)).unwrap();
        assert_eq!(frames, 23, "24 steps, the last at 460 ms");
    }

    #[test]
    fn a_fade_blocks_everything_else() {
        let mut m = Music::new(100);
        m.start(Some(3));
        m.fade_stop(false, None);
        let a = m.update(20, at(LEN));
        assert!(
            !a.contains(&Action::SetAttenuation(SILENT)),
            "no end check during the fade"
        );
        assert!(m.playing());
    }

    #[test]
    fn frames_that_are_not_20_ms_carry_over() {
        let mut m = Music::new(100);
        m.start(Some(3));
        m.fade_stop(false, None);
        let mut frames = 0;
        loop {
            frames += 1;
            if m.update(7, at(0)).contains(&Action::Stop) {
                break;
            }
        }
        assert_eq!(
            frames, 78,
            "the last of 28 steps is at 540 ms, in the 78th frame of 7 ms"
        );
    }

    #[test]
    fn the_start_if_silent_tests() {
        let mut m = Music::new(100);
        assert_eq!(loaded(&m.start_unless_playing(22)), Some(22));
        assert!(
            m.start_unless_playing(22).is_empty(),
            "the menu leaves the title music alone"
        );
        assert!(m.start_unless_playing(20).is_empty());
        // A once-track that reached its end: not playing, still started.
        m.start(Some(14));
        m.update(20, at(LEN));
        assert!(!m.playing() && m.started());
        assert!(
            m.start_unless_started(23).is_empty(),
            "credits don't start over a finished once-track"
        );
        assert_eq!(loaded(&m.start_unless_playing(20)), Some(20), "a briefing does");
        m.fade_stop(true, None);
        for _ in 0..40 {
            m.update(20, at(0));
        }
        assert_eq!(loaded(&m.start_unless_started(23)), Some(23));
    }

    #[test]
    fn restarting_the_same_track() {
        let mut m = Music::new(100);
        m.start(Some(3));
        let a = m.start(Some(3));
        assert_eq!(a.first(), Some(&Action::Stop), "a plain start restarts it");
        assert_eq!(loaded(&a), Some(3));
        assert!(m.start_unless_playing(3).is_empty(), "if-silent carries on");
        let none = m.start(None);
        assert_eq!(none, vec![Action::Stop]);
        assert_eq!(m.current(), None);
    }

    #[test]
    fn volume_zero_chooses_a_track_without_playing_it() {
        let mut m = Music::new(0);
        assert!(m.start(Some(4)).is_empty());
        assert_eq!(m.current(), Some(4));
        assert!(!m.started());
        assert!(m.start_unless_started(4).is_empty());
        assert_eq!(loaded(&m.apply_volume(40)), Some(4), "raising it starts the track");
    }

    #[test]
    fn nothing_plays_at_program_start() {
        let mut m = Music::new(75);
        assert!(m.apply_volume(75).is_empty(), "the current track is none");
        assert_eq!(loaded(&m.start_unless_playing(TITLE_TRACK)), Some(TITLE_TRACK));
    }

    #[test]
    fn ctrl_m_stops_then_restarts_from_the_start() {
        let mut m = Music::new(75);
        m.apply_volume(75);
        m.start(Some(2));
        assert_eq!(m.toggle_mute(), vec![Action::Stop]);
        assert_eq!(m.volume(), 0);
        assert!(!m.started());
        let a = m.toggle_mute();
        assert_eq!(m.volume(), 75);
        assert_eq!(loaded(&a), Some(2));
        assert_eq!(m.attenuation(), attenuation(75));
    }

    #[test]
    fn the_first_ctrl_m_with_volume_zero_gives_25() {
        let mut m = Music::new(0);
        m.apply_volume(0);
        m.start(Some(7));
        let a = m.toggle_mute();
        assert_eq!(m.volume(), 25);
        assert_eq!(loaded(&a), Some(7));
        assert_eq!(m.attenuation(), -1204);
    }

    #[test]
    fn the_sound_dialog_vectors() {
        let mut m = Music::new(40);
        m.apply_volume(40);
        m.start(Some(5));
        // 40 -> 60 while a track plays: only louder.
        assert_eq!(m.apply_volume(60), vec![Action::SetAttenuation(attenuation(60))]);
        assert!(m.started());
        // 0 applied, then 40 applied: the track starts from its start.
        assert_eq!(m.apply_volume(0), vec![Action::Stop]);
        let a = m.apply_volume(40);
        assert_eq!(loaded(&a), Some(5));
        // Later nonzero changes only change the loudness.
        assert_eq!(m.apply_volume(80), vec![Action::SetAttenuation(attenuation(80))]);
    }

    #[test]
    fn the_restart_is_armed_once_per_silence() {
        let mut m = Music::new(50);
        m.apply_volume(50);
        // Armed already: a track closed by a fade isn't restarted by a later volume change.
        m.start(Some(1));
        m.fade_stop(false, None);
        for _ in 0..100 {
            m.update(20, at(0));
        }
        assert!(!m.started());
        let a = m.apply_volume(60);
        assert!(loaded(&a).is_none(), "no volume 0 since the first apply: already done");
    }

    #[test]
    fn disco_fades_a_playing_track_and_starts_the_new_one() {
        let mut m = Music::new(100);
        m.start(Some(4));
        let (track, a) = m.disco(true, 4);
        assert_eq!(track, DISCO);
        assert_eq!(
            a,
            vec![Action::CutEffects, Action::SetAttenuation(-30)],
            "the first step at once"
        );
        let mut loaded_track = None;
        for _ in 0..40 {
            loaded_track = loaded_track.or(loaded(&m.update(20, at(0))));
        }
        assert_eq!(loaded_track, Some(DISCO));
        // Off again: back to the normal track.
        let (track, _) = m.disco(false, 9);
        assert_eq!(track, 9);
        // Not playing: only the game's track changes.
        let mut quiet = Music::new(100);
        let (track, a) = quiet.disco(true, 4);
        assert_eq!((track, a), (DISCO, vec![]));
        assert_eq!(quiet.current(), Some(DISCO), "heard the next time the music starts");
    }

    #[test]
    fn a_fade_with_nothing_open_starts_the_next_track_at_once() {
        let mut m = Music::new(100);
        let a = m.fade_stop(true, Some(16));
        assert_eq!(a.first(), Some(&Action::CutEffects));
        assert_eq!(loaded(&a), Some(16));
    }

    #[test]
    fn a_game_start_keeps_a_running_track() {
        let mut m = Music::new(100);
        m.start(Some(22));
        assert!(m.start_game(3).is_empty(), "a started track carries on");
        m.fade_stop(true, None);
        for _ in 0..40 {
            m.update(20, at(0));
        }
        assert_eq!(loaded(&m.start_game(3)), Some(3));
    }

    #[test]
    fn wav_data_length() {
        let mut w = b"RIFF\0\0\0\0WAVE".to_vec();
        w.extend_from_slice(b"fmt \x03\0\0\0abc\0");
        w.extend_from_slice(b"data");
        w.extend_from_slice(&1234u32.to_le_bytes());
        assert_eq!(wav_data_len(&w), Some(1234), "odd chunk sizes are padded");
        assert_eq!(wav_data_len(b"RIFX"), None);
        assert_eq!(wav_data_len(b"RIFF\0\0\0\0WAVEfmt \x01\0\0\0a\0"), None);
    }
}
