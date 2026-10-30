//! Storm's adaptive Huffman decoder (FGK variant with preset weight tables).
//!
//! Ported from StormLib `src/huffman/huff.cpp` by Ladislav Zezula and ShadowFlare,
//! <https://github.com/ladislav-zezula/StormLib>, MIT License, Copyright (c) 1999-2013 Ladislav Zezula.
//! Changes: pointers became indices into one arena (index 0 is the list head); StormLib's quick-link cache is replaced
//! by a lookup table of our own ([`Shortcuts`]); the compressor is left out.

use super::bits::Bits;
use super::{Error, Result};

const ITEM_COUNT: usize = 515;
const BYTE_ITEM_COUNT: usize = 258;
const NONE: usize = usize::MAX;
const HEAD: usize = 0;
const END_OF_STREAM: u32 = 0x100;
const NEW_BYTE: u32 = 0x101;

const BINARY: [u8; 256] = [
    0x54, 0x16, 0x16, 0x0D, 0x0C, 0x08, 0x06, 0x05, 0x06, 0x05, 0x06, 0x03, 0x04, 0x04, 0x03, 0x05, 0x0E, 0x0B, 0x14,
    0x13, 0x13, 0x09, 0x0B, 0x06, 0x05, 0x04, 0x03, 0x02, 0x03, 0x02, 0x02, 0x02, 0x0D, 0x07, 0x09, 0x06, 0x06, 0x04,
    0x03, 0x02, 0x04, 0x03, 0x03, 0x03, 0x03, 0x03, 0x02, 0x02, 0x09, 0x06, 0x04, 0x04, 0x04, 0x04, 0x03, 0x02, 0x03,
    0x02, 0x02, 0x02, 0x02, 0x03, 0x02, 0x04, 0x08, 0x03, 0x04, 0x07, 0x09, 0x05, 0x03, 0x03, 0x03, 0x03, 0x02, 0x02,
    0x02, 0x03, 0x02, 0x02, 0x03, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x01, 0x01, 0x01, 0x02, 0x01, 0x02,
    0x02, 0x06, 0x0A, 0x08, 0x08, 0x06, 0x07, 0x04, 0x03, 0x04, 0x04, 0x02, 0x02, 0x04, 0x02, 0x03, 0x03, 0x04, 0x03,
    0x07, 0x07, 0x09, 0x06, 0x04, 0x03, 0x03, 0x02, 0x01, 0x02, 0x02, 0x02, 0x02, 0x02, 0x0A, 0x02, 0x02, 0x03, 0x02,
    0x02, 0x01, 0x01, 0x02, 0x02, 0x02, 0x06, 0x03, 0x05, 0x02, 0x03, 0x02, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
    0x01, 0x01, 0x01, 0x02, 0x03, 0x01, 0x01, 0x01, 0x02, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x02, 0x04, 0x04, 0x04,
    0x07, 0x09, 0x08, 0x0C, 0x02, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x02, 0x01,
    0x01, 0x03, 0x04, 0x01, 0x02, 0x04, 0x05, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x02, 0x01, 0x01, 0x01, 0x04,
    0x01, 0x01, 0x01, 0x01, 0x01, 0x02, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x02, 0x01, 0x01, 0x01,
    0x01, 0x01, 0x01, 0x01, 0x03, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x02, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
    0x02, 0x02, 0x01, 0x01, 0x02, 0x02, 0x02, 0x06, 0x4B,
];

const TEXT: [u8; 128] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03, 0x27, 0x00, 0x00, 0x23, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0x01, 0x01, 0x01, 0x01, 0x01,
    0x01, 0x01, 0x02, 0x02, 0x01, 0x01, 0x06, 0x0E, 0x10, 0x04, 0x06, 0x08, 0x05, 0x04, 0x04, 0x03, 0x03, 0x02, 0x02,
    0x03, 0x03, 0x01, 0x01, 0x02, 0x01, 0x01, 0x01, 0x04, 0x02, 0x04, 0x02, 0x02, 0x02, 0x01, 0x01, 0x04, 0x01, 0x01,
    0x02, 0x03, 0x03, 0x02, 0x03, 0x01, 0x03, 0x06, 0x04, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x02, 0x01, 0x02, 0x01,
    0x01, 0x01, 0x29, 0x07, 0x16, 0x12, 0x40, 0x0A, 0x0A, 0x11, 0x25, 0x01, 0x03, 0x17, 0x10, 0x26, 0x2A, 0x10, 0x01,
    0x23, 0x23, 0x2F, 0x10, 0x06, 0x07, 0x02, 0x09, 0x01, 0x01, 0x01, 0x01, 0x01, 0x00,
];

