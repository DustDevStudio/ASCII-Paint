use ascii_core::model::{CanvasSpec, CellCoord, CellRect};

#[derive(Clone, Debug)]
pub struct Viewport {
    pub zoom: f32,
    pub origin: egui::Vec2,
    pub needs_fit: bool,
}

impl Viewport {
    pub const MIN_ZOOM: f32 = 0.1;
    pub const MAX_ZOOM: f32 = 16.0;
    pub const WHEEL_STEP: f32 = 1.1;

    pub fn new() -> Self {
        Self {
            zoom: 1.0,
            origin: egui::Vec2::ZERO,
            needs_fit: true,
        }
    }

    pub fn screen_to_cell(
        &self,
        p: egui::Pos2,
        canvas: &CanvasSpec,
        view: egui::Rect,
    ) -> Option<CellCoord> {
        let page_px = (p - view.min - self.origin) / self.zoom;
        if page_px.x < 0.0 || page_px.y < 0.0 {
            return None;
        }

        let col = (page_px.x / canvas.cell_w as f32).floor() as i32;
        let row = (page_px.y / canvas.cell_h as f32).floor() as i32;

        if col >= 0 && col < canvas.cols as i32 && row >= 0 && row < canvas.rows as i32 {
            Some(CellCoord::new(col as u16, row as u16))
        } else {
            None
        }
    }

    #[allow(dead_code)]
    pub fn screen_to_cell_clamped(
        &self,
        p: egui::Pos2,
        canvas: &CanvasSpec,
        view: egui::Rect,
    ) -> CellCoord {
        let page_px = (p - view.min - self.origin) / self.zoom;
        let col = (page_px.x / canvas.cell_w as f32).floor() as i32;
        let row = (page_px.y / canvas.cell_h as f32).floor() as i32;
        canvas.clamp(col, row)
    }

    pub fn cell_rect(&self, c: CellCoord, canvas: &CanvasSpec, view: egui::Rect) -> egui::Rect {
        let cw = canvas.cell_w as f32 * self.zoom;
        let ch = canvas.cell_h as f32 * self.zoom;
        let min = view.min
            + self.origin
            + egui::vec2(c.col as f32 * cw, c.row as f32 * ch);
        egui::Rect::from_min_size(min, egui::vec2(cw, ch))
    }

    pub fn page_rect(&self, canvas: &CanvasSpec, view: egui::Rect) -> egui::Rect {
        let pw = canvas.pixel_width() as f32 * self.zoom;
        let ph = canvas.pixel_height() as f32 * self.zoom;
        let min = view.min + self.origin;
        egui::Rect::from_min_size(min, egui::vec2(pw, ph))
    }

    pub fn visible_cells(&self, view: egui::Rect, canvas: &CanvasSpec) -> CellRect {
        let min_px = (view.min - view.min - self.origin) / self.zoom;
        let max_px = (view.max - view.min - self.origin) / self.zoom;

        let col_min = (min_px.x / canvas.cell_w as f32).floor() as i32;
        let row_min = (min_px.y / canvas.cell_h as f32).floor() as i32;
        let col_max = (max_px.x / canvas.cell_w as f32).ceil() as i32;
        let row_max = (max_px.y / canvas.cell_h as f32).ceil() as i32;

        let c1 = canvas.clamp(col_min, row_min);
        let c2 = canvas.clamp(col_max, row_max);
        CellRect::from_corners(c1, c2)
    }

    pub fn zoom_at(&mut self, cursor: egui::Pos2, notches: f32, view: egui::Rect) {
        let z_prime = (self.zoom * Self::WHEEL_STEP.powf(notches))
            .clamp(Self::MIN_ZOOM, Self::MAX_ZOOM);
        let cursor_rel = cursor - view.min;
        self.origin = cursor_rel - (cursor_rel - self.origin) * (z_prime / self.zoom);
        self.zoom = z_prime;
    }

    pub fn zoom_step(&mut self, direction: i32, view: egui::Rect) {
        let center = view.center();
        let notches = direction as f32 * 2.0;
        self.zoom_at(center, notches, view);
    }

    pub fn pan_by(&mut self, delta: egui::Vec2, view: egui::Rect, canvas: &CanvasSpec) {
        self.origin += delta;
        self.clamp_pan(view, canvas);
    }

    pub fn fit(&mut self, view: egui::Rect, canvas: &CanvasSpec) {
        let page_w = canvas.pixel_width() as f32;
        let page_h = canvas.pixel_height() as f32;
        if page_w > 0.0 && page_h > 0.0 && view.width() > 0.0 && view.height() > 0.0 {
            let scale_x = view.width() / page_w;
            let scale_y = view.height() / page_h;
            self.zoom = (scale_x.min(scale_y) * 0.95).clamp(Self::MIN_ZOOM, Self::MAX_ZOOM);
            let rendered_w = page_w * self.zoom;
            let rendered_h = page_h * self.zoom;
            self.origin = (view.size() - egui::vec2(rendered_w, rendered_h)) * 0.5;
            self.needs_fit = false;
        }
    }

    pub fn actual_size(&mut self, view: egui::Rect, canvas: &CanvasSpec) {
        self.zoom = 1.0;
        let page_w = canvas.pixel_width() as f32;
        let page_h = canvas.pixel_height() as f32;
        self.origin = (view.size() - egui::vec2(page_w, page_h)) * 0.5;
        self.clamp_pan(view, canvas);
    }

    fn clamp_pan(&mut self, view: egui::Rect, canvas: &CanvasSpec) {
        let page_w = canvas.pixel_width() as f32 * self.zoom;
        let page_h = canvas.pixel_height() as f32 * self.zoom;
        let margin = 64.0;

        let min_x = margin - page_w;
        let max_x = view.width() - margin;
        let min_y = margin - page_h;
        let max_y = view.height() - margin;

        self.origin.x = self.origin.x.clamp(min_x, max_x);
        self.origin.y = self.origin.y.clamp(min_y, max_y);
    }
}

impl Default for Viewport {
    fn default() -> Self {
        Self::new()
    }
}
