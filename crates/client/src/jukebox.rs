//! The music's player (`music.rs`, spec `docs/specs/ui/music.md`): the state machine and the open track's voice in the
//! core's mixer (`audio`), and [`play_music`], which runs it every frame. Every screen starts and stops music through the
//! methods here; the music files are read from the install's `Music` folder through the front end's [`Front`].
//!
//! A chained track's next track is read and decoded on a thread of its own while the open one plays ([`Prefetch`]):
//! a battle track is some 15 MB in `Install.mpq`, and reading it on the frame that starts it held that frame for
//! about 190 ms on a desktop, a visible stutter at every change of track in a game. Every other start reads its file
//! at once, as before.
//!
//! The game menu's sound dialog changes the volume through [`Jukebox::set_volume`] (the same call for the countdown
//! ending, the cancel and the menu closing: "Every time the music volume is applied").

use std::thread::{self, JoinHandle};
use std::time::Duration;

use tidewater_formats::install::Install;

use crate::audio::{self, VoiceId};
use crate::core::Core;
use crate::frontend::Front;
use crate::music::{self, Action, Music, Playback};
use crate::sound::Shuffle;

/// The music state machine and the open track's voice.
#[derive(Default)]
pub struct Jukebox {
    music: Music,
    /// Actions to apply on the next frame (from starts and key presses).
    pending: Vec<Action>,
    /// The open track's voice in the core's mixer.
    voice: Option<VoiceId>,
    /// Bytes of audio in the open track's file.
    length: u32,
    /// The attenuation the music asks for, and the one last given to the voice.
    atten: i32,
    applied: Option<i32>,
    /// Frame time not yet handed to the music, below a millisecond.
    carry: Duration,
    /// The open chained track's next track, being read.
    prefetch: Option<Prefetch>,
}

/// A track's file read and decoded for the mixer.
struct Prepared {
    sound: audio::Sound,
    /// Bytes of audio in the file.
    length: u32,
}

/// Reads and decodes a track's file; `None` if the install lacks it or it isn't PCM WAV.
fn prepare(install: &mut Install, track: u8) -> Option<Prepared> {
    let bytes = install.read(&music::path(track)).ok()?;
    Some(Prepared {
        sound: audio::Sound::from_wav(&bytes)?,
        length: music::wav_data_len(&bytes).unwrap_or(0),
    })
}

/// A track being read on its own thread, from a handle on the install of its own.
struct Prefetch {
    track: u8,
    read: JoinHandle<Option<Prepared>>,
}

impl Prefetch {
    /// Starts reading `track`; `None` if the install can't be opened a second time.
    fn start(install: &Install, track: u8) -> Option<Prefetch> {
        let mut install = install.reopen().ok()?;
        let read = thread::Builder::new()
            .name("music read".into())
            .spawn(move || prepare(&mut install, track))
            .ok()?;
        Some(Prefetch { track, read })
    }
}

#[allow(dead_code)] // not every screen calls every method yet
impl Jukebox {
    fn queue(&mut self, actions: Vec<Action>) {
        self.pending.extend(actions);
    }

    /// The music volume, 0 to 100.
    pub fn volume(&self) -> u8 {
        self.music.volume()
    }

    /// Applies the music volume (0 to 100). The game menu calls this whenever the volume takes effect.
    pub fn set_volume(&mut self, v: u8) {
        let a = self.music.apply_volume(v);
        self.queue(a);
    }

    /// Ctrl+M.
    pub fn toggle_mute(&mut self) {
        let a = self.music.toggle_mute();
        self.queue(a);
    }

    /// Starts a track at once (`None` is no music).
    pub fn start(&mut self, track: Option<u8>) {
        let a = self.music.start(track);
        self.queue(a);
    }

    /// Starts a track if none is playing (main menu, briefing).
    pub fn start_unless_playing(&mut self, track: u8) {
        let a = self.music.start_unless_playing(track);
        self.queue(a);
    }

    /// Starts a track if none has started (credits).
    pub fn start_unless_started(&mut self, track: u8) {
        let a = self.music.start_unless_started(track);
        self.queue(a);
    }

    /// A fade stop, or a fade of all sound, then `then` starts if given.
    pub fn fade_stop(&mut self, all_sound: bool, then: Option<u8>) {
        let a = self.music.fade_stop(all_sound, then);
        self.queue(a);
    }

