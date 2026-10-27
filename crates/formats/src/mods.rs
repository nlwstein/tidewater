//! Mods: folders of loose files that override the game's files, read through the same [`Install`](crate::install)
//! as the game's own (`docs/mods.md`).
//!
//! **Layout.** A mod is a folder `<data>/mods/<name>/` holding files laid out with the archive names (`rez/unitdata.dat`,
//! `Art/...`, `Maps/...`), in any letter case and with `/` or `\` between components, and an optional `mod.toml`:
//!
//! ```toml
//! name = "A readable title"
//! version = "1.2"
//! description = "One line."
//! authors = ["Ann", "Bo"]
//! ```
//!
//! Only that subset of TOML is read: `key = "text"` lines, `authors` as a list of texts, `#` comments; anything else is
//! ignored. A mod is data only: nothing in it is ever run, and the loader reads nothing but files named by the game.
//!
//! **Order.** The enabled mods are an ordered list, lowest priority first: a later mod overrides an earlier one, and the
//! mods override the game's loose files and archives.
//!
//! **Names.** A mod's name is its folder name and is checked ([`valid_name`]) before any path is built from it, so a
//! name from a settings file or the command line cannot point outside the mods folder.
//!
//! **Identity.** [`ModSet::hash`] hashes every file of the set (the name, length and bytes of each, in a fixed order),
//! so two machines or a recording and its playback can tell that they run the same mods
//! ([`crate::identity::DataIdentity`]). It is a checksum, not a defence against a hostile mod: a mod can already change
//! the game's data.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::identity::{FNV_BASIS, ModTag, fnv_extend};

/// The manifest file in a mod's folder.
pub const MANIFEST: &str = "mod.toml";

/// The folder name for mods inside the data folder.
pub const DIR: &str = "mods";

/// The longest mod name.
pub const MAX_NAME: usize = 48;

/// Why mods could not be set up.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ModError {
    #[error("{0:?} is not a valid mod name (letters, digits, space, - _ . only; no path)")]
    BadName(String),
    #[error("no mod named {0:?} in the mods folder")]
    NotFound(String),
}

/// The mods folder inside a data folder.
pub fn dir(data: &Path) -> PathBuf {
    data.join(DIR)
}

/// Whether `name` can be a mod's folder name: 1 to [`MAX_NAME`] ASCII letters, digits, spaces, `-`, `_` and `.`, not
/// starting with `.` or a space and not ending with `.` or a space. That rules out `..`, separators, drive letters and
/// hidden files.
pub fn valid_name(name: &str) -> bool {
    let ok_char = |c: char| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.');
    !name.is_empty()
        && name.len() <= MAX_NAME
        && name.chars().all(ok_char)
        && !name.starts_with(['.', ' '])
        && !name.ends_with(['.', ' '])
}

/// What a mod's `mod.toml` says (empty fields when it says nothing or is missing).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModInfo {
    /// The folder name: how settings and the command line name the mod.
    pub folder: String,
    /// The readable title (`name`), empty when none.
    pub title: String,
    pub version: String,
    pub description: String,
    pub authors: Vec<String>,
}

impl ModInfo {
    /// The title to show: the manifest's, else the folder name.
    pub fn display(&self) -> &str {
        if self.title.is_empty() {
            &self.folder
        } else {
            &self.title
        }
    }

    pub fn tag(&self) -> ModTag {
        ModTag {
            name: self.folder.clone(),
            version: self.version.clone(),
        }
    }

    /// Reads `mod.toml`'s text for the mod in folder `folder`.
    pub fn parse(folder: &str, text: &str) -> ModInfo {
        let mut info = ModInfo {
            folder: folder.to_string(),
            ..ModInfo::default()
        };
        for line in text.lines() {
            let line = line.trim();
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            match key {
                "name" => info.title = unquote(value).unwrap_or_default(),
                "version" => info.version = unquote(value).unwrap_or_default(),
                "description" => info.description = unquote(value).unwrap_or_default(),
                "authors" => info.authors = texts(value),
                _ => {}
            }
        }
        info
    }
}

