//! IFF readers: 8SVX sounds and ILBM pictures.

use super::{be16, be32};

/// An 8-bit signed sample, as stored in an 8SVX `BODY`.
#[derive(Clone, Debug, PartialEq)]
pub struct Sound {
    pub rate: u32,
    pub data: Vec<i8>,
    /// Loop start in samples, if the sound repeats.
    pub loop_start: Option<usize>,
    pub volume: f32,
}

#[derive(Clone, Debug)]
pub struct Picture {
    pub w: usize,
    pub h: usize,
    /// Palette indices, w * h.
    pub pixels: Vec<u8>,
    /// 8-bit RGB triples.
    pub palette: Vec<(u8, u8, u8)>,
}

/// (chunk id, chunk body) pairs.
type Chunks<'a> = Vec<(&'a [u8], &'a [u8])>;

fn chunks(d: &[u8]) -> Option<(&[u8], Chunks<'_>)> {
    if d.len() < 12 || &d[0..4] != b"FORM" {
        return None;
    }
    let end = (be32(d, 4) as usize + 8).min(d.len());
    let form = &d[8..12];
    let mut out = Vec::new();
    let mut p = 12;
    while p + 8 <= end {
        let id = &d[p..p + 4];
        let n = be32(d, p + 4) as usize;
        let body = d.get(p + 8..p + 8 + n)?;
        out.push((id, body));
        p += 8 + n + (n & 1);
    }
    Some((form, out))
}

pub fn is_8svx(d: &[u8]) -> bool {
    d.len() >= 12 && &d[0..4] == b"FORM" && &d[8..12] == b"8SVX"
}

pub fn read_8svx(d: &[u8]) -> Result<Sound, String> {
    let (form, chunks) = chunks(d).ok_or("not an IFF file")?;
    if form != b"8SVX" {
        return Err("not an 8SVX sound".into());
    }
    let vhdr = chunks
        .iter()
        .find(|c| c.0 == b"VHDR")
        .ok_or("no VHDR chunk")?
        .1;
    let body = chunks
        .iter()
        .find(|c| c.0 == b"BODY")
        .ok_or("no BODY chunk")?
        .1;
    if vhdr.len() < 20 {
        return Err("short VHDR".into());
    }
    let one_shot = be32(vhdr, 0) as usize;
    let repeat = be32(vhdr, 4) as usize;
    let rate = be16(vhdr, 12) as u32;
    if vhdr[15] != 0 {
        return Err("compressed 8SVX is not supported".into());
    }
    let volume = (be32(vhdr, 16) as f32 / 65536.0).clamp(0.0, 1.0);
    // Only the first octave is used; multi-octave sounds store more after it.
    let len = if one_shot + repeat > 0 {
        (one_shot + repeat).min(body.len())
    } else {
        body.len()
    };
    let data = body[..len].iter().map(|&b| b as i8).collect();
    let loop_start = (repeat > 2).then_some(one_shot);
    Ok(Sound {
        rate: rate.max(1000),
        data,
        loop_start,
        volume,
    })
}

pub fn read_ilbm(d: &[u8]) -> Result<Picture, String> {
    let (form, chunks) = chunks(d).ok_or("not an IFF file")?;
    if form != b"ILBM" {
        return Err("not an ILBM picture".into());
    }
    let bmhd = chunks
        .iter()
        .find(|c| c.0 == b"BMHD")
        .ok_or("no BMHD chunk")?
        .1;
    let body = chunks
        .iter()
        .find(|c| c.0 == b"BODY")
        .ok_or("no BODY chunk")?
        .1;
    let w = be16(bmhd, 0) as usize;
    let h = be16(bmhd, 2) as usize;
    let planes = bmhd[8] as usize;
    let masking = bmhd[9];
    let compression = bmhd[10];
    if planes == 0 || planes > 8 || w == 0 || h == 0 {
        return Err("unsupported bitmap layout".into());
    }
    let palette = chunks
        .iter()
        .find(|c| c.0 == b"CMAP")
        .map(|c| c.1.chunks_exact(3).map(|p| (p[0], p[1], p[2])).collect())
        .unwrap_or_else(|| {
            (0..1u16 << planes)
                .map(|i| {
                    let v = (i * 255 / ((1 << planes) - 1)) as u8;
                    (v, v, v)
                })
                .collect()
        });

    let row_bytes = w.div_ceil(16) * 2;
    let rows_per_line = planes + (masking == 1) as usize;
    let mut raw = Vec::with_capacity(row_bytes * rows_per_line * h);
    match compression {
        0 => raw.extend_from_slice(body),
        1 => {
            // ByteRun1
            let mut i = 0;
            while i < body.len() && raw.len() < row_bytes * rows_per_line * h {
                let n = body[i] as i8;
                i += 1;
                if n >= 0 {
                    let k = n as usize + 1;
                    raw.extend_from_slice(body.get(i..i + k).ok_or("ByteRun1 data ended early")?);
                    i += k;
                } else if n != -128 {
                    let b = *body.get(i).ok_or("ByteRun1 data ended early")?;
                    raw.extend(std::iter::repeat_n(b, (1 - n as isize) as usize));
                    i += 1;
                }
            }
        }
        _ => return Err(format!("unknown ILBM compression {compression}")),
    }
    if raw.len() < row_bytes * rows_per_line * h {
        return Err("BODY is shorter than the picture".into());
    }
    let mut pixels = vec![0u8; w * h];
    for y in 0..h {
        for p in 0..planes {
            let row = &raw[(y * rows_per_line + p) * row_bytes..][..row_bytes];
            for x in 0..w {
                if row[x >> 3] & (0x80 >> (x & 7)) != 0 {
                    pixels[y * w + x] |= 1 << p;
                }
            }
        }
    }
    Ok(Picture {
        w,
        h,
        pixels,
        palette,
    })
}

