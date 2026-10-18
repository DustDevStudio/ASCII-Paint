use std::collections::HashMap;
use std::sync::Arc;
use ascii_render::GlyphSource;

pub struct GlyphAtlas {
    src: Arc<dyn GlyphSource>,
    texture: Option<egui::TextureHandle>,
    slots: HashMap<char, usize>,
    grid_cols: usize,
    grid_rows: usize,
    cell_w: usize,
    cell_h: usize,
    pixel_data: Vec<u8>,
}

impl GlyphAtlas {
    pub fn new(ctx: &egui::Context, src: Arc<dyn GlyphSource>) -> Self {
        let (cw, ch) = src.cell_size();
        let cell_w = cw as usize;
        let cell_h = ch as usize;
        let grid_cols = 16;
        let grid_rows = 16;
        let total_w = grid_cols * cell_w;
        let total_h = grid_rows * cell_h;

        let mut atlas = Self {
            src,
            texture: None,
            slots: HashMap::new(),
            grid_cols,
            grid_rows,
            cell_w,
            cell_h,
            pixel_data: vec![0u8; total_w * total_h * 4],
        };

        // Prepopulate ASCII 32..=126
        for ch in ' '..='~' {
            atlas.get_or_insert_slot(ctx, ch);
        }

        atlas.upload_texture(ctx);
        atlas
    }

    pub fn texture_id(&mut self, ctx: &egui::Context) -> egui::TextureId {
        if self.texture.is_none() {
            self.upload_texture(ctx);
        }
        self.texture.as_ref().unwrap().id()
    }

    pub fn uv(&mut self, ctx: &egui::Context, ch: char) -> egui::Rect {
        let slot = self.get_or_insert_slot(ctx, ch);
        let col = slot % self.grid_cols;
        let row = slot / self.grid_cols;

        let total_w = (self.grid_cols * self.cell_w) as f32;
        let total_h = (self.grid_rows * self.cell_h) as f32;

        let u0 = (col * self.cell_w) as f32 / total_w;
        let v0 = (row * self.cell_h) as f32 / total_h;
        let u1 = ((col + 1) * self.cell_w) as f32 / total_w;
        let v1 = ((row + 1) * self.cell_h) as f32 / total_h;

        egui::Rect::from_min_max(egui::pos2(u0, v0), egui::pos2(u1, v1))
    }

    fn get_or_insert_slot(&mut self, ctx: &egui::Context, ch: char) -> usize {
        if let Some(&slot) = self.slots.get(&ch) {
            return slot;
        }

        let slot = self.slots.len();
        if slot >= self.grid_cols * self.grid_rows {
            // Atlas full, return slot for '?' or first slot
            return *self.slots.get(&'?').unwrap_or(&0);
        }

        let mask = self.src.mask(ch);
        let col = slot % self.grid_cols;
        let row = slot / self.grid_cols;
        let total_w = self.grid_cols * self.cell_w;

        for my in 0..self.cell_h {
            for mx in 0..self.cell_w {
                let mask_idx = my * self.cell_w + mx;
                let alpha = if mask_idx < mask.alpha.len() {
                    mask.alpha[mask_idx]
                } else {
                    0
                };

                let dest_x = col * self.cell_w + mx;
                let dest_y = row * self.cell_h + my;
                let dest_idx = (dest_y * total_w + dest_x) * 4;

                // Store white with alpha
                self.pixel_data[dest_idx] = 255;
                self.pixel_data[dest_idx + 1] = 255;
                self.pixel_data[dest_idx + 2] = 255;
                self.pixel_data[dest_idx + 3] = alpha;
            }
        }

        self.slots.insert(ch, slot);
        self.upload_texture(ctx);
        slot
    }

    fn upload_texture(&mut self, ctx: &egui::Context) {
        let total_w = self.grid_cols * self.cell_w;
        let total_h = self.grid_rows * self.cell_h;

        let image = egui::ColorImage::from_rgba_unmultiplied(
            [total_w, total_h],
            &self.pixel_data,
        );

        if let Some(ref mut handle) = self.texture {
            handle.set(image, egui::TextureOptions::NEAREST);
        } else {
            self.texture = Some(ctx.load_texture("glyph_atlas", image, egui::TextureOptions::NEAREST));
        }
    }
}
