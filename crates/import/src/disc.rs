//! Reads CD images: a small, read-only ISO 9660 reader with Joliet names, and the sector framings the common image
//! files use. [`DiscFiles`] is the disc as a [`SourceFiles`], so `validate`, `manifest` and `copy` work on it
//! unchanged (`docs/design/import.md`, phase I3b).
//!
//! Public format sources:
//!
//! - **ISO 9660**: ECMA-119 (the volume descriptor set from logical sector 16, the `CD001` identifier, the primary
//!   volume descriptor's root directory record at offset 156, directory records with both-endian numbers, the
//!   multi-extent flag, `;1` version suffixes on level 1 and 2 names).
//! - **Joliet**: Microsoft's Joliet specification (a supplementary volume descriptor, type 2, whose escape sequences
//!   `%/@`, `%/C` or `%/E` mark UCS-2 big-endian names, with its own directory tree beside the ISO one).
//! - **CUE sheets**: the CDRWIN cue sheet format (`FILE`, `TRACK nn MODE1/2352`, `INDEX 01 mm:ss:ff`, 75 frames per
//!   second).
//! - **Raw sectors**: a raw CD sector is 2352 bytes: 12 bytes of sync, a 4-byte header, then for mode 1 the 2048 user
//!   bytes (data at offset 16), for mode 2 form 1 an 8-byte subheader before them (data at offset 24); 2336-byte
//!   sectors omit sync and header (data at offset 8); 2448-byte sectors carry 96 bytes of sub-channel data after the
//!   2352 (ECMA-130 and the Yellow Book / Orange Book descriptions of sector formats).
//! - **MDF/MDS** (Alcohol 120%): the `.mdf` file is a plain dump of the sectors (2352 bytes, or 2448 with sub-channel
//!   data); the `.mds` file only describes them. We never need it, because the layout is found from the signature.
//! - **NRG** (Nero): the sectors are stored as they are and a footer describes them. The last 8 bytes are `NERO` and
//!   a 32-bit big-endian offset of the chunk table (version 1), or the last 12 are `NER5` and a 64-bit offset
//!   (version 2). The table is a list of `id(4) length(4, big-endian) data` chunks ending in `END!`; the `ETNF`
//!   (version 1, 20-byte entries) and `ETN2` (version 2, 32-byte entries) chunks list the tracks, each entry starting
//!   with the track's byte offset in the file.
//!
//! The layout (where the sectors start, how far apart they are, where the 2048 data bytes sit in each) is always
//! decided by finding `CD001` in sector 16, never by the file's extension. HFS on hybrid discs is ignored. Files are
//! read by extent straight from the image, so the image is never loaded into memory.
//!
//! A damaged image gives an error, never a panic or a hang: reads past the end of the file are errors, a directory
//! seen twice is a loop, and depth, entry counts and directory sizes are capped.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::ImportError;
use crate::copy::{Progress, Step};
use crate::inno::Staging;
use crate::source::{Source, SourceFile, SourceFiles, SourceKind};

/// The user-data size of a sector.
const SECTOR: u64 = 2048;
/// Sector sizes tried when finding the layout, in the order tried.
const STRIDES: [u32; 4] = [2048, 2352, 2448, 2336];
/// Directory depth, entries and one directory's size are capped so a damaged image ends quickly.
const MAX_DEPTH: usize = 32;
const MAX_ENTRIES: usize = 200_000;
const MAX_DIR_BYTES: u64 = 16 * 1024 * 1024;
/// How deep below the disc's root the game's archives are looked for (as for a folder).
const GAME_DEPTH: usize = 6;
/// Sectors read at once when streaming a file.
const CHUNK_SECTORS: u64 = 32;
/// The buffer staging copies through.
const STAGE_CHUNK: usize = 1 << 20;

fn bad(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

/// Where the sectors are in the image file and where the 2048 data bytes sit in each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    /// Byte offset of the first sector of the data track.
    pub base: u64,
    /// Bytes from one sector to the next: 2048, 2336, 2352 or 2448.
    pub stride: u32,
    /// Offset of the 2048 data bytes within a sector: 0, 8, 16 or 24.
    pub data_offset: u32,
}

