//! `twtool app-icon`: draws the app icon (`packaging/icon/*.svg`, our own artwork) at the sizes each platform's
//! package wants and writes PNG, Windows `.ico` and Apple `.icns` files. `packaging/icon/render.sh` runs it for every
//! asset, so changing the design is a change to the SVG files only.
//!
//! The containers are written directly:
//! - ICO: a 6-byte header (reserved 0, type 1, image count) and a 16-byte directory entry per image (width and height
//!   with 0 meaning 256, colour count, reserved, planes, bits per pixel, data size, data offset), then the images.
//!   Images under 256 pixels are stored as a 32-bit `BITMAPINFOHEADER` DIB (height doubled for the AND mask, rows
//!   bottom-up, BGRA, then a 1-bit AND mask padded to 4-byte rows); the 256-pixel image as a PNG, which Windows reads
//!   since Vista. Source: Microsoft, "Icons" (Win32 docs, the ICONDIR/ICONDIRENTRY layout) and "BITMAPINFOHEADER".
//! - ICNS: the `icns` magic and the file length (big-endian), then one element per image: a four-character type and
//!   the element length (8-byte header included), then the image. `ic04` (16) and `ic05` (32) hold `ARGB` data (the
//!   magic, then the four planes run-length compressed, see [`argb_rle`]); `ic07` (128), `ic08` (256), `ic09` (512),
//!   `ic10` (1024, 512@2x), `ic11` (32, 16@2x), `ic12` (64, 32@2x), `ic13` (256, 128@2x) and `ic14` (512, 256@2x)
//!   hold PNG data. Source: the "Apple Icon Image format" article on Wikipedia, which lists the element types and the
//!   compression; `iconutil -c iconset` reads the result back.
//! - Indexed PNG (`--indexed`): colour type 3 with a `PLTE` chunk of at most 256 entries and 8-bit indices, no `tRNS`.
//!   A real PS Vita refuses to install a VPK whose LiveArea images (`sce_sys/icon0.png`) are in any other form, with
//!   error 0x8010113D (GitHub issue #2: our RGBA icon; the Vita homebrew community's LiveArea guidance asks for 8-bit
//!   indexed PNGs). The palette comes from a median cut over the opaque image (see [`quantize`]).

use std::path::Path;

use anyhow::{Context, Result, bail};
use resvg::{tiny_skia, usvg};

/// How the drawing is framed in the square image.
#[derive(Clone, Copy, PartialEq, Eq, Debug, clap::ValueEnum)]
pub enum Shape {
    /// Edge to edge (iOS masks the corners itself; Android's layers are full squares).
    Square,
    /// A rounded square (superellipse) of 824/1024 of the side, centred with a transparent margin: the macOS grid,
    /// also used for the Windows, Linux and window icons.
    Rounded,
}

/// What `app-icon` draws: the layers (drawn in order, each scaled to the image), how much of the image the last layer
/// fills (`scale`, about the centre; Android's adaptive foreground keeps its art in the middle two thirds), and the
/// framing.
pub struct Spec<'a> {
    pub layers: &'a [std::path::PathBuf],
    pub scale: f32,
    pub shape: Shape,
    pub opaque: bool,
    /// An opaque PNG with a palette of at most 256 colours (the Vita's LiveArea images).
    pub indexed: bool,
}

/// The rounded square's side as a share of the image (macOS app icon grid: 824 of 1024).
const ROUNDED_SIDE: f32 = 824.0 / 1024.0;
/// The superellipse exponent of the rounded square's corners.
const ROUNDED_EXP: f32 = 5.0;

/// Writes `out`: a PNG at the one size in `sizes`, or an `.ico` / `.icns` holding every size.
pub fn run(spec: &Spec, sizes: &[u32], out: &Path) -> Result<()> {
    let trees = spec
        .layers
        .iter()
        .map(|p| {
            let text = std::fs::read_to_string(p).with_context(|| format!("reading {}", p.display()))?;
            usvg::Tree::from_str(&text, &usvg::Options::default()).with_context(|| format!("parsing {}", p.display()))
        })
        .collect::<Result<Vec<_>>>()?;
    if trees.is_empty() {
        bail!("no layers given");
    }
    let ext = out
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let bytes = match ext.as_str() {
        "png" => {
            let [size] = sizes else {
                bail!("a PNG takes exactly one --size");
            };
            let rgba = draw(&trees, spec, *size);
            if spec.indexed {
                indexed_png_bytes(&rgba, *size)?
            } else {
                png_bytes(&rgba, *size, spec.opaque)?
            }
        }
        "ico" => ico(&trees, spec, sizes)?,
        "icns" => icns(&trees, spec, sizes)?,
        _ => bail!("unknown output type {} (png, ico or icns)", out.display()),
    };
    std::fs::write(out, bytes).with_context(|| format!("writing {}", out.display()))?;
    println!(
        "wrote {} ({})",
        out.display(),
        sizes.iter().map(u32::to_string).collect::<Vec<_>>().join(", ")
    );
    Ok(())
}

