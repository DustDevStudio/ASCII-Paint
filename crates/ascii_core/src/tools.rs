use std::collections::HashMap;

pub use crate::algo::PaintChannel;
use crate::algo::{FloodFill, ShapeRasterizer};
use crate::history::{CellChange, CellPatchCommand, History};
use crate::model::{CanvasSpec, Cell, CellCoord, CellRect, FrameId, Project, Rgb};
use crate::selection::{Selection, SelectionMode};

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum ToolId {
    EditCell,
    ClearCell,
    PaintCell,
    FillCell,
    SelectRect,
    SelectPolygon,
    SelectEllipse,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Brush {
    pub paint: Rgb,
    pub erase: Option<Rgb>,
    pub channel: PaintChannel,
}

impl Brush {
    pub fn new(paint: Rgb, erase: Option<Rgb>, channel: PaintChannel) -> Self {
        Self {
            paint,
            erase,
            channel,
        }
    }
}

impl Default for Brush {
    fn default() -> Self {
        Self {
            paint: Rgb::WHITE,
            erase: None,
            channel: PaintChannel::Foreground,
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

impl Modifiers {
    pub fn selection_mode(&self) -> SelectionMode {
        if self.shift {
            SelectionMode::Add
        } else if self.alt {
            SelectionMode::Subtract
        } else {
            SelectionMode::Replace
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ToolKey {
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Enter,
    Backspace,
    Delete,
    Escape,
}

#[derive(Clone, PartialEq, Debug)]
pub enum ToolEvent {
    Activate,
    Deactivate,
    PointerDown(CellCoord, Modifiers),
    PointerMove(Option<CellCoord>, Modifiers),
    PointerUp(Option<CellCoord>, Modifiers),
    DoubleClick(CellCoord, Modifiers),
    Key(ToolKey, Modifiers),
    Text(char),
    Cancel,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ToolResponse {
    Consumed,
    Ignored,
}

pub struct ToolContext<'a> {
    pub project: &'a mut Project,
    pub selection: &'a mut Selection,
    pub brush: &'a Brush,
    pub history: &'a mut History,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ToolOverlay {
    None,
    TextCursor(CellCoord),
    RectPreview(CellRect, SelectionMode),
    EllipsePreview(CellRect, SelectionMode),
    PolygonPreview(Vec<CellCoord>, Option<CellCoord>, SelectionMode),
}

pub trait Tool: Send + Sync {
    fn id(&self) -> ToolId;
    fn display_name(&self) -> &str;
    fn on_event(&mut self, ev: ToolEvent, ctx: &mut ToolContext) -> ToolResponse;
    fn overlay(&self) -> ToolOverlay;
    fn is_busy(&self) -> bool;
}

#[derive(Clone, Debug)]
pub struct PatchBuilder {
    frame_id: FrameId,
    order: Vec<CellCoord>,
    before: HashMap<CellCoord, Cell>,
    after: HashMap<CellCoord, Cell>,
}

impl PatchBuilder {
    pub fn new(frame_id: FrameId) -> Self {
        Self {
            frame_id,
            order: Vec::new(),
            before: HashMap::new(),
            after: HashMap::new(),
        }
    }

    pub fn write(&mut self, project: &mut Project, c: CellCoord, new: Cell) -> bool {
        let frame = project.timeline.selected_frame_mut();
        let old = match frame.grid.get(c) {
            Some(cell) => *cell,
            None => return false,
        };

        if old == new {
            return false;
        }

        self.before.entry(c).or_insert(old);
        frame.set_cell(c, new);
        self.after.insert(c, new);

        if !self.order.contains(&c) {
            self.order.push(c);
        }

        true
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    pub fn finish(self, label: &str) -> Option<CellPatchCommand> {
        let mut changes = Vec::new();
        for c in self.order {
            if let (Some(&before), Some(&after)) = (self.before.get(&c), self.after.get(&c)) {
                if before != after {
                    changes.push(CellChange {
                        coord: c,
                        before,
                        after,
                    });
                }
            }
        }

        if changes.is_empty() {
            None
        } else {
            Some(CellPatchCommand {
                frame_id: self.frame_id,
                changes,
                label: label.to_string(),
            })
        }
    }
}

pub trait CellOperation: Send + Sync {
    fn label(&self) -> &str;
    fn apply(&self, old: Cell, brush: &Brush) -> Cell;
}

#[derive(Copy, Clone, Debug)]
pub struct ClearOperation;

impl CellOperation for ClearOperation {
    fn label(&self) -> &str {
        "Clear cells"
    }

    fn apply(&self, _old: Cell, brush: &Brush) -> Cell {
        Cell {
            glyph: ' ',
            fg: Rgb::WHITE,
            bg: brush.erase,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct PaintOperation;

impl CellOperation for PaintOperation {
    fn label(&self) -> &str {
        "Paint cells"
    }

    fn apply(&self, old: Cell, brush: &Brush) -> Cell {
        match brush.channel {
            PaintChannel::Foreground => Cell {
                fg: brush.paint,
                ..old
            },
            PaintChannel::Background => Cell {
                bg: Some(brush.paint),
                ..old
            },
        }
    }
}

#[derive(Clone, Debug)]
struct StrokeState {
    last: CellCoord,
    builder: PatchBuilder,
}

pub struct StrokeTool<O: CellOperation> {
    id: ToolId,
    name: &'static str,
    op: O,
    stroke: Option<StrokeState>,
}

impl<O: CellOperation> StrokeTool<O> {
    pub fn new(id: ToolId, name: &'static str, op: O) -> Self {
        Self {
            id,
            name,
            op,
            stroke: None,
        }
    }

    fn commit(&mut self, ctx: &mut ToolContext) {
        if let Some(state) = self.stroke.take() {
            if let Some(cmd) = state.builder.finish(self.op.label()) {
                ctx.history.record_applied(Box::new(cmd));
            }
        }
    }
}

impl<O: CellOperation> Tool for StrokeTool<O> {
    fn id(&self) -> ToolId {
        self.id
    }

    fn display_name(&self) -> &str {
        self.name
    }

    fn on_event(&mut self, ev: ToolEvent, ctx: &mut ToolContext) -> ToolResponse {
        match ev {
            ToolEvent::PointerDown(c, _mods) => {
                let frame_id = ctx.project.timeline.selected_frame().id;
                let mut builder = PatchBuilder::new(frame_id);
                if ctx.selection.permits(c) {
                    let old = ctx
                        .project
                        .timeline
                        .selected_frame()
                        .grid
                        .get(c)
                        .copied()
                        .unwrap_or_default();
                    builder.write(ctx.project, c, self.op.apply(old, ctx.brush));
                }
                self.stroke = Some(StrokeState { last: c, builder });
                ToolResponse::Consumed
            }
            ToolEvent::PointerMove(Some(c), _mods) => {
                if let Some(ref mut state) = self.stroke {
                    let pts = ShapeRasterizer::line(state.last, c);
                    for pt in pts.into_iter().skip(1) {
                        if ctx.selection.permits(pt) {
                            let old = ctx
                                .project
                                .timeline
                                .selected_frame()
                                .grid
                                .get(pt)
                                .copied()
                                .unwrap_or_default();
                            state
                                .builder
                                .write(ctx.project, pt, self.op.apply(old, ctx.brush));
                        }
                    }
                    state.last = c;
                    ToolResponse::Consumed
                } else {
                    ToolResponse::Ignored
                }
            }
            ToolEvent::PointerMove(None, _mods) => {
                if self.stroke.is_some() {
                    ToolResponse::Consumed
                } else {
                    ToolResponse::Ignored
                }
            }
            ToolEvent::PointerUp(_, _) | ToolEvent::Cancel | ToolEvent::Deactivate => {
                if self.stroke.is_some() {
                    self.commit(ctx);
                    ToolResponse::Consumed
                } else {
                    ToolResponse::Ignored
                }
            }
            _ => ToolResponse::Ignored,
        }
    }

    fn overlay(&self) -> ToolOverlay {
        ToolOverlay::None
    }

    fn is_busy(&self) -> bool {
        self.stroke.is_some()
    }
}

pub type ClearCellTool = StrokeTool<ClearOperation>;
pub type PaintCellTool = StrokeTool<PaintOperation>;

pub struct EditCellTool {
    cursor: Option<CellCoord>,
    anchor_col: u16,
    session: Option<PatchBuilder>,
}

impl EditCellTool {
    pub fn new() -> Self {
        Self {
            cursor: None,
            anchor_col: 0,
            session: None,
        }
    }

    fn commit(&mut self, ctx: &mut ToolContext) {
        if let Some(builder) = self.session.take() {
            if let Some(cmd) = builder.finish("Edit text") {
                ctx.history.record_applied(Box::new(cmd));
            }
        }
    }
}

impl Default for EditCellTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Tool for EditCellTool {
    fn id(&self) -> ToolId {
        ToolId::EditCell
    }

    fn display_name(&self) -> &str {
        "Edit text"
    }

    fn on_event(&mut self, ev: ToolEvent, ctx: &mut ToolContext) -> ToolResponse {
        match ev {
            ToolEvent::PointerDown(c, _mods) => {
                if !ctx.selection.permits(c) {
                    return ToolResponse::Ignored;
                }
                self.commit(ctx);
                self.cursor = Some(c);
                self.anchor_col = c.col;
                ToolResponse::Consumed
            }
            ToolEvent::Text(ch) => {
                if ch.is_control() {
                    return ToolResponse::Ignored;
                }
                let cursor = match self.cursor {
                    Some(c) => c,
                    None => return ToolResponse::Ignored,
                };

                if !ctx.selection.permits(cursor) {
                    return ToolResponse::Consumed;
                }

                if self.session.is_none() {
                    let frame_id = ctx.project.timeline.selected_frame().id;
                    self.session = Some(PatchBuilder::new(frame_id));
                }

                let old = ctx
                    .project
                    .timeline
                    .selected_frame()
                    .grid
                    .get(cursor)
                    .copied()
                    .unwrap_or_default();
                let new_cell = Cell {
                    glyph: ch,
                    fg: ctx.brush.paint,
                    bg: old.bg,
                };

                if let Some(ref mut session) = self.session {
                    session.write(ctx.project, cursor, new_cell);
                }

                let max_col = ctx.project.canvas.cols.saturating_sub(1);
                let next = CellCoord::new(cursor.col.saturating_add(1).min(max_col), cursor.row);
                if ctx.selection.permits(next) {
                    self.cursor = Some(next);
                }
                ToolResponse::Consumed
            }
            ToolEvent::Key(ToolKey::Enter, _mods) => {
                if let Some(cursor) = self.cursor {
                    let max_col = ctx.project.canvas.cols.saturating_sub(1);
                    let max_row = ctx.project.canvas.rows.saturating_sub(1);
                    self.cursor = Some(CellCoord::new(
                        self.anchor_col.min(max_col),
                        cursor.row.saturating_add(1).min(max_row),
                    ));
                    ToolResponse::Consumed
                } else {
                    ToolResponse::Ignored
                }
            }
            ToolEvent::Key(ToolKey::Backspace, _mods) => {
                if let Some(mut cursor) = self.cursor {
                    if cursor.col > 0 {
                        cursor.col -= 1;
                        self.cursor = Some(cursor);
                    }
                    if ctx.selection.permits(cursor) {
                        if self.session.is_none() {
                            let frame_id = ctx.project.timeline.selected_frame().id;
                            self.session = Some(PatchBuilder::new(frame_id));
                        }
                        let old = ctx
                            .project
                            .timeline
                            .selected_frame()
                            .grid
                            .get(cursor)
                            .copied()
                            .unwrap_or_default();
                        let new_cell = Cell {
                            glyph: ' ',
                            ..old
                        };
                        if let Some(ref mut session) = self.session {
                            session.write(ctx.project, cursor, new_cell);
                        }
                    }
                    ToolResponse::Consumed
                } else {
                    ToolResponse::Ignored
                }
            }
            ToolEvent::Key(ToolKey::Delete, _mods) => {
                if let Some(cursor) = self.cursor {
                    if ctx.selection.permits(cursor) {
                        if self.session.is_none() {
                            let frame_id = ctx.project.timeline.selected_frame().id;
                            self.session = Some(PatchBuilder::new(frame_id));
                        }
                        let old = ctx
                            .project
                            .timeline
                            .selected_frame()
                            .grid
                            .get(cursor)
                            .copied()
                            .unwrap_or_default();
                        let new_cell = Cell {
                            glyph: ' ',
                            ..old
                        };
                        if let Some(ref mut session) = self.session {
                            session.write(ctx.project, cursor, new_cell);
                        }
                    }
                    ToolResponse::Consumed
                } else {
                    ToolResponse::Ignored
                }
            }
            ToolEvent::Key(
                k @ (ToolKey::ArrowUp | ToolKey::ArrowDown | ToolKey::ArrowLeft | ToolKey::ArrowRight),
                _mods,
            ) => {
                if let Some(cursor) = self.cursor {
                    self.commit(ctx);
                    let mut col = cursor.col as i32;
                    let mut row = cursor.row as i32;
                    match k {
                        ToolKey::ArrowUp => row -= 1,
                        ToolKey::ArrowDown => row += 1,
                        ToolKey::ArrowLeft => col -= 1,
                        ToolKey::ArrowRight => col += 1,
                        _ => {}
                    }
                    let new_c = ctx.project.canvas.clamp(col, row);
                    self.cursor = Some(new_c);
                    if matches!(k, ToolKey::ArrowLeft | ToolKey::ArrowRight) {
                        self.anchor_col = new_c.col;
                    }
                    ToolResponse::Consumed
                } else {
                    ToolResponse::Ignored
                }
            }
            ToolEvent::Key(ToolKey::Escape, _) | ToolEvent::Cancel | ToolEvent::Deactivate => {
                self.commit(ctx);
                self.cursor = None;
                ToolResponse::Consumed
            }
            _ => ToolResponse::Ignored,
        }
    }

    fn overlay(&self) -> ToolOverlay {
        match self.cursor {
            Some(c) => ToolOverlay::TextCursor(c),
            None => ToolOverlay::None,
        }
    }

    fn is_busy(&self) -> bool {
        self.cursor.is_some() || self.session.as_ref().map_or(false, |s| !s.is_empty())
    }
}

pub struct FillCellTool;

impl FillCellTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for FillCellTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Tool for FillCellTool {
    fn id(&self) -> ToolId {
        ToolId::FillCell
    }

    fn display_name(&self) -> &str {
        "Fill cells"
    }

    fn on_event(&mut self, ev: ToolEvent, ctx: &mut ToolContext) -> ToolResponse {
        if let ToolEvent::PointerDown(c, _mods) = ev {
            if !ctx.selection.is_empty() && !ctx.selection.contains(c) {
                return ToolResponse::Consumed;
            }

            let frame = ctx.project.timeline.selected_frame();
            let seed = match frame.grid.get(c) {
                Some(s) => *s,
                None => return ToolResponse::Consumed,
            };

            let already_painted = match ctx.brush.channel {
                PaintChannel::Foreground => seed.fg == ctx.brush.paint,
                PaintChannel::Background => seed.bg == Some(ctx.brush.paint),
            };

            if already_painted {
                return ToolResponse::Consumed;
            }

            let cells =
                FloodFill::region(&frame.grid, c, ctx.brush.channel, ctx.selection);
            if cells.is_empty() {
                return ToolResponse::Consumed;
            }

            let frame_id = frame.id;
            let mut builder = PatchBuilder::new(frame_id);
            for k in cells {
                let old = ctx
                    .project
                    .timeline
                    .selected_frame()
                    .grid
                    .get(k)
                    .copied()
                    .unwrap_or_default();
                let new_cell = match ctx.brush.channel {
                    PaintChannel::Foreground => Cell {
                        fg: ctx.brush.paint,
                        ..old
                    },
                    PaintChannel::Background => Cell {
                        bg: Some(ctx.brush.paint),
                        ..old
                    },
                };
                builder.write(ctx.project, k, new_cell);
            }

            if let Some(cmd) = builder.finish("Fill cells") {
                ctx.history.record_applied(Box::new(cmd));
            }
            ToolResponse::Consumed
        } else {
            ToolResponse::Ignored
        }
    }

    fn overlay(&self) -> ToolOverlay {
        ToolOverlay::None
    }

    fn is_busy(&self) -> bool {
        false
    }
}

pub trait DragShape: Send + Sync {
    fn label(&self) -> &str;
    fn rasterize(&self, a: CellCoord, b: CellCoord, spec: &CanvasSpec) -> Vec<CellCoord>;
    fn make_overlay(&self, rect: CellRect, mode: SelectionMode) -> ToolOverlay;
}

#[derive(Copy, Clone, Debug)]
pub struct RectShape;

impl DragShape for RectShape {
    fn label(&self) -> &str {
        "Square select"
    }

    fn rasterize(&self, a: CellCoord, b: CellCoord, spec: &CanvasSpec) -> Vec<CellCoord> {
        ShapeRasterizer::rect(a, b, spec)
    }

    fn make_overlay(&self, rect: CellRect, mode: SelectionMode) -> ToolOverlay {
        ToolOverlay::RectPreview(rect, mode)
    }
}

#[derive(Copy, Clone, Debug)]
pub struct EllipseShape;

impl DragShape for EllipseShape {
    fn label(&self) -> &str {
        "Ellipse select"
    }

    fn rasterize(&self, a: CellCoord, b: CellCoord, spec: &CanvasSpec) -> Vec<CellCoord> {
        ShapeRasterizer::ellipse(a, b, spec)
    }

    fn make_overlay(&self, rect: CellRect, mode: SelectionMode) -> ToolOverlay {
        ToolOverlay::EllipsePreview(rect, mode)
    }
}

#[derive(Clone, Debug)]
struct DragState {
    anchor: CellCoord,
    current: CellCoord,
    mode: SelectionMode,
}

pub struct ShapeSelectTool<S: DragShape> {
    id: ToolId,
    shape: S,
    drag: Option<DragState>,
}

impl<S: DragShape> ShapeSelectTool<S> {
    pub fn new(id: ToolId, shape: S) -> Self {
        Self {
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

    fn display_name(&self) -> &str {
        self.shape.label()
    }

    fn on_event(&mut self, ev: ToolEvent, ctx: &mut ToolContext) -> ToolResponse {
        match ev {
            ToolEvent::PointerDown(c, mods) => {
                self.drag = Some(DragState {
                    anchor: c,
                    current: c,
                    mode: mods.selection_mode(),
                });
                ToolResponse::Consumed
            }
            ToolEvent::PointerMove(Some(c), _mods) => {
                if let Some(ref mut drag) = self.drag {
                    drag.current = c;
                    ToolResponse::Consumed
                } else {
                    ToolResponse::Ignored
                }
            }
            ToolEvent::PointerMove(None, _mods) => {
                if self.drag.is_some() {
                    ToolResponse::Consumed
                } else {
                    ToolResponse::Ignored
                }
            }
            ToolEvent::PointerUp(_, _) => {
                if let Some(drag) = self.drag.take() {
                    let cells = self.shape.rasterize(
                        drag.anchor,
                        drag.current,
                        &ctx.project.canvas,
                    );
                    ctx.selection.apply(&cells, drag.mode);
                    ToolResponse::Consumed
                } else {
                    ToolResponse::Ignored
                }
            }
            ToolEvent::Key(ToolKey::Escape, _) | ToolEvent::Cancel | ToolEvent::Deactivate => {
                self.drag = None;
                ToolResponse::Consumed
            }
            _ => ToolResponse::Ignored,
        }
    }

    fn overlay(&self) -> ToolOverlay {
        if let Some(ref drag) = self.drag {
            let rect = CellRect::from_corners(drag.anchor, drag.current);
            self.shape.make_overlay(rect, drag.mode)
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
        Self {
            vertices: Vec::new(),
            hover: None,
            mode: SelectionMode::Replace,
        }
    }

    fn close(&mut self, ctx: &mut ToolContext) {
        if self.vertices.len() >= 3 {
            let cells =
                ShapeRasterizer::polygon(&self.vertices, &ctx.project.canvas);
            ctx.selection.apply(&cells, self.mode);
        }
        self.vertices.clear();
        self.hover = None;
    }
}

impl Default for SelectPolygonTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Tool for SelectPolygonTool {
    fn id(&self) -> ToolId {
        ToolId::SelectPolygon
    }

    fn display_name(&self) -> &str {
        "Polygon select"
    }

    fn on_event(&mut self, ev: ToolEvent, ctx: &mut ToolContext) -> ToolResponse {
        match ev {
            ToolEvent::PointerDown(c, mods) => {
                if self.vertices.is_empty() {
                    self.mode = mods.selection_mode();
                    self.vertices.push(c);
                    ToolResponse::Consumed
                } else if Some(&c) == self.vertices.last() {
                    ToolResponse::Consumed
                } else if c == self.vertices[0] {
                    if self.vertices.len() >= 3 {
                        self.close(ctx);
                    }
                    ToolResponse::Consumed
                } else {
                    self.vertices.push(c);
                    ToolResponse::Consumed
                }
            }
            ToolEvent::PointerMove(cell, _mods) => {
                if !self.vertices.is_empty() {
                    self.hover = cell;
                    ToolResponse::Consumed
                } else {
                    ToolResponse::Ignored
                }
            }
            ToolEvent::DoubleClick(_, _) | ToolEvent::Key(ToolKey::Enter, _) => {
                if !self.vertices.is_empty() {
                    self.close(ctx);
                    ToolResponse::Consumed
                } else {
                    ToolResponse::Ignored
                }
            }
            ToolEvent::Key(ToolKey::Backspace, _) => {
                if !self.vertices.is_empty() {
                    self.vertices.pop();
                    if self.vertices.is_empty() {
                        self.hover = None;
                    }
                    ToolResponse::Consumed
                } else {
                    ToolResponse::Ignored
                }
            }
            ToolEvent::Key(ToolKey::Escape, _) | ToolEvent::Cancel | ToolEvent::Deactivate => {
                self.vertices.clear();
                self.hover = None;
                ToolResponse::Consumed
            }
            _ => ToolResponse::Ignored,
        }
    }

    fn overlay(&self) -> ToolOverlay {
        if self.vertices.is_empty() {
            ToolOverlay::None
        } else {
            ToolOverlay::PolygonPreview(self.vertices.clone(), self.hover, self.mode)
        }
    }

    fn is_busy(&self) -> bool {
        !self.vertices.is_empty()
    }
}

pub struct ToolRegistry {
    tools: Vec<Box<dyn Tool>>,
    active: ToolId,
}

impl ToolRegistry {
    pub fn new() -> ToolRegistry {
        let tools: Vec<Box<dyn Tool>> = vec![
            Box::new(EditCellTool::new()),
            Box::new(ClearCellTool::new(
                ToolId::ClearCell,
                "Clear cell",
                ClearOperation,
            )),
            Box::new(PaintCellTool::new(
                ToolId::PaintCell,
                "Paint cell",
                PaintOperation,
            )),
            Box::new(FillCellTool::new()),
            Box::new(SelectRectTool::new(ToolId::SelectRect, RectShape)),
            Box::new(SelectPolygonTool::new()),
            Box::new(SelectEllipseTool::new(ToolId::SelectEllipse, EllipseShape)),
        ];

        ToolRegistry {
            tools,
            active: ToolId::EditCell,
        }
    }

    pub fn active_id(&self) -> ToolId {
        self.active
    }

    pub fn active(&self) -> &dyn Tool {
        self.tools
            .iter()
            .find(|t| t.id() == self.active)
            .map(|t| t.as_ref())
            .expect("active tool exists")
    }

    pub fn active_mut(&mut self) -> &mut dyn Tool {
        let id = self.active;
        self.tools
            .iter_mut()
            .find(|t| t.id() == id)
            .map(|t| t.as_mut())
            .expect("active tool exists")
    }

    pub fn switch_to(&mut self, id: ToolId, ctx: &mut ToolContext) {
        if self.active == id {
            return;
        }
        self.active_mut().on_event(ToolEvent::Deactivate, ctx);
        self.active = id;
        self.active_mut().on_event(ToolEvent::Activate, ctx);
    }

    pub fn dispatch(&mut self, ev: ToolEvent, ctx: &mut ToolContext) -> ToolResponse {
        self.active_mut().on_event(ev, ctx)
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
