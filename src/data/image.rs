//! Indexed-colour PNG reading and writing for the editable assets.

use super::palette::Rgb;

pub struct Indexed {
    pub w: usize,
    pub h: usize,
    pub pixels: Vec<u8>,
    /// The PNG's own palette, if it had one.
    pub palette: Option<Vec<Rgb>>,
}

/// Writes an 8-bit indexed PNG. With `transparent_zero`, index 0 is marked
/// fully transparent so sprite sheets show their backgrounds as clear.
pub fn write_indexed(
    w: usize,
    h: usize,
    palette: &[Rgb],
    transparent_zero: bool,
    pixels: &[u8],
) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w as u32, h as u32);
        enc.set_color(png::ColorType::Indexed);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_palette(
            palette
                .iter()
                .flat_map(|c| [c.0, c.1, c.2])
                .collect::<Vec<u8>>(),
        );
        if transparent_zero {
            enc.set_trns(vec![0u8]);
        }
        let mut wr = enc.write_header().map_err(|e| e.to_string())?;
        wr.write_image_data(pixels).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// Reads a PNG as palette indices. Indexed images keep their indices; images
/// an editor has converted to RGB(A) are mapped to the nearest colour in
/// `fallback`, with fully transparent pixels becoming index 0.
pub fn read_indexed(bytes: &[u8], fallback: &[Rgb]) -> Result<Indexed, String> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::STRIP_16);
    let mut reader = dec.read_info().map_err(|e| e.to_string())?;
    let info = reader.info();
    let (w, h) = (info.width as usize, info.height as usize);
    let own_palette = info.palette.as_ref().map(|p| {
        p.chunks_exact(3)
            .map(|c| Rgb(c[0], c[1], c[2]))
            .collect::<Vec<_>>()
    });
    let mut buf = vec![0; reader.output_buffer_size().ok_or("image too large")?];
    let frame = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let line = frame.line_size;
    let mut pixels = Vec::with_capacity(w * h);

    match frame.color_type {
        png::ColorType::Indexed => {
            let bits = frame.bit_depth as usize;
            for y in 0..h {
                let row = &buf[y * line..(y + 1) * line];
                for x in 0..w {
                    let bit = x * bits;
                    let byte = row[bit / 8];
                    let shift = 8 - bits - (bit % 8);
                    pixels.push((byte >> shift) & ((1u16 << bits) - 1) as u8);
                }
            }
            return Ok(Indexed {
                w,
                h,
                pixels,
                palette: own_palette,
            });
        }
        png::ColorType::Rgb
        | png::ColorType::Rgba
        | png::ColorType::Grayscale
        | png::ColorType::GrayscaleAlpha => {
            if frame.bit_depth != png::BitDepth::Eight {
                return Err("unsupported bit depth; save as 8-bit".into());
            }
            let ch = frame.color_type.samples();
            for y in 0..h {
                let row = &buf[y * line..(y + 1) * line];
                for x in 0..w {
                    let p = &row[x * ch..(x + 1) * ch];
                    let (c, alpha) = match ch {
                        1 => (Rgb(p[0], p[0], p[0]), 255),
                        2 => (Rgb(p[0], p[0], p[0]), p[1]),
                        3 => (Rgb(p[0], p[1], p[2]), 255),
                        _ => (Rgb(p[0], p[1], p[2]), p[3]),
                    };
                    pixels.push(if alpha < 128 { 0 } else { nearest(c, fallback) });
                }
            }
        }
    }
    Ok(Indexed {
        w,
        h,
        pixels,
        palette: None,
    })
}

fn nearest(c: Rgb, pal: &[Rgb]) -> u8 {
    let d = |p: &Rgb| {
        let (dr, dg, db) = (
            c.0 as i32 - p.0 as i32,
            c.1 as i32 - p.1 as i32,
            c.2 as i32 - p.2 as i32,
        );
        dr * dr * 3 + dg * dg * 4 + db * db * 2
    };
    (0..pal.len()).min_by_key(|&i| d(&pal[i])).unwrap_or(0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexed_round_trip() {
        let pal: Vec<Rgb> = (0..16).map(|i| Rgb(i * 16, i * 8, 255 - i * 16)).collect();
        let px: Vec<u8> = (0..64u32).map(|i| (i * 7 % 16) as u8).collect();
        let png = write_indexed(8, 8, &pal, false, &px).unwrap();
        let back = read_indexed(&png, &pal).unwrap();
        assert_eq!((back.w, back.h), (8, 8));
        assert_eq!(back.pixels, px);
        assert_eq!(back.palette.unwrap(), pal);
    }
}
