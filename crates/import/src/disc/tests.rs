use std::fs;

use super::*;
use crate::source::classify;
use crate::testiso::{Framing, IsoOptions, both32, iso_image, nrg_image, raw_sectors, record};
use crate::testutil::mpq_bytes;
use crate::validate::{KeyCheck, validate_with};

fn pattern(n: usize, seed: u8) -> Vec<u8> {
    (0..n).map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed)).collect()
}

fn marker(i: &mut tidewater_formats::install::Install) -> Result<(), String> {
    i.read("rez\\marker.txt").map(|_| ()).map_err(|e| e.to_string())
}
const MARKER: &[KeyCheck] = &[KeyCheck {
    label: "the marker",
    check: marker,
}];

/// A disc whose game is in a folder, with a nested map folder and some files that must not be copied.
fn game_files() -> Vec<(String, Vec<u8>)> {
    vec![
        ("WAR2/War2Dat.mpq".into(), mpq_bytes(&[("rez\\marker.txt", b"one")])),
        ("WAR2/Install.mpq".into(), mpq_bytes(&[("x", &pattern(5000, 3))])),
        ("WAR2/Maps/a.pud".into(), pattern(3000, 5)),
        ("WAR2/Maps/Sub/b.pud".into(), b"pud2".to_vec()),
        ("WAR2/game.exe".into(), b"stray, must not be copied".to_vec()),
        ("README.TXT".into(), b"hello".to_vec()),
    ]
}

fn image_of(files: &[(String, Vec<u8>)], opts: IsoOptions) -> Vec<u8> {
    let refs: Vec<(&str, &[u8])> = files.iter().map(|(n, d)| (n.as_str(), d.as_slice())).collect();
    iso_image(&refs, opts)
}

fn write(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let p = dir.join(name);
    fs::write(&p, bytes).unwrap();
    p
}

fn read_all(disc: &Disc, path: &str) -> Vec<u8> {
    let f = disc
        .files()
        .iter()
        .find(|f| f.path == path)
        .unwrap_or_else(|| panic!("{path} missing"));
    let mut v = Vec::new();
    disc.read_file(f).unwrap().read_to_end(&mut v).unwrap();
    v
}

#[test]
fn an_iso_lists_nested_folders_and_reads_files_across_sectors() {
    let t = tempfile::tempdir().unwrap();
    let files = game_files();
    let iso = write(t.path(), "disc.iso", &image_of(&files, IsoOptions::default()));
    let disc = Disc::open(&iso).unwrap();
    assert!(disc.is_joliet());
    assert_eq!(disc.layout().stride, 2048);
    let listed: Vec<_> = disc.files().iter().map(|f| (f.path.as_str(), f.size)).collect();
    assert!(listed.contains(&("WAR2/Maps/Sub/b.pud", 4)));
    assert_eq!(disc.files().len(), files.len());
    for (name, data) in &files {
        assert_eq!(&read_all(&disc, name), data, "{name}");
    }
}

#[test]
fn joliet_names_win_and_iso_names_lose_their_version_suffix() {
    let t = tempfile::tempdir().unwrap();
    let files = vec![
        ("Install Notes.txt".to_string(), b"long".to_vec()),
        ("war2dat.mpq".to_string(), b"m".to_vec()),
    ];
    let both = write(t.path(), "both.iso", &image_of(&files, IsoOptions::default()));
    let disc = Disc::open(&both).unwrap();
    assert!(disc.is_joliet());
    let names: Vec<_> = disc.files().iter().map(|f| f.path.clone()).collect();
    assert_eq!(names, ["Install Notes.txt", "war2dat.mpq"]);

    let iso_only = write(
        t.path(),
        "iso.iso",
        &image_of(
            &files,
            IsoOptions {
                joliet: false,
                iso: true,
            },
        ),
    );
    let disc = Disc::open(&iso_only).unwrap();
    assert!(!disc.is_joliet());
    let names: Vec<_> = disc.files().iter().map(|f| f.path.clone()).collect();
    // The long name was mangled by the writer; the short one is upper-cased with ";1" removed.
    assert!(names.iter().any(|n| n == "WAR2DAT.MPQ"), "{names:?}");
    assert!(
        names.iter().any(|n| n.starts_with('F') && n.ends_with(".DAT")),
        "{names:?}"
    );

    let joliet_only = write(
        t.path(),
        "joliet.iso",
        &image_of(
            &files,
            IsoOptions {
                joliet: true,
                iso: false,
            },
        ),
    );
    assert!(Disc::open(&joliet_only).unwrap().is_joliet());
}

