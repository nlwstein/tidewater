//! Storm's IMA ADPCM decoder.
//!
//! Ported from StormLib `src/adpcm/adpcm.cpp` by Ladislav Zezula,
//! <https://github.com/ladislav-zezula/StormLib>, MIT License, Copyright (c) 1999-2013 Ladislav Zezula.
//! Changes: the compressor and the StarCraft beta variant are left out.

use super::{Error, Result};

const INITIAL_STEP_INDEX: usize = 0x2C;

const NEXT_STEP: [i32; 32] = [
    -1, 0, -1, 4, -1, 2, -1, 6, -1, 1, -1, 5, -1, 3, -1, 7, -1, 1, -1, 5, -1, 3, -1, 7, -1, 2, -1, 4, -1, 6, -1, 8,
];

const STEP_SIZE: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66, 73, 80, 88, 97, 107,
    118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449, 494, 544, 598, 658, 724, 796, 876, 963,
    1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272, 2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894,
    6484, 7132, 7845, 8630, 9493, 10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794,
    32767,
];

fn decode_sample(predicted: i32, encoded: u8, step: i32, mut diff: i32) -> i32 {
    for bit in 0..6 {
        if encoded & (1 << bit) != 0 {
            diff += step >> bit;
        }
    }
    if encoded & 0x40 != 0 {
        (predicted - diff).max(-32768)
    } else {
        (predicted + diff).min(32767)
    }
}

/// Decodes `channels` interleaved channels to 16-bit little-endian PCM, producing at most `max_out` bytes.
pub fn decompress(input: &[u8], max_out: usize, channels: usize) -> Result<Vec<u8>> {
    if input.len() < 2 + 2 * channels {
        return Err(Error::compression("adpcm: input too short"));
    }
    let shift = input[1] as u32;
    let mut out = Vec::with_capacity(max_out);
    let mut predicted = [0i32; 2];
    let mut step_index = [INITIAL_STEP_INDEX; 2];
    let mut pos = 2;
    for p in predicted.iter_mut().take(channels) {
        let s = i16::from_le_bytes([input[pos], input[pos + 1]]);
        pos += 2;
        *p = s as i32;
        out.extend_from_slice(&s.to_le_bytes());
    }
    // The next channel, without a division: a CPU with no divide instruction (the Vita's) calls a routine for `%`.
    let next = |ch: usize| if ch + 1 == channels { 0 } else { ch + 1 };
    let mut ch = channels - 1;
    for &encoded in &input[pos..] {
        if out.len() + 2 > max_out {
            break;
        }
        ch = next(ch);
        match encoded {
            0x80 => {
                step_index[ch] = step_index[ch].saturating_sub(1);
                out.extend_from_slice(&(predicted[ch] as i16).to_le_bytes());
            }
            0x81 => {
                step_index[ch] = (step_index[ch] + 8).min(88);
                ch = next(ch);
            }
            _ => {
                let step = STEP_SIZE[step_index[ch]];
                let shifted = if shift < 32 { step >> shift } else { 0 };
                predicted[ch] = decode_sample(predicted[ch], encoded, step, shifted);
                out.extend_from_slice(&(predicted[ch] as i16).to_le_bytes());
                step_index[ch] = (step_index[ch] as i32 + NEXT_STEP[(encoded & 0x1F) as usize]).clamp(0, 88) as usize;
            }
        }
    }
    Ok(out)
}
