use ascii_core::compose::FlatFrame;
use ascii_core::model::Rgba;
use crate::rasterizer::RgbaBuffer;

pub struct ThumbnailBuilder;

impl ThumbnailBuilder {
    pub fn build(flat: &FlatFrame) -> RgbaBuffer {
        let cols = flat.grid.cols() as u32;
        let rows = flat.grid.rows() as u32;
        let mut buf = RgbaBuffer::new(cols, rows);

        for (coord, cell) in flat.grid.iter() {
            let base = flat.page_color(coord);
            let [r, g, b] = if cell.glyph != ' ' {
                let ink_opaque = cell.ink.over(Rgba::opaque(base)).rgb();
                let r = ((base.r as u32 * 4 + ink_opaque.r as u32 * 6 + 5) / 10) as u8;
                let g = ((base.g as u32 * 4 + ink_opaque.g as u32 * 6 + 5) / 10) as u8;
                let b = ((base.b as u32 * 4 + ink_opaque.b as u32 * 6 + 5) / 10) as u8;
                [r, g, b]
            } else {
                [base.r, base.g, base.b]
            };

            buf.set_pixel(coord.col as u32, coord.row as u32, [r, g, b, 255]);
        }

        buf
    }
}
