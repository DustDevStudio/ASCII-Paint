use crate::model::{CanvasSpec, CellCoord, Frame, Grid, Rgb, Rgba};

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct FlatCell {
    pub glyph: char,
    pub ink: Rgba,
    pub fill: Rgba,
}

impl FlatCell {
    pub const EMPTY: FlatCell = FlatCell {
        glyph: ' ',
        ink: Rgba::TRANSPARENT,
        fill: Rgba::TRANSPARENT,
    };
}

impl Default for FlatCell {
    fn default() -> Self {
        Self::EMPTY
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlatFrame {
    pub background: Rgb,
    pub grid: Grid<FlatCell>,
}

impl FlatFrame {
    pub fn page_color(&self, c: CellCoord) -> Rgb {
        let cell = self.grid.get(c).copied().unwrap_or(FlatCell::EMPTY);
        cell.fill.over(Rgba::opaque(self.background)).rgb()
    }
}

pub struct Compositor;

impl Compositor {
    pub fn flatten(frame: &Frame, canvas: &CanvasSpec) -> FlatFrame {
        let mut grid = Grid::filled(canvas, FlatCell::EMPTY);
        for row in 0..canvas.rows {
            for col in 0..canvas.cols {
                let c = CellCoord::new(col, row);
                let flat_cell = Self::flatten_cell(frame, c);
                grid.set(c, flat_cell);
            }
        }

        FlatFrame {
            background: frame.background,
            grid,
        }
    }

    pub fn flatten_cell(frame: &Frame, c: CellCoord) -> FlatCell {
        let mut glyph = ' ';
        let mut ink = Rgba::TRANSPARENT;
        let mut fill = Rgba::TRANSPARENT;

        for layer in frame.layers() {
            if !layer.visible {
                continue;
            }
            let k = layer.opacity;
            if let Some(fc) = layer.fill.get(c) {
                let f = fc.scaled_alpha(k);
                fill = f.over(fill);
            }

            if let Some(ic) = layer.ink.get(c) {
                let a = ic.color.scaled_alpha(k);
                if ic.glyph != ' ' && a.a > 0 {
                    glyph = ic.glyph;
                    ink = a.over(ink);
                }
            }
        }

        FlatCell { glyph, ink, fill }
    }
}
