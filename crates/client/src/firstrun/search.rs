//! The first-run screen's automatic search (`docs/design/import.md`, "First-run screen"): where a player's copy of
//! Warcraft II: Battle.net Edition usually is. The places are listed by [`bases`] from the operating system and the
//! home folder; [`candidate_dirs`] picks the folders in them that are named like the game; the caller then runs each
//! through `tidewater_import`'s classify and validate. Nothing here opens a game file, and it reads no registry (that
//! would need `unsafe`), so on Windows it looks in the usual folders only.

use std::fs;
use std::path::{Path, PathBuf};

/// The operating system whose usual places are listed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Windows,
    Mac,
    Linux,
}

impl Os {
    /// The system this program was built for.
    pub fn current() -> Os {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::Mac
        } else {
            Os::Linux
        }
    }
}

/// What the list of places depends on.
#[derive(Clone, Debug, Default)]
pub struct Where {
    /// The user's home folder.
    pub home: Option<PathBuf>,
    /// Windows: the `Program Files` folders (`ProgramFiles` and `ProgramFiles(x86)`).
    pub program_files: Vec<PathBuf>,
    /// A phone's shared folder when it is not `Documents` under the home folder: Android's external files folder
    /// (the shell gives it in `TIDEWATER_SHARED_DIR`).
    pub shared: Option<PathBuf>,
}

impl Where {
    /// This computer's.
    pub fn here() -> Where {
        Where {
            home: directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf()),
            program_files: ["ProgramFiles", "ProgramFiles(x86)"]
                .iter()
                .filter_map(|v| std::env::var_os(v).map(PathBuf::from))
                .collect(),
            shared: std::env::var_os("TIDEWATER_SHARED_DIR")
                .map(PathBuf::from)
                .or_else(|| cfg!(target_os = "vita").then(|| PathBuf::from(crate::storage::VITA_DATA_DIR))),
        }
    }
}

/// Folders inside a Wine prefix's `drive_c` where a game is usually installed.
const IN_DRIVE_C: [&str; 7] = [
    "",
    "GOG Games",
    "Program Files",
    "Program Files (x86)",
    "Program Files (x86)/GOG Galaxy/Games",
    "Program Files (x86)/GOG.com",
    "Program Files/GOG Galaxy/Games",
];

/// The folders to look in, most likely first. A game's own folder is one of their children ([`candidate_dirs`]).
pub fn bases(os: Os, at: &Where) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = Vec::new();
    let home = at.home.as_deref();
    match os {
        Os::Windows => {
            v.extend(at.program_files.iter().cloned());
            for p in &at.program_files {
                v.push(p.join("GOG Galaxy").join("Games"));
                v.push(p.join("GOG.com"));
            }
            for drive in ['C', 'D', 'E', 'F', 'G'] {
                v.push(PathBuf::from(format!("{drive}:\\GOG Games")));
            }
            if let Some(h) = home {
                v.push(h.join("GOG Games"));
                v.push(h.join("Games"));
            }
        }
        Os::Mac => {
            v.push(PathBuf::from("/Applications"));
            v.push(PathBuf::from("/Volumes"));
            if let Some(h) = home {
                v.push(h.join("Applications"));
                v.push(h.join("GOG Games"));
                v.push(h.join("Games"));
            }
        }
        Os::Linux => {
            if let Some(h) = home {
                v.push(h.join("GOG Games"));
                v.push(h.join("Games"));
            }
        }
    }
    if os != Os::Windows
        && let Some(h) = home
    {
        for prefix in wine_prefixes(os, h) {
            for sub in IN_DRIVE_C {
                v.push(if sub.is_empty() {
                    prefix.join("drive_c")
                } else {
                    prefix.join("drive_c").join(sub)
                });
            }
        }
    }
    v
}

