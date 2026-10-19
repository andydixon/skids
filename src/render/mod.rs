//! Software rendering into a 0RGB framebuffer at the Amiga's PAL resolution.

pub mod font;

use crate::data::track;
use crate::data::vehicle::{SPR_H, SPR_W};

pub const SCREEN_W: usize = 320;
pub const SCREEN_H: usize = 256;

#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }
}

pub struct Frame {
    pub buf: Vec<u32>,
}

impl Frame {
    pub fn new() -> Frame {
        Frame {
            buf: vec![0; SCREEN_W * SCREEN_H],
        }
    }

    pub fn clear(&mut self, c: u32) {
        self.buf.fill(c);
    }

    #[inline]
    pub fn put(&mut self, x: i32, y: i32, c: u32) {
        if x >= 0 && y >= 0 && (x as usize) < SCREEN_W && (y as usize) < SCREEN_H {
            self.buf[y as usize * SCREEN_W + x as usize] = c;
        }
    }

    pub fn fill(&mut self, r: Rect, c: u32) {
        let x0 = r.x.max(0) as usize;
        let y0 = r.y.max(0) as usize;
        let x1 = ((r.x + r.w).max(0) as usize).min(SCREEN_W);
        let y1 = ((r.y + r.h).max(0) as usize).min(SCREEN_H);
        for y in y0..y1 {
            self.buf[y * SCREEN_W + x0..y * SCREEN_W + x1.max(x0)].fill(c);
        }
    }

    /// Darkens a region, for menus drawn over a running race.
    pub fn shade(&mut self, r: Rect, keep: u32) {
        let x0 = r.x.max(0) as usize;
        let y0 = r.y.max(0) as usize;
        let x1 = ((r.x + r.w).max(0) as usize).min(SCREEN_W);
        let y1 = ((r.y + r.h).max(0) as usize).min(SCREEN_H);
        for y in y0..y1 {
            for p in &mut self.buf[y * SCREEN_W + x0..y * SCREEN_W + x1.max(x0)] {
                let ch = |s: u32| ((((*p >> s) & 0xff) * keep) >> 8) << s;
                *p = ch(16) | ch(8) | ch(0);
            }
        }
    }

    pub fn frame_rect(&mut self, r: Rect, c: u32) {
        self.fill(Rect::new(r.x, r.y, r.w, 1), c);
        self.fill(Rect::new(r.x, r.y + r.h - 1, r.w, 1), c);
        self.fill(Rect::new(r.x, r.y, 1, r.h), c);
        self.fill(Rect::new(r.x + r.w - 1, r.y, 1, r.h), c);
    }

    /// Copies part of a track-sized image into `view`, with (cx, cy) the
    /// track pixel at the view's top-left corner.
    pub fn blit_track(&mut self, ground: &[u32], view: Rect, cx: i32, cy: i32) {
        for row in 0..view.h {
            let sy = view.y + row;
            let ty = cy + row;
            if sy < 0 || sy as usize >= SCREEN_H {
                continue;
            }
            for col in 0..view.w {
                let sx = view.x + col;
                let tx = cx + col;
                if sx < 0 || sx as usize >= SCREEN_W {
                    continue;
                }
                let c =
                    if tx >= 0 && ty >= 0 && (tx as usize) < track::W && (ty as usize) < track::H {
                        ground[ty as usize * track::W + tx as usize]
                    } else {
                        0x101010
                    };
                self.buf[sy as usize * SCREEN_W + sx as usize] = c;
            }
        }
    }

    /// Draws a 64x32 indexed sprite (index 0 transparent) clipped to `clip`.
    pub fn sprite(&mut self, pix: &[u8], pal: &[u32; 4], x: i32, y: i32, clip: Rect) {
        for row in 0..SPR_H as i32 {
            let sy = y + row;
            if sy < clip.y || sy >= clip.y + clip.h || sy < 0 || sy as usize >= SCREEN_H {
                continue;
            }
            for col in 0..SPR_W as i32 {
                let i = pix[(row * SPR_W as i32 + col) as usize];
                if i == 0 {
                    continue;
                }
                let sx = x + col;
                if sx < clip.x || sx >= clip.x + clip.w || sx < 0 || sx as usize >= SCREEN_W {
                    continue;
                }
                self.buf[sy as usize * SCREEN_W + sx as usize] = pal[i as usize];
            }
        }
    }