#[test]
fn raw_sector_framings_read_the_same_files() {
    let t = tempfile::tempdir().unwrap();
    let files = game_files();
    let iso = image_of(&files, IsoOptions::default());
    for (framing, stride, offset) in [
        (Framing::Mode1, 2352, 16),
        (Framing::Mode2Form1, 2352, 24),
        (Framing::Mode1Sub, 2448, 16),
    ] {
        // As a bare .bin, found by its signature.
        let bin = write(t.path(), "disc.bin", &raw_sectors(&iso, framing));
        let disc = Disc::open(&bin).unwrap();
        assert_eq!((disc.layout().stride, disc.layout().data_offset), (stride, offset));
        for (name, data) in &files {
            assert_eq!(&read_all(&disc, name), data, "{name}");
        }
        // Through a cue sheet whose file name has another letter case.
        let cue = write(
            t.path(),
            "disc.cue",
            b"FILE \"DISC.BIN\" BINARY\n  TRACK 01 MODE2/2352\n    INDEX 01 00:00:00\n",
        );
        let disc = Disc::open(&cue).unwrap();
        assert_eq!(disc.layout().data_offset, offset);
        assert_eq!(read_all(&disc, "README.TXT"), b"hello");
    }
}

#[test]
fn a_cue_sheet_skips_audio_and_follows_index_offsets() {
    let t = tempfile::tempdir().unwrap();
    let iso = image_of(&game_files(), IsoOptions::default());
    // Ten frames of audio, then the data track.
    let mut bin = vec![0x11u8; 2352 * 10];
    bin.extend_from_slice(&raw_sectors(&iso, Framing::Mode1));
    write(t.path(), "game.bin", &bin);
    let cue = write(
        t.path(),
        "game.cue",
        b"FILE \"game.bin\" BINARY\r\n  TRACK 01 AUDIO\r\n    INDEX 01 00:00:00\r\n  TRACK 02 MODE1/2352\r\n    INDEX 00 00:00:08\r\n    INDEX 01 00:00:10\r\n",
    );
    let disc = Disc::open(&cue).unwrap();
    assert_eq!(disc.layout().base, 2352 * 10);
    assert_eq!(read_all(&disc, "README.TXT"), b"hello");
    // Cue sheets with a missing file or no data track are errors.
    let none = write(
        t.path(),
        "none.cue",
        b"FILE \"nope.bin\" BINARY\n TRACK 01 MODE1/2352\n  INDEX 01 00:00:00\n",
    );
    assert!(Disc::open(&none).is_err());
    let audio = write(
        t.path(),
        "audio.cue",
        b"FILE \"game.bin\" BINARY\n TRACK 01 AUDIO\n  INDEX 01 00:00:00\n",
    );
    assert!(Disc::open(&audio).unwrap_err().to_string().contains("no data track"));
}

#[test]
fn mdf_with_mds_and_nrg_footers_are_read() {
    let t = tempfile::tempdir().unwrap();
    let files = game_files();
    let iso = image_of(&files, IsoOptions::default());
    write(t.path(), "game.mdf", &raw_sectors(&iso, Framing::Mode1));
    write(t.path(), "game.mds", b"MEDIA DESCRIPTOR");
    for name in ["game.mdf", "game.mds"] {
        let disc = Disc::open(&t.path().join(name)).unwrap();
        assert_eq!(read_all(&disc, "README.TXT"), b"hello");
    }
    // 2448-byte sectors with sub-channel data in an .mdf.
    write(t.path(), "sub.mdf", &raw_sectors(&iso, Framing::Mode1Sub));
    assert_eq!(Disc::open(&t.path().join("sub.mdf")).unwrap().layout().stride, 2448);

    for (junk, v2) in [(0, false), (4096, false), (0, true), (6144, true)] {
        let nrg = write(t.path(), "game.nrg", &nrg_image(&iso, junk, v2));
        let disc = Disc::open(&nrg).unwrap();
        assert_eq!(disc.layout().base, junk as u64, "junk {junk}, v2 {v2}");
        for (name, data) in &files {
            assert_eq!(&read_all(&disc, name), data);
        }
    }
    // An .mds with no .mdf beside it.
    let lone = write(t.path(), "lone.mds", b"x");
    assert!(Disc::open(&lone).is_err());
}