impl Layout {
    /// A phrase for `twtool disc-list`.
    pub fn describe(&self) -> String {
        let kind = match (self.stride, self.data_offset) {
            (2048, _) => "2048-byte sectors".to_string(),
            (s, 16) => format!("raw {s}-byte sectors, mode 1"),
            (s, 24) => format!("raw {s}-byte sectors, mode 2 form 1"),
            (s, o) => format!("{s}-byte sectors, data at offset {o}"),
        };
        format!("{kind}, track starts at byte {}", self.base)
    }

    fn data_offsets(stride: u32) -> &'static [u32] {
        match stride {
            2048 => &[0],
            2336 => &[8],
            _ => &[16, 24],
        }
    }

    /// The byte position of the user data of logical sector `lba`.
    fn pos(&self, lba: u64) -> io::Result<u64> {
        lba.checked_mul(u64::from(self.stride))
            .and_then(|p| p.checked_add(self.base + u64::from(self.data_offset)))
            .ok_or_else(|| bad("a sector number is out of range"))
    }
}

/// Reads exactly `buf.len()` bytes at `pos`, or says the image is cut short.
fn read_at(f: &mut File, pos: u64, buf: &mut [u8]) -> io::Result<()> {
    f.seek(SeekFrom::Start(pos))?;
    f.read_exact(buf).map_err(|e| {
        if e.kind() == io::ErrorKind::UnexpectedEof {
            io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the image file ends early (is it truncated or only partly downloaded?)",
            )
        } else {
            e
        }
    })
}

/// Finds the layout at `base` by looking for a volume descriptor (`CD001`, version 1) in sector 16.
fn probe(f: &mut File, base: u64, strides: &[u32]) -> Option<Layout> {
    for &stride in strides {
        for &data_offset in Layout::data_offsets(stride) {
            let layout = Layout {
                base,
                stride,
                data_offset,
            };
            let mut b = [0u8; 7];
            let found = layout
                .pos(16)
                .ok()
                .is_some_and(|p| read_at(f, p, &mut b).is_ok() && &b[1..6] == b"CD001" && b[6] == 1);
            if found && matches!(b[0], 0..=3 | 255) {
                return Some(layout);
            }
        }
    }
    None
}

/// A place a track may start: an image file, the byte offsets to try and the sector size the sheet declares.
struct Candidate {
    path: PathBuf,
    bases: Vec<u64>,
    stride: Option<u32>,
}

/// One file on a disc (its path with `/` separators, and its size).
#[derive(Clone, Debug)]
pub struct DiscFile {
    pub path: String,
    pub size: u64,
    /// (first sector, byte length) of each extent.
    extents: Vec<(u64, u64)>,
}

/// A disc image: its layout and every file on it.
#[derive(Debug)]
pub struct Disc {
    image: PathBuf,
    layout: Layout,
    joliet: bool,
    files: Vec<DiscFile>,
}

impl Disc {
    /// Opens the image at `path` (`.iso`, `.img`, `.bin`, a `.cue` sheet, `.mdf` or `.mds`, `.nrg`; anything with
    /// `CD001` in sector 16) and reads its directory tree.
    pub fn open(path: &Path) -> io::Result<Self> {
        let (image, layout) = find_layout(path)?;
        let mut f = File::open(&image)?;
        let (joliet, root) = volume_root(&mut f, &layout)?;
        let mut files = Vec::new();
        let mut seen = HashSet::new();
        walk(&mut f, &layout, joliet, root, "", 0, &mut seen, &mut files)?;
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(Self {
            image,
            layout,
            joliet,
            files,
        })
    }

    pub fn layout(&self) -> Layout {
        self.layout
    }

    /// Whether the names came from the Joliet tree (else the ISO 9660 one).
    pub fn is_joliet(&self) -> bool {
        self.joliet
    }

    /// Every file on the disc, sorted by path.
    pub fn files(&self) -> &[DiscFile] {
        &self.files
    }

    /// Streams a file's bytes from the image.
    pub fn read_file(&self, file: &DiscFile) -> io::Result<ExtentReader> {
        Ok(ExtentReader {
            f: File::open(&self.image)?,
            layout: self.layout,
            extents: file.extents.clone(),
            size: file.size,
            pos: 0,
            next: 0,
            lba: 0,
            left: 0,
            skip: 0,
            buf: Vec::new(),
            buf_pos: 0,
        })
    }
}

