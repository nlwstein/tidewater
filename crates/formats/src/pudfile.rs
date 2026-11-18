//! A PUD file as its list of sections, kept losslessly so a map editor can change some sections and write the rest
//! back byte for byte.
//!
//! Format source: the public PUD specification (Daniel Lemberg, "PUD file format", 1997; see [`crate::pud`]). A PUD is
//! a list of sections, each a 4-byte tag, a little-endian u32 length and the payload. [`crate::pud::Pud`] reads the
//! sections the game reads; this keeps all of them, in file order, including tags it doesn't know and second copies,
//! plus any bytes after the last whole section. Splitting and joining never fails and gives back the same bytes.
//!
//! Shipped maps use three section orders (with or without `SIGN`, with or without `ERAX`); a new section is inserted
//! where [`ORDER`] puts it relative to the sections present, so a written map keeps the order the game's tables read.

/// One section: its tag and payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub tag: [u8; 4],
    pub data: Vec<u8>,
}

/// The sections of a PUD, in file order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PudFile {
    pub sections: Vec<Section>,
    /// Bytes after the last whole section (a truncated section's header and payload), kept as they are.
    pub trailing: Vec<u8>,
}

/// The order the game's header and body tables read sections in (`crate::pud`), used to place a new section.
pub const ORDER: [&[u8; 4]; 21] = [
    b"TYPE", b"VER ", b"DESC", b"OWNR", b"ERA ", b"ERAX", b"SIGN", b"DIM ", b"UDTA", b"UGRD", b"ALOW", b"SIDE",
    b"SGLD", b"SLBR", b"SOIL", b"AIPL", b"MTXM", b"SQM ", b"OILM", b"REGM", b"UNIT",
];

impl PudFile {
    /// Splits a file into sections. A section whose length runs past the end ends the list; the rest is
    /// [`Self::trailing`].
    pub fn split(data: &[u8]) -> Self {
        let mut sections = Vec::new();
        let mut i = 0;
        while i + 8 <= data.len() {
            let len = u32::from_le_bytes(data[i + 4..i + 8].try_into().expect("4 bytes")) as usize;
            if len > data.len() - (i + 8) {
                break;
            }
            sections.push(Section {
                tag: data[i..i + 4].try_into().expect("4 bytes"),
                data: data[i + 8..i + 8 + len].to_vec(),
            });
            i += 8 + len;
        }
        Self {
            sections,
            trailing: data[i..].to_vec(),
        }
    }

    /// The file's bytes: every section in order, then the trailing bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.sections.iter().map(|s| 8 + s.data.len()).sum::<usize>());
        for s in &self.sections {
            out.extend(s.tag);
            out.extend((s.data.len() as u32).to_le_bytes());
            out.extend(&s.data);
        }
        out.extend(&self.trailing);
        out
    }

    /// The payload of the first section with this tag.
    pub fn get(&self, tag: &[u8; 4]) -> Option<&[u8]> {
        self.sections.iter().find(|s| &s.tag == tag).map(|s| s.data.as_slice())
    }

    /// Replaces the payload of the first section with this tag, or inserts the section where [`ORDER`] places it
    /// (after the last present section that comes before it there; first if none does).
    pub fn set(&mut self, tag: &[u8; 4], data: Vec<u8>) {
        if let Some(s) = self.sections.iter_mut().find(|s| &s.tag == tag) {
            s.data = data;
            return;
        }
        let rank = |t: &[u8; 4]| ORDER.iter().position(|o| *o == t);
        let at = match rank(tag) {
            Some(r) => self
                .sections
                .iter()
                .rposition(|s| rank(&s.tag).is_some_and(|k| k < r))
                .map_or(0, |k| k + 1),
            None => self.sections.len(),
        };
        self.sections.insert(at, Section { tag: *tag, data });
    }

    /// Removes every section with this tag.
    pub fn remove(&mut self, tag: &[u8; 4]) {
        self.sections.retain(|s| &s.tag != tag);
    }
}

/// Little-endian u16 words to bytes.
pub fn words(w: &[u16]) -> Vec<u8> {
    w.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// Bytes to little-endian u16 words (an odd last byte is dropped).
pub fn from_words(b: &[u8]) -> Vec<u16> {
    b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pud::build::{Map, section};

    #[test]
    fn split_and_join_give_the_same_bytes() {
        let mut bytes = Map::default().bytes();
        // An unknown section, a second copy, and a truncated tail are all kept.
        bytes.extend(section(b"ZZZZ", &[1, 2, 3]));
        bytes.extend(section(b"UNIT", &[]));
        bytes.extend(b"MTXM\xff\x00\x00\x00ab");
        let f = PudFile::split(&bytes);
        assert_eq!(f.trailing, b"MTXM\xff\x00\x00\x00ab");
        assert_eq!(f.sections.iter().filter(|s| &s.tag == b"UNIT").count(), 2);
        assert_eq!(f.to_bytes(), bytes);
    }

    #[test]
    fn set_replaces_or_inserts_in_table_order() {
        let mut f = PudFile::split(&Map::default().bytes());
        f.set(b"DESC", vec![7; 32]);
        assert_eq!(f.get(b"DESC"), Some(&[7u8; 32][..]));
        // AIPL goes after SOIL, ERAX after ERA .
        f.set(b"AIPL", vec![0; 16]);
        f.set(b"ERAX", vec![1, 0]);
        let tags: Vec<&[u8; 4]> = f.sections.iter().map(|s| &s.tag).collect();
        let at = |t: &[u8; 4]| tags.iter().position(|x| *x == t).unwrap();
        assert_eq!(at(b"AIPL"), at(b"SOIL") + 1);
        assert_eq!(at(b"ERAX"), at(b"ERA ") + 1);
        // The result still loads.
        crate::pud::Pud::parse(&f.to_bytes()).unwrap();
        f.remove(b"AIPL");
        assert!(f.get(b"AIPL").is_none());
    }

    #[test]
    fn word_helpers() {
        assert_eq!(words(&[0x1234, 1]), [0x34, 0x12, 1, 0]);
        assert_eq!(from_words(&[0x34, 0x12, 9]), [0x1234]);
    }
}