/// The icon at `px` × `px`, straight (not premultiplied) RGBA.
fn draw(trees: &[usvg::Tree], spec: &Spec, px: u32) -> Vec<u8> {
    let inner = match spec.shape {
        Shape::Square => px,
        Shape::Rounded => ((px as f32 * ROUNDED_SIDE).round() as u32).max(1),
    };
    let mut pixmap = tiny_skia::Pixmap::new(inner, inner).expect("icon size is not zero");
    let last = trees.len() - 1;
    for (i, tree) in trees.iter().enumerate() {
        let size = tree.size();
        let k = inner as f32 / size.width().max(size.height());
        let s = if i == last { spec.scale } else { 1.0 };
        let off = inner as f32 * (1.0 - s) / 2.0;
        let t = tiny_skia::Transform::from_scale(k * s, k * s).post_translate(off, off);
        resvg::render(tree, t, &mut pixmap.as_mut());
    }
    let mut rgba: Vec<u8> = pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    if spec.shape == Shape::Square {
        return rgba;
    }
    apply_superellipse(&mut rgba, inner);
    let mut out = vec![0u8; (px * px * 4) as usize];
    let off = (px - inner) / 2;
    for y in 0..inner {
        let src = (y * inner * 4) as usize;
        let dst = (((y + off) * px + off) * 4) as usize;
        out[dst..dst + inner as usize * 4].copy_from_slice(&rgba[src..src + inner as usize * 4]);
    }
    out
}

/// Clears everything outside a superellipse filling the `n` × `n` image, anti-aliased by 4 × 4 samples a pixel.
fn apply_superellipse(rgba: &mut [u8], n: u32) {
    let half = n as f32 / 2.0;
    for y in 0..n {
        for x in 0..n {
            let mut inside = 0u32;
            for sy in 0..4 {
                for sx in 0..4 {
                    let u = ((x as f32 + (sx as f32 + 0.5) / 4.0) - half).abs() / half;
                    let v = ((y as f32 + (sy as f32 + 0.5) / 4.0) - half).abs() / half;
                    if u.powf(ROUNDED_EXP) + v.powf(ROUNDED_EXP) <= 1.0 {
                        inside += 1;
                    }
                }
            }
            let a = &mut rgba[((y * n + x) * 4 + 3) as usize];
            *a = (u32::from(*a) * inside / 16) as u8;
        }
    }
}

/// A PNG of the `px` × `px` RGBA image; `opaque` drops the alpha channel (the iOS App Store icon must have none).
fn png_bytes(rgba: &[u8], px: u32, opaque: bool) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, px, px);
        enc.set_depth(png::BitDepth::Eight);
        if opaque {
            enc.set_color(png::ColorType::Rgb);
            let rgb: Vec<u8> = rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
            enc.write_header()?.write_image_data(&rgb)?;
        } else {
            enc.set_color(png::ColorType::Rgba);
            enc.write_header()?.write_image_data(rgba)?;
        }
    }
    Ok(out)
}

/// An opaque indexed PNG of the `px` × `px` RGBA image (alpha dropped), with a palette from [`quantize`].
fn indexed_png_bytes(rgba: &[u8], px: u32) -> Result<Vec<u8>> {
    let rgb: Vec<[u8; 3]> = rgba.chunks_exact(4).map(|p| [p[0], p[1], p[2]]).collect();
    let (palette, indices) = quantize(&rgb, 256);
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, px, px);
        enc.set_color(png::ColorType::Indexed);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_palette(palette.concat());
        enc.write_header()?.write_image_data(&indices)?;
    }
    Ok(out)
}

