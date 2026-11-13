//! Bounded WAV loading on the control thread. No audio callback filesystem work.
use super::playback::Source;
use std::{io::Read, path::Path};
pub(super) fn read(path: &Path) -> Result<Source, String> {
    read_limit(path,8_388_608)
}
pub(crate) fn read_timeline(path:&Path)->Result<Source,String>{read_limit(path,67_108_864+65_536)}
fn read_limit(path:&Path,limit:usize)->Result<Source,String>{
    let mut b = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(limit as u64+1)
        .read_to_end(&mut b)
        .map_err(|e| e.to_string())?;
    decode_limit(&b,limit)
}
fn decode(b: &[u8]) -> Result<Source, String> {decode_limit(b,8_388_608)}
fn decode_limit(b:&[u8],limit:usize)->Result<Source,String>{
    if b.len() > limit || b.len() < 12 || &b[..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return Err("WAV file exceeds the selected loader budget or has an invalid header".into());
    }
    let u16le = |p: usize| u16::from_le_bytes(b[p..p + 2].try_into().unwrap());
    let u32le = |p: usize| u32::from_le_bytes(b[p..p + 4].try_into().unwrap());
    let declared = u32le(4);
    let end = if declared == u32::MAX { b.len() } else { declared as usize + 8 };
    if end > b.len() || end < 12 {
        return Err("truncated WAV".into());
    }
    let (mut format, mut data) = (None, None);
    let mut p = 12;
    while p + 8 <= end {
        let declared = u32le(p + 4);
        let start = p + 8;
        let size = if &b[p..p + 4] == b"data" && declared == u32::MAX {
            end - start
        } else { declared as usize };
        let finish = start
            .checked_add(size)
            .filter(|n| *n <= end)
            .ok_or("truncated WAV chunk")?;
        match &b[p..p + 4] {
            b"fmt " => {
                if size < 16 {
                    return Err("short WAV format".into());
                }
                format = Some((
                    u16le(start),
                    u16le(start + 2) as usize,
                    u32le(start + 4),
                    u16le(start + 12) as usize,
                    u16le(start + 14) as usize,
                ));
            }
            b"data" => data = Some(&b[start..finish]),
            _ => {}
        }
        p = finish + size % 2;
    }
    let (encoding, channels, sample_rate, align, bits) = format.ok_or("WAV format missing")?;
    let data = data.ok_or("WAV data missing")?;
    if !(1..=2).contains(&channels)
        || !(8000..=192000).contains(&sample_rate)
        || !matches!((encoding, bits), (1, 8 | 16 | 24 | 32) | (3, 32))
        || align != channels * (bits / 8)
        || data.is_empty()
        || data.len() % align != 0
    {
        return Err("WAV needs mono/stereo PCM8/16/24/32 or float32 at 8–192 kHz".into());
    }
    if data.len()/(bits/8)>16_777_216{return Err("decoded source memory capacity reached".into());}
    let samples: Result<Vec<f32>, String> = data
        .chunks_exact(bits / 8)
        .map(|v| {
            let x = match (encoding, bits) {
                (1, 8) => (v[0] as f32 - 128.) / 128.,
                (1, 16) => i16::from_le_bytes(v.try_into().unwrap()) as f32 / 32768.,
                (1, 24) => {
                    ((v[0] as i32 | ((v[1] as i32) << 8) | ((v[2] as i32) << 16)) << 8 >> 8) as f32
                        / 8388608.
                }
                (1, 32) => i32::from_le_bytes(v.try_into().unwrap()) as f32 / 2147483648.,
                _ => f32::from_le_bytes(v.try_into().unwrap()),
            };
            if x.is_finite() && x.abs() <= 64. {
                Ok(x)
            } else {
                Err("non-finite/out-of-range WAV sample".into())
            }
        })
        .collect();
    Ok(Source {
        sample_rate,
        channels,
        samples: samples?.into(),
    })
}
#[cfg(test)]
mod drum_rack_sample_tests {
    use super::*;
    fn wav(format: u16, bits: u16, data: &[u8]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend(b"RIFF");
        b.extend((36 + data.len() as u32).to_le_bytes());
        b.extend(b"WAVEfmt ");
        b.extend(16u32.to_le_bytes());
        b.extend(format.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(8000u32.to_le_bytes());
        b.extend((8000u32 * (bits as u32 / 8)).to_le_bytes());
        b.extend((bits / 8).to_le_bytes());
        b.extend(bits.to_le_bytes());
        b.extend(b"data");
        b.extend((data.len() as u32).to_le_bytes());
        b.extend(data);
        b
    }
    #[test]
    fn signed_pcm24_extremes_decode_without_sign_errors() {
        let b = wav(1, 24, &[0, 0, 128, 255, 255, 127]);
        let s = decode(&b).unwrap();
        assert_eq!(s.samples[0], -1.);
        assert!(s.samples[1] > 0.999);
    }
    #[test]
    fn rejects_truncated_chunks_and_nonfinite_float() {
        let mut b = wav(1, 16, &[0, 1]);
        b.pop();
        assert!(decode(&b).is_err());
        assert!(decode(&wav(3, 32, &f32::NAN.to_le_bytes())).is_err());
    }
}
