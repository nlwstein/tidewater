//! PKWARE Data Compression Library "implode" decoder.
//!
//! Written from `blast.c` by Mark Adler, <https://github.com/madler/zlib/tree/master/contrib/blast>, zlib License,
//! Copyright (C) 2003, 2012, 2013 Mark Adler; the code tables are the `explode` crate's (Aaron Griffith,
//! <https://crates.io/crates/explode>, MIT License), which this replaces. Changes: each code is read through a table
//! indexed by its longest length's worth of bits, instead of a bit at a time, the whole stream is decoded in one call.
//!
//! The stream: a byte saying whether literals are coded (1) or plain (0), a byte giving the low bits of a distance
//! (4 to 6), then LSB-first bits: 0 and a literal, or 1, a length and a distance back into the output so far. Codes
//! are canonical Huffman codes with their bits inverted; length 519 ends the stream.

use super::bits::Bits;
use super::{Error, Result};

/// A canonical code: how many codes there are of each length (from 0), then the symbols in code order.
struct Code {
    counts: &'static [u8],
    symbols: &'static [u8],
}

const LITERAL: Code = Code {
    counts: &[0, 0, 0, 0, 1, 11, 20, 21, 16, 7, 5, 10, 91, 74],
    symbols: &[
        0x20, 0x45, 0x61, 0x65, 0x69, 0x6c, 0x6e, 0x6f, 0x72, 0x73, 0x74, 0x75, 0x2d, 0x31, 0x41, 0x43, 0x44, 0x49,
        0x4c, 0x4e, 0x4f, 0x52, 0x53, 0x54, 0x62, 0x63, 0x64, 0x66, 0x67, 0x68, 0x6d, 0x70, 0x0a, 0x0d, 0x28, 0x29,
        0x2c, 0x2e, 0x30, 0x32, 0x33, 0x34, 0x35, 0x37, 0x38, 0x3d, 0x42, 0x46, 0x4d, 0x50, 0x55, 0x6b, 0x77, 0x09,
        0x22, 0x27, 0x2a, 0x2f, 0x36, 0x39, 0x3a, 0x47, 0x48, 0x57, 0x5b, 0x5f, 0x76, 0x78, 0x79, 0x2b, 0x3e, 0x4b,
        0x56, 0x58, 0x59, 0x5d, 0x21, 0x24, 0x26, 0x71, 0x7a, 0x00, 0x3c, 0x3f, 0x4a, 0x51, 0x5a, 0x5c, 0x6a, 0x7b,
        0x7c, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x0b, 0x0c, 0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14,
        0x15, 0x16, 0x17, 0x18, 0x19, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x23, 0x25, 0x3b, 0x40, 0x5e, 0x60, 0x7d, 0x7e,
        0x7f, 0xb0, 0xb1, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xbb, 0xbc, 0xbd, 0xbe, 0xbf, 0xc0,
        0xc1, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xcb, 0xcc, 0xcd, 0xce, 0xcf, 0xd0, 0xd1, 0xd2,
        0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xdb, 0xdc, 0xdd, 0xde, 0xdf, 0xe1, 0xe5, 0xe9, 0xee, 0xf2,
        0xf3, 0xf4, 0x1a, 0x80, 0x81, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8a, 0x8b, 0x8c, 0x8d, 0x8e,
        0x8f, 0x90, 0x91, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0x9b, 0x9c, 0x9d, 0x9e, 0x9f, 0xa0,
        0xa1, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xab, 0xac, 0xad, 0xae, 0xaf, 0xe0, 0xe2, 0xe3,
        0xe4, 0xe6, 0xe7, 0xe8, 0xea, 0xeb, 0xec, 0xed, 0xef, 0xf0, 0xf1, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9, 0xfa, 0xfb,
        0xfc, 0xfd, 0xfe, 0xff,
    ],
};

const LENGTH: Code = Code {
    counts: &[0, 0, 1, 3, 3, 4, 3, 2],
    symbols: &[
        0x0, 0x1, 0x2, 0x3, 0x4, 0x5, 0x6, 0x7, 0x8, 0x9, 0xa, 0xb, 0xc, 0xd, 0xe, 0xf,
    ],
};

const DISTANCE: Code = Code {
    counts: &[0, 0, 1, 0, 2, 4, 15, 26, 16],
    symbols: &[
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10, 0x11,
        0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x20, 0x21, 0x22, 0x23,
        0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x2b, 0x2c, 0x2d, 0x2e, 0x2f, 0x30, 0x31, 0x32, 0x33, 0x34, 0x35,
        0x36, 0x37, 0x38, 0x39, 0x3a, 0x3b, 0x3c, 0x3d, 0x3e, 0x3f,
    ],
};

/// A length code's base and how many extra bits follow it.
const LEN_BASE: [u32; 16] = [3, 2, 4, 5, 6, 7, 8, 9, 10, 12, 16, 24, 40, 72, 136, 264];
const LEN_EXTRA: [u32; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8];
const END: u32 = 519;