/// Whether `path` is a disc image: a cue sheet, an `.mdf`/`.mds`/`.nrg`/`.img`, or any file with `CD001` in sector 16.
pub fn looks_like_disc(path: &Path) -> bool {
    matches!(ext_of(path).as_str(), "cue" | "mdf" | "mds" | "nrg" | "img") || find_layout(path).is_ok()
}

fn ext_of(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// The image file to read and the layout of its first data track.
fn find_layout(path: &Path) -> io::Result<(PathBuf, Layout)> {
    let mut path = path.to_path_buf();
    let mut candidates = Vec::new();
    match ext_of(&path).as_str() {
        "cue" => {
            candidates = parse_cue(&path)?;
            if candidates.is_empty() {
                return Err(bad(
                    "the cue sheet lists no data track (it has only audio tracks, or no tracks at all)",
                ));
            }
        }
        ext => {
            if ext == "mds" {
                // The descriptor of an .mdf: the sectors are in the file beside it.
                path = sibling_ci(&path.with_extension("mdf"))
                    .ok_or_else(|| bad("this .mds file has no .mdf file beside it"))?;
            }
            if ext == "bin"
                && let Some(cue) = sibling_ci(&path.with_extension("cue"))
            {
                // A .bin with its cue sheet beside it: the sheet says where the data track starts and how its
                // sectors are framed. A sheet that can't be read or doesn't name this file is not an error here: the
                // signature search below still finds a plain disc.
                if let Ok(found) = parse_cue(&cue) {
                    let me = std::fs::canonicalize(&path).ok();
                    candidates.extend(
                        found
                            .into_iter()
                            .filter(|c| me.is_some() && std::fs::canonicalize(&c.path).ok() == me),
                    );
                }
            }
            let mut bases = nrg_tracks(&path)?;
            bases.push(0);
            candidates.push(Candidate {
                path: path.clone(),
                bases,
                stride: None,
            });
        }
    }
    for c in &candidates {
        let mut f = File::open(&c.path)?;
        // The sheet's declared sector size is tried first, then the others.
        let strides: Vec<u32> = c
            .stride
            .into_iter()
            .chain(STRIDES.iter().copied().filter(|s| Some(*s) != c.stride))
            .collect();
        for &base in &c.bases {
            if let Some(layout) = probe(&mut f, base, &strides) {
                return Ok((c.path.clone(), layout));
            }
        }
    }
    Err(bad(
        "no ISO 9660 disc was found in this image (no CD001 signature in sector 16)",
    ))
}

/// `path`, or a file of the same name in another letter case in the same folder.
fn sibling_ci(path: &Path) -> Option<PathBuf> {
    if path.is_file() {
        return Some(path.to_path_buf());
    }
    let want = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::read_dir(dir).ok()?.filter_map(Result::ok).find_map(|e| {
        (e.file_name().to_string_lossy().to_ascii_lowercase() == want && e.path().is_file()).then(|| e.path())
    })
}

/// The data tracks a cue sheet lists, in order.
fn parse_cue(cue: &Path) -> io::Result<Vec<Candidate>> {
    let text = std::fs::read(cue)?;
    let text = String::from_utf8_lossy(&text);
    let dir = cue
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut out = Vec::new();
    let mut file: Option<PathBuf> = None;
    // The track being described: its sector size (0 for audio and anything else), INDEX 00 and INDEX 01 offsets.
    let (mut stride, mut idx0, mut idx1) = (0u32, None::<u64>, None::<u64>);
    fn flush(out: &mut Vec<Candidate>, file: &Option<PathBuf>, stride: u32, idx0: Option<u64>, idx1: Option<u64>) {
        if let Some(path) = file
            && stride != 0
        {
            let bases: Vec<u64> = idx1.into_iter().chain(idx0).collect();
            if !bases.is_empty() {
                out.push(Candidate {
                    path: path.clone(),
                    bases,
                    stride: Some(stride),
                });
            }
        }
    }
    for line in text.lines() {
        let line = line.trim().trim_start_matches('\u{feff}');
        let (word, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let rest = rest.trim();
        match word.to_ascii_uppercase().as_str() {
            "FILE" => {
                flush(&mut out, &file, stride, idx0, idx1);
                (stride, idx0, idx1) = (0, None, None);
                let name = match rest.strip_prefix('"') {
                    Some(r) => r.split('"').next().unwrap_or(""),
                    None => rest.split_whitespace().next().unwrap_or(""),
                }
                .replace('\\', "/");
                let leaf = Path::new(&name).file_name().unwrap_or_default().to_owned();
                file = Some(
                    sibling_ci(&dir.join(&name))
                        .or_else(|| sibling_ci(&dir.join(leaf)))
                        .ok_or_else(|| {
                            bad(format!(
                                "the cue sheet names the file {name}, which isn't in the same folder as the sheet"
                            ))
                        })?,
                );
            }
            "TRACK" => {
                flush(&mut out, &file, stride, idx0, idx1);
                (idx0, idx1) = (None, None);
                stride = match rest
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("")
                    .to_ascii_uppercase()
                    .as_str()
                {
                    "MODE1/2048" => 2048,
                    "MODE1/2352" | "MODE2/2352" => 2352,
                    "MODE2/2336" => 2336,
                    _ => 0,
                };
            }
            "INDEX" => {
                let mut it = rest.split_whitespace();
                let which = it.next().unwrap_or("");
                let off = it
                    .next()
                    .and_then(msf_to_frames)
                    .and_then(|fr| fr.checked_mul(u64::from(stride.max(1))));
                match which {
                    "00" => idx0 = off,
                    "01" => idx1 = off,
                    _ => {}
                }
            }
            _ => {}
        }
    }
    flush(&mut out, &file, stride, idx0, idx1);
    Ok(out)
}

fn msf_to_frames(s: &str) -> Option<u64> {
    let mut p = s.split(':').map(|x| x.parse::<u64>().ok());
    let (m, sec, f) = (p.next()??, p.next()??, p.next()??);
    m.checked_mul(60)?.checked_add(sec)?.checked_mul(75)?.checked_add(f)
}

/// The start offsets of the tracks in a Nero image's footer (empty when the file has none).
fn nrg_tracks(path: &Path) -> io::Result<Vec<u64>> {
    let mut f = File::open(path)?;
    let len = f.metadata()?.len();
    if len < 12 {
        return Ok(Vec::new());
    }
    let mut tail = [0u8; 12];
    read_at(&mut f, len - 12, &mut tail)?;
    let be64 = |b: &[u8]| u64::from_be_bytes(b.try_into().unwrap_or([0; 8]));
    let be32 = |b: &[u8]| u32::from_be_bytes(b.try_into().unwrap_or([0; 4]));
    let (table, trailer) = if &tail[0..4] == b"NER5" {
        (be64(&tail[4..12]), 12)
    } else if &tail[4..8] == b"NERO" {
        (u64::from(be32(&tail[8..12])), 8)
    } else {
        return Ok(Vec::new());
    };
    let end = len - trailer;
    if table >= end || end - table > 1 << 20 {
        return Ok(Vec::new());
    }
    let mut chunks = vec![0u8; (end - table) as usize];
    read_at(&mut f, table, &mut chunks)?;
    let mut bases = Vec::new();
    let mut p = 0usize;
    while let Some(head) = chunks.get(p..p + 8) {
        let n = be32(&head[4..8]) as usize;
        let Some(body) = chunks.get(p + 8..p + 8 + n) else {
            break;
        };
        match &head[0..4] {
            b"ETNF" => bases.extend(body.chunks_exact(20).map(|e| u64::from(be32(&e[0..4])))),
            b"ETN2" => bases.extend(body.chunks_exact(32).map(|e| be64(&e[0..8]))),
            b"END!" => break,
            _ => {}
        }
        p += 8 + n;
    }
    Ok(bases)
}

/// One directory record's fields that we use.
struct Record {
    lba: u64,
    len: u64,
    dir: bool,
    more: bool,
    name: String,
}

/// Where the volume descriptors say the root directory is, preferring Joliet: (is Joliet, (first sector, length)).
fn volume_root(f: &mut File, layout: &Layout) -> io::Result<(bool, (u64, u64))> {
    let (mut primary, mut joliet) = (None, None);
    let mut sector = [0u8; SECTOR as usize];
    for lba in 16..16 + 64 {
        read_at(f, layout.pos(lba)?, &mut sector)?;
        if &sector[1..6] != b"CD001" {
            break;
        }
        match sector[0] {
            1 if primary.is_none() => primary = Some(root_of(&sector)),
            2 if joliet.is_none() && matches!(&sector[88..91], b"%/@" | b"%/C" | b"%/E") => {
                joliet = Some(root_of(&sector));
            }
            255 => break,
            _ => {}
        }
    }
    match (joliet, primary) {
        (Some(r), _) => Ok((true, r)),
        (None, Some(r)) => Ok((false, r)),
        _ => Err(bad("the image has no primary volume descriptor")),
    }
}

fn root_of(sector: &[u8]) -> (u64, u64) {
    (u64::from(le32(&sector[158..162])), u64::from(le32(&sector[166..170])))
}

fn le32(b: &[u8]) -> u32 {
    u32::from_le_bytes(b[..4].try_into().unwrap_or([0; 4]))
}

/// Reads a whole directory extent.
fn read_dir_bytes(f: &mut File, layout: &Layout, lba: u64, len: u64) -> io::Result<Vec<u8>> {
    if len > MAX_DIR_BYTES {
        return Err(bad("a directory claims an impossible size"));
    }
    let mut out = Vec::with_capacity(len.div_ceil(SECTOR) as usize * SECTOR as usize);
    let mut s = [0u8; SECTOR as usize];
    for i in 0..len.div_ceil(SECTOR) {
        let at = lba
            .checked_add(i)
            .ok_or_else(|| bad("a sector number is out of range"))?;
        read_at(f, layout.pos(at)?, &mut s)?;
        out.extend_from_slice(&s);
    }
    out.truncate(len as usize);
    Ok(out)
}

fn parse_records(data: &[u8], joliet: bool) -> io::Result<Vec<Record>> {
    let mut out = Vec::new();
    let mut p = 0usize;
    while p < data.len() {
        let n = usize::from(data[p]);
        if n == 0 {
            // Records never cross a sector boundary; a zero length means "continue in the next sector".
            p = (p / SECTOR as usize + 1) * SECTOR as usize;
            continue;
        }
        let rec = data
            .get(p..p + n)
            .filter(|r| r.len() >= 34 && 33 + usize::from(r[32]) <= r.len())
            .ok_or_else(|| bad("a directory record is damaged"))?;
        p += n;
        let name = &rec[33..33 + usize::from(rec[32])];
        if name == [0] || name == [1] {
            continue; // "." and ".."
        }
        out.push(Record {
            lba: u64::from(le32(&rec[2..6])),
            len: u64::from(le32(&rec[10..14])),
            dir: rec[25] & 2 != 0,
            more: rec[25] & 0x80 != 0,
            name: decode_name(name, joliet),
        });
    }
    Ok(out)
}

/// A name from a record: UCS-2 big-endian for Joliet, else Latin-1; the `;1` version of a file (and a trailing dot
/// on ISO 9660 names) is removed. Slashes can't be in a name and become `_`.
fn decode_name(raw: &[u8], joliet: bool) -> String {
    let mut s: String = if joliet {
        char::decode_utf16(raw.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])))
            .map(|r| r.unwrap_or('\u{fffd}'))
            .collect()
    } else {
        raw.iter().map(|&b| char::from(b)).collect()
    };
    if let Some((base, ver)) = s.rsplit_once(';')
        && !ver.is_empty()
        && ver.bytes().all(|b| b.is_ascii_digit())
    {
        s = base.to_string();
    }
    if !joliet && s.len() > 1 && s.ends_with('.') {
        s.pop();
    }
    s.replace(['/', '\\'], "_")
}