/// Picks at most `max` colours for `pixels` by median cut (split the box with the widest channel range at its median
/// until there are `max` boxes; each box's colour is its mean) and maps every pixel to its nearest palette entry.
/// Returns the palette and one index per pixel.
fn quantize(pixels: &[[u8; 3]], max: usize) -> (Vec<[u8; 3]>, Vec<u8>) {
    let mut unique = pixels.to_vec();
    unique.sort_unstable();
    unique.dedup();
    let palette: Vec<[u8; 3]> = if unique.len() <= max {
        unique
    } else {
        let mut boxes: Vec<Vec<[u8; 3]>> = vec![pixels.to_vec()];
        while boxes.len() < max {
            let range = |b: &[[u8; 3]], c: usize| {
                let (lo, hi) = b.iter().fold((255, 0), |(lo, hi), p| (p[c].min(lo), p[c].max(hi)));
                hi - lo
            };
            let Some((i, c, r)) = boxes
                .iter()
                .enumerate()
                .flat_map(|(i, b)| (0..3).map(move |c| (i, c, range(b, c))))
                .max_by_key(|&(_, _, r)| r)
            else {
                break;
            };
            if r == 0 {
                break;
            }
            let mut b = boxes.swap_remove(i);
            b.sort_unstable_by_key(|p| p[c]);
            let upper = b.split_off(b.len() / 2);
            boxes.push(b);
            boxes.push(upper);
        }
        boxes
            .iter()
            .map(|b| {
                let n = b.len() as u32;
                let sum = b.iter().fold([0u32; 3], |s, p| {
                    [s[0] + u32::from(p[0]), s[1] + u32::from(p[1]), s[2] + u32::from(p[2])]
                });
                sum.map(|v| ((v + n / 2) / n) as u8)
            })
            .collect()
    };
    let indices = pixels
        .iter()
        .map(|p| {
            let dist = |q: &[u8; 3]| (0..3).map(|c| (i32::from(p[c]) - i32::from(q[c])).pow(2)).sum::<i32>();
            palette
                .iter()
                .enumerate()
                .min_by_key(|(_, q)| dist(q))
                .map_or(0, |(i, _)| i as u8)
        })
        .collect();
    (palette, indices)
}

