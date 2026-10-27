//! The identity of the game data a game ran on: a hash of the data tables the simulation reads, so two runs (two
//! machines in a multiplayer game, a recording and its playback) can tell at once that they used different files.
//!
//! The multiplayer lobby (`docs/design/network.md`, "Compatibility check") and the replay header
//! (`docs/design/multiplayer-determinism.md`, "Replays") use the same fingerprint. The hash is FNV-1a 64, a plain
//! checksum with no security meaning: it finds accidents, not forgeries.

use serde::{Deserialize, Serialize};

use crate::install::{Install, InstallError};

/// The starting value of an FNV-1a 64 hash.
pub const FNV_BASIS: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 of `bytes`, folded into `h`: the data identity hashes the tables one after another.
pub fn fnv_extend(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// The data tables the simulation reads, by archive name.
pub const TABLES: [&str; 4] = [
    "rez\\unitdata.dat",
    "rez\\upgrades.dat",
    "rez\\script.bin",
    "rez\\ai.bin",
];

/// The hash of the data tables the simulation reads, through `read` (an installation's `read`). Each table's name and
/// length go in with its bytes, so moving bytes between tables changes the hash.
pub fn data_hash<E>(mut read: impl FnMut(&str) -> Result<Vec<u8>, E>) -> Result<u64, E> {
    let mut h = FNV_BASIS;
    for name in TABLES {
        let bytes = read(name)?;
        h = fnv_extend(h, name.as_bytes());
        h = fnv_extend(h, &(bytes.len() as u64).to_le_bytes());
        h = fnv_extend(h, &bytes);
    }
    Ok(h)
}

/// One mod of the set a game ran with: enough to name it to a player.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct ModTag {
    /// The mod's folder name.
    pub name: String,
    /// The version its `mod.toml` gives, empty when it gives none.
    pub version: String,
}

impl std::fmt::Display for ModTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.version.is_empty() {
            f.write_str(&self.name)
        } else {
            write!(f, "{} {}", self.name, self.version)
        }
    }
}

/// A list of mods in words for a player: "none" when empty.
pub fn describe_mods(mods: &[ModTag]) -> String {
    if mods.is_empty() {
        "none".to_string()
    } else {
        mods.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
    }
}

/// Which game data a game ran on.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct DataIdentity {
    /// [`data_hash`] of the tables as the game read them, mods included.
    pub data_hash: u64,
    /// Hash of every file of the active mods, 0 with no mod (`Install::mods_hash`).
    pub mods_hash: u64,
    /// The active mods, in the order they apply (later ones override earlier ones).
    pub mods: Vec<ModTag>,
}

/// How two game data identities differ.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DataMismatch {
    /// The mods differ (a different set, order or content); `recorded` and `current` list them.
    Mods {
        recorded: Vec<ModTag>,
        current: Vec<ModTag>,
    },
    /// The mods agree, but the unit, upgrade, script or AI tables do not: other game files (another version or
    /// edition).
    Tables,
}

impl std::fmt::Display for DataMismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DataMismatch::Mods { recorded, current } => write!(
                f,
                "the mod set differs (recorded with: {}; now: {})",
                describe_mods(recorded),
                describe_mods(current)
            ),
            DataMismatch::Tables => {
                f.write_str("the data hash differs (the unit, upgrade, script or AI tables are not the same)")
            }
        }
    }
}

impl DataIdentity {
    /// The identity of the data `install` reads.
    pub fn of(install: &mut Install) -> Result<DataIdentity, InstallError> {
        let data_hash = data_hash(|n| install.read(n))?;
        Ok(DataIdentity {
            data_hash,
            mods_hash: install.mods_hash(),
            mods: install.mod_tags(),
        })
    }