#[allow(clippy::too_many_arguments)]
fn walk(
    f: &mut File,
    layout: &Layout,
    joliet: bool,
    dir: (u64, u64),
    prefix: &str,
    depth: usize,
    seen: &mut HashSet<u64>,
    out: &mut Vec<DiscFile>,
) -> io::Result<()> {
    if depth > MAX_DEPTH {
        return Err(bad("the directories are nested too deeply (a loop?)"));
    }
    if !seen.insert(dir.0) {
        return Err(bad("the directory tree loops back on itself"));
    }
    let records = parse_records(&read_dir_bytes(f, layout, dir.0, dir.1)?, joliet)?;
    let mut i = 0;
    while i < records.len() {
        let r = &records[i];
        i += 1;
        let path = if prefix.is_empty() {
            r.name.clone()
        } else {
            format!("{prefix}/{}", r.name)
        };
        if r.dir {
            walk(f, layout, joliet, (r.lba, r.len), &path, depth + 1, seen, out)?;
            continue;
        }
        // A file larger than one extent continues in the following records with the same name.
        let mut extents = vec![(r.lba, r.len)];
        let mut more = r.more;
        while more && i < records.len() && records[i].name == r.name && !records[i].dir {
            extents.push((records[i].lba, records[i].len));
            more = records[i].more;
            i += 1;
        }
        if out.len() >= MAX_ENTRIES {
            return Err(bad("the disc lists too many files"));
        }
        let size = extents
            .iter()
            .try_fold(0u64, |a, e| a.checked_add(e.1))
            .ok_or_else(|| bad("a file is impossibly large"))?;
        out.push(DiscFile { path, size, extents });
    }
    Ok(())
}

