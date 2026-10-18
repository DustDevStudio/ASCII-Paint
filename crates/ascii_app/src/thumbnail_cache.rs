use std::collections::HashMap;
use ascii_core::model::{Frame, FrameId};
use ascii_render::ThumbnailBuilder;

pub struct ThumbEntry {
    pub revision: u64,
    pub texture: egui::TextureHandle,
}

pub struct ThumbnailCache {
    entries: HashMap<FrameId, ThumbEntry>,
}

impl ThumbnailCache {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    pub fn get(&mut self, ctx: &egui::Context, frame: &Frame) -> &egui::TextureHandle {
        let rev = frame.revision();
        let id = frame.id;

        let needs_build = match self.entries.get(&id) {
            Some(entry) => entry.revision != rev,
            None => true,
        };

        if needs_build {
            let buf = ThumbnailBuilder::build(frame);
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [buf.width as usize, buf.height as usize],
                &buf.pixels,
            );
            let handle = ctx.load_texture(
                format!("thumb_{}", id.0),
                image,
                egui::TextureOptions::NEAREST,
            );
            self.entries.insert(id, ThumbEntry { revision: rev, texture: handle });
        }

        &self.entries.get(&id).unwrap().texture
    }

    #[allow(dead_code)]
    pub fn retain_only(&mut self, ids: &[FrameId]) {
        self.entries.retain(|id, _| ids.contains(id));
    }
}

impl Default for ThumbnailCache {
    fn default() -> Self {
        Self::new()
    }
}
