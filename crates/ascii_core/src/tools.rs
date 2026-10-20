use std::collections::HashMap;
use std::sync::Arc;

use crate::algo::{FloodFill, ShapeRasterizer};
use crate::brush::{BrushId, BrushLibrary};
use crate::compose::Compositor;
use crate::history::{
    CellPatchCommand, FillChange, History, InkChange, SelectionChange,
};
use crate::model::{
    CanvasSpec, CellCoord, CellRect, FrameId, InkCell, LayerId, Plane, PlaneSet, Project, Rgba,
    CellBuffer,
};
use crate::selection::{Selection, SelectionMode};

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum ToolId {
    Paint,
    Erase,
    Fill,
    SelectRect,
    SelectPolygon,
    SelectEllipse,
    Move,
    Pick,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ToolKey {
    Enter,
    Backspace,
    Escape,
    Delete,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

#[derive(Clone, Debug)]
pub enum ToolEvent {
    PointerDown(Option<CellCoord>),
    PointerMove(Option<CellCoord>),
    PointerUp(Option<CellCoord>),
    DoubleClick(Option<CellCoord>),
    Key(ToolKey),
    Activate,
    Deactivate,
    Cancel,
}

#[derive(Clone, Debug)]
pub enum ToolOverlay {
    None,
    BrushFootprint(Vec<CellCoord>),
    RectPreview(CellRect),
    EllipsePreview(CellRect),
    PolygonRubberBand(Vec<CellCoord>, Option<CellCoord>),
    MovePreview {
        floating: Option<Arc<CellBuffer>>,
        offset: (i32, i32),
    },
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct PickedColors {
    pub ink: Option<Rgba>,
    pub fill: Option<Rgba>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolResponse {
    Consumed,
    Picked(PickedColors),
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum MoveMode {
    Contents,
    OutlineOnly,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum PickSource {
    Merged,
    ActiveLayer,
}

#[derive(Clone, Debug)]
pub struct ToolOptions {
    pub brush: BrushId,
    pub planes: PlaneSet,
    pub ink_color: Rgba,
    pub fill_color: Rgba,
    pub erase_size: u8,
    pub selection_mode: SelectionMode,
    pub move_mode: MoveMode,
    pub pick_source: PickSource,
}

impl ToolOptions {
    pub fn default() -> Self {
        ToolOptions {
            brush: BrushLibrary::default_id(),
            planes: PlaneSet::FOREGROUND,
            ink_color: Rgba::WHITE,
            fill_color: Rgba::new(128, 128, 128, 255),
            erase_size: 1,
            selection_mode: SelectionMode::Replace,
            move_mode: MoveMode::Contents,
            pick_source: PickSource::Merged,
        }
    }

    pub fn mode_for(&self, mods: Modifiers) -> SelectionMode {
        if mods.shift {
            SelectionMode::Add
        } else if mods.alt {
            SelectionMode::Subtract
        } else {
            self.selection_mode
        }
    }
}

impl Default for ToolOptions {
    fn default() -> Self {
        Self::default()
    }
}

pub struct ToolContext<'a> {
    pub project: &'a mut Project,
    pub history: &'a mut History,
    pub selection: &'a mut Selection,
    pub options: &'a ToolOptions,
    pub brushes: &'a BrushLibrary,
    pub mods: Modifiers,
}

pub struct ToolView<'a> {
    pub project: &'a Project,
    pub selection: &'a Selection,
    pub options: &'a ToolOptions,
    pub brushes: &'a BrushLibrary,
}

pub struct PatchBuilder {
    pub frame_id: FrameId,
    pub layer_id: LayerId,
    ink_before: HashMap<CellCoord, InkCell>,
    ink_after: HashMap<CellCoord, InkCell>,
    ink_order: Vec<CellCoord>,
    fill_before: HashMap<CellCoord, Rgba>,
    fill_after: HashMap<CellCoord, Rgba>,
    fill_order: Vec<CellCoord>,
}

impl PatchBuilder {
    pub fn new(frame_id: FrameId, layer_id: LayerId) -> PatchBuilder {
        PatchBuilder {
            frame_id,
            layer_id,
            ink_before: HashMap::new(),
            ink_after: HashMap::new(),
            ink_order: Vec::new(),
            fill_before: HashMap::new(),
            fill_after: HashMap::new(),
            fill_order: Vec::new(),
        }
    }

    pub fn write_ink(&mut self, project: &mut Project, c: CellCoord, v: InkCell) -> bool {
        if !project.canvas.contains(c) {
            return false;
        }
        let norm = v.normalized();
        let fidx = match project.timeline.index_of(self.frame_id) {
            Some(i) => i,
            None => return false,
        };
        let frame = match project.timeline.frame_mut(fidx) {
            Some(f) => f,
            None => return false,
        };
        let lidx = match frame.index_of_layer(self.layer_id) {
            Some(i) => i,
            None => return false,
        };

        let old = match frame.layers()[lidx].ink.get(c) {
            Some(&ic) => ic,
            None => return false,
        };

        if old == norm {
            return false;
        }

        if !self.ink_before.contains_key(&c) {
            self.ink_before.insert(c, old);
            self.ink_order.push(c);
        }
        frame.set_ink(self.layer_id, c, norm);
        self.ink_after.insert(c, norm);
        true
    }

    pub fn write_fill(&mut self, project: &mut Project, c: CellCoord, v: Rgba) -> bool {
        if !project.canvas.contains(c) {
            return false;
        }
        let norm = v.normalized();
        let fidx = match project.timeline.index_of(self.frame_id) {
            Some(i) => i,
            None => return false,
        };
        let frame = match project.timeline.frame_mut(fidx) {
            Some(f) => f,
            None => return false,
        };
        let lidx = match frame.index_of_layer(self.layer_id) {
            Some(i) => i,
            None => return false,
        };

        let old = match frame.layers()[lidx].fill.get(c) {
            Some(&fc) => fc,
            None => return false,
        };

        if old == norm {
            return false;
        }

        if !self.fill_before.contains_key(&c) {
            self.fill_before.insert(c, old);
            self.fill_order.push(c);
        }
        frame.set_fill(self.layer_id, c, norm);
        self.fill_after.insert(c, norm);
        true
    }

    pub fn clear(&mut self, project: &mut Project, cells: &[CellCoord]) {
        for &c in cells {
            self.write_ink(project, c, InkCell::EMPTY);
            self.write_fill(project, c, Rgba::TRANSPARENT);
        }
    }

    pub fn blit(&mut self, project: &mut Project, buf: &CellBuffer, offset: (i32, i32)) {
        for (c, ink, fill) in buf.cells() {
            let target_c = c.col as i32 + offset.0;
            let target_r = c.row as i32 + offset.1;
            if target_c >= 0
                && target_c < project.canvas.cols as i32
                && target_r >= 0
                && target_r < project.canvas.rows as i32
            {
                let tc = CellCoord::new(target_c as u16, target_r as u16);
                if !ink.is_empty() {
                    self.write_ink(project, tc, *ink);
                }
                if fill.a > 0 {
                    self.write_fill(project, tc, *fill);
                }
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.ink_order.is_empty() && self.fill_order.is_empty()
    }

    pub fn finish(&mut self, label: &str) -> Option<CellPatchCommand> {
        let mut ink_changes = Vec::new();
        for &coord in &self.ink_order {
            let before = self.ink_before.get(&coord).copied().unwrap_or(InkCell::EMPTY);
            let after = self.ink_after.get(&coord).copied().unwrap_or(InkCell::EMPTY);
            if before != after {
                ink_changes.push(InkChange {
                    coord,
                    before,
                    after,
                });
            }
        }

        let mut fill_changes = Vec::new();
        for &coord in &self.fill_order {
            let before = self
                .fill_before
                .get(&coord)
                .copied()
                .unwrap_or(Rgba::TRANSPARENT);
            let after = self
                .fill_after
                .get(&coord)
                .copied()
                .unwrap_or(Rgba::TRANSPARENT);
            if before != after {
                fill_changes.push(FillChange {
                    coord,
                    before,
                    after,
                });
            }
        }

        if ink_changes.is_empty() && fill_changes.is_empty() {
            None
        } else {
            Some(CellPatchCommand {
                frame_id: self.frame_id,
                layer_id: self.layer_id,
                ink_changes,
                fill_changes,
                label: label.to_string(),
            })
        }
    }

    pub fn rollback(&mut self, project: &mut Project) {
        let fidx = match project.timeline.index_of(self.frame_id) {
            Some(i) => i,
            None => return,
        };
        let frame = match project.timeline.frame_mut(fidx) {
            Some(f) => f,
            None => return,
        };
        for coord in self.ink_order.iter().rev() {
            if let Some(before) = self.ink_before.get(coord) {
                frame.set_ink(self.layer_id, *coord, *before);
            }
        }
        for coord in self.fill_order.iter().rev() {
            if let Some(before) = self.fill_before.get(coord) {
                frame.set_fill(self.layer_id, *coord, *before);
            }
        }
        self.ink_before.clear();
        self.ink_after.clear();
        self.ink_order.clear();
        self.fill_before.clear();
        self.fill_after.clear();
        self.fill_order.clear();
    }
}

pub trait Tool: Send + Sync {
    fn id(&self) -> ToolId;
    fn on_event(&mut self, event: ToolEvent, ctx: &mut ToolContext) -> ToolResponse;
    fn overlay(&self, view: &ToolView) -> ToolOverlay;
    fn is_busy(&self) -> bool;
}

pub trait StrokeOperation: Send + Sync {
    fn label(&self) -> &str;
    fn footprint(&self, at: CellCoord, view: &ToolView) -> Vec<CellCoord>;
    fn stamp(&self, at: CellCoord, ctx: &mut ToolContext, b: &mut PatchBuilder);
}

pub struct PaintOperation;

impl StrokeOperation for PaintOperation {
    fn label(&self) -> &str {
        "Paint cells"
    }

    fn footprint(&self, at: CellCoord, view: &ToolView) -> Vec<CellCoord> {
        let brush = match view.brushes.get(view.options.brush) {
            Some(b) => b,
            None => return vec![at],
        };
        let stamp = &brush.stamp;
        let mut out = Vec::new();
        for r in 0..stamp.height {
            for c in 0..stamp.width {
                if stamp.glyph_at(c, r).is_some() {
                    let col = at.col as i32 + c as i32 - stamp.anchor.0 as i32;
                    let row = at.row as i32 + r as i32 - stamp.anchor.1 as i32;
                    if col >= 0
                        && col < view.project.canvas.cols as i32
                        && row >= 0
                        && row < view.project.canvas.rows as i32
                    {
                        out.push(CellCoord::new(col as u16, row as u16));
                    }
                }
            }
        }
        out
    }

    fn stamp(&self, at: CellCoord, ctx: &mut ToolContext, b: &mut PatchBuilder) {
        let brush = match ctx.brushes.get(ctx.options.brush) {
            Some(b) => b.clone(),
            None => return,
        };
        let stamp = &brush.stamp;
        for r in 0..stamp.height {
            for c in 0..stamp.width {
                if let Some(ch) = stamp.glyph_at(c, r) {
                    let col = at.col as i32 + c as i32 - stamp.anchor.0 as i32;
                    let row = at.row as i32 + r as i32 - stamp.anchor.1 as i32;
                    if col >= 0
                        && col < ctx.project.canvas.cols as i32
                        && row >= 0
                        && row < ctx.project.canvas.rows as i32
                    {
                        let coord = CellCoord::new(col as u16, row as u16);
                        if ctx.selection.permits(coord) {
                            if ctx.options.planes.foreground {
                                b.write_ink(
                                    ctx.project,
                                    coord,
                                    InkCell {
                                        glyph: ch,
                                        color: ctx.options.ink_color,
                                    },
                                );
                            }
                            if ctx.options.planes.background {
                                b.write_fill(ctx.project, coord, ctx.options.fill_color);
                            }
                        }
                    }
                }
            }
        }
    }
}

pub struct EraseOperation;

impl StrokeOperation for EraseOperation {
    fn label(&self) -> &str {
        "Erase cells"
    }

    fn footprint(&self, at: CellCoord, view: &ToolView) -> Vec<CellCoord> {
        let sz = view.options.erase_size.max(1) as i32;
        let anchor = (sz - 1) / 2;
        let mut out = Vec::new();
        for r in 0..sz {
            for c in 0..sz {
                let col = at.col as i32 + c - anchor;
                let row = at.row as i32 + r - anchor;
                if col >= 0
                    && col < view.project.canvas.cols as i32
                    && row >= 0
                    && row < view.project.canvas.rows as i32
                {
                    out.push(CellCoord::new(col as u16, row as u16));
                }
            }
        }
        out
    }

    fn stamp(&self, at: CellCoord, ctx: &mut ToolContext, b: &mut PatchBuilder) {
        let sz = ctx.options.erase_size.max(1) as i32;
        let anchor = (sz - 1) / 2;
        for r in 0..sz {
            for c in 0..sz {
                let col = at.col as i32 + c - anchor;
                let row = at.row as i32 + r - anchor;
                if col >= 0
                    && col < ctx.project.canvas.cols as i32
                    && row >= 0
                    && row < ctx.project.canvas.rows as i32
                {
                    let coord = CellCoord::new(col as u16, row as u16);
                    if ctx.selection.permits(coord) {
                        if ctx.options.planes.foreground {
                            b.write_ink(ctx.project, coord, InkCell::EMPTY);
                        }
                        if ctx.options.planes.background {
                            b.write_fill(ctx.project, coord, Rgba::TRANSPARENT);
                        }
                    }
                }
            }
        }
    }
}

pub struct StrokeState {
    pub last: CellCoord,
    pub builder: PatchBuilder,
}

pub struct StrokeTool<O: StrokeOperation> {
    id: ToolId,
    op: O,
    hover: Option<CellCoord>,
    stroke: Option<StrokeState>,
}

impl<O: StrokeOperation> StrokeTool<O> {
    pub fn new(id: ToolId, op: O) -> Self {
        StrokeTool {
            id,
            op,
            hover: None,
            stroke: None,
        }
    }
}

impl<O: StrokeOperation> Tool for StrokeTool<O> {
    fn id(&self) -> ToolId {
        self.id
    }

    fn on_event(&mut self, event: ToolEvent, ctx: &mut ToolContext) -> ToolResponse {
        match event {
            ToolEvent::PointerMove(cell) => {
                self.hover = cell;
                if let (Some(c), Some(ref mut st)) = (cell, &mut self.stroke) {
                    let line = ShapeRasterizer::line(st.last, c);
                    for pt in line.into_iter().skip(1) {
                        self.op.stamp(pt, ctx, &mut st.builder);
                    }
                    st.last = c;
                }
            }
            ToolEvent::PointerDown(Some(c)) => {
                let frame = ctx.project.timeline.selected_frame();
                let layer = frame.active_layer();
                if !layer.visible {
                    return ToolResponse::Consumed;
                }
                let mut builder = PatchBuilder::new(frame.id, layer.id);
                self.op.stamp(c, ctx, &mut builder);
                self.stroke = Some(StrokeState {
                    last: c,
                    builder,
                });
            }
            ToolEvent::PointerUp(_) | ToolEvent::Deactivate | ToolEvent::Cancel => {
                if let Some(mut st) = self.stroke.take() {
                    if let Some(cmd) = st.builder.finish(self.op.label()) {
                        ctx.history.record_applied(Box::new(cmd));
                    }
                }
            }
            _ => {}
        }
        ToolResponse::Consumed
    }

    fn overlay(&self, view: &ToolView) -> ToolOverlay {
        if let Some(h) = self.hover {
            ToolOverlay::BrushFootprint(self.op.footprint(h, view))
        } else {
            ToolOverlay::None
        }
    }

    fn is_busy(&self) -> bool {
        self.stroke.is_some()
    }
}

pub type PaintTool = StrokeTool<PaintOperation>;
pub type EraseTool = StrokeTool<EraseOperation>;

pub struct FillTool;

impl Tool for FillTool {
    fn id(&self) -> ToolId {
        ToolId::Fill
    }

    fn on_event(&mut self, event: ToolEvent, ctx: &mut ToolContext) -> ToolResponse {
        if let ToolEvent::PointerDown(Some(c)) = event {
            let (frame_id, layer_id, is_visible) = {
                let frame = ctx.project.timeline.selected_frame();
                let layer = frame.active_layer();
                (frame.id, layer.id, layer.visible)
            };
            if !is_visible {
                return ToolResponse::Consumed;
            }
            if !ctx.selection.permits(c) {
                return ToolResponse::Consumed;
            }

            let fill_glyph = if ctx.options.planes.foreground {
                let brush = ctx
                    .brushes
                    .get(ctx.options.brush)
                    .cloned()
                    .unwrap_or_else(|| {
                        ctx.brushes.get(BrushLibrary::default_id()).unwrap().clone()
                    });
                Some(brush.fill_glyph())
            } else {
                None
            };

            let fg_region = if ctx.options.planes.foreground {
                let frame = ctx.project.timeline.selected_frame();
                let layer = frame.active_layer();
                FloodFill::region(layer, c, Plane::Foreground, ctx.selection)
            } else {
                Vec::new()
            };

            let bg_region = if ctx.options.planes.background {
                let frame = ctx.project.timeline.selected_frame();
                let layer = frame.active_layer();
                FloodFill::region(layer, c, Plane::Background, ctx.selection)
            } else {
                Vec::new()
            };

            let mut builder = PatchBuilder::new(frame_id, layer_id);

            if let Some(glyph) = fill_glyph {
                for coord in fg_region {
                    builder.write_ink(
                        ctx.project,
                        coord,
                        InkCell {
                            glyph,
                            color: ctx.options.ink_color,
                        },
                    );
                }
            }

            for coord in bg_region {
                builder.write_fill(ctx.project, coord, ctx.options.fill_color);
            }

            if let Some(cmd) = builder.finish("Fill cells") {
                ctx.history.record_applied(Box::new(cmd));
            }
        }
        ToolResponse::Consumed
    }

    fn overlay(&self, _view: &ToolView) -> ToolOverlay {
        ToolOverlay::None
    }

    fn is_busy(&self) -> bool {
        false
    }
}

pub trait DragShape: Send + Sync {
    fn label(&self) -> &str;
    fn rasterize(&self, a: CellCoord, b: CellCoord, spec: &CanvasSpec) -> Vec<CellCoord>;
    fn preview(&self, a: CellCoord, b: CellCoord) -> ToolOverlay;
}

pub struct RectShape;

impl DragShape for RectShape {
    fn label(&self) -> &str {
        "Rectangle select"
    }

    fn rasterize(&self, a: CellCoord, b: CellCoord, spec: &CanvasSpec) -> Vec<CellCoord> {
        ShapeRasterizer::rect(a, b, spec)
    }

    fn preview(&self, a: CellCoord, b: CellCoord) -> ToolOverlay {
        ToolOverlay::RectPreview(CellRect::from_corners(a, b))
    }
}

pub struct EllipseShape;

impl DragShape for EllipseShape {
    fn label(&self) -> &str {
        "Ellipse select"
    }

    fn rasterize(&self, a: CellCoord, b: CellCoord, spec: &CanvasSpec) -> Vec<CellCoord> {
        ShapeRasterizer::ellipse(a, b, spec)
    }

    fn preview(&self, a: CellCoord, b: CellCoord) -> ToolOverlay {
        ToolOverlay::EllipsePreview(CellRect::from_corners(a, b))
    }
}

pub struct DragState {
    pub anchor: CellCoord,
    pub current: CellCoord,
    pub mode: SelectionMode,
}

pub struct ShapeSelectTool<S: DragShape> {
    id: ToolId,
    shape: S,
    drag: Option<DragState>,
}

impl<S: DragShape> ShapeSelectTool<S> {
    pub fn new(id: ToolId, shape: S) -> Self {
        ShapeSelectTool {
            id,
            shape,
            drag: None,
        }
    }
}

impl<S: DragShape> Tool for ShapeSelectTool<S> {
    fn id(&self) -> ToolId {
        self.id
    }

    fn on_event(&mut self, event: ToolEvent, ctx: &mut ToolContext) -> ToolResponse {
        match event {
            ToolEvent::PointerDown(Some(c)) => {
                let mode = ctx.options.mode_for(ctx.mods);
                self.drag = Some(DragState {
                    anchor: c,
                    current: c,
                    mode,
                });
            }
            ToolEvent::PointerMove(cell) => {
                if let Some(ref mut ds) = self.drag {
                    if let Some(c) = cell {
                        ds.current = c;
                    }
                }
            }
            ToolEvent::PointerUp(cell) => {
                if let Some(ds) = self.drag.take() {
                    let end = cell.unwrap_or(ds.current);
                    let cells = self.shape.rasterize(ds.anchor, end, &ctx.project.canvas);
                    ctx.selection.apply(&cells, ds.mode);
                }
            }
            ToolEvent::Cancel | ToolEvent::Deactivate => {
                self.drag = None;
            }
            _ => {}
        }
        ToolResponse::Consumed
    }

    fn overlay(&self, _view: &ToolView) -> ToolOverlay {
        if let Some(ref ds) = self.drag {
            self.shape.preview(ds.anchor, ds.current)
        } else {
            ToolOverlay::None
        }
    }

    fn is_busy(&self) -> bool {
        self.drag.is_some()
    }
}

pub type SelectRectTool = ShapeSelectTool<RectShape>;
pub type SelectEllipseTool = ShapeSelectTool<EllipseShape>;

pub struct SelectPolygonTool {
    vertices: Vec<CellCoord>,
    hover: Option<CellCoord>,
    mode: SelectionMode,
}

impl SelectPolygonTool {
    pub fn new() -> Self {
        SelectPolygonTool {
            vertices: Vec::new(),
            hover: None,
            mode: SelectionMode::Replace,
        }
    }

    fn close(&mut self, ctx: &mut ToolContext) {
        if self.vertices.len() >= 3 {
            let cells = ShapeRasterizer::polygon(&self.vertices, &ctx.project.canvas);
            ctx.selection.apply(&cells, self.mode);
        }
        self.vertices.clear();
    }
}

impl Tool for SelectPolygonTool {
    fn id(&self) -> ToolId {
        ToolId::SelectPolygon
    }

    fn on_event(&mut self, event: ToolEvent, ctx: &mut ToolContext) -> ToolResponse {
        match event {
            ToolEvent::PointerDown(Some(c)) => {
                if self.vertices.is_empty() {
                    self.mode = ctx.options.mode_for(ctx.mods);
                    self.vertices.push(c);
                } else if self.vertices.last() == Some(&c) {
                    // absorb double click or same vertex
                } else if self.vertices.first() == Some(&c) {
                    if self.vertices.len() >= 3 {
                        self.close(ctx);
                    }
                } else {
                    self.vertices.push(c);
                }
            }
            ToolEvent::PointerMove(cell) => {
                self.hover = cell;
            }
            ToolEvent::DoubleClick(_) | ToolEvent::Key(ToolKey::Enter) => {
                self.close(ctx);
            }
            ToolEvent::Key(ToolKey::Backspace) => {
                self.vertices.pop();
            }
            ToolEvent::Key(ToolKey::Escape) | ToolEvent::Cancel | ToolEvent::Deactivate => {
                self.vertices.clear();
            }
            _ => {}
        }
        ToolResponse::Consumed
    }

    fn overlay(&self, _view: &ToolView) -> ToolOverlay {
        if !self.vertices.is_empty() {
            ToolOverlay::PolygonRubberBand(self.vertices.clone(), self.hover)
        } else {
            ToolOverlay::None
        }
    }

    fn is_busy(&self) -> bool {
        !self.vertices.is_empty()
    }
}

pub struct MoveDrag {
    pub anchor: CellCoord,
    pub offset: (i32, i32),
    pub selection_before: Selection,
    pub floating: Option<Arc<CellBuffer>>,
    pub builder: Option<PatchBuilder>,
}

pub struct MoveTool {
    drag: Option<MoveDrag>,
}

impl MoveTool {
    pub fn new() -> Self {
        MoveTool { drag: None }
    }

    fn clamp_offset(sel: &Selection, offset: (i32, i32), spec: &CanvasSpec) -> (i32, i32) {
        let bounds = match sel.bounds() {
            Some(b) => b,
            None => return (0, 0),
        };
        let min_c = bounds.min.col as i32;
        let max_c = bounds.max.col as i32;
        let min_r = bounds.min.row as i32;
        let max_r = bounds.max.row as i32;

        let clamped_x = offset.0.clamp(-min_c, (spec.cols as i32 - 1) - max_c);
        let clamped_y = offset.1.clamp(-min_r, (spec.rows as i32 - 1) - max_r);
        (clamped_x, clamped_y)
    }

    fn execute_nudge(
        &mut self,
        offset: (i32, i32),
        ctx: &mut ToolContext,
    ) {
        if ctx.selection.is_empty() {
            return;
        }
        let clamped = Self::clamp_offset(ctx.selection, offset, &ctx.project.canvas);
        if clamped == (0, 0) {
            return;
        }

        let frame = ctx.project.timeline.selected_frame();
        let layer = frame.active_layer();
        if ctx.options.move_mode == MoveMode::Contents && !layer.visible {
            return;
        }

        let selection_before = ctx.selection.clone();

        if ctx.options.move_mode == MoveMode::Contents {
            let buffer = CellBuffer::extract(layer, ctx.selection);
            let mut builder = PatchBuilder::new(frame.id, layer.id);
            let selected_cells: Vec<CellCoord> = ctx.selection.bounds().map(|b| {
                b.iter().filter(|&c| ctx.selection.contains(c)).collect()
            }).unwrap_or_default();

            builder.clear(ctx.project, &selected_cells);
            builder.blit(ctx.project, &buffer, clamped);
            ctx.selection.translate(clamped);

            if let Some(cmd) = builder.finish("Move cells") {
                ctx.history.record_applied_with_selection(
                    Box::new(cmd),
                    SelectionChange {
                        before: selection_before,
                        after: ctx.selection.clone(),
                    },
                );
            }
        } else {
            ctx.selection.translate(clamped);
        }
    }
}

impl Tool for MoveTool {
    fn id(&self) -> ToolId {
        ToolId::Move
    }

    fn on_event(&mut self, event: ToolEvent, ctx: &mut ToolContext) -> ToolResponse {
        match event {
            ToolEvent::PointerDown(Some(c)) => {
                if !ctx.selection.contains(c) {
                    return ToolResponse::Consumed;
                }
                let frame = ctx.project.timeline.selected_frame();
                let layer = frame.active_layer();
                let selection_before = ctx.selection.clone();

                if ctx.options.move_mode == MoveMode::Contents {
                    if !layer.visible {
                        return ToolResponse::Consumed;
                    }
                    let buffer = CellBuffer::extract(layer, ctx.selection);
                    let mut builder = PatchBuilder::new(frame.id, layer.id);
                    let selected_cells: Vec<CellCoord> = ctx.selection.bounds().map(|b| {
                        b.iter().filter(|&k| ctx.selection.contains(k)).collect()
                    }).unwrap_or_default();
                    builder.clear(ctx.project, &selected_cells);

                    self.drag = Some(MoveDrag {
                        anchor: c,
                        offset: (0, 0),
                        selection_before,
                        floating: Some(Arc::new(buffer)),
                        builder: Some(builder),
                    });
                } else {
                    self.drag = Some(MoveDrag {
                        anchor: c,
                        offset: (0, 0),
                        selection_before,
                        floating: None,
                        builder: None,
                    });
                }
            }
            ToolEvent::PointerMove(Some(c)) => {
                if let Some(ref mut d) = self.drag {
                    let raw_offset = (c.col as i32 - d.anchor.col as i32, c.row as i32 - d.anchor.row as i32);
                    d.offset = Self::clamp_offset(&d.selection_before, raw_offset, &ctx.project.canvas);
                }
            }
            ToolEvent::PointerUp(_) => {
                if let Some(d) = self.drag.take() {
                    if d.offset == (0, 0) {
                        if let Some(mut b) = d.builder {
                            b.rollback(ctx.project);
                        }
                    } else if let (Some(floating), Some(mut b)) = (d.floating, d.builder) {
                        b.blit(ctx.project, &floating, d.offset);
                        ctx.selection.translate(d.offset);
                        if let Some(cmd) = b.finish("Move cells") {
                            ctx.history.record_applied_with_selection(
                                Box::new(cmd),
                                SelectionChange {
                                    before: d.selection_before,
                                    after: ctx.selection.clone(),
                                },
                            );
                        }
                    } else {
                        ctx.selection.translate(d.offset);
                    }
                }
            }
            ToolEvent::Cancel | ToolEvent::Deactivate => {
                if let Some(d) = self.drag.take() {
                    if let Some(mut b) = d.builder {
                        b.rollback(ctx.project);
                    }
                }
            }
            ToolEvent::Key(k) if self.drag.is_none() => {
                let step = if ctx.mods.shift { 10 } else { 1 };
                let delta = match k {
                    ToolKey::ArrowUp => (0, -step),
                    ToolKey::ArrowDown => (0, step),
                    ToolKey::ArrowLeft => (-step, 0),
                    ToolKey::ArrowRight => (step, 0),
                    _ => (0, 0),
                };
                if delta != (0, 0) {
                    self.execute_nudge(delta, ctx);
                }
            }
            _ => {}
        }
        ToolResponse::Consumed
    }

    fn overlay(&self, _view: &ToolView) -> ToolOverlay {
        if let Some(ref d) = self.drag {
            ToolOverlay::MovePreview {
                floating: d.floating.clone(),
                offset: d.offset,
            }
        } else {
            ToolOverlay::None
        }
    }

    fn is_busy(&self) -> bool {
        self.drag.is_some()
    }
}

pub struct PickTool {
    hover: Option<CellCoord>,
    dragging: bool,
}

impl PickTool {
    pub fn new() -> Self {
        PickTool {
            hover: None,
            dragging: false,
        }
    }

    fn sample(&self, c: CellCoord, ctx: &mut ToolContext) -> ToolResponse {
        let frame = ctx.project.timeline.selected_frame();
        let mut picked = PickedColors::default();

        match ctx.options.pick_source {
            PickSource::Merged => {
                let flat = Compositor::flatten_cell(frame, c);
                if ctx.options.planes.foreground && flat.glyph != ' ' {
                    picked.ink = Some(flat.ink);
                }
                if ctx.options.planes.background && flat.fill.a > 0 {
                    picked.fill = Some(flat.fill);
                }
            }
            PickSource::ActiveLayer => {
                let layer = frame.active_layer();
                if ctx.options.planes.foreground {
                    if let Some(ic) = layer.ink.get(c) {
                        if ic.glyph != ' ' {
                            picked.ink = Some(ic.color);
                        }
                    }
                }
                if ctx.options.planes.background {
                    if let Some(fc) = layer.fill.get(c) {
                        if fc.a > 0 {
                            picked.fill = Some(*fc);
                        }
                    }
                }
            }
        }

        ToolResponse::Picked(picked)
    }
}

impl Tool for PickTool {
    fn id(&self) -> ToolId {
        ToolId::Pick
    }

    fn on_event(&mut self, event: ToolEvent, ctx: &mut ToolContext) -> ToolResponse {
        match event {
            ToolEvent::PointerMove(cell) => {
                self.hover = cell;
                if self.dragging {
                    if let Some(c) = cell {
                        return self.sample(c, ctx);
                    }
                }
            }
            ToolEvent::PointerDown(Some(c)) => {
                self.dragging = true;
                return self.sample(c, ctx);
            }
            ToolEvent::PointerUp(_) | ToolEvent::Cancel | ToolEvent::Deactivate => {
                self.dragging = false;
            }
            _ => {}
        }
        ToolResponse::Consumed
    }

    fn overlay(&self, _view: &ToolView) -> ToolOverlay {
        if let Some(h) = self.hover {
            ToolOverlay::BrushFootprint(vec![h])
        } else {
            ToolOverlay::None
        }
    }

    fn is_busy(&self) -> bool {
        false
    }
}

pub struct ToolRegistry {
    tools: HashMap<ToolId, Box<dyn Tool>>,
    active: ToolId,
}

impl ToolRegistry {
    pub fn new() -> ToolRegistry {
        let mut tools: HashMap<ToolId, Box<dyn Tool>> = HashMap::new();
        tools.insert(
            ToolId::Paint,
            Box::new(PaintTool::new(ToolId::Paint, PaintOperation)),
        );
        tools.insert(
            ToolId::Erase,
            Box::new(EraseTool::new(ToolId::Erase, EraseOperation)),
        );
        tools.insert(ToolId::Fill, Box::new(FillTool));
        tools.insert(
            ToolId::SelectRect,
            Box::new(SelectRectTool::new(ToolId::SelectRect, RectShape)),
        );
        tools.insert(
            ToolId::SelectEllipse,
            Box::new(SelectEllipseTool::new(ToolId::SelectEllipse, EllipseShape)),
        );
        tools.insert(
            ToolId::SelectPolygon,
            Box::new(SelectPolygonTool::new()),
        );
        tools.insert(ToolId::Move, Box::new(MoveTool::new()));
        tools.insert(ToolId::Pick, Box::new(PickTool::new()));

        ToolRegistry {
            tools,
            active: ToolId::Paint,
        }
    }

    pub fn active_id(&self) -> ToolId {
        self.active
    }

    pub fn active_tool(&self) -> &dyn Tool {
        self.tools.get(&self.active).unwrap().as_ref()
    }

    pub fn active_tool_mut(&mut self) -> &mut dyn Tool {
        self.tools.get_mut(&self.active).unwrap().as_mut()
    }

    pub fn switch_to(&mut self, id: ToolId, ctx: &mut ToolContext) {
        if self.active != id {
            self.active_tool_mut().on_event(ToolEvent::Deactivate, ctx);
            self.active = id;
            self.active_tool_mut().on_event(ToolEvent::Activate, ctx);
        }
    }

    pub fn dispatch(&mut self, event: ToolEvent, ctx: &mut ToolContext) -> ToolResponse {
        self.active_tool_mut().on_event(event, ctx)
    }

    pub fn is_busy(&self) -> bool {
        self.active_tool().is_busy()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