/// Reads one file's bytes from the image, a few sectors at a time. It also seeks, so an archive kept inside a file on
/// the disc can be read in place.
pub struct ExtentReader {
    f: File,
    layout: Layout,
    extents: Vec<(u64, u64)>,
    size: u64,
    /// The position the next read returns.
    pos: u64,
    next: usize,
    lba: u64,
    left: u64,
    /// Bytes to drop from the front of the next chunk read (after a seek into the middle of a sector).
    skip: usize,
    buf: Vec<u8>,
    buf_pos: usize,
}

impl ExtentReader {
    /// Reads the next chunk into `buf`; false at the end of the file.
    fn fill(&mut self) -> io::Result<bool> {
        while self.left == 0 {
            let Some(&(lba, len)) = self.extents.get(self.next) else {
                return Ok(false);
            };
            self.next += 1;
            (self.lba, self.left) = (lba, len);
        }
        let sectors = self.left.div_ceil(SECTOR).min(CHUNK_SECTORS);
        let stride = u64::from(self.layout.stride);
        let mut raw = vec![0u8; ((sectors - 1) * stride + SECTOR) as usize];
        read_at(&mut self.f, self.layout.pos(self.lba)?, &mut raw)?;
        self.buf.clear();
        for i in 0..sectors {
            let start = (i * stride) as usize;
            self.buf.extend_from_slice(&raw[start..start + SECTOR as usize]);
        }
        let take = (sectors * SECTOR).min(self.left);
        self.buf.truncate(take as usize);
        self.left -= take;
        self.lba += sectors;
        self.buf_pos = self.skip.min(self.buf.len());
        self.skip = 0;
        Ok(true)
    }
}

