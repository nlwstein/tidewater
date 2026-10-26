//! Smacker (`.smk`) video: the installation's cinematics.
//!
//! Format source: the MultimediaWiki page "Smacker" at <https://wiki.multimedia.cx/index.php/Smacker> (header and
//! frame layout, palette chunk, Huffman trees, block types, audio). The decoder below is written from that
//! description alone; it does not use or port any existing Smacker library.
//!
//! Layout as implemented:
//!
//! - **Header** (little endian): signature `SMK2` or `SMK4`, width, height, frame count, frame rate, flags (bit 0 ring
//!   frame, bit 1 Y-interlaced, bit 2 Y-doubled), seven audio buffer sizes, the size of the packed tree data, four
//!   unpacked tree sizes, seven audio rate words, one reserved word. Then one `u32` size per frame (the low two bits
//!   are flags, bit 0 marks a key frame), one type byte per frame (bit 0: palette chunk present, bit `n + 1`: audio
//!   track `n` present), and the tree data. A ring frame adds one more entry to both tables.
//! - **Frame rate**: a positive value is milliseconds per frame, a negative value is `-n` hundred-thousandths of a
//!   second, zero means ten frames per second.
//! - **Bit streams** are read least significant bit first.
//! - **Trees**: a presence bit, then the tree (bit 0 = leaf followed by an 8-bit value, bit 1 = branch followed by
//!   the 0 subtree and then the 1 subtree), then one trailing bit. Video uses four "big" trees (map, colour, full,
//!   type), each holding 16-bit values. A big tree stores a low-byte tree and a high-byte tree, three 16-bit escape
//!   values, and a tree whose leaves are coded with those two byte trees. A leaf equal to escape `k` means "take
//!   cache entry `k`"; every decoded value is moved to the front of a three-entry cache, and the caches are zeroed at
//!   the start of every frame.
//! - **Frame data**: an optional palette chunk (first byte is its length in 4-byte words, including that byte), then
//!   one length-prefixed chunk per audio track present, then the video bit stream.
//! - **Palette chunk**: starting from the previous palette, `1xxxxxxx` keeps `x + 1` entries, `01xxxxxx` copies
//!   `x + 1` entries from the previous palette starting at the index in the next byte, anything else is a red byte
//!   followed by green and blue bytes (6 bits each, scaled to 8 bits by `v * 255 / 63`).
//! - **Video**: 4x4 blocks in row-major order. A type code from the type tree carries the block kind in bits 0-1
//!   (0 mono, 1 full, 2 void, 3 solid), a run-length index in bits 2-7 and, for solid blocks, the colour in bits 8-15.
//!   The run index maps through a table (1 to 59, then 128, 256, 512, 1024, 2048). A mono block takes a colour pair
//!   from the colour tree (a set mask bit picks the low byte) and a 16-bit pixel mask from the map tree, least
//!   significant bit first, row by row; a full block takes pixel pairs from the full tree (right pair first); a void
//!   block keeps the previous frame's pixels. `SMK4` full blocks start with mode bits: `1` repeats each row, `01` is
//!   a form with one pixel pair per two rows (see the notes below), anything else is the plain form.
//! - **Audio**: a chunk is a `u32` length (including itself) and then either raw PCM (8-bit unsigned or 16-bit signed)
//!   or, when the track is flagged as packed, a `u32` unpacked size and a bit stream of Huffman-coded deltas: a
//!   has-data bit, a stereo bit, a 16-bit bit, one 8-bit tree per byte lane, the starting value of each channel
//!   (right channel first, high byte first), then deltas. Tracks flagged with the DCT/FFT codecs of later versions
//!   are reported but not decoded.
//!
//! Details taken on trust because no sample file uses them: the `SMK4` "mode 2" full block layout (here, one pixel
//! pair per two rows, each pixel doubled horizontally) and `SMK4` in general. The Y-scale flags are reported by
//! [`Smacker::y_scale`] but the frames are returned at the stored size.

use std::sync::Arc;

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SmkError {
    #[error("not a Smacker file")]
    NotSmacker,
    #[error("data ends early")]
    Truncated,
    #[error("corrupt Smacker data: {0}")]
    Corrupt(&'static str),
}

pub type Result<T> = std::result::Result<T, SmkError>;

const MAX_TRACKS: usize = 7;
const HEADER_LEN: usize = 104;
const MAX_TREE_DEPTH: usize = 600;
const MAX_TREE_NODES: usize = 1 << 18;

/// Run lengths for the 6-bit run index of a type code.
const RUNS: [u32; 64] = {
    let mut t = [0u32; 64];
    let mut i = 0;
    while i < 59 {
        t[i] = i as u32 + 1;
        i += 1;
    }
    t[59] = 128;
    t[60] = 256;
    t[61] = 512;
    t[62] = 1024;
    t[63] = 2048;
    t
};

/// Scales a 6-bit palette component to 8 bits.
fn scale6(v: u8) -> u8 {
    ((v as u32 * 255 + 31) / 63) as u8
}

/// A least-significant-bit-first bit reader.
struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn bit(&mut self) -> Result<u32> {
        let byte = *self.data.get(self.pos >> 3).ok_or(SmkError::Truncated)?;
        let b = (byte >> (self.pos & 7)) & 1;
        self.pos += 1;
        Ok(b as u32)
    }

    fn bits(&mut self, n: u32) -> Result<u32> {
        let mut v = 0;
        for i in 0..n {
            v |= self.bit()? << i;
        }
        Ok(v)
    }
}