#[test]
fn a_file_in_several_extents_is_joined() {
    // Volume: PVD at 16, terminator at 17, root at 18, data at 19 (a full sector) and 20 (100 bytes).
    let mut img = vec![0u8; 21 * 2048];
    let pvd = &mut img[16 * 2048..17 * 2048];
    pvd[0] = 1;
    pvd[1..6].copy_from_slice(b"CD001");
    pvd[6] = 1;
    pvd[156..190].copy_from_slice(&record(&[0], 18, 2048, true)[..34]);
    img[17 * 2048] = 255;
    img[17 * 2048 + 1..17 * 2048 + 6].copy_from_slice(b"CD001");
    img[17 * 2048 + 6] = 1;
    let mut first = record(b"BIG.BIN;1", 19, 2048, false);
    first[25] |= 0x80;
    let mut dir = record(&[0], 18, 2048, true);
    dir.extend(record(&[1], 18, 2048, true));
    dir.extend(first);
    dir.extend(record(b"BIG.BIN;1", 20, 100, false));
    img[18 * 2048..18 * 2048 + dir.len()].copy_from_slice(&dir);
    img[19 * 2048..20 * 2048].fill(b'a');
    img[20 * 2048..20 * 2048 + 100].fill(b'b');
    let _ = both32(0);
    let t = tempfile::tempdir().unwrap();
    let disc = Disc::open(&write(t.path(), "multi.iso", &img)).unwrap();
    assert_eq!(disc.files().len(), 1);
    assert_eq!(disc.files()[0].size, 2148);
    let data = read_all(&disc, "BIG.BIN");
    assert_eq!(data.len(), 2148);
    assert!(data[..2048].iter().all(|&b| b == b'a') && data[2048..].iter().all(|&b| b == b'b'));
}

#[test]
fn many_files_make_a_directory_that_spans_sectors() {
    let t = tempfile::tempdir().unwrap();
    let files: Vec<(String, Vec<u8>)> = (0..120)
        .map(|i| (format!("Maps/map{i:04}.pud"), vec![i as u8; 10 + i]))
        .collect();
    let iso = write(t.path(), "many.iso", &image_of(&files, IsoOptions::default()));
    let disc = Disc::open(&iso).unwrap();
    assert_eq!(disc.files().len(), 120);
    assert_eq!(read_all(&disc, "Maps/map0119.pud"), vec![119u8; 129]);
}

#[test]
fn classify_and_import_a_full_synthetic_disc() {
    let t = tempfile::tempdir().unwrap();
    let files = game_files();
    let iso = image_of(&files, IsoOptions::default());
    let images = [
        write(t.path(), "plain.iso", &iso),
        write(t.path(), "raw.bin", &raw_sectors(&iso, Framing::Mode2Form1)),
        write(t.path(), "nero.nrg", &nrg_image(&iso, 0, true)),
        write(t.path(), "alcohol.mdf", &raw_sectors(&iso, Framing::Mode1)),
    ];
    write(
        t.path(),
        "raw.cue",
        b"FILE \"raw.bin\" BINARY\n  TRACK 01 MODE2/2352\n    INDEX 01 00:00:00\n",
    );
    for image in images.iter().chain([&t.path().join("raw.cue")]) {
        let source = classify(image).unwrap_or_else(|e| panic!("{}: {e}", image.display()));
        assert_eq!(source.kind, SourceKind::CdImage);
        let report = validate_with(&source, MARKER);
        assert!(report.is_ok(), "{:?}", report.issues);
        let data = t
            .path()
            .join(format!("data-{}", image.file_name().unwrap().to_string_lossy()));
        let manifest = crate::import(&source, &report, &data, &mut |_| {}).unwrap();
        assert_eq!(manifest.source_kind, "cd-image");
        let mut got: Vec<_> = manifest.files.iter().map(|f| f.path.as_str()).collect();
        got.sort_unstable();
        assert_eq!(got, ["Install.mpq", "Maps/Sub/b.pud", "Maps/a.pud", "War2Dat.mpq"]);
        for (name, bytes) in &files {
            if let Some(rest) = name.strip_prefix("WAR2/")
                && !rest.ends_with(".exe")
            {
                assert_eq!(&fs::read(crate::game_dir(&data).join(rest)).unwrap(), bytes, "{rest}");
            }
        }
        assert!(!crate::game_dir(&data).join("game.exe").exists());
    }
    // The staging folder goes away with the source.
    let staged = classify(&images[0]).unwrap();
    let dir = staged.staged.as_ref().unwrap().dir().to_path_buf();
    assert!(dir.exists());
    drop(staged);
    assert!(!dir.exists());
}