    /// The music state, for the screens that need a test.
    pub fn music(&self) -> &Music {
        &self.music
    }

    /// The open track's voice in the mixer: a new one each time a track starts from its start.
    #[cfg(test)]
    pub(crate) fn voice(&self) -> Option<VoiceId> {
        self.voice
    }

    /// A game starts: the game's track plays if no music has started. The track is drawn after the map has loaded, from
    /// the setup-shuffle generator.
    pub fn start_game(&mut self, orc: bool, campaign: Option<u8>, shuffle: &mut Shuffle) {
        let m = &mut self.music;
        m.set_campaign(campaign);
        let track = music::game_track(orc, campaign, false, shuffle);
        let a = m.start_game(track);
        self.queue(a);
    }

    /// A saved game is loaded: the saved track plays if no music has started (a loaded game "uses the saved track",
    /// `ui/music.md`). The campaign bump has been applied to the saved track already, and applying it again changes
    /// nothing.
    pub fn resume_game(&mut self, campaign: Option<u8>, track: Option<u8>) {
        self.music.set_campaign(campaign);
        if let Some(t) = track {
            let a = self.music.start_game(t);
            self.queue(a);
        }
    }

    /// The disco cheat turned `on` or off: track 25, or the game's normal track from `normal` (called only when turning
    /// off, as only that draws from the setup-shuffle generator).
    pub fn disco(&mut self, on: bool, normal: impl FnOnce() -> u8) {
        let normal = if on { music::DISCO } else { normal() };
        let (_, a) = self.music.disco(on, normal);
        self.queue(a);
    }

    /// The game is over: the bump no longer applies.
    pub fn end_game(&mut self) {
        self.music.set_campaign(None);
    }
}

/// Runs the music each frame and applies what it asks for in the core's mixer: loads a file from the install's
/// `Music` folder, sets its loudness and stops it. The open track's position is the mixer's, so it runs on the mixer's
/// clock; a loop track loops in the mixer, from its loop point, without a gap (`audio`), so the loop point needs no
/// seek.
///
/// The starts and stops asked for since the last frame are applied first, so the frame's checks measure the voice of
/// the track that is open now, never the one a start has just replaced (a new track was otherwise measured at the old
/// one's position: started near the old one's end, it began its end fade at once, and started after the old one had
/// ended, it counted as ended itself).
pub fn play_music(c: &mut Core) {
    let Core {
        time,
        jukebox: jb,
        front,
        audio: core,
        ..
    } = c;

    jb.carry += time.delta();
    let ms = jb.carry.as_millis() as u32;
    jb.carry -= Duration::from_millis(ms as u64);
    let pending = std::mem::take(&mut jb.pending);
    apply(jb, front, core, pending);
    let length = jb.length;
    // A voice the mixer no longer has has played to its end.
    let playback = jb.voice.filter(|_| length > 0).map(|v| Playback {
        position: core
            .position(v)
            .map_or(length, |frame| frame as u32 * music::BYTES_PER_FRAME),
        length,
    });
    let actions = jb.music.update(ms, playback);
    apply(jb, front, core, actions);
    if let Some(v) = jb.voice
        && jb.applied != Some(jb.atten)
    {
        core.set_gain(v, audio::attenuation_gain(jb.atten));
        jb.applied = Some(jb.atten);
    }
}

