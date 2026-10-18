use ascii_core::model::Frame;
use crate::rasterizer::RgbaBuffer;

pub struct ThumbnailBuilder;

impl ThumbnailBuilder {
    pub fn build(frame: &Frame) -> RgbaBuffer {
        let cols = frame.grid.cols() as u32;
        let rows = frame.grid.rows() as u32;
        let mut buf = RgbaBuffer::new(cols, rows);

        for (coord, cell) in frame.grid.iter() {
            let base = cell.bg.unwrap_or(frame.background);
            let [r, g, b] = if cell.glyph != ' ' {
                // lerp(base, cell.fg, 0.6)
                let r = (base.r as f32 * 0.4 + cell.fg.r as f32 * 0.6).round() as u8;
                let g = (base.g as f32 * 0.4 + cell.fg.g as f32 * 0.6).round() as u8;
                let b = (base.b as f32 * 0.4 + cell.fg.b as f32 * 0.6).round() as u8;
                [r, g, b]
            } else {
                [base.r, base.g, base.b]
            };

            buf.set_pixel(coord.col as u32, coord.row as u32, [r, g, b, 255]);
        }

        buf
    }
}