#[test]
fn game_files_in_the_root_and_in_any_letter_case_are_found() {
    let t = tempfile::tempdir().unwrap();
    let files = vec![
        ("war2dat.MPQ".to_string(), mpq_bytes(&[("rez\\marker.txt", b"m")])),
        ("MAPS/one.pud".to_string(), b"p".to_vec()),
    ];
    let iso = write(t.path(), "d.iso", &image_of(&files, IsoOptions::default()));
    let source = classify(&iso).unwrap();
    assert!(validate_with(&source, MARKER).is_ok());
    let found = DiscFiles::open(&iso).unwrap();
    assert_eq!(found.root(), "");
    let rels: Vec<_> = found.list().unwrap().into_iter().map(|f| f.rel).collect();
    assert_eq!(rels, ["Maps/one.pud", "war2dat.MPQ"]);
    // Lookup ignores case.
    let mut v = Vec::new();
    found.open("WAR2DAT.mpq").unwrap().read_to_end(&mut v).unwrap();
    assert_eq!(v, files[0].1);
    assert!(found.open("nothing.mpq").is_err());
}

#[test]
fn a_disc_that_holds_an_installer_or_the_dos_edition_says_so() {
    let t = tempfile::tempdir().unwrap();
    let make = |name: &str, files: &[(&str, &[u8])]| {
        let refs: Vec<(&str, &[u8])> = files.to_vec();
        write(t.path(), name, &iso_image(&refs, IsoOptions::default()))
    };
    let installer = make("inst.iso", &[("SETUP.EXE", b"MZ"), ("DATA1.CAB", b"cab")]);
    let err = classify(&installer).unwrap_err();
    assert!(matches!(err, ImportError::DiscHoldsInstaller(_)), "{err}");
    assert!(err.to_string().contains("this disc holds an installer"));
    assert_eq!(err.exit_code(), 19);
    let dos = make("dos.iso", &[("DATA/WAR2.WAR", b"w")]);
    assert!(matches!(classify(&dos), Err(ImportError::DosEdition)));
    let empty = make("empty.iso", &[("NOTES.TXT", b"n")]);
    assert!(matches!(classify(&empty), Err(ImportError::NoGameFiles(_))));
}

#[test]
fn what_is_not_a_readable_disc_is_an_error() {
    let t = tempfile::tempdir().unwrap();
    // Recognised by extension but empty or junk.
    for name in ["a.img", "b.mdf", "c.nrg", "d.cue", "e.mds"] {
        let p = write(t.path(), name, b"not a disc at all");
        assert!(
            matches!(
                classify(&p),
                Err(ImportError::DiscImageUnreadable { .. } | ImportError::NoGameFiles(_))
            ),
            "{name}"
        );
    }
    let err = classify(&t.path().join("a.img")).unwrap_err();
    assert_eq!(err.exit_code(), 19);
    // A .iso or .bin without the signature isn't a disc image at all.
    let notes = write(t.path(), "notes.iso", &vec![0u8; 20 * 2048]);
    assert!(matches!(classify(&notes), Err(ImportError::NoGameFiles(_))));
}