/// The text of a `"quoted"` value (no escapes beyond `\"` and `\\`), a trailing `# comment` allowed.
fn unquote(value: &str) -> Option<String> {
    let rest = value.strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out.chars().filter(|c| !c.is_control()).collect()),
            '\\' => out.push(chars.next()?),
            c => out.push(c),
        }
    }
    None
}

/// The texts of a `["a", "b"]` list on one line.
fn texts(value: &str) -> Vec<String> {
    let Some(inner) = value.trim().strip_prefix('[') else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut rest = inner;
    while let Some(start) = rest.find('"') {
        let tail = &rest[start..];
        let Some(text) = unquote(tail) else { break };
        // Skip past the closing quote of the text just read.
        let mut esc = false;
        let mut end = 1;
        for (i, c) in tail.char_indices().skip(1) {
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                end = i + 1;
                break;
            }
        }
        out.push(text);
        rest = &tail[end..];
    }
    out
}

/// One enabled mod.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModRoot {
    pub info: ModInfo,
    pub path: PathBuf,
}

/// The enabled mods, lowest priority first.
#[derive(Debug, Default)]
pub struct ModSet {
    roots: Vec<ModRoot>,
    hash: OnceLock<u64>,
}

impl ModSet {
    /// No mods.
    pub fn none() -> ModSet {
        ModSet::default()
    }

    /// The mods named `names` in the mods folder `dir`, in that order (a repeated name counts once, at its first
    /// place). A name is matched to its folder in any letter case.
    pub fn resolve(dir: &Path, names: &[String]) -> Result<ModSet, ModError> {
        let mut roots: Vec<ModRoot> = Vec::new();
        for name in names {
            if !valid_name(name) {
                return Err(ModError::BadName(name.clone()));
            }
            let Some(found) = find_folder(dir, name) else {
                return Err(ModError::NotFound(name.clone()));
            };
            let folder = found
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            if roots.iter().any(|r| r.info.folder.eq_ignore_ascii_case(&folder)) {
                continue;
            }
            roots.push(read_root(&folder, found));
        }
        Ok(ModSet {
            roots,
            hash: OnceLock::new(),
        })
    }

    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }

    pub fn roots(&self) -> &[ModRoot] {
        &self.roots
    }

    /// The mods as a replay or the lobby names them.
    pub fn tags(&self) -> Vec<ModTag> {
        self.roots.iter().map(|r| r.info.tag()).collect()
    }

    /// The path of the file `name` (archive-style, any letter case) in the highest-priority mod that has it.
    pub fn find(&self, name: &str) -> Option<PathBuf> {
        self.roots
            .iter()
            .rev()
            .find_map(|r| crate::install::find_case_insensitive(&r.path, name))
    }

    /// The `.pud` files of every mod's `Maps` folder (and folders below it) as archive-style names starting `Maps\\`, once
    /// each (a map two mods have counts once).
    pub fn maps(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for r in &self.roots {
            for f in files_of(&r.path) {
                let Some(rest) = f.lower.strip_prefix("maps\\") else {
                    continue;
                };
                if !rest.ends_with(".pud") {
                    continue;
                }
                // The mod's own spelling of the name below `Maps`.
                let shown = format!("Maps\\{}", &f.shown["maps\\".len()..]);
                if !out.iter().any(|o| o.eq_ignore_ascii_case(&shown)) {
                    out.push(shown);
                }
            }
        }
        out
    }

    /// The hash of every file of the set, in order; 0 with no mod. The first call reads the files, later calls reuse it.
    pub fn hash(&self) -> u64 {
        if self.roots.is_empty() {
            return 0;
        }
        *self.hash.get_or_init(|| {
            let mut h = FNV_BASIS;
            for r in &self.roots {
                h = fnv_extend(h, r.info.folder.to_ascii_lowercase().as_bytes());
                h = fnv_extend(h, &[0]);
                for f in files_of(&r.path) {
                    let path = f.path;
                    h = fnv_extend(h, f.lower.as_bytes());
                    let len = std::fs::metadata(&path).map_or(0, |m| m.len());
                    h = fnv_extend(h, &len.to_le_bytes());
                    if let Ok(mut f) = std::fs::File::open(&path) {
                        let mut buf = vec![0u8; 1 << 16];
                        while let Ok(n) = f.read(&mut buf) {
                            if n == 0 {
                                break;
                            }
                            h = fnv_extend(h, &buf[..n]);
                        }
                    }
                }
            }
            h | 1
        })
    }
}