/// Applies the music's actions in the mixer. A load puts the new track in the mixer's music slot, which stops the old
/// one (`audio`, "The music slot").
fn apply(jb: &mut Jukebox, front: &mut Front, core: &mut audio::Audio, actions: Vec<Action>) {
    for action in actions {
        match action {
            Action::Stop => {
                if let Some(v) = jb.voice.take() {
                    core.stop(v);
                }
            }
            Action::Load {
                track,
                looped,
                attenuation,
                ..
            } => {
                if let Some(v) = jb.voice.take() {
                    core.stop(v);
                }
                jb.atten = attenuation;
                jb.length = 0;
                // Our choice: the whole file is read at once, by the prefetch if it read this track (waiting for it
                // if it hasn't finished), or now; a file the install lacks leaves the music silent.
                let prefetched = match jb.prefetch.take() {
                    Some(p) if p.track == track => p.read.join().ok().flatten(),
                    _ => None,
                };
                if let music::End::Chain { next } = music::TRACKS[track as usize].end {
                    jb.prefetch = Prefetch::start(&front.install, next);
                }
                let Some(Prepared { sound, length }) = prefetched.or_else(|| prepare(&mut front.install, track)) else {
                    continue;
                };
                jb.length = length;
                let loop_from = match music::TRACKS[track as usize].end {
                    music::End::Loop { point } if looped => Some(point as usize),
                    _ => None,
                };
                let volume = audio::decibels_to_linear(music::decibels(attenuation));
                jb.voice = Some(core.play_music(&sound, volume, audio::attenuation_gain(attenuation), loop_from));
                jb.applied = Some(attenuation);
            }
            Action::SetAttenuation(a) => jb.atten = a,
            // The mixer loops from the loop point itself.
            Action::SeekLoop { .. } => {}
            // Music spec, "Effects and narration": a fade of all sound first cuts every sound effect, narration
            // included, at once.
            Action::CutEffects => core.cut_effects(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::sync::{Arc, Mutex};
    use std::thread::ThreadId;

    use clap::Parser;
    use tidewater_formats::mpq::{HashType, ReadSeek, encrypt, hash_string};

    use super::*;

    use crate::args::Args;
    use crate::audio::Group;
    use crate::frontend::AppState;
    use crate::host;

    /// An install held in memory: an empty archive and the music files `music` names, each a short WAV of its own
    /// length. It notes which thread opened each file.
    struct Files {
        music: Vec<(String, Vec<u8>)>,
        archive: Vec<u8>,
        opened: Arc<Mutex<Vec<(String, ThreadId)>>>,
    }

    impl tidewater_formats::install::Files for Files {
        fn open(&self, name: &str) -> std::io::Result<Option<Box<dyn ReadSeek>>> {
            self.opened
                .lock()
                .unwrap()
                .push((name.to_string(), thread::current().id()));
            let bytes = if name.eq_ignore_ascii_case("Install.mpq") {
                Some(self.archive.clone())
            } else {
                self.music
                    .iter()
                    .find(|(n, _)| n.eq_ignore_ascii_case(name))
                    .map(|(_, b)| b.clone())
            };
            Ok(bytes.map(|b| Box::new(Cursor::new(b)) as Box<dyn ReadSeek>))
        }
    }

    /// An archive with no files: the header and empty, encrypted hash and block tables ("The MoPaQ Archive Format").
    fn empty_archive() -> Vec<u8> {
        let mut hashes = vec![0xFFu8; 4 * 16];
        encrypt(&mut hashes, hash_string("(hash table)", HashType::FileKey));
        let mut out = b"MPQ\x1A".to_vec();
        for w in [32u32, 32 + hashes.len() as u32] {
            out.extend_from_slice(&w.to_le_bytes());
        }
        out.extend_from_slice(&[0, 0, 3, 0]); // format version 0, 4096-byte sectors
        for w in [32u32, 32 + hashes.len() as u32, 4, 0] {
            out.extend_from_slice(&w.to_le_bytes());
        }
        out.extend_from_slice(&hashes);
        out
    }

    /// A 16-bit stereo WAV at the mixer's rate holding `frames` silent sample frames.
    fn wav(frames: usize) -> Vec<u8> {
        let data = vec![0u8; frames * 4];
        let mut w = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        for (v, n) in [
            (16, 4),
            (1, 2),
            (2, 2),
            (audio::SAMPLE_RATE, 4),
            (audio::SAMPLE_RATE * 4, 4),
            (4, 2),
            (16, 2),
        ] {
            w.extend_from_slice(&u32::to_le_bytes(v)[..n]);
        }
        w.extend_from_slice(b"data");
        w.extend_from_slice(&(data.len() as u32).to_le_bytes());
        w.extend_from_slice(&data);
        w
    }

    fn load(track: u8) -> Action {
        Action::Load {
            track,
            file: music::TRACKS[track as usize].file,
            looped: false,
            attenuation: 0,
        }
    }

    /// A chained track's next track is read on another thread while it plays, and starting it reads nothing on the
    /// frame; a start the prefetch didn't guess reads its file at once.
    #[test]
    fn the_next_track_of_a_chain_is_read_ahead_off_the_frame() {
        let opened = Arc::new(Mutex::new(Vec::new()));
        let files = Files {
            music: [("HUMAN1.WAV", 100), ("HUMAN2.WAV", 200), ("HDEFEAT.WAV", 300)]
                .map(|(n, f)| (format!("Music\\{n}"), wav(f)))
                .into(),
            archive: empty_archive(),
            opened: opened.clone(),
        };
        let mut front = Front::none();
        front.install = Install::from_files(Box::new(files)).expect("the install opens");
        let (mut jb, mut core) = (Jukebox::default(), audio::Audio::default());
        let main = thread::current().id();
        let reads = |name: &str| -> Vec<bool> {
            let opened = opened.lock().unwrap();
            opened
                .iter()
                .filter(|(n, _)| n.ends_with(name))
                .map(|(_, t)| *t == main)
                .collect()
        };

        // HUMAN1 chains to HUMAN2.
        apply(&mut jb, &mut front, &mut core, vec![load(0)]);
        assert_eq!(reads("HUMAN1.WAV"), [true], "the first track is read at once");
        assert_eq!(jb.length, 400);
        assert_eq!(jb.prefetch.as_ref().map(|p| p.track), Some(1));

        apply(&mut jb, &mut front, &mut core, vec![load(1)]);
        assert_eq!(
            reads("HUMAN2.WAV"),
            [false],
            "the chain's next track was read once, off the frame"
        );
        assert_eq!(jb.length, 800, "and it is the one that plays");
        assert_eq!(core.playing_in(audio::Group::Music), 1);
        assert_eq!(jb.prefetch.as_ref().map(|p| p.track), Some(2));

        // A defeat track, not the guess: read at once, and it chains nowhere.
        apply(&mut jb, &mut front, &mut core, vec![load(12)]);
        assert_eq!(reads("HDEFEAT.WAV"), [true]);
        assert_eq!(jb.length, 1200);
        assert!(jb.prefetch.is_none());
        assert_eq!(core.playing_in(audio::Group::Music), 1);
    }

    /// A screen the run showed: the open music track on its first and last frames, and the tracks started from their
    /// start while it showed, in order.
    #[derive(Debug, PartialEq)]
    struct Visit {
        state: AppState,
        first: Option<u8>,
        last: Option<u8>,
        started: Vec<u8>,
    }

    /// Runs a scripted scene (`script`) without a window on the install in `TIDEWATER_INSTALL` (`None` without one)
    /// and checks at every frame that at most one music track sounds, and none while the music has no track open.
    /// Returns the screens it went through.
    fn run(scene: &str) -> Option<Vec<Visit>> {
        let Some(install) = std::env::var_os("TIDEWATER_INSTALL") else {
            eprintln!("skipped: set TIDEWATER_INSTALL to run");
            return None;
        };
        let out = tempfile::tempdir().expect("a temporary folder");
        let mut argv: Vec<std::ffi::OsString> = vec!["tidewater".into(), "--install".into(), install];
        argv.extend(["--script", scene, "--headless", "--script-out"].map(Into::into));
        argv.push(out.path().into());
        let core = host::launch(Args::parse_from(argv)).expect("the client starts").core;
        let mut visits: Vec<Visit> = Vec::new();
        let mut frame = 0;
        let mut voice = None;
        host::run_headless_watched(core, |c| {
            frame += 1;
            let state = c.states.get();
            let track = c.jukebox.music().open_track();
            let live = c.audio.playing_in(Group::Music);
            assert!(
                live <= 1,
                "{scene}, frame {frame} ({state:?}): {live} music tracks sound at once"
            );
            assert!(
                track.is_some() || live == 0,
                "{scene}, frame {frame} ({state:?}): music sounds with no track open"
            );
            let now = c.jukebox.voice();
            let started = (now.is_some() && now != voice).then_some(track).flatten();
            voice = now;
            match visits.last_mut() {
                Some(v) if v.state == state => v.last = track,
                _ => visits.push(Visit {
                    state,
                    first: track,
                    last: track,
                    started: Vec::new(),
                }),
            }
            if let (Some(t), Some(v)) = (started, visits.last_mut()) {
                v.started.push(t);
            }
        });
        Some(visits)
    }

    fn states(visits: &[Visit]) -> Vec<AppState> {
        visits.iter().map(|v| v.state).collect()
    }

    /// The tracks each game started.
    fn game_tracks(visits: &[Visit]) -> Vec<Vec<u8>> {
        visits
            .iter()
            .filter(|v| v.state == AppState::Game)
            .map(|v| v.started.clone())
            .collect()
    }

    /// Music spec, "Leaving a game for the main menu": each main menu after a game opens with nothing playing (the
    /// game's track faded and closed while the game's picture stayed) and starts track 22 from its start.
    fn quit_to_menu_restarts_22(visits: &[Visit]) {
        for w in visits.windows(2) {
            if let [game, menu] = w
                && (game.state, menu.state) == (AppState::Game, AppState::MainMenu)
            {
                assert_eq!(game.last, None, "the game's track is closed before the main menu");
                assert_eq!(menu.first, None, "the main menu opens with nothing playing");
                assert_eq!(menu.started, [22], "22 starts from its start");
            }
        }
    }

    /// Choosing a campaign fades all sound: nothing is open when the next screen opens, so the briefing starts its own
    /// track from its start, and its close fades that out before the game starts its track.
    fn campaign_starts_fade_everything(visits: &[Visit]) {
        for w in visits.windows(2) {
            if let [from, to] = w
                && matches!(from.state, AppState::NewCampaign | AppState::Briefing)
                && matches!(to.state, AppState::Act | AppState::Briefing | AppState::Game)
            {
                assert_eq!(from.last, None, "{:?} -> {:?}: all sound faded", from.state, to.state);
            }
            if let [_, b] = w
                && b.state == AppState::Briefing
            {
                assert_eq!(b.started.len(), 1, "the briefing starts its track from its start");
                assert!(matches!(b.started[0], 20 | 21));
            }
        }
    }

    /// The playtest: a campaign's game, back to the main menu by Ctrl+Q, the other race's campaign through its act
    /// screen and briefing into its game. The first game's track never sounds with another, and never reaches the menu.
    #[test]
    fn one_music_track_from_a_campaign_quit_into_the_other_campaign() {
        let Some(visits) = run("music-quit") else { return };
        use AppState::*;
        assert_eq!(
            states(&visits),
            [
                Boot,
                MainMenu,
                SinglePlayer,
                NewCampaign,
                Act,
                Briefing,
                Game,
                MainMenu,
                SinglePlayer,
                NewCampaign,
                Act,
                Briefing,
                Game
            ]
        );
        // Tides of Darkness, the first missions: human track 0 and orc track 6, bumped to 1 and 7 (`ui/music.md`).
        assert_eq!(game_tracks(&visits), [vec![1], vec![7]]);
        quit_to_menu_restarts_22(&visits);
        campaign_starts_fade_everything(&visits);
        // The briefings: human 20, then orc 21.
        let briefings: Vec<_> = visits
            .iter()
            .filter(|v| v.state == Briefing)
            .map(|v| v.started.clone())
            .collect();
        assert_eq!(briefings, [vec![20], vec![21]]);
    }

    /// The other ways back to the main menu and on into a game.
    #[test]
    fn one_music_track_through_the_other_ways_back() {
        let Some(visits) = run("music-back") else { return };
        use AppState::*;
        assert_eq!(
            states(&visits),
            [
                Boot,
                MainMenu,
                SinglePlayer,
                NewCampaign,
                SinglePlayer,
                MainMenu,
                SinglePlayer,
                NewCampaign,
                Act,
                // Escape does nothing here: the briefing stays until Enter.
                Briefing,
                Game,
                // The restart shows the briefing again.
                Briefing,
                Game,
                MainMenu,
                SinglePlayer,
                NewCampaign,
                // Ctrl held: no act screen or briefing.
                Game
            ]
        );
        // Previous Menu (Escape on the campaign menu, then on the single-player menu) goes back with no fade: 22 plays
        // on, not restarted.
        assert_eq!(visits[3].last, Some(22));
        for v in &visits[4..6] {
            assert_eq!((v.first, v.last), (Some(22), Some(22)));
            assert!(v.started.is_empty(), "22 isn't restarted");
        }
        // Restart Scenario fades the game's track out first (round 75), so the restart's briefing starts its own
        // track (21, the orc briefing's) from its start; each game starts its own track.
        assert_eq!(game_tracks(&visits), [vec![7], vec![7], vec![1]]);
        assert_eq!(visits[10].last, None, "the game's track has closed when the game ends");
        assert_eq!(
            visits[11].started,
            [21],
            "the restart's briefing starts its track from its start"
        );
        quit_to_menu_restarts_22(&visits);
        assert_eq!(visits[13].started, [22]);
        // The end menu's Quit to Main Menu is the same path as Ctrl+Q; the campaign after it fades 22 out.
        assert_eq!(visits[15].last, None);
    }
}