#[test]
fn a_truncated_image_is_an_error_not_a_panic() {
    let t = tempfile::tempdir().unwrap();
    let files = game_files();
    let iso = image_of(&files, IsoOptions::default());
    for cut in (0..iso.len()).step_by(1000).chain([iso.len() - 1]) {
        let p = write(t.path(), "cut.iso", &iso[..cut]);
        match Disc::open(&p) {
            Err(_) => {}
            Ok(disc) => {
                // The tree was complete; reading a file past the end must fail rather than invent data.
                for f in disc.files() {
                    let _ = io::copy(&mut disc.read_file(f).unwrap(), &mut io::sink());
                }
            }
        }
        let _ = classify(&p);
    }
    // Cut inside the game data: opening works, importing says the image is damaged.
    let full = write(t.path(), "full.iso", &iso);
    let disc = Disc::open(&full).unwrap();
    let last_data_end = disc
        .files()
        .iter()
        .flat_map(|f| f.extents.iter())
        .map(|e| e.0)
        .max()
        .unwrap()
        * 2048;
    let cut = write(t.path(), "cut2.iso", &iso[..last_data_end as usize]);
    let mut failed = false;
    for f in Disc::open(&cut).unwrap().files() {
        let d = Disc::open(&cut).unwrap();
        failed |= io::copy(&mut d.read_file(f).unwrap(), &mut io::sink()).is_err();
    }
    assert!(failed);
}

#[test]
fn a_directory_loop_is_an_error() {
    let t = tempfile::tempdir().unwrap();
    let files = vec![("SUB/FILE.TXT".to_string(), b"x".to_vec())];
    let mut iso = image_of(
        &files,
        IsoOptions {
            joliet: false,
            iso: true,
        },
    );
    // The root record is in the primary descriptor; find its sector, then point the SUB record back at the root.
    let root = u32::from_le_bytes(iso[16 * 2048 + 158..16 * 2048 + 162].try_into().unwrap()) as usize;
    let at = root * 2048;
    let pos = iso[at..at + 2048].windows(3).position(|w| w == b"SUB").unwrap() - 33;
    iso[at + pos + 2..at + pos + 10].copy_from_slice(&both32(root as u32));
    let p = write(t.path(), "loop.iso", &iso);
    let err = Disc::open(&p).unwrap_err();
    assert!(err.to_string().contains("loops"), "{err}");
    assert!(matches!(classify(&p), Err(ImportError::DiscImageUnreadable { .. })));
}

#[test]
fn damaged_images_never_panic_or_hang() {
    let t = tempfile::tempdir().unwrap();
    let base = image_of(&game_files(), IsoOptions::default());
    let mut seed = 0x1234_5678u32;
    let mut next = move || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        seed >> 8
    };
    for round in 0..300 {
        let mut img = base.clone();
        for _ in 0..1 + round % 8 {
            let at = next() as usize % (24 * 2048);
            img[at] = next() as u8;
        }
        let p = write(t.path(), "fuzz.iso", &img);
        if let Ok(disc) = Disc::open(&p) {
            for f in disc.files().iter().filter(|f| f.size < 1 << 22) {
                if let Ok(mut r) = disc.read_file(f) {
                    let _ = io::copy(&mut r, &mut io::sink());
                }
            }
        }
    }
}

/// A disc like the Battle.net Edition CD: no loose archives, only a program with an archive behind it that holds
/// the installer's files under their install names.
type DiscFilesList = Vec<(String, Vec<u8>)>;

fn installer_disc() -> (DiscFilesList, Vec<u8>, Vec<u8>) {
    let war2dat = mpq_bytes(&[("rez\\marker.txt", b"from the installer")]);
    let map = pattern(3000, 9);
    let music = pattern(1_200_000, 4);
    let inner = mpq_bytes(&[
        ("Files\\War2Dat.mpq", &war2dat),
        ("Maps\\Dark Paths BNE.pud", &map),
        ("Maps\\Not A Stock Map.pud", b"unlisted"),
        ("music\\theme.wav", &music),
    ]);
    // A program in front of the archive, padded so the archive starts on a 512-byte boundary.
    let mut exe = b"MZ".to_vec();
    exe.resize(3 * 512, 0x90);
    exe.extend_from_slice(&inner);
    let files = vec![
        ("INSTALL.EXE".to_string(), exe),
        ("SETUP.EXE".to_string(), b"MZsmall".to_vec()),
        ("SUPPORT/PC/FAQ.HTM".to_string(), b"<html>".to_vec()),
    ];
    (files, war2dat, map)
}