fn read_root(folder: &str, path: PathBuf) -> ModRoot {
    let text = crate::install::find_case_insensitive(&path, MANIFEST)
        .and_then(|p| std::fs::read(p).ok())
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();
    ModRoot {
        info: ModInfo::parse(folder, &text),
        path,
    }
}

/// The folder of mod `name` in `dir`, matching the name in any letter case.
fn find_folder(dir: &Path, name: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name))
        .map(|e| e.path())
        .find(|p| p.is_dir())
}

/// Every mod in the mods folder `dir` with a valid name, by name.
pub fn list(dir: &Path) -> Vec<ModInfo> {
    let mut v: Vec<ModInfo> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            valid_name(&name).then(|| read_root(&name, e.path()).info)
        })
        .collect();
    v.sort_by_key(|m| m.folder.to_ascii_lowercase());
    v
}

/// A file of a mod.
struct ModFile {
    /// Its name below the mod's folder, lower case, `\` between components.
    lower: String,
    /// The same name as the folders spell it.
    shown: String,
    path: PathBuf,
}

/// The files below `root`, sorted by lower-case name, without the manifest. Folders that are links are not followed.
fn files_of(root: &Path) -> Vec<ModFile> {
    fn walk(dir: &Path, prefix: &str, out: &mut Vec<ModFile>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let path = e.path();
            let shown = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}\\{name}")
            };
            match e.file_type() {
                Ok(t) if t.is_dir() => walk(&path, &shown, out),
                _ if std::fs::metadata(&path).is_ok_and(|m| m.is_file()) => out.push(ModFile {
                    lower: shown.to_ascii_lowercase(),
                    shown,
                    path,
                }),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(root, "", &mut out);
    out.retain(|f| f.lower != MANIFEST);
    out.sort_by(|a, b| a.lower.cmp(&b.lower));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder for one test.
    pub(crate) fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tidewater-mods-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn put(root: &Path, rel: &str, bytes: &[u8]) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }

    #[test]
    fn names_that_could_leave_the_folder_are_refused() {
        for good in ["pack", "My Pack 2", "a-b_c.d", "x"] {
            assert!(valid_name(good), "{good}");
        }
        for bad in [
            "",
            ".",
            "..",
            "../x",
            "a/b",
            "a\\b",
            ".hidden",
            " lead",
            "trail ",
            "trail.",
            "C:",
            "a:b",
            "é",
            "nul\0",
            &"x".repeat(MAX_NAME + 1),
        ] {
            assert!(!valid_name(bad), "{bad:?}");
        }
        let dir = temp("names");
        assert_eq!(
            ModSet::resolve(&dir, &["..".into()]).unwrap_err(),
            ModError::BadName("..".into())
        );
        assert_eq!(
            ModSet::resolve(&dir, &["nothere".into()]).unwrap_err(),
            ModError::NotFound("nothere".into())
        );
    }

    #[test]
    fn the_manifest_is_a_small_subset_of_toml() {
        let text = "# a mod\nname = \"Big \\\"Axe\\\"\"\nversion = \"1.2\"  # now\ndescription = \"Hits harder.\"\nauthors = [\"Ann\", \"B\\\\o\"]\nextra = 5\nbroken\n";
        let m = ModInfo::parse("axe", text);
        assert_eq!(m.folder, "axe");
        assert_eq!(m.title, "Big \"Axe\"");
        assert_eq!(m.version, "1.2");
        assert_eq!(m.description, "Hits harder.");
        assert_eq!(m.authors, ["Ann", "B\\o"]);
        assert_eq!(m.display(), "Big \"Axe\"");
        let none = ModInfo::parse("plain", "");
        assert_eq!((none.display(), none.version.as_str()), ("plain", ""));
        assert!(ModInfo::parse("x", "name = unquoted\nauthors = nope").title.is_empty());
    }

    #[test]
    fn mods_resolve_in_order_in_any_letter_case_and_once() {
        let dir = temp("order");
        put(&dir, "Alpha/mod.toml", b"version = \"2\"\n");
        put(&dir, "beta/x.txt", b"1");
        let set = ModSet::resolve(&dir, &["BETA".into(), "alpha".into(), "Beta".into()]).unwrap();
        let tags = set.tags();
        assert_eq!(
            tags.iter()
                .map(|t| (t.name.as_str(), t.version.as_str()))
                .collect::<Vec<_>>(),
            [("beta", ""), ("Alpha", "2")]
        );
        assert!(ModSet::none().is_empty() && ModSet::none().hash() == 0);
    }

    #[test]
    fn a_later_mod_wins_and_names_match_in_any_case() {
        let dir = temp("wins");
        put(&dir, "a/Rez/UnitData.dat", b"from a");
        put(&dir, "a/art/only_a.bin", b"a");
        put(&dir, "b/rez/unitdata.dat", b"from b");
        let set = ModSet::resolve(&dir, &["a".into(), "b".into()]).unwrap();
        let read = |n: &str| set.find(n).map(|p| std::fs::read(p).unwrap());
        assert_eq!(read("rez\\unitdata.dat").unwrap(), b"from b");
        assert_eq!(read("REZ/UNITDATA.DAT").unwrap(), b"from b");
        assert_eq!(read("Art\\Only_A.bin").unwrap(), b"a");
        assert_eq!(read("rez\\other.dat"), None);
        // The names that try to climb out find nothing.
        assert_eq!(set.find("..\\a\\art\\only_a.bin"), None);
    }

    #[test]
    fn the_hash_follows_content_names_order_and_ignores_the_manifest() {
        let dir = temp("hash");
        put(&dir, "a/rez/x.dat", b"1");
        put(&dir, "b/rez/y.dat", b"2");
        let ab = |names: &[&str]| {
            ModSet::resolve(&dir, &names.iter().map(|s| s.to_string()).collect::<Vec<_>>())
                .unwrap()
                .hash()
        };
        let base = ab(&["a", "b"]);
        assert_ne!(base, 0);
        assert_eq!(base, ab(&["a", "b"]));
        assert_ne!(base, ab(&["b", "a"]), "order");
        assert_ne!(base, ab(&["a"]), "set");
        put(&dir, "a/mod.toml", b"description = \"changed\"\n");
        assert_eq!(base, ab(&["a", "b"]), "the manifest is not game data");
        put(&dir, "a/rez/x.dat", b"3");
        assert_ne!(base, ab(&["a", "b"]), "content");
    }

    #[test]
    fn maps_of_mods_are_listed_once() {
        let dir = temp("maps");
        put(&dir, "a/Maps/One.pud", b"x");
        put(&dir, "a/Maps/sub/Two.PUD", b"x");
        put(&dir, "b/MAPS/one.pud", b"y");
        put(&dir, "b/art/notamap.pud", b"y");
        let set = ModSet::resolve(&dir, &["a".into(), "b".into()]).unwrap();
        let mut maps = set.maps();
        maps.sort();
        assert_eq!(maps, ["Maps\\One.pud", "Maps\\sub\\Two.PUD"]);
    }

    #[test]
    fn list_shows_valid_folders_by_name() {
        let dir = temp("list");
        put(&dir, "Zed/mod.toml", b"name = \"Last\"\n");
        put(&dir, "alpha/x", b"1");
        put(&dir, ".hidden/x", b"1");
        put(&dir, "file.txt", b"1");
        let names: Vec<String> = list(&dir).into_iter().map(|m| m.folder).collect();
        assert_eq!(names, ["alpha", "Zed"]);
        assert!(list(&dir.join("missing")).is_empty());
    }
}