impl Read for ExtentReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        while self.buf_pos >= self.buf.len() {
            if !self.fill()? {
                return Ok(0);
            }
        }
        let n = out.len().min(self.buf.len() - self.buf_pos);
        out[..n].copy_from_slice(&self.buf[self.buf_pos..self.buf_pos + n]);
        self.buf_pos += n;
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for ExtentReader {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let target = match to {
            SeekFrom::Start(p) => Some(p),
            SeekFrom::End(d) => self.size.checked_add_signed(d),
            SeekFrom::Current(d) => self.pos.checked_add_signed(d),
        }
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek before the start of the file"))?;
        self.pos = target;
        self.buf.clear();
        self.buf_pos = 0;
        self.skip = 0;
        self.left = 0;
        // Find the extent holding `target`; a position at or past the end reads nothing.
        let mut start = 0u64;
        self.next = self.extents.len();
        for (i, &(lba, len)) in self.extents.iter().enumerate() {
            if target < start + len {
                let into = target - start;
                let whole = into / SECTOR;
                self.next = i + 1;
                self.lba = lba + whole;
                self.left = len - whole * SECTOR;
                self.skip = (into - whole * SECTOR) as usize;
                break;
            }
            start += len;
        }
        Ok(target)
    }
}

/// The game files on a disc as a [`SourceFiles`]: the archives in the shallowest folder that has them, and its `Maps`.
pub struct DiscFiles {
    disc: Disc,
    /// The folder on the disc the game is in ("" for the root), without a trailing slash.
    root: String,
    /// Relative path (lower case) to where its bytes are.
    index: HashMap<String, Entry>,
    listing: Vec<SourceFile>,
    /// The installer archive the files come from, when the disc keeps them in one.
    installer: Option<installer::Installer>,
}