/// A 32-bit DIB for an ICO entry: header, BGRA rows bottom-up, then the 1-bit AND mask (all zero: the alpha decides).
fn dib(rgba: &[u8], px: u32) -> Vec<u8> {
    let mask_row = (px.div_ceil(32) * 4) as usize;
    let mut out = Vec::with_capacity(40 + rgba.len() + mask_row * px as usize);
    out.extend(40u32.to_le_bytes());
    out.extend((px as i32).to_le_bytes());
    out.extend((2 * px as i32).to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(32u16.to_le_bytes());
    out.extend([0u8; 24]); // no compression, image size, resolution, palette counts: all 0
    for y in (0..px).rev() {
        for x in 0..px {
            let p = &rgba[((y * px + x) * 4) as usize..][..4];
            out.extend([p[2], p[1], p[0], p[3]]);
        }
    }
    out.extend(std::iter::repeat_n(0u8, mask_row * px as usize));
    out
}

fn ico(trees: &[usvg::Tree], spec: &Spec, sizes: &[u32]) -> Result<Vec<u8>> {
    let mut images = Vec::new();
    for &px in sizes {
        if !(1..=256).contains(&px) {
            bail!("ICO sizes are 1 to 256, not {px}");
        }
        let rgba = draw(trees, spec, px);
        images.push(if px == 256 {
            png_bytes(&rgba, px, false)?
        } else {
            dib(&rgba, px)
        });
    }
    let mut out = Vec::new();
    out.extend([0u8, 0, 1, 0]);
    out.extend((sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len();
    for (&px, data) in sizes.iter().zip(&images) {
        let side = if px == 256 { 0 } else { px as u8 };
        out.extend([side, side, 0, 0]);
        out.extend(1u16.to_le_bytes());
        out.extend(32u16.to_le_bytes());
        out.extend((data.len() as u32).to_le_bytes());
        out.extend((offset as u32).to_le_bytes());
        offset += data.len();
    }
    for data in images {
        out.extend(data);
    }
    Ok(out)
}

fn icns(trees: &[usvg::Tree], spec: &Spec, sizes: &[u32]) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    for &px in sizes {
        // Each pixel size goes in under every type that holds it (a 1x and a 2x slot where both exist). The 16 and 32
        // pixel 1x slots hold `ARGB` data, as `iconutil` writes them: its reader garbles PNG data in `icp4`/`icp5`.
        let (argb, png_types): (Option<&[u8; 4]>, &[&[u8; 4]]) = match px {
            16 => (Some(b"ic04"), &[]),
            32 => (Some(b"ic05"), &[b"ic11"]),
            64 => (None, &[b"ic12"]),
            128 => (None, &[b"ic07"]),
            256 => (None, &[b"ic08", b"ic13"]),
            512 => (None, &[b"ic09", b"ic14"]),
            1024 => (None, &[b"ic10"]),
            _ => bail!("ICNS sizes are 16, 32, 64, 128, 256, 512 and 1024, not {px}"),
        };
        let rgba = draw(trees, spec, px);
        let mut push = |ty: &[u8; 4], data: &[u8]| {
            body.extend(ty);
            body.extend(((data.len() + 8) as u32).to_be_bytes());
            body.extend(data);
        };
        if let Some(ty) = argb {
            push(ty, &argb_rle(&rgba));
        }
        if !png_types.is_empty() {
            let data = png_bytes(&rgba, px, false)?;
            for ty in png_types {
                push(ty, &data);
            }
        }
    }
    let mut out = Vec::with_capacity(body.len() + 8);
    out.extend(b"icns");
    out.extend(((body.len() + 8) as u32).to_be_bytes());
    out.extend(body);
    Ok(out)
}

/// An ICNS `ARGB` element's data: the magic, then the alpha, red, green and blue planes, each compressed on its own.
/// A byte under 0x80 is followed by that many plus one literal bytes; a byte from 0x80 up by one byte, repeated that
/// many minus 0x80 plus three times.
fn argb_rle(rgba: &[u8]) -> Vec<u8> {
    fn flush(out: &mut Vec<u8>, literal: &mut Vec<u8>) {
        for chunk in literal.chunks(128) {
            out.push((chunk.len() - 1) as u8);
            out.extend(chunk);
        }
        literal.clear();
    }
    let mut out = b"ARGB".to_vec();
    for channel in [3, 0, 1, 2] {
        let plane: Vec<u8> = rgba.chunks_exact(4).map(|p| p[channel]).collect();
        let mut literal = Vec::new();
        let mut i = 0;
        while i < plane.len() {
            let run = plane[i..].iter().take(130).take_while(|&&b| b == plane[i]).count();
            if run >= 3 {
                flush(&mut out, &mut literal);
                out.extend([0x80 + (run - 3) as u8, plane[i]]);
                i += run;
            } else {
                literal.push(plane[i]);
                i += 1;
            }
        }
        flush(&mut out, &mut literal);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SQUARE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 8 8"><rect width="8" height="8" fill="#ff0000"/></svg>"##;

    fn spec_with(dir: &Path, shape: Shape) -> (Vec<std::path::PathBuf>, Shape) {
        let p = dir.join("a.svg");
        std::fs::write(&p, SQUARE).unwrap();
        (vec![p], shape)
    }

    #[test]
    fn square_fills_and_rounded_leaves_corners_clear() {
        let dir = tempfile::tempdir().unwrap();
        let (layers, _) = spec_with(dir.path(), Shape::Square);
        let trees: Vec<_> = layers
            .iter()
            .map(|p| usvg::Tree::from_str(&std::fs::read_to_string(p).unwrap(), &usvg::Options::default()).unwrap())
            .collect();
        let mut spec = Spec {
            layers: &layers,
            scale: 1.0,
            shape: Shape::Square,
            opaque: false,
            indexed: false,
        };
        let sq = draw(&trees, &spec, 32);
        assert!(sq.chunks_exact(4).all(|p| p == [255, 0, 0, 255]));
        spec.shape = Shape::Rounded;
        let r = draw(&trees, &spec, 32);
        assert_eq!(r[3], 0, "the corner is transparent");
        assert_eq!(&r[(16 * 32 + 16) * 4..][..4], &[255, 0, 0, 255], "the middle is drawn");
    }

    #[test]
    fn ico_and_icns_containers() {
        let dir = tempfile::tempdir().unwrap();
        let (layers, _) = spec_with(dir.path(), Shape::Square);
        let spec = Spec {
            layers: &layers,
            scale: 1.0,
            shape: Shape::Square,
            opaque: false,
            indexed: false,
        };
        let ico_path = dir.path().join("a.ico");
        run(&spec, &[16, 256], &ico_path).unwrap();
        let b = std::fs::read(&ico_path).unwrap();
        assert_eq!(&b[..6], &[0, 0, 1, 0, 2, 0]);
        assert_eq!(b[6], 16);
        assert_eq!(b[22], 0, "256 is written as 0");
        let off = u32::from_le_bytes(b[6 + 16 + 12..6 + 16 + 16].try_into().unwrap()) as usize;
        assert_eq!(&b[off..off + 4], b"\x89PNG");
        let first = u32::from_le_bytes(b[6 + 12..6 + 16].try_into().unwrap()) as usize;
        assert_eq!(
            u32::from_le_bytes(b[first..first + 4].try_into().unwrap()),
            40,
            "a DIB header"
        );

        let icns_path = dir.path().join("a.icns");
        run(&spec, &[16, 32], &icns_path).unwrap();
        let b = std::fs::read(&icns_path).unwrap();
        assert_eq!(&b[..4], b"icns");
        assert_eq!(u32::from_be_bytes(b[4..8].try_into().unwrap()) as usize, b.len());
        assert_eq!(&b[8..12], b"ic04");
        assert_eq!(&b[16..20], b"ARGB");
    }

    #[test]
    fn opaque_png_has_no_alpha() {
        let dir = tempfile::tempdir().unwrap();
        let (layers, _) = spec_with(dir.path(), Shape::Square);
        let spec = Spec {
            layers: &layers,
            scale: 1.0,
            shape: Shape::Square,
            opaque: true,
            indexed: false,
        };
        let out = dir.path().join("a.png");
        run(&spec, &[8], &out).unwrap();
        let dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&out).unwrap()));
        let reader = dec.read_info().unwrap();
        assert_eq!(reader.info().color_type, png::ColorType::Rgb);
    }

    #[test]
    fn indexed_png_has_a_palette_of_at_most_256_colours() {
        let pixels: Vec<[u8; 3]> = (0..4096u32)
            .map(|i| [(i % 64 * 4) as u8, (i / 64 * 4) as u8, 128])
            .collect();
        let (palette, indices) = quantize(&pixels, 256);
        assert_eq!(palette.len(), 256);
        assert_eq!(indices.len(), pixels.len());
        let worst = pixels
            .iter()
            .zip(&indices)
            .map(|(p, &i)| {
                (0..3)
                    .map(|c| i32::from(p[c]).abs_diff(i32::from(palette[usize::from(i)][c])))
                    .max()
                    .unwrap()
            })
            .max()
            .unwrap();
        assert!(worst <= 16, "every pixel is near its palette colour (worst {worst})");

        let few = [[1, 2, 3], [9, 9, 9], [1, 2, 3]];
        let (palette, indices) = quantize(&few, 256);
        assert_eq!(palette, vec![[1, 2, 3], [9, 9, 9]], "few colours are kept exactly");
        assert_eq!(indices, vec![0, 1, 0]);

        let dir = tempfile::tempdir().unwrap();
        let (layers, _) = spec_with(dir.path(), Shape::Square);
        let spec = Spec {
            layers: &layers,
            scale: 1.0,
            shape: Shape::Square,
            opaque: false,
            indexed: true,
        };
        let out = dir.path().join("a.png");
        run(&spec, &[8], &out).unwrap();
        let info = png_info(&out);
        assert_eq!((info.0, info.1), (png::ColorType::Indexed, png::BitDepth::Eight));
    }

    fn png_info(path: &Path) -> (png::ColorType, png::BitDepth) {
        let dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap()));
        let reader = dec.read_info().unwrap();
        (reader.info().color_type, reader.info().bit_depth)
    }

    /// The Vita refuses a VPK whose LiveArea images are not 8-bit indexed PNGs (error 0x8010113D at install).
    #[test]
    fn the_vita_icon_is_an_8_bit_indexed_png() {
        let icon = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../platform/vita/sce_sys/icon0.png");
        assert_eq!(
            png_info(&icon),
            (png::ColorType::Indexed, png::BitDepth::Eight),
            "redraw it with packaging/icon/render.sh"
        );
    }
}