/// Writes 8-bit unsigned mono WAV, with the loop start in a `smpl` chunk so
/// sample editors keep it.
pub fn sound_to_wav(s: &Sound) -> Vec<u8> {
    let data: Vec<u8> = s.data.iter().map(|&v| (v as i16 + 128) as u8).collect();
    let mut chunks: Vec<(&[u8; 4], Vec<u8>)> = Vec::new();
    let mut fmt = Vec::new();
    fmt.extend_from_slice(&1u16.to_le_bytes());
    fmt.extend_from_slice(&1u16.to_le_bytes());
    fmt.extend_from_slice(&s.rate.to_le_bytes());
    fmt.extend_from_slice(&s.rate.to_le_bytes());
    fmt.extend_from_slice(&1u16.to_le_bytes());
    fmt.extend_from_slice(&8u16.to_le_bytes());
    chunks.push((b"fmt ", fmt));
    chunks.push((b"data", data));
    if let Some(start) = s.loop_start {
        let mut smpl = vec![0u8; 36];
        smpl[28..32].copy_from_slice(&1u32.to_le_bytes());
        let mut lp = vec![0u8; 24];
        lp[8..12].copy_from_slice(&(start as u32).to_le_bytes());
        lp[12..16].copy_from_slice(&(s.data.len() as u32 - 1).to_le_bytes());
        smpl.extend(lp);
        chunks.push((b"smpl", smpl));
    }
    let mut out = b"RIFF\0\0\0\0WAVE".to_vec();
    for (id, body) in chunks {
        out.extend_from_slice(id);
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&body);
        if body.len() & 1 == 1 {
            out.push(0);
        }
    }
    let n = (out.len() - 8) as u32;
    out[4..8].copy_from_slice(&n.to_le_bytes());
    out
}

/// Reads mono or stereo 8/16-bit PCM WAV (stereo is mixed down).
pub fn wav_to_sound(d: &[u8]) -> Result<Sound, String> {
    if d.len() < 12 || &d[0..4] != b"RIFF" || &d[8..12] != b"WAVE" {
        return Err("not a WAV file".into());
    }
    let le16 = |o: usize| u16::from_le_bytes([d[o], d[o + 1]]);
    let le32 = |o: usize| u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]]);
    let (mut fmt, mut data, mut loop_start) = (None, None, None);
    let mut p = 12;
    while p + 8 <= d.len() {
        let n = le32(p + 4) as usize;
        let body = p + 8..(p + 8 + n).min(d.len());
        match &d[p..p + 4] {
            b"fmt " if n >= 16 => {
                fmt = Some((le16(p + 8), le16(p + 10), le32(p + 12), le16(p + 22)))
            }
            b"data" => data = Some(body),
            b"smpl" if n >= 60 && le32(p + 8 + 28) > 0 => {
                loop_start = Some(le32(p + 8 + 36 + 8) as usize)
            }
            _ => {}
        }
        p += 8 + n + (n & 1);
    }
    let (format, channels, rate, bits) = fmt.ok_or("no fmt chunk")?;
    let body = &d[data.ok_or("no data chunk")?];
    if format != 1 || channels == 0 || !(bits == 8 || bits == 16) {
        return Err("only 8- or 16-bit PCM WAV is supported".into());
    }
    let step = channels as usize * (bits as usize / 8);
    let sample = |frame: &[u8]| -> i32 {
        let mut acc = 0i32;
        for c in 0..channels as usize {
            acc += if bits == 8 {
                (frame[c] as i32 - 128) << 8
            } else {
                i16::from_le_bytes([frame[2 * c], frame[2 * c + 1]]) as i32
            };
        }
        acc / channels as i32
    };
    let data: Vec<i8> = body
        .chunks_exact(step)
        .map(|f| (sample(f) >> 8) as i8)
        .collect();
    let loop_start = loop_start.filter(|&s| s + 2 < data.len());
    Ok(Sound {
        rate: rate.max(1000),
        data,
        loop_start,
        volume: 1.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_round_trip() {
        let s = Sound {
            rate: 8363,
            data: (-50..50).map(|v| v as i8).collect(),
            loop_start: Some(20),
            volume: 1.0,
        };
        let back = wav_to_sound(&sound_to_wav(&s)).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn byterun1_picture() {
        // 16x1, 1 plane: run of 2 x 0xff.
        let mut d = b"FORM\0\0\0\0ILBMBMHD\0\0\0\x14".to_vec();
        let mut bmhd = vec![0u8; 20];
        bmhd[1] = 16;
        bmhd[3] = 1;
        bmhd[8] = 1;
        bmhd[10] = 1;
        d.extend(bmhd);
        d.extend(b"BODY\0\0\0\x02\xff\xff");
        let n = (d.len() - 8) as u32;
        d[4..8].copy_from_slice(&n.to_be_bytes());
        let p = read_ilbm(&d).unwrap();
        assert_eq!(p.pixels, vec![1; 16]);
    }
}