#[test]
fn a_disc_whose_game_is_inside_an_installer_archive_is_imported() {
    let t = tempfile::tempdir().unwrap();
    let (files, war2dat, map) = installer_disc();
    let iso = image_of(&files, IsoOptions::default());
    let images = [
        write(t.path(), "plain.iso", &iso),
        write(t.path(), "raw.bin", &raw_sectors(&iso, Framing::Mode1)),
        write(t.path(), "raw2.bin", &raw_sectors(&iso, Framing::Mode2Form1)),
    ];
    write(
        t.path(),
        "raw2.cue",
        b"FILE \"raw2.bin\" BINARY\n  TRACK 01 MODE2/2352\n    INDEX 01 00:00:00\n",
    );
    for image in images.iter().chain([&t.path().join("raw2.cue")]) {
        let source = classify(image).unwrap_or_else(|e| panic!("{}: {e}", image.display()));
        assert_eq!(source.kind, SourceKind::CdImage);
        let report = validate_with(&source, MARKER);
        assert!(report.is_ok(), "{:?}", report.issues);
        let data = t
            .path()
            .join(format!("data-{}", image.file_name().unwrap().to_string_lossy()));
        let manifest = crate::import(&source, &report, &data, &mut |_| {}).unwrap();
        let mut got: Vec<_> = manifest.files.iter().map(|f| f.path.as_str()).collect();
        got.sort_unstable();
        assert_eq!(got, ["Install.mpq", "Maps/Dark Paths BNE.pud", "War2Dat.mpq"]);
        let game = crate::game_dir(&data);
        assert_eq!(fs::read(game.join("War2Dat.mpq")).unwrap(), war2dat);
        assert_eq!(fs::read(game.join("Maps/Dark Paths BNE.pud")).unwrap(), map);
        // The whole program-plus-archive file is kept as Install.mpq, and still opens as an archive.
        assert_eq!(fs::read(game.join("Install.mpq")).unwrap(), files[0].1);
        let mut inst = tidewater_formats::mpq::Archive::open(game.join("Install.mpq")).unwrap();
        assert_eq!(inst.read("music\\theme.wav").unwrap().len(), 1_200_000);
    }
}

#[test]
fn a_big_program_without_an_archive_behind_it_is_still_just_an_installer() {
    let t = tempfile::tempdir().unwrap();
    let mut exe = b"MZ".to_vec();
    exe.resize(2_000_000, 0x90);
    let iso = write(
        t.path(),
        "inst.iso",
        &iso_image(&[("INSTALL.EXE", &exe), ("SETUP.EXE", b"MZ")], IsoOptions::default()),
    );
    assert!(matches!(classify(&iso), Err(ImportError::DiscHoldsInstaller(_))));
}

#[test]
fn a_file_on_a_disc_can_be_read_after_seeking() {
    let t = tempfile::tempdir().unwrap();
    let data = pattern(9000, 1);
    let iso_bytes = iso_image(&[("A/F.BIN", &data), ("B.BIN", b"x")], IsoOptions::default());
    let iso = write(t.path(), "s.iso", &iso_bytes);
    let bin = write(t.path(), "s.bin", &raw_sectors(&iso_bytes, Framing::Mode1));
    for image in [iso, bin] {
        let disc = Disc::open(&image).unwrap();
        let f = disc.files().iter().find(|f| f.path == "A/F.BIN").unwrap();
        let mut r = disc.read_file(f).unwrap();
        for &(at, len) in &[
            (0usize, 10usize),
            (2047, 3),
            (2048, 5),
            (5000, 4000),
            (8990, 10),
            (1, 8999),
        ] {
            r.seek(SeekFrom::Start(at as u64)).unwrap();
            let mut got = vec![0u8; len];
            r.read_exact(&mut got).unwrap();
            assert_eq!(got, data[at..at + len], "at {at}");
        }
        assert_eq!(r.seek(SeekFrom::End(-4)).unwrap(), 8996);
        let mut tail = Vec::new();
        r.read_to_end(&mut tail).unwrap();
        assert_eq!(tail, data[8996..]);
        assert_eq!(r.seek(SeekFrom::Current(-2)).unwrap(), 8998);
        assert!(r.seek(SeekFrom::Current(-9000)).is_err());
        r.seek(SeekFrom::Start(20_000)).unwrap();
        assert_eq!(r.read(&mut [0u8; 4]).unwrap(), 0);
    }
}