const GENERAL: [u8; 256] = [
    0xFF, 0x0B, 0x07, 0x05, 0x0B, 0x02, 0x02, 0x02, 0x06, 0x02, 0x02, 0x01, 0x04, 0x02, 0x01, 0x03, 0x09, 0x01, 0x01,
    0x01, 0x03, 0x04, 0x01, 0x01, 0x02, 0x01, 0x01, 0x01, 0x02, 0x01, 0x01, 0x01, 0x05, 0x01, 0x01, 0x01, 0x0D, 0x01,
    0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x02, 0x01, 0x01, 0x03, 0x01, 0x01, 0x01, 0x01, 0x01,
    0x01, 0x01, 0x02, 0x01, 0x01, 0x01, 0x01, 0x0A, 0x04, 0x02, 0x01, 0x06, 0x03, 0x02, 0x01, 0x01, 0x01, 0x01, 0x01,
    0x03, 0x01, 0x01, 0x01, 0x05, 0x02, 0x03, 0x04, 0x03, 0x03, 0x03, 0x02, 0x01, 0x01, 0x01, 0x02, 0x01, 0x02, 0x03,
    0x03, 0x01, 0x03, 0x01, 0x01, 0x02, 0x05, 0x01, 0x01, 0x04, 0x03, 0x05, 0x01, 0x03, 0x01, 0x03, 0x03, 0x02, 0x01,
    0x04, 0x03, 0x0A, 0x06, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x02, 0x02, 0x01, 0x0A, 0x02,
    0x05, 0x01, 0x01, 0x02, 0x07, 0x02, 0x17, 0x01, 0x05, 0x01, 0x01, 0x0E, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
    0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
    0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
    0x01, 0x01, 0x06, 0x02, 0x01, 0x04, 0x05, 0x01, 0x01, 0x02, 0x01, 0x01, 0x01, 0x01, 0x02, 0x01, 0x01, 0x01, 0x01,
    0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
    0x01, 0x01, 0x01, 0x01, 0x07, 0x01, 0x01, 0x02, 0x01, 0x01, 0x01, 0x01, 0x02, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
    0x01, 0x02, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x11,
];

const ADPCM_4: [u8; 16] = [
    0xFF, 0xFB, 0x98, 0x9A, 0x84, 0x85, 0x63, 0x64, 0x3E, 0x3E, 0x22, 0x22, 0x13, 0x13, 0x18, 0x17,
];

const ADPCM_6: [u8; 64] = [
    0xFF, 0xF1, 0x9D, 0x9E, 0x9A, 0x9B, 0x9A, 0x97, 0x93, 0x93, 0x8C, 0x8E, 0x86, 0x88, 0x80, 0x82, 0x7C, 0x7C, 0x72,
    0x73, 0x69, 0x6B, 0x5F, 0x60, 0x55, 0x56, 0x4A, 0x4B, 0x40, 0x41, 0x37, 0x37, 0x2F, 0x2F, 0x27, 0x27, 0x21, 0x21,
    0x1B, 0x1C, 0x17, 0x17, 0x13, 0x13, 0x10, 0x10, 0x0D, 0x0D, 0x0B, 0x0B, 0x09, 0x09, 0x08, 0x08, 0x07, 0x07, 0x06,
    0x05, 0x05, 0x04, 0x04, 0x04, 0x19, 0x18,
];

/// Stereo tables: (offset, values) runs.
const STEREO_3: &[(usize, &[u8])] = &[
    (0x00, &[0xC3, 0xCB, 0xF5, 0x41, 0xFF, 0x7B, 0xF7, 0x21]),
    (0x40, &[0xBF, 0xCC, 0xF2, 0x40, 0xFD, 0x7C, 0xF7, 0x22]),
    (0x80, &[0x7A, 0x46]),
];
const STEREO_4: &[(usize, &[u8])] = &[
    (
        0x00,
        &[
            0xC3, 0xD9, 0xEF, 0x3D, 0xF9, 0x7C, 0xE9, 0x1E, 0xFD, 0xAB, 0xF1, 0x2C, 0xFC, 0x5B, 0xFE, 0x17,
        ],
    ),
    (
        0x40,
        &[
            0xBD, 0xD9, 0xEC, 0x3D, 0xF5, 0x7D, 0xE8, 0x1D, 0xFB, 0xAE, 0xF0, 0x2C, 0xFB, 0x5C, 0xFF, 0x18,
        ],
    ),
    (0x80, &[0x70, 0x6C]),
];
const STEREO_5: &[(usize, &[u8])] = &[
    (
        0x00,
        &[
            0xBA, 0xC5, 0xDA, 0x33, 0xE3, 0x6D, 0xD8, 0x18, 0xE5, 0x94, 0xDA, 0x23, 0xDF, 0x4A, 0xD1, 0x10, 0xEE, 0xAF,
            0xE4, 0x2C, 0xEA, 0x5A, 0xDE, 0x15, 0xF4, 0x87, 0xE9, 0x21, 0xF6, 0x43, 0xFC, 0x12,
        ],
    ),
    (
        0x40,
        &[
            0xB0, 0xC7, 0xD8, 0x33, 0xE3, 0x6B, 0xD6, 0x18, 0xE7, 0x95, 0xD8, 0x23, 0xDB, 0x49, 0xD0, 0x11, 0xE9, 0xB2,
            0xE2, 0x2B, 0xE8, 0x5C, 0xDD, 0x15, 0xF1, 0x87, 0xE7, 0x20, 0xF7, 0x44, 0xFF, 0x13,
        ],
    ),
    (0x80, &[0x5F, 0x9E]),
];

