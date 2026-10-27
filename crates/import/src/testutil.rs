//! Test helpers: synthetic archives built with our own writer (`tidewater_formats::testmpq`), no game data anywhere.

use std::path::Path;

pub use tidewater_formats::testmpq::mpq_bytes;

pub fn write_mpq(path: &Path, files: &[(&str, &[u8])]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, mpq_bytes(files)).unwrap();
}

#[test]
fn the_writer_makes_archives_the_reader_opens() {
    let bytes = mpq_bytes(&[("a\\b.txt", b"hello"), ("c.bin", b"0123456789")]);
    let mut a = tidewater_formats::mpq::Archive::from_reader(Box::new(std::io::Cursor::new(bytes))).unwrap();
    assert_eq!(a.read("A\\B.TXT").unwrap(), b"hello");
    assert_eq!(a.read("c.bin").unwrap(), b"0123456789");
}