/// Wine prefixes under the home folder: Wine's own, CrossOver's and Whisky's bottles on a Mac, and Lutris, Heroic
/// and Bottles prefixes on Linux. Only folders that have a `drive_c` count.
fn wine_prefixes(os: Os, home: &Path) -> Vec<PathBuf> {
    let mut roots = vec![home.join(".wine"), home.join(".wine32")];
    let mut parents: Vec<PathBuf> = Vec::new();
    match os {
        Os::Mac => {
            parents.push(home.join("Library/Application Support/CrossOver/Bottles"));
            if let Ok(containers) = fs::read_dir(home.join("Library/Containers")) {
                for c in containers.flatten() {
                    parents.push(c.path().join("Bottles"));
                }
            }
            parents.push(home.join("Games"));
        }
        _ => {
            parents.push(home.join("Games"));
            parents.push(home.join(".local/share/lutris/runners/wine"));
            parents.push(home.join(".local/share/bottles/bottles"));
            parents.push(home.join(".var/app/com.usebottles.bottles/data/bottles/bottles"));
            parents.push(home.join("Games/Heroic/Prefixes/default"));
        }
    }
    for p in parents {
        if let Ok(rd) = fs::read_dir(&p) {
            roots.extend(rd.flatten().map(|e| e.path()));
        }
    }
    roots.retain(|r| r.join("drive_c").is_dir());
    roots
}

/// What could be a game in the app's shared folder on a phone (`Documents` in the app's own folder, which the Files app
/// and a computer can fill, or `at.shared`): the folder itself (loose game files), and each folder and file in it, folders first, by
/// name. Hidden entries are left out. Nothing is opened here: the caller classifies each.
pub fn shared_folder_candidates(at: &Where) -> Vec<PathBuf> {
    let Some(dir) = at
        .shared
        .clone()
        .or_else(|| at.home.as_ref().map(|h| h.join("Documents")))
    else {
        return Vec::new();
    };
    let Ok(rd) = fs::read_dir(&dir) else { return Vec::new() };
    let mut entries: Vec<PathBuf> = rd
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .map(|e| e.path())
        .collect();
    entries.sort_by_key(|p| {
        let name = p.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase());
        (!p.is_dir(), name)
    });
    let mut out = vec![dir];
    out.extend(entries);
    out
}

/// Whether a folder or file name looks like the game's: it names Warcraft II (`Warcraft II`, `Warcraft2`, `war2`).
pub fn named_like_the_game(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("warcraft") || n.contains("war2") || n.contains("war 2")
}