/// The byte-weight table for a data type (0..=8); bytes with weight 0 start outside the tree.
fn weights(kind: usize) -> [u8; 256] {
    let mut t = [0u8; 256];
    let runs: &[(usize, &[u8])] = match kind {
        0 => {
            t[0x00] = 0x0A;
            t[0xFF] = 0x02;
            return t;
        }
        1 => &[(0, &BINARY)],
        2 => &[(0, &TEXT)],
        3 => &[(0, &GENERAL)],
        4 => &[(0, &ADPCM_4)],
        5 => &[(0, &ADPCM_6)],
        6 => STEREO_3,
        7 => STEREO_4,
        _ => STEREO_5,
    };
    for &(off, vals) in runs {
        t[off..off + vals.len()].copy_from_slice(vals);
    }
    t
}

#[derive(Clone, Copy)]
struct Item {
    next: usize,
    prev: usize,
    value: u32,
    weight: u32,
    parent: usize,
    child_lo: usize,
}

const EMPTY: Item = Item {
    next: NONE,
    prev: NONE,
    value: 0,
    weight: 0,
    parent: NONE,
    child_lo: NONE,
};

#[derive(Clone, Copy, PartialEq)]
enum Insert {
    After,
    Before,
}

struct Tree {
    items: Vec<Item>,
    by_byte: [usize; BYTE_ITEM_COUNT],
}

impl Tree {
    fn new() -> Self {
        let mut items = vec![EMPTY; ITEM_COUNT + 1];
        items[HEAD].next = HEAD;
        items[HEAD].prev = HEAD;
        Self {
            items,
            by_byte: [NONE; BYTE_ITEM_COUNT],
        }
    }

    fn remove(&mut self, i: usize) {
        if self.items[i].next != NONE {
            let (p, n) = (self.items[i].prev, self.items[i].next);
            self.items[p].next = n;
            self.items[n].prev = p;
            self.items[i].next = NONE;
            self.items[i].prev = NONE;
        }
    }

    /// Links `i2` right after `i1`.
    fn link(&mut self, i1: usize, i2: usize) {
        let n = self.items[i1].next;
        self.items[i2].next = n;
        self.items[i2].prev = self.items[n].prev;
        self.items[n].prev = i2;
        self.items[i1].next = i2;
    }

    fn insert(&mut self, i: usize, at: Insert) {
        self.remove(i);
        match at {
            Insert::After => self.link(HEAD, i),
            Insert::Before => {
                let p = self.items[HEAD].prev;
                self.link(p, i)
            }
        }
    }

    fn find_higher_or_equal(&self, mut i: usize, weight: u32) -> usize {
        if i != NONE {
            while i != HEAD {
                if self.items[i].weight >= weight {
                    return i;
                }
                i = self.items[i].prev;
            }
        }
        HEAD
    }

    fn create(&mut self, used: &mut usize, value: u32, weight: u32, at: Insert) -> Option<usize> {
        if *used >= ITEM_COUNT {
            return None;
        }
        *used += 1;
        let i = *used; // arena slot 0 is the head
        self.insert(i, at);
        let it = &mut self.items[i];
        it.value = value;
        it.weight = weight;
        it.parent = NONE;
        it.child_lo = NONE;
        Some(i)
    }

    fn fixup_pos(&mut self, i: usize, max_weight: u32) -> u32 {
        if self.items[i].weight < max_weight {
            let higher = self.find_higher_or_equal(self.items[HEAD].prev, self.items[i].weight);
            self.remove(i);
            self.link(higher, i);
            max_weight
        } else {
            self.items[i].weight
        }
    }