/// Where a listed file's bytes are.
enum Entry {
    /// A file on the disc (an index into its files).
    Disc(usize),
    /// A file inside the installer archive.
    Inner(installer::Inner),
}

impl DiscFiles {
    /// Opens the image and finds the game on it. Fails with the specific error when the image can't be read or the
    /// disc holds no loose game files.
    pub fn open(path: &Path) -> Result<Self, ImportError> {
        let disc = Disc::open(path).map_err(|e| ImportError::DiscImageUnreadable {
            path: path.to_path_buf(),
            detail: e.to_string(),
        })?;
        Self::from_disc(disc, path)
    }

    fn from_disc(disc: Disc, given: &Path) -> Result<Self, ImportError> {
        let lower = |s: &str| s.to_ascii_lowercase();
        // Pick the folder like a folder search does: shallower beats deeper, then War2Dat.mpq beats Install.mpq
        // beats any other archive.
        let mut best: Option<(usize, std::cmp::Reverse<u8>, String)> = None;
        for f in disc.files() {
            let (dir, name) = f.path.rsplit_once('/').unwrap_or(("", &f.path));
            let depth = if dir.is_empty() {
                0
            } else {
                dir.matches('/').count() + 1
            };
            if depth > GAME_DEPTH || !lower(name).ends_with(".mpq") {
                continue;
            }
            let score = match lower(name).as_str() {
                "war2dat.mpq" => 3,
                "install.mpq" => 2,
                _ => 1,
            };
            let key = (depth, std::cmp::Reverse(score), dir.to_string());
            if best.as_ref().is_none_or(|b| key < *b) {
                best = Some(key);
            }
        }
        let Some((_, _, root)) = best else {
            if let Some(found) = installer::find(&disc) {
                return Ok(Self::from_installer(disc, found));
            }
            if disc.files().iter().any(|f| lower(&f.path).ends_with(".war")) {
                return Err(ImportError::DosEdition);
            }
            if disc.files().iter().any(|f| is_installer_file(&f.path)) {
                return Err(ImportError::DiscHoldsInstaller(given.to_path_buf()));
            }
            return Err(ImportError::NoGameFiles(given.to_path_buf()));
        };
        let prefix = if root.is_empty() {
            String::new()
        } else {
            format!("{root}/")
        };
        let mut index = HashMap::new();
        let mut listing = Vec::new();
        for (i, f) in disc.files().iter().enumerate() {
            // Lower-casing ASCII keeps byte positions, so the prefix length also cuts the original.
            if !lower(&f.path).starts_with(&lower(&prefix)) {
                continue;
            }
            let rel = &f.path[prefix.len()..];
            let rel_lower = lower(rel);
            let rel = if let Some(rest) = rel_lower.strip_prefix("maps/") {
                if rest.is_empty() {
                    continue;
                }
                format!("Maps/{}", &rel[5..])
            } else if !rel.contains('/') && rel_lower.ends_with(".mpq") {
                rel.to_string()
            } else {
                continue;
            };
            index.insert(lower(&rel), Entry::Disc(i));
            listing.push(SourceFile { rel, size: f.size });
        }
        Ok(Self {
            disc,
            root,
            index,
            listing,
            installer: None,
        })
    }