#[derive(Clone, Copy, Debug)]
enum Node {
    Leaf(u16),
    /// A leaf standing for cache entry `k`.
    Escape(u8),
    Branch(u32, u32),
}

/// A Huffman tree. An empty tree decodes to zero without reading any bits.
#[derive(Clone, Debug, Default)]
struct Tree {
    nodes: Vec<Node>,
}

impl Tree {
    /// Reads an 8-bit-leaf tree: presence bit, tree, trailing bit.
    fn read8(bits: &mut Bits) -> Result<Tree> {
        let mut t = Tree::default();
        if bits.bit()? != 0 {
            t.build(bits, 0, &mut |b| Ok(Node::Leaf(b.bits(8)? as u16)))?;
        }
        bits.bit()?;
        Ok(t)
    }

    fn build(&mut self, bits: &mut Bits, depth: usize, leaf: &mut dyn FnMut(&mut Bits) -> Result<Node>) -> Result<u32> {
        if depth > MAX_TREE_DEPTH || self.nodes.len() > MAX_TREE_NODES {
            return Err(SmkError::Corrupt("tree too large"));
        }
        let idx = self.nodes.len() as u32;
        if bits.bit()? == 0 {
            let n = leaf(bits)?;
            self.nodes.push(n);
        } else {
            self.nodes.push(Node::Branch(0, 0));
            let l = self.build(bits, depth + 1, leaf)?;
            let r = self.build(bits, depth + 1, leaf)?;
            self.nodes[idx as usize] = Node::Branch(l, r);
        }
        Ok(idx)
    }

    fn walk(&self, bits: &mut Bits) -> Result<Node> {
        if self.nodes.is_empty() {
            return Ok(Node::Leaf(0));
        }
        let mut i = 0usize;
        loop {
            match self.nodes[i] {
                Node::Branch(l, r) => i = if bits.bit()? == 0 { l } else { r } as usize,
                n => return Ok(n),
            }
        }
    }

    fn decode8(&self, bits: &mut Bits) -> Result<u8> {
        match self.walk(bits)? {
            Node::Leaf(v) => Ok(v as u8),
            _ => Err(SmkError::Corrupt("escape in byte tree")),
        }
    }
}

/// A tree of 16-bit values with the three-entry escape cache.
#[derive(Clone, Debug, Default)]
struct BigTree {
    tree: Tree,
    cache: [u16; 3],
}

impl BigTree {
    fn read(bits: &mut Bits) -> Result<BigTree> {
        let mut out = BigTree::default();
        if bits.bit()? == 0 {
            bits.bit()?;
            return Ok(out);
        }
        let lo = Tree::read8(bits)?;
        let hi = Tree::read8(bits)?;
        let mut esc = [0u16; 3];
        for e in &mut esc {
            *e = bits.bits(16)? as u16;
        }
        out.tree.build(bits, 0, &mut |b| {
            let l = lo.decode8(b)? as u16;
            let h = hi.decode8(b)? as u16;
            let v = l | h << 8;
            Ok(match esc.iter().position(|&e| e == v) {
                Some(k) => Node::Escape(k as u8),
                None => Node::Leaf(v),
            })
        })?;
        bits.bit()?;
        Ok(out)
    }

    fn reset(&mut self) {
        self.cache = [0; 3];
    }

    fn decode(&mut self, bits: &mut Bits) -> Result<u16> {
        let v = match self.tree.walk(bits)? {
            Node::Leaf(v) => v,
            Node::Escape(k) => self.cache[k as usize],
            Node::Branch(..) => unreachable!("walk stops at leaves"),
        };
        if self.cache[0] != v {
            self.cache = [v, self.cache[0], self.cache[1]];
        }
        Ok(v)
    }
}

/// One audio track as described by the header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioTrack {
    /// Track number, 0 to 6.
    pub track: usize,
    /// Samples per second per channel.
    pub rate: u32,
    pub channels: u8,
    /// Bits per sample, 8 or 16.
    pub bits: u8,
    /// Smacker's own Huffman delta coding (otherwise raw PCM).
    pub packed: bool,
    /// Flagged with a codec from a later format version; chunks are skipped.
    pub unsupported_codec: bool,
    /// Largest unpacked chunk in bytes.
    pub max_bytes: u32,
}

/// Decoded audio for one track in one frame, interleaved by channel, as signed 16-bit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioChunk {
    pub track: usize,
    pub samples: Vec<i16>,
}

/// One decoded video frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub index: usize,
    pub keyframe: bool,
    /// `width * height` palette indices, row-major.
    pub pixels: Vec<u8>,
    pub palette: [[u8; 3]; 256],
    pub audio: Vec<AudioChunk>,
}

/// How the stored picture is meant to be stretched vertically for display.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum YScale {
    None,
    /// Header flag 2: every line is shown twice, the second copy interlaced.
    Interlaced,
    /// Header flag 4: every line is shown twice.
    Doubled,
}

