pub mod glyph_source;
pub mod rasterizer;
pub mod thumbnail;

pub use glyph_source::{FontError, GlyphMask, GlyphSource, TtfGlyphSource};
pub use rasterizer::{FrameRasterizer, RgbaBuffer};
pub use thumbnail::ThumbnailBuilder;

#[cfg(test)]
mod tests {
    use super::*;
    use ascii_core::model::{CanvasSpec, Cell, CellCoord, Frame, FrameId, Rgb};

    #[test]
    fn test_ttf_glyph_source_loading_and_mask() {
        let source = TtfGlyphSource::load_default(8, 15).expect("default font must load");
        assert_eq!(source.cell_size(), (8, 15));
        assert!(source.contains('A'));
        assert!(source.contains(' '));

        let mask = source.mask('A');
        assert_eq!(mask.width, 8);
        assert_eq!(mask.height, 15);
        assert_eq!(mask.alpha.len(), 8 * 15);
        // Alpha for 'A' should have non-zero pixels
        assert!(mask.alpha.iter().any(|&a| a > 0));
    }

    #[test]
    fn test_frame_rasterizer() {
        let spec = CanvasSpec {
            cols: 2,
            rows: 1,
            cell_w: 8,
            cell_h: 15,
        };
        let mut frame = Frame::blank(FrameId(1), &spec, Rgb::BLACK);
        frame.set_cell(
            CellCoord::new(0, 0),
            Cell {
                glyph: 'H',
                fg: Rgb::WHITE,
                bg: None,
            },
        );
        frame.set_cell(
            CellCoord::new(1, 0),
            Cell {
                glyph: 'i',
                fg: Rgb::new(255, 0, 0),
                bg: Some(Rgb::new(0, 0, 255)),
            },
        );

        let glyphs = TtfGlyphSource::load_default(8, 15).unwrap();
        let buf = FrameRasterizer::render(&frame, &spec, &glyphs);
        assert_eq!(buf.width, 16);
        assert_eq!(buf.height, 15);
        assert_eq!(buf.pixels.len(), 16 * 15 * 4);

        // Cell 1 has blue background: check a pixel in cell 1 that is not covered by 'i'
        let p = buf.get_pixel(8, 0).unwrap();
        assert_eq!(p[2], 255); // Blue
        assert_eq!(p[3], 255); // Alpha
    }

    #[test]
    fn test_thumbnail_builder() {
        let spec = CanvasSpec {
            cols: 2,
            rows: 1,
            cell_w: 8,
            cell_h: 15,
        };
        let frame = Frame::blank(FrameId(1), &spec, Rgb::new(10, 20, 30));
        let thumb = ThumbnailBuilder::build(&frame);
        assert_eq!(thumb.width, 2);
        assert_eq!(thumb.height, 1);
        assert_eq!(thumb.get_pixel(0, 0), Some([10, 20, 30, 255]));
    }
}
