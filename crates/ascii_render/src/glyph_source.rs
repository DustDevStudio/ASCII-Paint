use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum FontError {
    #[error("Font load error: {0}")]
    Load(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlyphMask {
    pub width: u8,
    pub height: u8,
    pub alpha: Vec<u8>,
}

pub trait GlyphSource: Send + Sync {
    fn font_name(&self) -> &str;
    fn cell_size(&self) -> (u8, u8);
    fn contains(&self, ch: char) -> bool;
    fn mask(&self, ch: char) -> Arc<GlyphMask>;
}

pub struct TtfGlyphSource {
    font: fontdue::Font,
    name: String,
    cell_w: u8,
    cell_h: u8,
    baseline: i32,
    cache: RwLock<HashMap<char, Arc<GlyphMask>>>,
}

const DEFAULT_FONT_BYTES: &[u8] = include_bytes!("../../../assets/fonts/default.ttf");

impl TtfGlyphSource {
    pub const DEFAULT_FONT_PATH: &'static str = "assets/fonts/default.ttf";

    pub fn load_default(cell_w: u8, cell_h: u8) -> Result<TtfGlyphSource, FontError> {
        Self::from_bytes(DEFAULT_FONT_BYTES, "default", cell_w, cell_h)
    }

    pub fn from_bytes(
        bytes: &[u8],
        name: &str,
        cell_w: u8,
        cell_h: u8,
    ) -> Result<TtfGlyphSource, FontError> {
        let settings = fontdue::FontSettings::default();
        let font = fontdue::Font::from_bytes(bytes, settings)
            .map_err(|e| FontError::Load(e.to_string()))?;

        // Fontdue baseline calculation: cell_h is usually ~15px, baseline ~ 12px from top
        let baseline = (cell_h as f32 * 0.8) as i32;

        Ok(TtfGlyphSource {
            font,
            name: name.to_string(),
            cell_w,
            cell_h,
            baseline,
            cache: RwLock::new(HashMap::new()),
        })
    }

    fn rasterize_glyph(&self, ch: char) -> Arc<GlyphMask> {
        // Calculate font size: scale so character fits inside cell_w and cell_h
        let font_size = self.cell_h as f32 * 0.9;
        let (metrics, bitmap) = self.font.rasterize(ch, font_size);

        let w = self.cell_w as usize;
        let h = self.cell_h as usize;
        let mut alpha = vec![0u8; w * h];

        // Center glyph horizontally if smaller, and align to baseline
        let x_offset = if metrics.width < w {
            ((w - metrics.width) / 2) as i32
        } else {
            metrics.xmin
        };
        // In fontdue, ymin is relative to baseline (positive upwards)
        let y_offset = self.baseline - (metrics.height as i32 + metrics.ymin);

        for gy in 0..metrics.height {
            for gx in 0..metrics.width {
                let dest_x = gx as i32 + x_offset;
                let dest_y = gy as i32 + y_offset;
                if dest_x >= 0 && dest_x < w as i32 && dest_y >= 0 && dest_y < h as i32 {
                    let src_idx = gy * metrics.width + gx;
                    let dest_idx = dest_y as usize * w + dest_x as usize;
                    if src_idx < bitmap.len() && dest_idx < alpha.len() {
                        alpha[dest_idx] = bitmap[src_idx];
                    }
                }
            }
        }

        Arc::new(GlyphMask {
            width: self.cell_w,
            height: self.cell_h,
            alpha,
        })
    }
}

impl GlyphSource for TtfGlyphSource {
    fn font_name(&self) -> &str {
        &self.name
    }

    fn cell_size(&self) -> (u8, u8) {
        (self.cell_w, self.cell_h)
    }

    fn contains(&self, ch: char) -> bool {
        self.font.lookup_glyph_index(ch) != 0
    }

    fn mask(&self, ch: char) -> Arc<GlyphMask> {
        let ch_to_lookup = if self.contains(ch) { ch } else { '?' };

        if let Ok(reader) = self.cache.read() {
            if let Some(mask) = reader.get(&ch_to_lookup) {
                return Arc::clone(mask);
            }
        }

        let mask = self.rasterize_glyph(ch_to_lookup);
        if let Ok(mut writer) = self.cache.write() {
            writer.insert(ch_to_lookup, Arc::clone(&mask));
        }
        mask
    }
}