/// A Smacker stream being decoded frame by frame.
pub struct Smacker {
    /// The file, shared: a video's picture and its sound are decoded by two readers of one copy.
    data: Arc<Vec<u8>>,
    version: u8,
    pub width: u32,
    pub height: u32,
    frame_count: usize,
    frame_rate: i32,
    flags: u32,
    audio: Vec<AudioTrack>,
    trees: [BigTree; 4],
    /// `(start, length)` of each frame's data within `data`.
    spans: Vec<(usize, usize)>,
    types: Vec<u8>,
    keys: Vec<bool>,
    next: usize,
    stride: usize,
    rows: usize,
    buf: Vec<u8>,
    palette: [[u8; 3]; 256],
    decode_audio: bool,
}

fn rd32(d: &[u8], o: usize) -> Result<u32> {
    d.get(o..o + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or(SmkError::Truncated)
}

impl Smacker {
    pub fn parse(data: &[u8]) -> Result<Smacker> {
        Self::from_vec(data.to_vec())
    }

    pub fn from_vec(data: Vec<u8>) -> Result<Smacker> {
        Self::from_shared(Arc::new(data))
    }

    /// A reader of a file another reader may hold too (`Arc::clone` it for each), without a copy.
    pub fn from_shared(data: Arc<Vec<u8>>) -> Result<Smacker> {
        if data.len() < 4 || &data[0..3] != b"SMK" {
            return Err(SmkError::NotSmacker);
        }
        let version = data[3];
        if version != b'2' && version != b'4' {
            return Err(SmkError::NotSmacker);
        }
        if data.len() < HEADER_LEN {
            return Err(SmkError::Truncated);
        }
        let width = rd32(&data, 4)?;
        let height = rd32(&data, 8)?;
        let frames = rd32(&data, 12)? as usize;
        let frame_rate = rd32(&data, 16)? as i32;
        let flags = rd32(&data, 20)?;
        let mut max_bytes = [0u32; MAX_TRACKS];
        for (i, m) in max_bytes.iter_mut().enumerate() {
            *m = rd32(&data, 24 + 4 * i)?;
        }
        let trees_size = rd32(&data, 52)? as usize;
        let mut audio = Vec::new();
        for (t, &max) in max_bytes.iter().enumerate() {
            let w = rd32(&data, 72 + 4 * t)?;
            if w & 0x4000_0000 == 0 {
                continue;
            }
            audio.push(AudioTrack {
                track: t,
                rate: w & 0x00FF_FFFF,
                channels: if w & 0x1000_0000 != 0 { 2 } else { 1 },
                bits: if w & 0x2000_0000 != 0 { 16 } else { 8 },
                packed: w & 0x8000_0000 != 0,
                unsupported_codec: w & 0x0C00_0000 != 0,
                max_bytes: max,
            });
        }
        if width == 0 || height == 0 || width > 4096 || height > 4096 {
            return Err(SmkError::Corrupt("implausible dimensions"));
        }
        if frames > 1 << 20 {
            return Err(SmkError::Corrupt("implausible frame count"));
        }
        let entries = frames + (flags & 1) as usize;
        let sizes_at = HEADER_LEN;
        let types_at = sizes_at + 4 * entries;
        let trees_at = types_at + entries;
        let mut pos = trees_at + trees_size;
        if pos > data.len() {
            return Err(SmkError::Truncated);
        }
        let mut spans = Vec::with_capacity(frames);
        let mut keys = Vec::with_capacity(frames);
        let mut types = Vec::with_capacity(frames);
        for i in 0..frames {
            let raw = rd32(&data, sizes_at + 4 * i)?;
            let len = (raw & !3) as usize;
            if pos + len > data.len() {
                return Err(SmkError::Truncated);
            }
            spans.push((pos, len));
            keys.push(raw & 1 != 0);
            types.push(data[types_at + i]);
            pos += len;
        }
        let mut bits = Bits::new(&data[trees_at..trees_at + trees_size]);
        let trees = [
            BigTree::read(&mut bits)?,
            BigTree::read(&mut bits)?,
            BigTree::read(&mut bits)?,
            BigTree::read(&mut bits)?,
        ];
        let stride = (width as usize).div_ceil(4) * 4;
        let rows = (height as usize).div_ceil(4) * 4;
        Ok(Smacker {
            data,
            version,
            width,
            height,
            frame_count: frames,
            frame_rate,
            flags,
            audio,
            trees,
            spans,
            types,
            keys,
            next: 0,
            stride,
            rows,
            buf: vec![0; stride * rows],
            palette: [[0; 3]; 256],
            decode_audio: true,
        })
    }

    /// Number of frames (not counting a ring frame).
    pub fn frame_count(&self) -> usize {
        self.frame_count
    }

    /// Frames per second.
    pub fn fps(&self) -> f64 {
        match self.frame_rate {
            0 => 10.0,
            n if n > 0 => 1000.0 / n as f64,
            n => 100_000.0 / -(n as f64),
        }
    }

    pub fn y_scale(&self) -> YScale {
        if self.flags & 4 != 0 {
            YScale::Doubled
        } else if self.flags & 2 != 0 {
            YScale::Interlaced
        } else {
            YScale::None
        }
    }

    pub fn has_ring_frame(&self) -> bool {
        self.flags & 1 != 0
    }

    /// `b'2'` or `b'4'`.
    pub fn version(&self) -> u8 {
        self.version
    }

    /// The audio tracks the header declares.
    pub fn audio_tracks(&self) -> &[AudioTrack] {
        &self.audio
    }

    /// Skips audio decoding in [`Smacker::next_frame`] (the frames then carry no audio).
    pub fn set_decode_audio(&mut self, on: bool) {
        self.decode_audio = on;
    }

    /// Index of the frame [`Smacker::next_frame`] will return.
    pub fn position(&self) -> usize {
        self.next
    }

    /// Starts over at frame 0. Frames depend on their predecessors, so decoding always runs forward from here.
    pub fn rewind(&mut self) {
        self.next = 0;
        self.buf.fill(0);
        self.palette = [[0; 3]; 256];
    }

    /// Decodes the next frame, or returns `None` after the last.
    pub fn next_frame(&mut self) -> Result<Option<Frame>> {
        if self.next >= self.frame_count {
            return Ok(None);
        }
        let index = self.next;
        let (start, len) = self.spans[index];
        let kind = self.types[index];
        // Borrow the frame's bytes separately from the decoder state.
        let data = Arc::clone(&self.data);
        let res = self.decode_frame(&data[start..start + len], kind);
        let audio = res?;
        self.next += 1;
        let (w, h) = (self.width as usize, self.height as usize);
        let mut pixels = Vec::with_capacity(w * h);
        for y in 0..h {
            pixels.extend_from_slice(&self.buf[y * self.stride..y * self.stride + w]);
        }
        Ok(Some(Frame {
            index,
            keyframe: self.keys[index],
            pixels,
            palette: self.palette,
            audio,
        }))
    }

    fn decode_frame(&mut self, mut d: &[u8], kind: u8) -> Result<Vec<AudioChunk>> {
        if kind & 1 != 0 {
            let words = *d.first().ok_or(SmkError::Truncated)? as usize;
            let n = words * 4;
            if n == 0 || n > d.len() {
                return Err(SmkError::Corrupt("palette chunk size"));
            }
            self.read_palette(&d[1..n])?;
            d = &d[n..];
        }
        let mut audio = Vec::new();
        for t in 0..MAX_TRACKS {
            if kind >> (t + 1) & 1 == 0 {
                continue;
            }
            let n = rd32(d, 0)? as usize;
            if n < 4 || n > d.len() {
                return Err(SmkError::Corrupt("audio chunk size"));
            }
            let chunk = &d[4..n];
            d = &d[n..];
            if !self.decode_audio {
                continue;
            }
            let Some(track) = self.audio.iter().find(|a| a.track == t).copied() else {
                continue;
            };
            if track.unsupported_codec {
                continue;
            }
            audio.push(AudioChunk {
                track: t,
                samples: decode_audio(&track, chunk)?,
            });
        }
        self.decode_video(d)?;
        Ok(audio)
    }

    fn read_palette(&mut self, chunk: &[u8]) -> Result<()> {
        let old = self.palette;
        let mut i = 0;
        let mut pos = 0usize;
        let mut next = || -> Result<u8> {
            let b = *chunk.get(i).ok_or(SmkError::Truncated)?;
            i += 1;
            Ok(b)
        };
        while pos < 256 {
            let b = next()?;
            if b & 0x80 != 0 {
                pos += (b & 0x7F) as usize + 1;
            } else if b & 0x40 != 0 {
                let n = (b & 0x3F) as usize + 1;
                let src = next()? as usize;
                if pos + n > 256 || src + n > 256 {
                    return Err(SmkError::Corrupt("palette copy range"));
                }
                self.palette[pos..pos + n].copy_from_slice(&old[src..src + n]);
                pos += n;
            } else {
                let g = next()?;
                let bl = next()?;
                self.palette[pos] = [scale6(b & 0x3F), scale6(g & 0x3F), scale6(bl & 0x3F)];
                pos += 1;
            }
        }
        if pos > 256 {
            return Err(SmkError::Corrupt("palette overrun"));
        }
        Ok(())
    }

    fn decode_video(&mut self, d: &[u8]) -> Result<()> {
        let mut bits = Bits::new(d);
        for t in &mut self.trees {
            t.reset();
        }
        let stride = self.stride;
        let bw = stride / 4;
        let total = bw * (self.rows / 4);
        let smk4 = self.version == b'4';
        let [mmap, mclr, full, kind] = &mut self.trees;
        let buf = &mut self.buf;
        let mut blk = 0usize;
        while blk < total {
            let code = kind.decode(&mut bits)?;
            let ty = code & 3;
            let run = RUNS[(code >> 2 & 0x3F) as usize] as usize;
            let colour = (code >> 8) as u8;
            for _ in 0..run {
                if blk >= total {
                    break;
                }
                let base = (blk / bw) * 4 * stride + (blk % bw) * 4;
                match ty {
                    0 => {
                        let pair = mclr.decode(&mut bits)?;
                        let mask = mmap.decode(&mut bits)?;
                        let (c1, c0) = ((pair & 0xFF) as u8, (pair >> 8) as u8);
                        for p in 0..16 {
                            buf[base + (p / 4) * stride + p % 4] = if mask >> p & 1 != 0 { c1 } else { c0 };
                        }
                    }
                    1 => {
                        let mut mode = 0;
                        if smk4 {
                            if bits.bit()? != 0 {
                                mode = 1;
                            } else if bits.bit()? != 0 {
                                mode = 2;
                            }
                        }
                        let mut put = |row: usize, x: usize, pair: u16| {
                            buf[base + row * stride + x] = pair as u8;
                            buf[base + row * stride + x + 1] = (pair >> 8) as u8;
                        };
                        match mode {
                            0 => {
                                for row in 0..4 {
                                    let right = full.decode(&mut bits)?;
                                    let left = full.decode(&mut bits)?;
                                    put(row, 2, right);
                                    put(row, 0, left);
                                }
                            }
                            1 => {
                                for row in [0, 2] {
                                    let right = full.decode(&mut bits)?;
                                    let left = full.decode(&mut bits)?;
                                    for r in [row, row + 1] {
                                        put(r, 2, right);
                                        put(r, 0, left);
                                    }
                                }
                            }
                            _ => {
                                for row in [0, 2] {
                                    let pair = full.decode(&mut bits)?;
                                    let (a, b) = (pair & 0xFF, pair >> 8);
                                    for r in [row, row + 1] {
                                        put(r, 0, a | a << 8);
                                        put(r, 2, b | b << 8);
                                    }
                                }
                            }
                        }
                    }
                    2 => {}
                    _ => {
                        for row in 0..4 {
                            buf[base + row * stride..base + row * stride + 4].fill(colour);
                        }
                    }
                }
                blk += 1;
            }
        }
        Ok(())
    }
}

/// Decodes one audio chunk (the bytes after its length word) to interleaved signed 16-bit samples.
fn decode_audio(track: &AudioTrack, chunk: &[u8]) -> Result<Vec<i16>> {
    if !track.packed {
        return Ok(if track.bits == 16 {
            chunk
                .chunks_exact(2)
                .map(|b| i16::from_le_bytes([b[0], b[1]]))
                .collect()
        } else {
            chunk.iter().map(|&b| ((b as i16) - 128) << 8).collect()
        });
    }
    let unpacked = rd32(chunk, 0)? as usize;
    let mut bits = Bits::new(&chunk[4..]);
    if bits.bit()? == 0 {
        return Ok(Vec::new());
    }
    let stereo = bits.bit()? != 0;
    let wide = bits.bit()? != 0;
    let channels = 1 + stereo as usize;
    let lanes = channels * (1 + wide as usize);
    let mut trees = Vec::with_capacity(lanes);
    for _ in 0..lanes {
        trees.push(Tree::read8(&mut bits)?);
    }
    let total = unpacked / (1 + wide as usize);
    if total > 1 << 28 {
        return Err(SmkError::Corrupt("audio too large"));
    }
    let mut pred = [0i32; 2];
    for ch in (0..channels).rev() {
        pred[ch] = if wide {
            let hi = bits.bits(8)? as i32;
            let lo = bits.bits(8)? as i32;
            (hi << 8 | lo) as i16 as i32
        } else {
            bits.bits(8)? as i32
        };
    }
    let conv = |v: i32| -> i16 { if wide { v as i16 } else { ((v as u8 as i16) - 128) << 8 } };
    let mut out = Vec::with_capacity(total);
    for &p in pred.iter().take(channels) {
        out.push(conv(p));
    }
    while out.len() < total {
        for ch in 0..channels {
            let delta = if wide {
                let lo = trees[ch * 2].decode8(&mut bits)? as u16;
                let hi = trees[ch * 2 + 1].decode8(&mut bits)? as u16;
                (lo | hi << 8) as i16 as i32
            } else {
                trees[ch].decode8(&mut bits)? as i8 as i32
            };
            pred[ch] = if wide {
                (pred[ch] + delta) as i16 as i32
            } else {
                (pred[ch] + delta) & 0xFF
            };
            out.push(conv(pred[ch]));
        }
    }
    out.truncate(total);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    struct W {
        bytes: Vec<u8>,
        n: usize,
    }

    impl W {
        fn new() -> Self {
            Self {
                bytes: Vec::new(),
                n: 0,
            }
        }
        fn bit(&mut self, b: u32) {
            if self.n.is_multiple_of(8) {
                self.bytes.push(0);
            }
            if b & 1 != 0 {
                *self.bytes.last_mut().unwrap() |= 1 << (self.n % 8);
            }
            self.n += 1;
        }
        fn put(&mut self, v: u32, n: u32) {
            for i in 0..n {
                self.bit(v >> i);
            }
        }
        fn code(&mut self, c: &[bool]) {
            for &b in c {
                self.bit(b as u32);
            }
        }
    }

    enum Shape {
        Leaf(u32),
        Node(Box<Shape>, Box<Shape>),
    }

    fn shape(vals: &[u32]) -> Shape {
        if vals.len() == 1 {
            Shape::Leaf(vals[0])
        } else {
            let (a, b) = vals.split_at(vals.len() / 2);
            Shape::Node(Box::new(shape(a)), Box::new(shape(b)))
        }
    }

    fn sorted(vals: impl IntoIterator<Item = u32>) -> Vec<u32> {
        let mut v: Vec<u32> = vals.into_iter().collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    fn codes(s: &Shape, prefix: &mut Vec<bool>, out: &mut BTreeMap<u32, Vec<bool>>) {
        match s {
            Shape::Leaf(v) => {
                out.insert(*v, prefix.clone());
            }
            Shape::Node(a, b) => {
                for (bit, child) in [(false, a), (true, b)] {
                    prefix.push(bit);
                    codes(child, prefix, out);
                    prefix.pop();
                }
            }
        }
    }

    fn code_map(s: &Shape) -> BTreeMap<u32, Vec<bool>> {
        let mut m = BTreeMap::new();
        codes(s, &mut Vec::new(), &mut m);
        m
    }

    /// Writes the tree structure with `leaf` called for each leaf in order.
    fn write_shape(w: &mut W, s: &Shape, leaf: &mut dyn FnMut(&mut W, u32)) {
        match s {
            Shape::Leaf(v) => {
                w.bit(0);
                leaf(w, *v);
            }
            Shape::Node(a, b) => {
                w.bit(1);
                write_shape(w, a, leaf);
                write_shape(w, b, leaf);
            }
        }
    }

    /// Writes a complete 8-bit tree (presence bit, tree, trailing bit); returns the code table.
    fn write_tree8(w: &mut W, vals: &[u32]) -> BTreeMap<u32, Vec<bool>> {
        let vals = sorted(vals.iter().copied());
        let s = shape(&vals);
        w.bit(1);
        write_shape(w, &s, &mut |w, v| w.put(v, 8));
        w.bit(0);
        code_map(&s)
    }

    struct BigCodes {
        main: BTreeMap<u32, Vec<bool>>,
    }

    /// Writes a complete big tree holding `vals` plus the escape values; returns its code table.
    fn write_big(w: &mut W, vals: &[u32], esc: [u32; 3]) -> BigCodes {
        let all = sorted(vals.iter().chain(esc.iter()).copied());
        w.bit(1);
        let lo = write_tree8(w, &all.iter().map(|v| v & 0xFF).collect::<Vec<_>>());
        let hi = write_tree8(w, &all.iter().map(|v| v >> 8).collect::<Vec<_>>());
        for e in esc {
            w.put(e, 16);
        }
        let s = shape(&all);
        write_shape(w, &s, &mut |w, v| {
            w.code(&lo[&(v & 0xFF)]);
            w.code(&hi[&(v >> 8)]);
        });
        w.bit(0);
        BigCodes { main: code_map(&s) }
    }

    const ESC: [u32; 3] = [0xFFFF, 0xFFFE, 0xFFFD];

    #[test]
    fn big_tree_round_trip_with_cache() {
        let vals = [0x0102, 0x0304, 0x0506, 0x0700];
        let mut w = W::new();
        let c = write_big(&mut w, &vals, ESC);
        let seq: [u32; 6] = [0x0102, 0x0304, ESC[1], ESC[0], 0x0506, ESC[2]];
        for s in seq {
            w.code(&c.main[&s]);
        }
        let mut bits = Bits::new(&w.bytes);
        let mut t = BigTree::read(&mut bits).unwrap();
        let got: Vec<u16> = (0..6).map(|_| t.decode(&mut bits).unwrap()).collect();
        // After A, B the cache is [B, A, 0]; esc1 -> A gives [A, B, 0]; esc0 -> A; 0x0506 gives [0x0506, A, B];
        // esc2 -> B.
        assert_eq!(got, vec![0x0102, 0x0304, 0x0102, 0x0102, 0x0506, 0x0304]);
    }

    #[test]
    fn tree8_round_trip_and_single_leaf() {
        let mut w = W::new();
        let c = write_tree8(&mut w, &[3, 9, 200, 17, 17]);
        for v in [200, 3, 17, 9, 9] {
            w.code(&c[&v]);
        }
        let mut b = Bits::new(&w.bytes);
        let t = Tree::read8(&mut b).unwrap();
        let got: Vec<u8> = (0..5).map(|_| t.decode8(&mut b).unwrap()).collect();
        assert_eq!(got, vec![200, 3, 17, 9, 9]);

        let mut w = W::new();
        write_tree8(&mut w, &[42]);
        let mut b = Bits::new(&w.bytes);
        let t = Tree::read8(&mut b).unwrap();
        assert_eq!(t.decode8(&mut b).unwrap(), 42);
        assert_eq!(t.decode8(&mut b).unwrap(), 42);
    }

    #[test]
    fn palette_scaling() {
        assert_eq!(scale6(0), 0);
        assert_eq!(scale6(1), 4);
        assert_eq!(scale6(16), 0x41);
        assert_eq!(scale6(32), 0x82);
        assert_eq!(scale6(63), 255);
    }

    fn type_code(kind: u32, run_index: u32, colour: u32) -> u32 {
        kind | run_index << 2 | colour << 8
    }

    const MASK: u32 = 0b1010_0101_0000_1111;

    /// Two frames of an 8x4 (or 7x4) picture: solid + mono, then void + solid, each with a palette chunk.
    fn synthetic(width: u32) -> Vec<u8> {
        let mono_pair = 0x0201u32; // colour 1 where the mask bit is set, 2 where it is clear
        let t_solid1 = type_code(3, 0, 1);
        let t_mono = type_code(0, 0, 0);
        let t_void = type_code(2, 0, 0);
        let t_solid0 = type_code(3, 0, 0);
        let mut tw = W::new();
        let mmap = write_big(&mut tw, &[MASK, 0], ESC);
        let mclr = write_big(&mut tw, &[mono_pair, 0], ESC);
        write_big(&mut tw, &[0x1122, 0x3344], ESC);
        let kind = write_big(&mut tw, &[t_solid1, t_mono, t_void, t_solid0], ESC);
        let trees = tw.bytes;

        let mut f0 = W::new();
        f0.code(&kind.main[&t_solid1]);
        f0.code(&kind.main[&t_mono]);
        f0.code(&mclr.main[&mono_pair]);
        f0.code(&mmap.main[&MASK]);
        let mut f1 = W::new();
        f1.code(&kind.main[&t_void]);
        f1.code(&kind.main[&t_solid0]);

        // Palette chunk 0: entry 0 = (63, 0, 0), entry 1 = (0, 63, 0), entry 2 = (0, 0, 32), keep the other 253.
        let mut p0 = vec![0u8, 63, 0, 0, 0, 63, 0, 0, 0, 32, 0x80 | 127, 0x80 | 124];
        // Palette chunk 1: copy entry 1 over entry 0, keep the other 255.
        let mut p1 = vec![0u8, 0x40, 1, 0x80 | 127, 0x80 | 125];
        for p in [&mut p0, &mut p1] {
            while p.len() % 4 != 0 {
                p.push(0);
            }
            p[0] = (p.len() / 4) as u8;
        }
        let mut frames = Vec::new();
        for (p, v) in [(p0, f0.bytes), (p1, f1.bytes)] {
            let mut d = p;
            d.extend(v);
            while d.len() % 4 != 0 {
                d.push(0);
            }
            frames.push(d);
        }

        let mut out = b"SMK2".to_vec();
        for v in [width, 4, 2, 100u32, 0] {
            out.extend(v.to_le_bytes());
        }
        out.extend([0u8; 28]);
        out.extend((trees.len() as u32).to_le_bytes());
        out.extend([0u8; 16]);
        out.extend([0u8; 28]);
        out.extend([0u8; 4]);
        for (i, f) in frames.iter().enumerate() {
            out.extend((f.len() as u32 | (i == 0) as u32).to_le_bytes());
        }
        out.extend([1u8, 1u8]);
        out.extend(trees);
        for f in frames {
            out.extend(f);
        }
        out
    }

    #[test]
    fn decodes_synthetic_stream() {
        let mut s = Smacker::parse(&synthetic(8)).unwrap();
        assert_eq!((s.width, s.height, s.frame_count()), (8, 4, 2));
        assert!((s.fps() - 10.0).abs() < 1e-9);
        assert_eq!(s.y_scale(), YScale::None);
        let f0 = s.next_frame().unwrap().unwrap();
        assert!(f0.keyframe);
        assert_eq!(f0.palette[0], [255, 0, 0]);
        assert_eq!(f0.palette[1], [0, 255, 0]);
        assert_eq!(f0.palette[2], [0, 0, scale6(32)]);
        // Block 0 solid 1; block 1 mono: mask bit set -> colour 1, clear -> colour 2.
        for y in 0..4 {
            for x in 0..4 {
                assert_eq!(f0.pixels[y * 8 + x], 1);
                let want = if MASK >> (y * 4 + x) & 1 != 0 { 1 } else { 2 };
                assert_eq!(f0.pixels[y * 8 + 4 + x], want, "({x},{y})");
            }
        }
        let f1 = s.next_frame().unwrap().unwrap();
        assert!(!f1.keyframe);
        assert_eq!(f1.palette[0], [0, 255, 0]);
        assert_eq!(f1.palette[2], [0, 0, scale6(32)]);
        // Block 0 void keeps solid 1; block 1 now solid 0.
        for y in 0..4 {
            assert_eq!(&f1.pixels[y * 8..y * 8 + 8], &[1, 1, 1, 1, 0, 0, 0, 0]);
        }
        assert!(s.next_frame().unwrap().is_none());
        s.rewind();
        assert_eq!(s.next_frame().unwrap().unwrap(), f0);
    }

    #[test]
    fn crops_non_multiple_of_four_width() {
        let mut s = Smacker::parse(&synthetic(7)).unwrap();
        let f = s.next_frame().unwrap().unwrap();
        assert_eq!(f.pixels.len(), 7 * 4);
        assert_eq!(&f.pixels[0..7], &[1, 1, 1, 1, 1, 1, 1]);
        assert_eq!(&f.pixels[21..28], &[1, 1, 1, 1, 2, 1, 2]);
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(Smacker::parse(b"nope").err(), Some(SmkError::NotSmacker));
        assert_eq!(Smacker::parse(b"SMK2").err(), Some(SmkError::Truncated));
        let d = synthetic(8);
        assert!(Smacker::parse(&d[..d.len() - 8]).is_err());
    }

    #[test]
    fn frame_rate_forms() {
        let mut d = synthetic(8);
        d[16..20].copy_from_slice(&40u32.to_le_bytes());
        assert!((Smacker::parse(&d).unwrap().fps() - 25.0).abs() < 1e-9);
        d[16..20].copy_from_slice(&(-8333i32).to_le_bytes());
        assert!((Smacker::parse(&d).unwrap().fps() - 12.0005).abs() < 1e-3);
    }

    #[test]
    fn run_table() {
        assert_eq!((RUNS[0], RUNS[58], RUNS[59], RUNS[63]), (1, 59, 128, 2048));
    }

    fn track(bits: u8, channels: u8, packed: bool) -> AudioTrack {
        AudioTrack {
            track: 0,
            rate: 22050,
            channels,
            bits,
            packed,
            unsupported_codec: false,
            max_bytes: 0,
        }
    }

    #[test]
    fn raw_audio() {
        let a = decode_audio(&track(8, 1, false), &[128, 129, 0]).unwrap();
        assert_eq!(a, vec![0, 256, -32768]);
        let a = decode_audio(&track(16, 1, false), &[0x34, 0x12, 0xFF, 0xFF]).unwrap();
        assert_eq!(a, vec![0x1234, -1]);
    }

    #[test]
    fn packed_audio_8bit_mono() {
        let deltas: [i8; 4] = [1, -3, 5, 0];
        let mut w = W::new();
        w.put(5, 32); // unpacked size: initial value plus four deltas
        w.bit(1);
        w.bit(0);
        w.bit(0);
        let c = write_tree8(&mut w, &[1, 0xFD, 5, 0]);
        w.put(130, 8);
        for d in deltas {
            w.code(&c[&(d as u8 as u32)]);
        }
        let a = decode_audio(&track(8, 1, true), &w.bytes).unwrap();
        let want: Vec<i16> = [130i32, 131, 128, 133, 133]
            .iter()
            .map(|&v| ((v as i16) - 128) << 8)
            .collect();
        assert_eq!(a, want);
    }

    #[test]
    fn packed_audio_16bit_stereo() {
        let left: [i16; 4] = [1000, 1100, 900, -32768];
        let right: [i16; 4] = [-5, 7, -300, 20];
        let delta = |s: &[i16; 4], i: usize| s[i].wrapping_sub(s[i - 1]) as u16 as u32;
        let mut w = W::new();
        w.put(4 * 2 * 2, 32);
        w.bit(1);
        w.bit(1);
        w.bit(1);
        let mut lanes = Vec::new();
        for s in [&left, &right] {
            let lo: Vec<u32> = (1..4).map(|i| delta(s, i) & 0xFF).collect();
            let hi: Vec<u32> = (1..4).map(|i| delta(s, i) >> 8).collect();
            lanes.push(write_tree8(&mut w, &lo));
            lanes.push(write_tree8(&mut w, &hi));
        }
        // Starting values, right channel first, high byte first.
        for v in [right[0], left[0]] {
            w.put((v as u16 >> 8) as u32, 8);
            w.put((v as u16 & 0xFF) as u32, 8);
        }
        for i in 1..4 {
            for (ch, s) in [&left, &right].into_iter().enumerate() {
                let d = delta(s, i);
                w.code(&lanes[ch * 2][&(d & 0xFF)]);
                w.code(&lanes[ch * 2 + 1][&(d >> 8)]);
            }
        }
        let a = decode_audio(&track(16, 2, true), &w.bytes).unwrap();
        let want: Vec<i16> = (0..4).flat_map(|i| [left[i], right[i]]).collect();
        assert_eq!(a, want);
    }

    #[test]
    fn smk4_full_block_modes() {
        let full_vals = [0x0201u32, 0x0403, 0x0605, 0x0807];
        let kind_full = type_code(1, 0, 0);
        let mut tw = W::new();
        write_big(&mut tw, &[0], ESC);
        write_big(&mut tw, &[0], ESC);
        let full = write_big(&mut tw, &full_vals, ESC);
        let kind = write_big(&mut tw, &[kind_full], ESC);
        let mut d = synthetic(8);
        d[3] = b'4';
        let mut s = Smacker::parse(&d).unwrap();
        let mut b = Bits::new(&tw.bytes);
        s.trees = [
            BigTree::read(&mut b).unwrap(),
            BigTree::read(&mut b).unwrap(),
            BigTree::read(&mut b).unwrap(),
            BigTree::read(&mut b).unwrap(),
        ];
        // An 8x4 picture is two blocks: plain mode, then repeated-row mode. Each type code covers one block.
        let mut w = W::new();
        w.code(&kind.main[&kind_full]);
        w.bit(0);
        w.bit(0);
        for v in [0x0201, 0x0403, 0x0201, 0x0403, 0x0201, 0x0403, 0x0201, 0x0403] {
            w.code(&full.main[&v]);
        }
        w.code(&kind.main[&kind_full]);
        w.bit(1);
        for v in [0x0605, 0x0807, 0x0201, 0x0403] {
            w.code(&full.main[&v]);
        }
        s.decode_video(&w.bytes).unwrap();
        let row = |y: usize| s.buf[y * 8..y * 8 + 8].to_vec();
        // Plain: the right pair is read first, so pixels 2,3 = 1,2 and pixels 0,1 = 3,4.
        assert_eq!(&row(0)[0..4], &[3, 4, 1, 2]);
        // Repeated rows come in identical pairs.
        assert_eq!(&row(0)[4..8], &[7, 8, 5, 6]);
        assert_eq!(&row(1)[4..8], &[7, 8, 5, 6]);
        assert_eq!(&row(2)[4..8], &[3, 4, 1, 2]);
        assert_eq!(&row(3)[4..8], &[3, 4, 1, 2]);
    }
}