    /// Soft drop shadow under a sprite: darkens what is already drawn.
    pub fn sprite_shadow(&mut self, pix: &[u8], x: i32, y: i32, clip: Rect) {
        for row in 0..SPR_H as i32 {
            let sy = y + row;
            if sy < clip.y || sy >= clip.y + clip.h || sy < 0 || sy as usize >= SCREEN_H {
                continue;
            }
            for col in 0..SPR_W as i32 {
                if pix[(row * SPR_W as i32 + col) as usize] == 0 {
                    continue;
                }
                let sx = x + col;
                if sx < clip.x || sx >= clip.x + clip.w || sx < 0 || sx as usize >= SCREEN_W {
                    continue;
                }
                let p = &mut self.buf[sy as usize * SCREEN_W + sx as usize];
                *p = (*p >> 1) & 0x7f7f7f;
            }
        }
    }

    pub fn text(&mut self, x: i32, y: i32, s: &str, c: u32, scale: usize) {
        let mut cx = x;
        for ch in s.chars() {
            let g = font::glyph(ch);
            for (row, bits) in g.iter().enumerate() {
                for col in 0..font::GLYPH_W {
                    if bits & (0x10 >> col) != 0 {
                        for dy in 0..scale {
                            for dx in 0..scale {
                                self.put(
                                    cx + (col * scale + dx) as i32,
                                    y + (row * scale + dy) as i32,
                                    c,
                                );
                            }
                        }
                    }
                }
            }
            cx += (font::ADVANCE * scale) as i32;
        }
    }

    /// Text with a one-pixel drop shadow so it reads over the track.
    pub fn text_shadow(&mut self, x: i32, y: i32, s: &str, c: u32, scale: usize) {
        self.text(x + scale as i32, y + scale as i32, s, 0x000000, scale);
        self.text(x, y, s, c, scale);
    }

    pub fn text_centered(&mut self, cx: i32, y: i32, s: &str, c: u32, scale: usize) {
        let w = font::text_width(s, scale) as i32;
        self.text_shadow(cx - w / 2, y, s, c, scale);
    }

    pub fn image(&mut self, src: &[u32], w: usize, h: usize, x: i32, y: i32) {
        for row in 0..h {
            for col in 0..w {
                self.put(x + col as i32, y + row as i32, src[row * w + col]);
            }
        }
    }
}

/// Box-filtered downscale of a track-sized image.
pub fn downscale(src: &[u32], w: usize, h: usize) -> Vec<u32> {
    let mut out = vec![0; w * h];
    let (fx, fy) = (track::W / w, track::H / h);
    for y in 0..h {
        for x in 0..w {
            let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
            for yy in y * fy..(y + 1) * fy {
                for xx in x * fx..(x + 1) * fx {
                    let p = src[yy * track::W + xx];
                    r += (p >> 16) & 0xff;
                    g += (p >> 8) & 0xff;
                    b += p & 0xff;
                }
            }
            let n = (fx * fy) as u32;
            out[y * w + x] = ((r / n) << 16) | ((g / n) << 8) | (b / n);
        }
    }
    out
}

pub fn save_png(path: &std::path::Path, w: usize, h: usize, px: &[u32]) -> std::io::Result<()> {
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut enc = png::Encoder::new(file, w as u32, h as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut wr = enc.write_header().map_err(std::io::Error::other)?;
    let bytes: Vec<u8> = px
        .iter()
        .flat_map(|&p| [(p >> 16) as u8, (p >> 8) as u8, p as u8])
        .collect();
    wr.write_image_data(&bytes).map_err(std::io::Error::other)
}

impl Default for Frame {
    fn default() -> Self {
        Self::new()
    }
}