    fn build(&mut self, used: &mut usize, kind: usize) -> Result<()> {
        let table = weights(kind);
        let mut max_weight = 0;
        for (b, &w) in table.iter().enumerate() {
            if w != 0 {
                let i = self.create(used, b as u32, w as u32, Insert::After).ok_or_else(full)?;
                self.by_byte[b] = i;
                max_weight = self.fixup_pos(i, max_weight);
            }
        }
        self.by_byte[0x100] = self.create(used, 0x100, 1, Insert::Before).ok_or_else(full)?;
        self.by_byte[0x101] = self.create(used, 0x101, 1, Insert::Before).ok_or_else(full)?;

        let mut lo = self.items[HEAD].prev;
        while lo != HEAD {
            let hi = self.items[lo].prev;
            if hi == HEAD {
                break;
            }
            let w = self.items[hi].weight + self.items[lo].weight;
            let p = self.create(used, 0, w, Insert::After).ok_or_else(full)?;
            self.items[lo].parent = p;
            self.items[hi].parent = p;
            self.items[p].child_lo = lo;
            max_weight = self.fixup_pos(p, max_weight);
            lo = self.items[hi].prev;
        }
        Ok(())
    }

    fn inc_weights(&mut self, mut i: usize) {
        while i != NONE {
            self.items[i].weight += 1;
            let higher = self.find_higher_or_equal(self.items[i].prev, self.items[i].weight);
            let hi = self.items[higher].next;
            if hi != i {
                self.remove(hi);
                self.link(i, hi);
                self.remove(i);
                self.link(higher, i);

                let lo = self.items[self.items[hi].parent].child_lo;
                let parent = self.items[i].parent;
                if self.items[parent].child_lo == i {
                    self.items[parent].child_lo = hi;
                }
                if lo == hi {
                    let hp = self.items[hi].parent;
                    self.items[hp].child_lo = i;
                }
                let parent = self.items[i].parent;
                self.items[i].parent = self.items[hi].parent;
                self.items[hi].parent = parent;
            }
            i = self.items[i].parent;
        }
    }

    fn insert_new_branch(&mut self, used: &mut usize, v1: u32, v2: u32) -> Result<()> {
        let last = self.items[HEAD].prev;
        let hi = self
            .create(used, v1, self.items[last].weight, Insert::Before)
            .ok_or_else(full)?;
        self.items[hi].parent = last;
        self.by_byte[v1 as usize] = hi;
        let lo = self.create(used, v2, 0, Insert::Before).ok_or_else(full)?;
        self.items[lo].parent = last;
        self.items[last].child_lo = lo;
        self.by_byte[v2 as usize] = lo;
        self.inc_weights(lo);
        Ok(())
    }
}

fn full() -> Error {
    Error::compression("huffman: tree item pool exhausted")
}

/// How many bits one table lookup decodes.
const PEEK_BITS: u32 = 7;

/// Where the next `PEEK_BITS` bits lead from the root, for each of their values, filled in as
/// they are first met. The tree only changes on a new byte (or every symbol, in the sparse
/// kind), so between changes most symbols decode in one lookup instead of a step per bit.
struct Shortcuts {
    /// The item reached, how many bits it took, and the tree version it holds for.
    entries: Vec<(usize, u32, u32)>,
    version: u32,
}

impl Shortcuts {
    fn new() -> Self {
        Self {
            entries: vec![(HEAD, 0, 0); 1 << PEEK_BITS],
            version: 1,
        }
    }

    /// Forgets every entry, once the tree has changed.
    fn invalidate(&mut self) {
        self.version += 1;
    }

    /// The item `peeked` leads to: a leaf within its bits, or the branch after all of them.
    fn get(&mut self, tree: &Tree, root: usize, peeked: usize) -> (usize, u32) {
        let (i, n, v) = self.entries[peeked];
        if v == self.version {
            return (i, n);
        }
        let (mut i, mut n) = (root, 0);
        while n < PEEK_BITS && tree.items[i].child_lo != NONE {
            let lo = tree.items[i].child_lo;
            i = if (peeked >> n) & 1 == 1 {
                tree.items[lo].prev
            } else {
                lo
            };
            n += 1;
        }
        self.entries[peeked] = (i, n, self.version);
        (i, n)
    }
}