/// A code's symbol and length for every value of its longest length's worth of bits, as `blast.c`'s `decode` would
/// read them a bit at a time.
struct Table {
    bits: u32,
    entries: Vec<(u8, u8)>,
}

impl Table {
    fn new(code: &Code) -> Self {
        let bits = code.counts.len() as u32 - 1;
        let entries = (0..1usize << bits)
            .map(|peeked| {
                let (mut code_so_far, mut first, mut index) = (0usize, 0usize, 0usize);
                for len in 1..=bits {
                    code_so_far |= ((peeked >> (len - 1)) & 1) ^ 1;
                    let count = code.counts[len as usize] as usize;
                    if code_so_far < first + count {
                        return (code.symbols[index + code_so_far - first], len as u8);
                    }
                    index += count;
                    first = (first + count) << 1;
                    code_so_far <<= 1;
                }
                unreachable!("the PKWARE codes are complete")
            })
            .collect();
        Self { bits, entries }
    }

    fn decode(&self, bits: &mut Bits) -> Option<u32> {
        let (peeked, real) = bits.peek(self.bits);
        let (symbol, len) = self.entries[peeked];
        // Past the end of the data the peeked bits are padding: a code that needs them is cut off.
        if u32::from(len) > real {
            return None;
        }
        bits.skip(u32::from(len));
        Some(u32::from(symbol))
    }
}

struct Tables {
    literal: Table,
    length: Table,
    distance: Table,
}

fn tables() -> &'static Tables {
    static TABLES: std::sync::OnceLock<Tables> = std::sync::OnceLock::new();
    TABLES.get_or_init(|| Tables {
        literal: Table::new(&LITERAL),
        length: Table::new(&LENGTH),
        distance: Table::new(&DISTANCE),
    })
}

/// Decompresses a whole imploded stream.
pub fn explode(data: &[u8]) -> Result<Vec<u8>> {
    let cut = || Error::compression("pkware: unexpected end of data");
    let t = tables();
    let mut bits = Bits::new(data);
    let coded = match bits.take(8).ok_or_else(cut)? {
        0 => false,
        1 => true,
        _ => return Err(Error::compression("pkware: bad literal flag")),
    };
    let dict = bits.take(8).ok_or_else(cut)?;
    if !(4..=6).contains(&dict) {
        return Err(Error::compression("pkware: bad dictionary size"));
    }
    let mut out = Vec::with_capacity(data.len() * 4);
    loop {
        if bits.take(1).ok_or_else(cut)? == 0 {
            let lit = if coded {
                t.literal.decode(&mut bits).ok_or_else(cut)?
            } else {
                bits.take(8).ok_or_else(cut)?
            };
            out.push(lit as u8);
            continue;
        }
        let symbol = t.length.decode(&mut bits).ok_or_else(cut)? as usize;
        let len = LEN_BASE[symbol] + bits.take(LEN_EXTRA[symbol]).ok_or_else(cut)?;
        if len == END {
            return Ok(out);
        }
        let extra = if len == 2 { 2 } else { dict };
        let high = t.distance.decode(&mut bits).ok_or_else(cut)?;
        let dist = ((high << extra) + bits.take(extra).ok_or_else(cut)? + 1) as usize;
        let len = len as usize;
        if dist > out.len() {
            return Err(Error::compression("pkware: distance too far back"));
        }
        let from = out.len() - dist;
        if dist >= len {
            out.extend_from_within(from..from + len);
        } else {
            // The copy overlaps what it writes: a run of the last `dist` bytes.
            for i in 0..len {
                out.push(out[from + i]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `blast.c`'s own test stream.
    #[test]
    fn the_blast_example_decodes() {
        let got = explode(&[0x00, 0x04, 0x82, 0x24, 0x25, 0x8f, 0x80, 0x7f]).unwrap();
        assert_eq!(got, b"AIAIAIAIAIAIA");
    }

    #[test]
    fn a_cut_stream_fails() {
        let data = [0x00, 0x04, 0x82, 0x24, 0x25, 0x8f, 0x80, 0x7f];
        for n in 0..data.len() {
            assert!(explode(&data[..n]).is_err(), "cut at {n}");
        }
    }

    #[test]
    fn a_bad_header_fails() {
        assert!(explode(&[0x02, 0x04, 0x00]).is_err());
        assert!(explode(&[0x00, 0x07, 0x00]).is_err());
    }

    /// The tables give every code of each length in turn, so each symbol is reached by exactly its share of values.
    #[test]
    fn every_symbol_has_its_share_of_each_table() {
        for code in [&LITERAL, &LENGTH, &DISTANCE] {
            let t = Table::new(code);
            for (len, &count) in code.counts.iter().enumerate().skip(1) {
                let share = 1usize << (t.bits as usize - len);
                let hits = t.entries.iter().filter(|e| e.1 as usize == len).count();
                assert_eq!(hits, count as usize * share, "length {len}");
            }
        }
    }
}