/// The children of `bases` named like the game, in the order of `bases` (each base's children sorted by name).
pub fn candidate_dirs(bases: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for base in bases {
        let Ok(rd) = fs::read_dir(base) else { continue };
        let mut found: Vec<PathBuf> = rd
            .flatten()
            .filter(|e| named_like_the_game(&e.file_name().to_string_lossy()))
            .map(|e| e.path())
            .collect();
        found.sort();
        for p in found {
            if !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(root: &Path, rel: &str) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b"x").unwrap();
    }

    #[test]
    fn names_that_look_like_the_game() {
        for n in [
            "Warcraft II",
            "Warcraft II BNE.app",
            "WARCRAFT2",
            "war2",
            "Warcraft 2 Battle.net",
        ] {
            assert!(named_like_the_game(n), "{n}");
        }
        for n in ["Starcraft", "Diablo II", "GOG Games", "Wine"] {
            assert!(!named_like_the_game(n), "{n}");
        }
    }

    #[test]
    fn a_mac_looks_in_applications_and_wine_bottles() {
        let home = tempfile::tempdir().unwrap();
        let h = home.path();
        fs::create_dir_all(h.join("Library/Application Support/CrossOver/Bottles/Win10/drive_c")).unwrap();
        fs::create_dir_all(h.join("Library/Containers/com.example.Bottler/Bottles/One/drive_c")).unwrap();
        fs::create_dir_all(h.join(".wine/drive_c")).unwrap();
        // Not a prefix: no `drive_c`.
        fs::create_dir_all(h.join("Library/Application Support/CrossOver/Bottles/Empty")).unwrap();
        let at = Where {
            home: Some(h.to_path_buf()),
            program_files: Vec::new(),
            shared: None,
        };
        let b = bases(Os::Mac, &at);
        assert!(b.contains(&PathBuf::from("/Applications")));
        assert!(b.contains(&h.join("Applications")));
        for prefix in [
            "Library/Application Support/CrossOver/Bottles/Win10",
            "Library/Containers/com.example.Bottler/Bottles/One",
            ".wine",
        ] {
            assert!(b.contains(&h.join(prefix).join("drive_c/GOG Games")), "{prefix}");
            assert!(
                b.contains(&h.join(prefix).join("drive_c/Program Files (x86)/GOG Galaxy/Games")),
                "{prefix}"
            );
        }
        assert!(
            !b.iter()
                .any(|p| p.starts_with(h.join("Library/Application Support/CrossOver/Bottles/Empty")))
        );
    }

    #[test]
    fn a_phone_looks_at_everything_in_its_shared_folder() {
        let home = tempfile::tempdir().unwrap();
        let h = home.path();
        touch(h, "Documents/Zeta/War2Dat.mpq");
        touch(h, "Documents/alpha/note.txt");
        touch(h, "Documents/disc.iso");
        touch(h, "Documents/.hidden");
        touch(h, "Library/elsewhere.iso");
        let at = Where {
            home: Some(h.to_path_buf()),
            program_files: Vec::new(),
            shared: None,
        };
        let docs = h.join("Documents");
        assert_eq!(
            shared_folder_candidates(&at),
            [
                docs.clone(),
                docs.join("alpha"),
                docs.join("Zeta"),
                docs.join("disc.iso")
            ],
            "the folder itself, then folders by name, then files"
        );
        // Android names its shared folder (the external files folder) instead of `Documents`.
        let shared = h.join("Documents/Zeta");
        let at = Where {
            shared: Some(shared.clone()),
            ..at
        };
        assert_eq!(
            shared_folder_candidates(&at),
            [shared.clone(), shared.join("War2Dat.mpq")]
        );
        let none = Where::default();
        assert!(shared_folder_candidates(&none).is_empty());
    }

    #[test]
    fn linux_looks_in_gog_games_and_prefixes() {
        let home = tempfile::tempdir().unwrap();
        let h = home.path();
        fs::create_dir_all(h.join("Games/lutris-prefix/drive_c")).unwrap();
        let at = Where {
            home: Some(h.to_path_buf()),
            program_files: Vec::new(),
            shared: None,
        };
        let b = bases(Os::Linux, &at);
        assert_eq!(b[0], h.join("GOG Games"));
        assert!(b.contains(&h.join("Games/lutris-prefix/drive_c/Program Files (x86)/GOG.com")));
        assert!(!b.contains(&PathBuf::from("/Applications")));
    }

    #[test]
    fn windows_uses_the_program_files_folders_and_the_gog_folders() {
        let at = Where {
            home: Some(PathBuf::from("/home/<user>")),
            program_files: vec![PathBuf::from("/pf"), PathBuf::from("/pf86")],
            shared: None,
        };
        let b = bases(Os::Windows, &at);
        assert!(b.contains(&PathBuf::from("/pf")));
        assert!(b.contains(&PathBuf::from("/pf86").join("GOG Galaxy").join("Games")));
        assert!(b.contains(&PathBuf::from("C:\\GOG Games")));
        assert!(b.contains(&PathBuf::from("/home/<user>").join("GOG Games")));
    }

    #[test]
    fn only_children_named_like_the_game_are_candidates() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        touch(r, "GOG Games/Warcraft II BNE/War2Dat.mpq");
        touch(r, "GOG Games/Warcraft2-copy/note.txt");
        touch(r, "GOG Games/Other Game/file.bin");
        touch(r, "Apps/Warcraft II.app/Contents/x");
        let got = candidate_dirs(&[r.join("GOG Games"), r.join("Apps"), r.join("Missing")]);
        assert_eq!(
            got,
            [
                r.join("GOG Games/Warcraft II BNE"),
                r.join("GOG Games/Warcraft2-copy"),
                r.join("Apps/Warcraft II.app"),
            ]
        );
    }
}
