//! A minimal MPQ writer, so tests build synthetic archives with our own code (no game data anywhere).
//! Layout: header, file data (stored, unencrypted, one block each), hash table, block table; the tables encrypted
//! (Zezula, "The MoPaQ Archive Format").

use crate::mpq::{HashType, encrypt, flags, hash_string};

/// The bytes of an archive holding `files` (name, contents).
pub fn mpq_bytes(files: &[(&str, &[u8])]) -> Vec<u8> {
    let hash_n = (files.len() * 2).next_power_of_two().max(4);
    let mut data = Vec::new();
    let mut blocks = Vec::new();
    for (_, bytes) in files {
        blocks.push([
            32 + data.len() as u32,
            bytes.len() as u32,
            bytes.len() as u32,
            flags::EXISTS,
        ]);
        data.extend_from_slice(bytes);
    }
    let mut hashes = vec![[0xFFFF_FFFFu32; 4]; hash_n];
    for (i, (name, _)) in files.iter().enumerate() {
        let mut slot = hash_string(name, HashType::TableOffset) as usize & (hash_n - 1);
        while hashes[slot][3] != 0xFFFF_FFFF {
            slot = (slot + 1) & (hash_n - 1);
        }
        hashes[slot] = [
            hash_string(name, HashType::NameA),
            hash_string(name, HashType::NameB),
            0,
            i as u32,
        ];
    }
    let table = |rows: &[[u32; 4]], key: &str| {
        let mut b: Vec<u8> = rows.iter().flatten().flat_map(|w| w.to_le_bytes()).collect();
        encrypt(&mut b, hash_string(key, HashType::FileKey));
        b
    };
    let (hash_t, block_t) = (table(&hashes, "(hash table)"), table(&blocks, "(block table)"));
    let hash_pos = 32 + data.len() as u32;
    let block_pos = hash_pos + hash_t.len() as u32;
    let total = block_pos + block_t.len() as u32;
    let mut out = Vec::new();
    out.extend_from_slice(b"MPQ\x1A");
    for w in [32u32, total] {
        out.extend_from_slice(&w.to_le_bytes());
    }
    out.extend_from_slice(&0u16.to_le_bytes()); // format version 0
    out.extend_from_slice(&3u16.to_le_bytes()); // 4096-byte sectors
    for w in [hash_pos, block_pos, hash_n as u32, blocks.len() as u32] {
        out.extend_from_slice(&w.to_le_bytes());
    }
    out.extend_from_slice(&data);
    out.extend_from_slice(&hash_t);
    out.extend_from_slice(&block_t);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_writer_makes_archives_the_reader_opens() {
        let bytes = mpq_bytes(&[("a\\b.txt", b"hello"), ("c.bin", b"0123456789")]);
        let mut a = crate::mpq::Archive::from_reader(Box::new(std::io::Cursor::new(bytes))).unwrap();
        assert_eq!(a.read("A\\B.TXT").unwrap(), b"hello");
        assert_eq!(a.read("c.bin").unwrap(), b"0123456789");
    }
}
