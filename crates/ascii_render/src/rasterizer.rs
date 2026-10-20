use ascii_core::compose::FlatFrame;
use ascii_core::model::CanvasSpec;
use crate::glyph_source::GlyphSource;

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RgbaBuffer {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl RgbaBuffer {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; (width * height * 4) as usize],
        }
    }

    pub fn filled(width: u32, height: u32, rgba: [u8; 4]) -> Self {
        let count = (width * height) as usize;
        let mut pixels = Vec::with_capacity(count * 4);
        for _ in 0..count {
            pixels.extend_from_slice(&rgba);
        }
        Self {
            width,
            height,
            pixels,
        }
    }

    pub fn set_pixel(&mut self, x: u32, y: u32, rgba: [u8; 4]) {
        if x < self.width && y < self.height {
            let idx = ((y * self.width + x) * 4) as usize;
            self.pixels[idx..idx + 4].copy_from_slice(&rgba);
        }
    }

    pub fn get_pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x < self.width && y < self.height {
            let idx = ((y * self.width + x) * 4) as usize;
            Some([
                self.pixels[idx],
                self.pixels[idx + 1],
                self.pixels[idx + 2],
                self.pixels[idx + 3],
            ])
        } else {
            None
        }
    }
}

pub struct FrameRasterizer;

impl FrameRasterizer {
    pub fn render(flat: &FlatFrame, canvas: &CanvasSpec, glyphs: &dyn GlyphSource) -> RgbaBuffer {
        let cell_w = canvas.cell_w as u32;
        let cell_h = canvas.cell_h as u32;
        let width = canvas.cols as u32 * cell_w;
        let height = canvas.rows as u32 * cell_h;

        let bg_rgba = [
            flat.background.r,
            flat.background.g,
            flat.background.b,
            255,
        ];
        let mut buf = RgbaBuffer::filled(width, height, bg_rgba);

        for (coord, cell) in flat.grid.iter() {
            let x0 = coord.col as u32 * cell_w;
            let y0 = coord.row as u32 * cell_h;

            if cell.fill.a > 0 {
                let base = flat.page_color(coord);
                let cell_bg_rgba = [base.r, base.g, base.b, 255];
                for dy in 0..cell_h {
                    for dx in 0..cell_w {
                        buf.set_pixel(x0 + dx, y0 + dy, cell_bg_rgba);
                    }
                }
            }

            if cell.glyph != ' ' {
                let mask = glyphs.mask(cell.glyph);
                let ink = cell.ink;
                for my in 0..cell_h {
                    for mx in 0..cell_w {
                        let mask_idx = (my * cell_w + mx) as usize;
                        if mask_idx < mask.alpha.len() {
                            let m = mask.alpha[mask_idx] as u32;
                            if m > 0 {
                                let a = (m * ink.a as u32 + 127) / 255;
                                if a > 0 {
                                    let px_x = x0 + mx;
                                    let px_y = y0 + my;
                                    if px_x < width && px_y < height {
                                        let idx = ((px_y * width + px_x) * 4) as usize;
                                        let pr = buf.pixels[idx] as u32;
                                        let pg = buf.pixels[idx + 1] as u32;
                                        let pb = buf.pixels[idx + 2] as u32;

                                        let out_r = ((pr * (255 - a) + ink.r as u32 * a + 127) / 255) as u8;
                                        let out_g = ((pg * (255 - a) + ink.g as u32 * a + 127) / 255) as u8;
                                        let out_b = ((pb * (255 - a) + ink.b as u32 * a + 127) / 255) as u8;

                                        buf.pixels[idx] = out_r;
                                        buf.pixels[idx + 1] = out_g;
                                        buf.pixels[idx + 2] = out_b;
                                        buf.pixels[idx + 3] = 255;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        buf
    }
}