    /// How `other` differs from `self`, if it does. A difference in the mods is reported before one in the tables,
    /// because the mods usually explain it.
    pub fn mismatch(&self, other: &DataIdentity) -> Option<DataMismatch> {
        if self.mods_hash != other.mods_hash || self.mods != other.mods {
            return Some(DataMismatch::Mods {
                recorded: self.mods.clone(),
                current: other.mods.clone(),
            });
        }
        (self.data_hash != other.data_hash).then_some(DataMismatch::Tables)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(data_hash: u64, mods_hash: u64, mods: &[&str]) -> DataIdentity {
        DataIdentity {
            data_hash,
            mods_hash,
            mods: mods
                .iter()
                .map(|n| ModTag {
                    name: (*n).into(),
                    version: "1".into(),
                })
                .collect(),
        }
    }

    #[test]
    fn the_data_hash_covers_names_and_lengths() {
        let tables = |a: &[u8], b: &[u8]| {
            data_hash::<()>(|n| {
                Ok(match n {
                    "rez\\unitdata.dat" => a.to_vec(),
                    "rez\\upgrades.dat" => b.to_vec(),
                    _ => vec![1],
                })
            })
            .unwrap()
        };
        assert_eq!(tables(b"ab", b"c"), tables(b"ab", b"c"));
        assert_ne!(tables(b"ab", b"c"), tables(b"a", b"bc"), "moving a byte between tables");
        assert_ne!(tables(b"ab", b"c"), tables(b"ab", b"d"));
    }

    #[test]
    fn identities_say_which_part_differs() {
        let a = id(1, 0, &[]);
        assert_eq!(a.mismatch(&id(1, 0, &[])), None);
        assert_eq!(a.mismatch(&id(2, 0, &[])), Some(DataMismatch::Tables));
        let m = a.mismatch(&id(2, 9, &["pack"])).expect("mods differ");
        assert!(matches!(&m, DataMismatch::Mods { recorded, current } if recorded.is_empty() && current.len() == 1));
        let text = m.to_string();
        assert!(
            text.contains("mod set differs") && text.contains("none") && text.contains("pack 1"),
            "{text}"
        );
        // The same mods but other content (or order) is a mod difference too.
        assert!(matches!(
            id(1, 5, &["a", "b"]).mismatch(&id(1, 6, &["a", "b"])),
            Some(DataMismatch::Mods { .. })
        ));
        assert!(matches!(
            id(1, 5, &["a", "b"]).mismatch(&id(1, 5, &["b", "a"])),
            Some(DataMismatch::Mods { .. })
        ));
    }

    /// A folder holding an archive with the four tables, each `byte` repeated.
    fn install_with(dir: &std::path::Path, byte: u8) -> Install {
        let tables: Vec<(&str, Vec<u8>)> = TABLES.iter().map(|n| (*n, vec![byte; 8])).collect();
        let files: Vec<(&str, &[u8])> = tables.iter().map(|(n, b)| (*n, b.as_slice())).collect();
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("War2Dat.mpq"), crate::testmpq::mpq_bytes(&files)).unwrap();
        Install::open(dir).unwrap()
    }

    #[test]
    fn an_installation_has_an_identity_that_follows_its_tables_and_loose_files() {
        let root = std::env::temp_dir().join(format!("tidewater-identity-{}", std::process::id()));
        let mut a = install_with(&root.join("a"), 1);
        let mut b = install_with(&root.join("b"), 2);
        let (ia, ib) = (DataIdentity::of(&mut a).unwrap(), DataIdentity::of(&mut b).unwrap());
        assert_eq!(ia.mods_hash, 0);
        assert!(ia.mods.is_empty());
        assert_eq!(ia.mismatch(&ib), Some(DataMismatch::Tables));
        // A loose file beats the archive's table, so it changes the identity too.
        std::fs::create_dir_all(root.join("a").join("rez")).unwrap();
        std::fs::write(root.join("a").join("rez").join("unitdata.dat"), [9; 8]).unwrap();
        assert_ne!(DataIdentity::of(&mut a).unwrap().data_hash, ia.data_hash);
        let _ = std::fs::remove_dir_all(&root);
    }
}