#[test]
fn a_bin_beside_its_cue_uses_the_sheet() {
    let t = tempfile::tempdir().unwrap();
    let iso = image_of(&game_files(), IsoOptions::default());
    // Audio first, so the data track doesn't start at byte 0: only the sheet knows where it is.
    let mut bin = vec![0x22u8; 2352 * 150];
    bin.extend_from_slice(&raw_sectors(&iso, Framing::Mode2Form1));
    let bin_path = write(t.path(), "Game.BIN", &bin);
    write(
        t.path(),
        "game.cue",
        b"FILE \"GAME.bin\" BINARY\n  TRACK 01 AUDIO\n    INDEX 01 00:00:00\n  TRACK 02 MODE2/2352\n    INDEX 01 00:02:00\n",
    );
    let disc = Disc::open(&bin_path).unwrap();
    assert_eq!(disc.layout().base, 2352 * 150);
    assert_eq!(read_all(&disc, "README.TXT"), b"hello");
    let source = classify(&bin_path).unwrap();
    assert_eq!(source.kind, SourceKind::CdImage);
}

#[test]
fn mode1_2048_sheets_and_bins_without_a_sheet_work() {
    let t = tempfile::tempdir().unwrap();
    let iso = image_of(&game_files(), IsoOptions::default());
    write(t.path(), "cooked.bin", &iso);
    let cue = write(
        t.path(),
        "cooked.cue",
        b"FILE \"cooked.bin\" BINARY\n  TRACK 01 MODE1/2048\n    INDEX 01 00:00:00\n",
    );
    assert_eq!(Disc::open(&cue).unwrap().layout().stride, 2048);
    // No sheet at all: the layout comes from the sync pattern's place in the file.
    let lone = write(t.path(), "lone.bin", &raw_sectors(&iso, Framing::Mode1));
    let disc = Disc::open(&lone).unwrap();
    assert_eq!((disc.layout().stride, disc.layout().data_offset), (2352, 16));
    assert_eq!(read_all(&disc, "README.TXT"), b"hello");
}

#[test]
fn a_bad_sheet_beside_a_bin_does_not_hide_a_plain_disc() {
    let t = tempfile::tempdir().unwrap();
    let iso = image_of(&game_files(), IsoOptions::default());
    let bin = write(t.path(), "d.bin", &raw_sectors(&iso, Framing::Mode1));
    write(
        t.path(),
        "d.cue",
        b"FILE \"missing.bin\" BINARY\n TRACK 01 MODE1/2352\n  INDEX 01 00:00:00\n",
    );
    assert!(Disc::open(&bin).is_ok());
}

#[test]
fn a_sheet_with_a_missing_file_or_only_audio_says_so() {
    let t = tempfile::tempdir().unwrap();
    let iso = image_of(&game_files(), IsoOptions::default());
    write(t.path(), "real.bin", &raw_sectors(&iso, Framing::Mode1));
    let missing = write(
        t.path(),
        "missing.cue",
        b"FILE \"gone.bin\" BINARY\n TRACK 01 MODE1/2352\n  INDEX 01 00:00:00\n",
    );
    let err = classify(&missing).unwrap_err();
    assert!(matches!(err, ImportError::DiscImageUnreadable { .. }), "{err}");
    assert!(err.to_string().contains("gone.bin"), "{err}");
    assert!(err.to_string().contains("isn't in the same folder"), "{err}");
    assert_eq!(err.exit_code(), 19);
    let audio = write(
        t.path(),
        "audio.cue",
        b"FILE \"real.bin\" BINARY\n TRACK 01 AUDIO\n  INDEX 01 00:00:00\n TRACK 02 AUDIO\n  INDEX 01 00:03:00\n",
    );
    let err = classify(&audio).unwrap_err();
    assert!(matches!(err, ImportError::DiscImageUnreadable { .. }), "{err}");
    assert!(err.to_string().contains("no data track"), "{err}");
    assert!(err.to_string().contains("only audio"), "{err}");
}

