//! Sector decompressors for MPQ archives. The Huffman and IMA ADPCM decoders are ports of StormLib (see each file's
//! header); the PKWARE DCL "implode" decoder is written from Mark Adler's `blast.c` (its header).

mod adpcm;
mod bits;
mod explode;
mod huffman;

#[derive(Debug)]
pub struct Error(pub String);

impl Error {
    pub fn compression(msg: impl Into<String>) -> Self {
        Self(msg.into())
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Compression mask bits of a sector's first byte (Zezula, "File data").
pub mod method {
    pub const HUFFMAN: u8 = 0x01;
    pub const PKWARE: u8 = 0x08;
    pub const ADPCM_MONO: u8 = 0x40;
    pub const ADPCM_STEREO: u8 = 0x80;
}

/// Decompresses one sector given its compression mask. Decompression runs in the reverse of the order in which the
/// compressor applied the methods: Huffman, then PKWARE, then ADPCM.
pub fn decompress(mask: u8, data: &[u8], size: usize) -> Result<Vec<u8>> {
    let supported = method::HUFFMAN | method::PKWARE | method::ADPCM_MONO | method::ADPCM_STEREO;
    if mask & !supported != 0 {
        return Err(Error(format!("unsupported compression mask {mask:#04x}")));
    }
    let mut cur = data.to_vec();
    if mask & method::HUFFMAN != 0 {
        // The Huffman layer's output is the input of the next layer, whose size isn't stored; ADPCM never expands.
        cur = huffman::decompress(&cur, size)?;
    }
    if mask & method::PKWARE != 0 {
        cur = explode::explode(&cur)?;
    }
    if mask & method::ADPCM_STEREO != 0 {
        cur = adpcm::decompress(&cur, size, 2)?;
    } else if mask & method::ADPCM_MONO != 0 {
        cur = adpcm::decompress(&cur, size, 1)?;
    }
    Ok(cur)
}
