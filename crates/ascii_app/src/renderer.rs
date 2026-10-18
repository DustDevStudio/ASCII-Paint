use std::sync::Arc;
use ascii_core::model::{CanvasSpec, CellCoord, Frame};
use ascii_core::selection::{EdgeSegment, Selection, SelectionMode};
use ascii_core::tools::ToolOverlay;
use ascii_render::GlyphSource;

use crate::atlas::GlyphAtlas;
use crate::viewport::Viewport;

pub struct CanvasRenderer {
    pub atlas: GlyphAtlas,
    edges_cache: Option<(u64, Vec<EdgeSegment>)>,
}

impl CanvasRenderer {
    pub fn new(ctx: &egui::Context, glyphs: Arc<dyn GlyphSource>) -> Self {
        Self {
            atlas: GlyphAtlas::new(ctx, glyphs),
            edges_cache: None,
        }
    }

    pub fn paint(
        &mut self,
        painter: &egui::Painter,
        view: egui::Rect,
        canvas: &CanvasSpec,
        frame: &Frame,
        selection: &Selection,
        viewport: &Viewport,
        tool_overlay: &ToolOverlay,
        hovered_cell: Option<CellCoord>,
        show_grid: bool,
        is_playing: bool,
        time_seconds: f64,
    ) {
        let ctx = painter.ctx();

        // 1. Dark neutral workspace fill (#2B2B2B) over the whole widget
        painter.rect_filled(view, 0.0, egui::Color32::from_rgb(0x2B, 0x2B, 0x2B));

        // 2. PAGE: filled rect with frame background
        let page_rect = viewport.page_rect(canvas, view);
        let bg = frame.background;
        let page_color = egui::Color32::from_rgb(bg.r, bg.g, bg.b);
        painter.rect_filled(page_rect, 0.0, page_color);

        // 3 & 4. Cells
        let vis = viewport.visible_cells(view, canvas);
        let tex_id = self.atlas.texture_id(ctx);

        let mut mesh = egui::Mesh::with_texture(tex_id);

        for r in vis.min.row..=vis.max.row {
            for c in vis.min.col..=vis.max.col {
                let coord = CellCoord::new(c, r);
                if let Some(cell) = frame.grid.get(coord) {
                    let rect = viewport.cell_rect(coord, canvas, view);

                    // 3. Cell background
                    if let Some(bg) = cell.bg {
                        let col = egui::Color32::from_rgb(bg.r, bg.g, bg.b);
                        painter.rect_filled(rect, 0.0, col);
                    }

                    // 4. Glyph
                    if cell.glyph != ' ' {
                        let uv = self.atlas.uv(ctx, cell.glyph);
                        let tint = egui::Color32::from_rgb(cell.fg.r, cell.fg.g, cell.fg.b);
                        mesh.add_rect_with_uv(rect, uv, tint);
                    }
                }
            }
        }

        if !mesh.is_empty() {
            painter.add(egui::Shape::mesh(mesh));
        }

        // 5. Grid lines (1 px, white at 12% opacity) when show_grid && zoom >= 2.0
        if show_grid && viewport.zoom >= 2.0 {
            let grid_color = egui::Color32::from_white_alpha(31); // ~12%
            let stroke = egui::Stroke::new(1.0_f32, grid_color);

            for c in vis.min.col..=vis.max.col + 1 {
                let x = view.min.x
                    + viewport.origin.x
                    + c as f32 * canvas.cell_w as f32 * viewport.zoom;
                if x >= view.min.x && x <= view.max.x {
                    let y1 = (view.min.y + viewport.origin.y).max(view.min.y);
                    let y2 = (view.min.y
                        + viewport.origin.y
                        + canvas.pixel_height() as f32 * viewport.zoom)
                        .min(view.max.y);
                    if y1 < y2 {
                        painter.line_segment([egui::pos2(x, y1), egui::pos2(x, y2)], stroke);
                    }
                }
            }

            for r in vis.min.row..=vis.max.row + 1 {
                let y = view.min.y
                    + viewport.origin.y
                    + r as f32 * canvas.cell_h as f32 * viewport.zoom;
                if y >= view.min.y && y <= view.max.y {
                    let x1 = (view.min.x + viewport.origin.x).max(view.min.x);
                    let x2 = (view.min.x
                        + viewport.origin.x
                        + canvas.pixel_width() as f32 * viewport.zoom)
                        .min(view.max.y);
                    if x1 < x2 {
                        painter.line_segment([egui::pos2(x1, y), egui::pos2(x2, y)], stroke);
                    }
                }
            }
        }

        // 8. Page border (1 px #000000 at 60%)
        let border_stroke = egui::Stroke::new(1.0_f32, egui::Color32::from_black_alpha(153));
        painter.rect_stroke(page_rect, 0.0, border_stroke);

        // Steps 6 and 7 are skipped while playing
        if is_playing {
            return;
        }

        // 6. Selection: boundary_edges() as dashed black/white marching ants
        let sel_rev = selection.revision();
        if self.edges_cache.as_ref().map_or(true, |(r, _)| *r != sel_rev) {
            self.edges_cache = Some((sel_rev, selection.boundary_edges()));
        }

        if let Some((_, ref edges)) = self.edges_cache {
            if !edges.is_empty() {
                let cw = canvas.cell_w as f32 * viewport.zoom;
                let ch = canvas.cell_h as f32 * viewport.zoom;
                let ant_offset = ((time_seconds * 10.0) as i64) % 8;

                for edge in edges {
                    let p1 = view.min
                        + viewport.origin
                        + egui::vec2(edge.from.0 as f32 * cw, edge.from.1 as f32 * ch);
                    let p2 = view.min
                        + viewport.origin
                        + egui::vec2(edge.to.0 as f32 * cw, edge.to.1 as f32 * ch);

                    let ant_color = if ant_offset < 4 {
                        egui::Color32::WHITE
                    } else {
                        egui::Color32::BLACK
                    };
                    painter.line_segment([p1, p2], egui::Stroke::new(1.5_f32, ant_color));
                }
            }
        }

        // 7. Tool overlay
        match tool_overlay {
            ToolOverlay::TextCursor(cell) => {
                // Blinking cursor 500 ms
                let blink = (time_seconds * 2.0).fract() < 0.5;
                if blink {
                    let rect = viewport.cell_rect(*cell, canvas, view);
                    painter.rect_stroke(
                        rect,
                        0.0,
                        egui::Stroke::new(2.0_f32, egui::Color32::LIGHT_BLUE),
                    );
                }
            }
            ToolOverlay::RectPreview(r, mode) | ToolOverlay::EllipsePreview(r, mode) => {
                let c1 = viewport.cell_rect(r.min, canvas, view);
                let c2 = viewport.cell_rect(r.max, canvas, view);
                let bounds = c1.union(c2);

                let color = match mode {
                    SelectionMode::Replace => egui::Color32::from_rgb(100, 200, 255),
                    SelectionMode::Add => egui::Color32::from_rgb(100, 255, 100),
                    SelectionMode::Subtract => egui::Color32::from_rgb(255, 100, 100),
                };
                let stroke = egui::Stroke::new(1.5_f32, color);

                if matches!(tool_overlay, ToolOverlay::EllipsePreview(_, _)) {
                    // Ellipse outline
                    painter.rect_stroke(bounds, bounds.height() / 2.0, stroke);
                } else {
                    painter.rect_stroke(bounds, 0.0, stroke);
                }
            }
            ToolOverlay::PolygonPreview(vertices, hover, mode) => {
                let color = match mode {
                    SelectionMode::Replace => egui::Color32::from_rgb(100, 200, 255),
                    SelectionMode::Add => egui::Color32::from_rgb(100, 255, 100),
                    SelectionMode::Subtract => egui::Color32::from_rgb(255, 100, 100),
                };
                let stroke = egui::Stroke::new(1.5_f32, color);

                let pts: Vec<egui::Pos2> = vertices
                    .iter()
                    .map(|v| viewport.cell_rect(*v, canvas, view).center())
                    .collect();

                for i in 0..pts.len().saturating_sub(1) {
                    painter.line_segment([pts[i], pts[i + 1]], stroke);
                }

                if let (Some(last), Some(h)) = (pts.last(), hover) {
                    let hover_pt = viewport.cell_rect(*h, canvas, view).center();
                    painter.line_segment([*last, hover_pt], stroke);
                }
            }
            ToolOverlay::None => {}
        }

        // Hovered cell outline
        if let Some(h) = hovered_cell {
            if canvas.contains(h) {
                let rect = viewport.cell_rect(h, canvas, view);
                painter.rect_stroke(
                    rect,
                    0.0,
                    egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(100)),
                );
            }
        }
    }
}