/// Classifies `path` and returns what its progress callback heard, as (step, file, done, total) for each call.
fn classify_heard(path: &Path) -> (Source, Vec<(crate::Step, String, u64, u64)>) {
    let mut heard = Vec::new();
    let source = crate::source::classify_with(path, &mut |p| {
        heard.push((p.step, p.file.to_string(), p.done, p.total));
    })
    .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    (source, heard)
}

/// The bytes of the files under `dir`.
fn bytes_under(dir: &Path) -> u64 {
    fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| {
            if e.file_type().unwrap().is_dir() {
                bytes_under(&e.path())
            } else {
                e.metadata().unwrap().len()
            }
        })
        .sum()
}

/// The checks every staged disc's progress passes: one step, a steady total that is what was staged, bytes that only
/// go up from 0 to that total, and every staged file named.
fn check_staging_progress(source: &Source, heard: &[(crate::Step, String, u64, u64)], files: &[&str]) {
    assert!(!heard.is_empty());
    let staged = bytes_under(source.staged.as_ref().unwrap().dir());
    assert!(heard.iter().all(|h| h.0 == crate::Step::ReadingDisc), "{heard:?}");
    assert!(
        heard.iter().all(|h| h.3 == staged),
        "the total is what gets staged ({staged})"
    );
    assert_eq!(heard[0].2, 0, "the first report is before any bytes");
    assert_eq!(heard.last().unwrap().2, staged, "the last report is the whole");
    assert!(heard.windows(2).all(|w| w[0].2 <= w[1].2), "the bytes never go back");
    for f in files {
        assert!(heard.iter().any(|h| h.1 == *f), "{f} is named");
    }
}

#[test]
fn reading_a_cd_image_reports_its_progress_chunk_by_chunk() {
    let t = tempfile::tempdir().unwrap();
    // The Battle.net Edition layout (the game inside the installer archive), as a raw bin with a cue sheet: the case
    // whose bar used to stand still for the whole read.
    let (files, _, _) = installer_disc();
    let iso = image_of(&files, IsoOptions::default());
    write(t.path(), "raw2.bin", &raw_sectors(&iso, Framing::Mode2Form1));
    let cue = write(
        t.path(),
        "raw2.cue",
        b"FILE \"raw2.bin\" BINARY\n  TRACK 01 MODE2/2352\n    INDEX 01 00:00:00\n",
    );
    let (source, heard) = classify_heard(&cue);
    check_staging_progress(
        &source,
        &heard,
        &["Install.mpq", "Maps/Dark Paths BNE.pud", "War2Dat.mpq"],
    );
    // The 1.2 MB installer file moves the bar many times on its way, not once at its end.
    let install = heard.iter().filter(|h| h.1 == "Install.mpq").count();
    assert!(install >= 10, "Install.mpq reported {install} times");

    // Loose files on a plain ISO report the same way.
    let iso = write(t.path(), "plain.iso", &image_of(&game_files(), IsoOptions::default()));
    let (source, heard) = classify_heard(&iso);
    check_staging_progress(
        &source,
        &heard,
        &["War2Dat.mpq", "Install.mpq", "Maps/a.pud", "Maps/Sub/b.pud"],
    );
}

#[test]
fn a_game_folder_needs_no_reading_step() {
    let t = tempfile::tempdir().unwrap();
    fs::write(t.path().join("War2Dat.mpq"), mpq_bytes(&[("rez\\marker.txt", b"one")])).unwrap();
    let mut heard = 0;
    crate::source::classify_with(t.path(), &mut |_| heard += 1).unwrap();
    assert_eq!(heard, 0);
}