/// Decompresses a Huffman stream, producing at most `max_out` bytes.
pub fn decompress(data: &[u8], max_out: usize) -> Result<Vec<u8>> {
    let mut bits = Bits::new(data);
    let eof = || Error::compression("huffman: unexpected end of data");
    let kind = bits.take(8).ok_or_else(eof)? as usize;
    if kind & 0x0F > 8 {
        return Err(Error::compression(format!("huffman: unknown data type {kind}")));
    }
    let sparse = kind == 0;
    let mut tree = Tree::new();
    let mut used = 0usize;
    tree.build(&mut used, kind & 0x0F)?;

    let mut out = Vec::with_capacity(max_out);
    let mut shortcuts = Shortcuts::new();
    loop {
        let mut i = tree.items[HEAD].next;
        if i == HEAD {
            return Err(Error::compression("huffman: empty tree"));
        }
        let (peeked, real) = bits.peek(PEEK_BITS);
        let (to, n) = shortcuts.get(&tree, i, peeked);
        // Past the end of the data the peeked bits are padding: walk on so it fails as before.
        if n <= real {
            bits.skip(n);
            i = to;
        }
        while tree.items[i].child_lo != NONE {
            let lo = tree.items[i].child_lo;
            i = if bits.take(1).ok_or_else(eof)? == 1 {
                tree.items[lo].prev
            } else {
                lo
            };
        }
        let mut value = tree.items[i].value;
        if value == END_OF_STREAM {
            break;
        }
        if value == NEW_BYTE {
            value = bits.take(8).ok_or_else(eof)?;
            let last_value = tree.items[tree.items[HEAD].prev].value;
            tree.insert_new_branch(&mut used, last_value, value)?;
            if !sparse {
                tree.inc_weights(tree.by_byte[value as usize]);
            }
            shortcuts.invalidate();
        }
        if out.len() >= max_out {
            break;
        }
        out.push(value as u8);
        if sparse {
            tree.inc_weights(tree.by_byte[value as usize]);
            shortcuts.invalidate();
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encodes `data` as [`decompress`] reads it: each byte's code is its path from the root, found walking up from its
    /// item, and the tree changes after each symbol as the decoder's does.
    fn encode(kind: u8, data: &[u8]) -> Vec<u8> {
        let mut tree = Tree::new();
        let mut used = 0;
        tree.build(&mut used, kind as usize & 0x0F).unwrap();
        let sparse = kind == 0;
        let (mut out, mut acc, mut n) = (Vec::new(), 0u64, 0);
        let mut put = |v: u32, bits: u32| {
            acc |= u64::from(v) << n;
            n += bits;
            while n >= 8 {
                out.push(acc as u8);
                acc >>= 8;
                n -= 8;
            }
        };
        put(u32::from(kind), 8);
        let code = |tree: &Tree, mut i: usize| {
            let mut path = Vec::new();
            while tree.items[i].parent != NONE {
                let p = tree.items[i].parent;
                path.push(u32::from(tree.items[p].child_lo != i));
                i = p;
            }
            path.into_iter().rev()
        };
        for &b in data {
            let i = tree.by_byte[b as usize];
            if i != NONE {
                code(&tree, i).for_each(|bit| put(bit, 1));
                if sparse {
                    tree.inc_weights(i);
                }
                continue;
            }
            code(&tree, tree.by_byte[NEW_BYTE as usize]).for_each(|bit| put(bit, 1));
            put(u32::from(b), 8);
            let last_value = tree.items[tree.items[HEAD].prev].value;
            tree.insert_new_branch(&mut used, last_value, u32::from(b)).unwrap();
            tree.inc_weights(tree.by_byte[b as usize]);
        }
        code(&tree, tree.by_byte[END_OF_STREAM as usize]).for_each(|bit| put(bit, 1));
        put(0, 7);
        out
    }

    /// Bytes with a skew, as real data has, and some the preset weights lack (so new bytes are added).
    fn sample(len: usize) -> Vec<u8> {
        let mut x = 0x1234_5678u32;
        (0..len)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                if x.is_multiple_of(4) {
                    (x >> 8) as u8
                } else {
                    (x >> 8) as u8 & 0x0F
                }
            })
            .collect()
    }

    #[test]
    fn what_is_encoded_decodes() {
        let data = sample(5000);
        for kind in [0, 1, 2, 7, 8] {
            assert_eq!(
                decompress(&encode(kind, &data), data.len()).unwrap(),
                data,
                "kind {kind}"
            );
        }
    }

    #[test]
    fn a_cut_stream_fails() {
        let data = sample(300);
        for kind in [0, 7] {
            let stream = encode(kind, &data);
            for n in 0..stream.len() {
                assert!(decompress(&stream[..n], data.len()).is_err(), "kind {kind}, cut at {n}");
            }
        }
    }
}
