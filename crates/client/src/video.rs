//! Video playback helpers, SDL-free: Smacker frames to RGBA, the frame clock, and the audio track gathered into a
//! WAV file the audio player can take. The Smacker reader is `tidewater_formats::smk` (format notes and sources in
//! its module doc). The WAV layout written here is the canonical RIFF/WAVE PCM layout (public: the "Multimedia
//! Programming Interface and Data Specifications 1.0", 1991, "WAVE" chapter).

use std::sync::Arc;

use tidewater_formats::smk::{Frame, Smacker, YScale};

/// The shown height of a video: twice the stored height when its rows are doubled or interlaced.
pub fn shown_height(height: u32, y: YScale) -> u32 {
    match y {
        YScale::None => height,
        YScale::Doubled | YScale::Interlaced => height * 2,
    }
}

/// A frame as RGBA at its shown size: doubled rows repeat, interlaced rows leave a black row between.
pub fn frame_rgba(f: &Frame, width: u32, height: u32, y: YScale) -> Vec<u8> {
    let w = width as usize;
    let out_h = shown_height(height, y) as usize;
    let mut out = vec![0u8; w * out_h * 4];
    for row in 0..height as usize {
        let src = &f.pixels[row * w..(row + 1) * w];
        let mut put = |r: usize| {
            for (x, &i) in src.iter().enumerate() {
                let c = f.palette[i as usize];
                let d = (r * w + x) * 4;
                out[d..d + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
            }
        };
        match y {
            YScale::None => put(row),
            YScale::Doubled => {
                put(row * 2);
                put(row * 2 + 1);
            }
            YScale::Interlaced => {
                put(row * 2);
                // The other row stays black.
                let d = (row * 2 + 1) * w * 4;
                for px in out[d..d + w * 4].chunks_exact_mut(4) {
                    px[3] = 255;
                }
            }
        }
    }
    out
}

/// The frame to show `elapsed_ms` after the start, at `fps` frames a second.
pub fn frame_at(elapsed_ms: f64, fps: f64) -> usize {
    (elapsed_ms.max(0.0) * fps / 1000.0) as usize
}

/// A PCM WAV file of 16-bit samples.
pub fn wav_bytes(samples: &[i16], rate: u32, channels: u16) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut v = Vec::with_capacity(44 + data_len as usize);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&channels.to_le_bytes());
    v.extend_from_slice(&rate.to_le_bytes());
    v.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
    v.extend_from_slice(&(channels * 2).to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        v.extend_from_slice(&s.to_le_bytes());
    }
    v
}

/// A PCM WAV file's length in ms, from its `fmt ` and `data` chunks; `None` when it isn't one.
pub fn wav_length_ms(d: &[u8]) -> Option<u32> {
    if d.len() < 12 || &d[0..4] != b"RIFF" || &d[8..12] != b"WAVE" {
        return None;
    }
    let mut at = 12;
    let mut bytes_per_sec = None;
    while at + 8 <= d.len() {
        let id = &d[at..at + 4];
        let len = u32::from_le_bytes(d[at + 4..at + 8].try_into().ok()?) as usize;
        let body = at + 8;
        if id == b"fmt " && body + 12 <= d.len() {
            bytes_per_sec = Some(u32::from_le_bytes(d[body + 8..body + 12].try_into().ok()?));
        } else if id == b"data" {
            let rate = bytes_per_sec.filter(|&r| r > 0)?;
            let len = len.min(d.len() - body) as u64;
            return Some((len * 1000 / rate as u64) as u32);
        }
        at = body + len + (len & 1);
    }
    None
}

/// The first audio track of a video as a WAV file, decoding the whole video (`None` without a usable track).
pub fn audio_wav(data: Arc<Vec<u8>>) -> Option<Vec<u8>> {
    let mut smk = Smacker::from_shared(data).ok()?;
    let t = *smk.audio_tracks().iter().find(|t| !t.unsupported_codec)?;
    smk.set_decode_audio(true);
    let mut samples = Vec::new();
    while let Ok(Some(f)) = smk.next_frame() {
        for c in f.audio {
            if c.track == t.track {
                samples.extend_from_slice(&c.samples);
            }
        }
    }
    (!samples.is_empty()).then(|| wav_bytes(&samples, t.rate, t.channels as u16))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(pixels: Vec<u8>) -> Frame {
        let mut palette = [[0u8; 3]; 256];
        palette[1] = [10, 20, 30];
        palette[2] = [40, 50, 60];
        Frame {
            index: 0,
            keyframe: true,
            pixels,
            palette,
            audio: Vec::new(),
        }
    }

    #[test]
    fn rows_are_scaled() {
        let f = frame(vec![1, 2]);
        assert_eq!(frame_rgba(&f, 2, 1, YScale::None), [10, 20, 30, 255, 40, 50, 60, 255]);
        let d = frame_rgba(&f, 2, 1, YScale::Doubled);
        assert_eq!(d.len(), 16);
        assert_eq!(d[8..], d[..8]);
        let i = frame_rgba(&f, 2, 1, YScale::Interlaced);
        assert_eq!(i[8..], [0, 0, 0, 255, 0, 0, 0, 255]);
        assert_eq!(shown_height(144, YScale::Doubled), 288);
    }

    #[test]
    fn the_frame_clock() {
        assert_eq!(frame_at(0.0, 12.0), 0);
        assert_eq!(frame_at(1000.0, 12.0), 12);
        assert_eq!(frame_at(83.0, 12.0), 0);
        assert_eq!(frame_at(84.0, 12.0), 1);
    }

    #[test]
    fn wav_round_trip() {
        let w = wav_bytes(&[0; 22050 * 2], 22050, 2);
        assert_eq!(w.len(), 44 + 22050 * 4);
        assert_eq!(wav_length_ms(&w), Some(1000));
        assert_eq!(wav_length_ms(b"nope"), None);
    }
}