    /// The game on a disc that keeps it in an installer archive: the archive itself as `Install.mpq` (the videos and
    /// music), and the data archive and stock maps taken out of it.
    fn from_installer(disc: Disc, found: installer::Installer) -> Self {
        let mut index = HashMap::new();
        let mut listing = Vec::new();
        let whole = &disc.files()[found.file];
        index.insert("install.mpq".to_string(), Entry::Disc(found.file));
        listing.push(SourceFile {
            rel: "Install.mpq".into(),
            size: whole.size,
        });
        for (rel, inner, size) in found.entries() {
            index.insert(rel.to_ascii_lowercase(), Entry::Inner(inner));
            listing.push(SourceFile { rel, size });
        }
        Self {
            disc,
            root: String::new(),
            index,
            listing,
            installer: Some(found),
        }
    }

    /// The disc, for listing.
    pub fn disc(&self) -> &Disc {
        &self.disc
    }

    /// The folder on the disc the game files were found in ("" for the root).
    pub fn root(&self) -> &str {
        &self.root
    }

    /// Copies the game files into a staging folder and returns them as an ordinary source, so `validate` can open the
    /// archives by path. The staging folder goes away with the last copy of the source. `progress` hears each file as
    /// it starts and each chunk as it is written ([`Step::ReadingDisc`]), against the listed sizes' total.
    pub fn stage(&self, given: &Path, progress: &mut dyn FnMut(&Progress)) -> Result<Source, ImportError> {
        let staging = Staging::create().map_err(|e| ImportError::io("creating the staging folder", e))?;
        let total: u64 = self.listing.iter().map(|f| f.size).sum();
        let mut done = 0u64;
        let mut buf = vec![0u8; STAGE_CHUNK];
        for f in &self.listing {
            let dest = staging.dir().join(&f.rel);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).map_err(|e| ImportError::io("creating a staging folder", e))?;
            }
            let unreadable = |e: io::Error| match e.kind() {
                io::ErrorKind::UnexpectedEof | io::ErrorKind::InvalidData => ImportError::DiscImageUnreadable {
                    path: given.to_path_buf(),
                    detail: format!("{}: {e}", f.rel),
                },
                _ => ImportError::io(format!("copying {}", f.rel), e),
            };
            // Reported before the file is opened: a file in the installer archive is unpacked whole when it opens.
            progress(&Progress {
                step: Step::ReadingDisc,
                file: &f.rel,
                done,
                total,
            });
            let mut input = self
                .open(&f.rel)
                .map_err(|e| ImportError::io(format!("reading {} from the disc", f.rel), e))?;
            let mut out = File::create(&dest).map_err(|e| ImportError::io(format!("writing {}", f.rel), e))?;
            loop {
                let n = match input.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(unreadable(e)),
                };
                out.write_all(&buf[..n])
                    .map_err(|e| ImportError::io(format!("copying {}", f.rel), e))?;
                done += n as u64;
                progress(&Progress {
                    step: Step::ReadingDisc,
                    file: &f.rel,
                    done,
                    total,
                });
            }
        }
        let mut source = crate::source::source_at(given, staging.dir());
        source.kind = SourceKind::CdImage;
        source.staged = Some(Arc::new(staging));
        if source.archives.is_empty() {
            return Err(ImportError::NoGameFiles(given.to_path_buf()));
        }
        Ok(source)
    }
}

/// A file that suggests the disc holds an installer rather than loose game files.
fn is_installer_file(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    matches!(name.as_str(), "setup.exe" | "install.exe") || name.ends_with(".cab") || name.ends_with(".msi")
}

impl SourceFiles for DiscFiles {
    fn list(&self) -> io::Result<Vec<SourceFile>> {
        Ok(self.listing.clone())
    }

    fn open(&self, rel: &str) -> io::Result<Box<dyn Read>> {
        let entry = self
            .index
            .get(&rel.to_ascii_lowercase())
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        match (entry, &self.installer) {
            (Entry::Disc(i), _) => Ok(Box::new(self.disc.read_file(&self.disc.files()[*i])?)),
            (Entry::Inner(inner), Some(installer)) => installer.read(inner),
            (Entry::Inner(_), None) => Err(io::Error::from(io::ErrorKind::NotFound)),
        }
    }
}

/// Opens a disc image as a [`Source`] (the game files staged in a temporary folder).
pub fn open_source(path: &Path, progress: &mut dyn FnMut(&Progress)) -> Result<Source, ImportError> {
    DiscFiles::open(path)?.stage(path, progress)
}

mod installer;
#[cfg(test)]
mod tests;
