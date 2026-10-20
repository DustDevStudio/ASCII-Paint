use crate::model::{
    CanvasSpec, CellCoord, DomainError, Fps, Frame, FrameId, InkCell, Layer, LayerId, LayerProps,
    Project, Rgb, Rgba,
};
use crate::selection::Selection;

pub trait Command: Send + Sync {
    fn label(&self) -> &str;
    fn apply(&self, project: &mut Project);
    fn revert(&self, project: &mut Project);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionChange {
    pub before: Selection,
    pub after: Selection,
}

pub struct HistoryEntry {
    pub cmd: Box<dyn Command>,
    pub selection: Option<SelectionChange>,
}

pub struct UndoStep {
    pub label: String,
    pub selection: Option<Selection>,
}

pub struct History {
    undo_stack: Vec<HistoryEntry>,
    redo_stack: Vec<HistoryEntry>,
    limit: usize,
    clean_marker: Option<usize>,
}

impl History {
    pub fn new() -> History {
        History {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            limit: 200,
            clean_marker: Some(0),
        }
    }

    pub fn execute(&mut self, cmd: Box<dyn Command>, project: &mut Project) {
        cmd.apply(project);
        self.record_applied(cmd);
    }

    pub fn execute_with_selection(
        &mut self,
        cmd: Box<dyn Command>,
        project: &mut Project,
        sel: SelectionChange,
    ) {
        cmd.apply(project);
        self.record_applied_with_selection(cmd, sel);
    }

    pub fn record_applied(&mut self, cmd: Box<dyn Command>) {
        self.redo_stack.clear();
        if let Some(m) = self.clean_marker {
            if m > self.undo_stack.len() {
                self.clean_marker = None;
            }
        }
        self.undo_stack.push(HistoryEntry {
            cmd,
            selection: None,
        });
        if self.undo_stack.len() > self.limit {
            self.undo_stack.remove(0);
            self.clean_marker = self.clean_marker.and_then(|m| m.checked_sub(1));
        }
    }

    pub fn record_applied_with_selection(
        &mut self,
        cmd: Box<dyn Command>,
        sel: SelectionChange,
    ) {
        self.redo_stack.clear();
        if let Some(m) = self.clean_marker {
            if m > self.undo_stack.len() {
                self.clean_marker = None;
            }
        }
        self.undo_stack.push(HistoryEntry {
            cmd,
            selection: Some(sel),
        });
        if self.undo_stack.len() > self.limit {
            self.undo_stack.remove(0);
            self.clean_marker = self.clean_marker.and_then(|m| m.checked_sub(1));
        }
    }

    pub fn undo(&mut self, project: &mut Project) -> Option<UndoStep> {
        let entry = self.undo_stack.pop()?;
        entry.cmd.revert(project);
        let label = entry.cmd.label().to_string();
        let sel = entry.selection.as_ref().map(|s| s.before.clone());
        self.redo_stack.push(entry);
        Some(UndoStep {
            label,
            selection: sel,
        })
    }

    pub fn redo(&mut self, project: &mut Project) -> Option<UndoStep> {
        let entry = self.redo_stack.pop()?;
        entry.cmd.apply(project);
        let label = entry.cmd.label().to_string();
        let sel = entry.selection.as_ref().map(|s| s.after.clone());
        self.undo_stack.push(entry);
        Some(UndoStep {
            label,
            selection: sel,
        })
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo_stack.last().map(|e| e.cmd.label())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo_stack.last().map(|e| e.cmd.label())
    }

    pub fn mark_clean(&mut self) {
        self.clean_marker = Some(self.undo_stack.len());
    }

    pub fn is_dirty(&self) -> bool {
        self.clean_marker != Some(self.undo_stack.len())
    }
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct InkChange {
    pub coord: CellCoord,
    pub before: InkCell,
    pub after: InkCell,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct FillChange {
    pub coord: CellCoord,
    pub before: Rgba,
    pub after: Rgba,
}

pub struct CellPatchCommand {
    pub frame_id: FrameId,
    pub layer_id: LayerId,
    pub ink_changes: Vec<InkChange>,
    pub fill_changes: Vec<FillChange>,
    pub label: String,
}

impl Command for CellPatchCommand {
    fn label(&self) -> &str {
        &self.label
    }

    fn apply(&self, project: &mut Project) {
        let fidx = match project.timeline.index_of(self.frame_id) {
            Some(i) => i,
            None => return,
        };
        let frame = match project.timeline.frame_mut(fidx) {
            Some(f) => f,
            None => return,
        };
        let lidx = match frame.index_of_layer(self.layer_id) {
            Some(i) => i,
            None => return,
        };
        for ch in &self.ink_changes {
            frame.set_ink(self.layer_id, ch.coord, ch.after);
        }
        for ch in &self.fill_changes {
            frame.set_fill(self.layer_id, ch.coord, ch.after);
        }
        let _ = project.timeline.select(fidx);
        if let Some(f) = project.timeline.frame_mut(fidx) {
            let _ = f.select_layer(lidx);
        }
    }

    fn revert(&self, project: &mut Project) {
        let fidx = match project.timeline.index_of(self.frame_id) {
            Some(i) => i,
            None => return,
        };
        let frame = match project.timeline.frame_mut(fidx) {
            Some(f) => f,
            None => return,
        };
        let lidx = match frame.index_of_layer(self.layer_id) {
            Some(i) => i,
            None => return,
        };
        for ch in self.ink_changes.iter().rev() {
            frame.set_ink(self.layer_id, ch.coord, ch.before);
        }
        for ch in self.fill_changes.iter().rev() {
            frame.set_fill(self.layer_id, ch.coord, ch.before);
        }
        let _ = project.timeline.select(fidx);
        if let Some(f) = project.timeline.frame_mut(fidx) {
            let _ = f.select_layer(lidx);
        }
    }
}

pub struct SetFrameBackgroundCommand {
    pub frame_id: FrameId,
    pub before: Rgb,
    pub after: Rgb,
}

impl SetFrameBackgroundCommand {
    pub fn new(
        project: &Project,
        frame_id: FrameId,
        after: Rgb,
    ) -> Option<SetFrameBackgroundCommand> {
        let fidx = project.timeline.index_of(frame_id)?;
        let frame = project.timeline.frame(fidx)?;
        if frame.background == after {
            None
        } else {
            Some(SetFrameBackgroundCommand {
                frame_id,
                before: frame.background,
                after,
            })
        }
    }
}

impl Command for SetFrameBackgroundCommand {
    fn label(&self) -> &str {
        "Set frame background"
    }

    fn apply(&self, project: &mut Project) {
        if let Some(fidx) = project.timeline.index_of(self.frame_id) {
            if let Some(frame) = project.timeline.frame_mut(fidx) {
                frame.set_background(self.after);
            }
        }
    }

    fn revert(&self, project: &mut Project) {
        if let Some(fidx) = project.timeline.index_of(self.frame_id) {
            if let Some(frame) = project.timeline.frame_mut(fidx) {
                frame.set_background(self.before);
            }
        }
    }
}

pub struct AddFrameCommand {
    pub index: usize,
    pub frame: Frame,
    pub selected_before: usize,
}

impl AddFrameCommand {
    pub fn blank_after(project: &mut Project, index: usize) -> AddFrameCommand {
        let selected_before = project.timeline.selected_index();
        let bg = project
            .timeline
            .frame(index)
            .map(|f| f.background)
            .unwrap_or(Rgb::BLACK);
        let id = project.alloc_frame_id();
        let frame = Frame::blank(id, &project.canvas, bg);
        AddFrameCommand {
            index: index + 1,
            frame,
            selected_before,
        }
    }

    pub fn duplicate(project: &mut Project, index: usize) -> AddFrameCommand {
        let selected_before = project.timeline.selected_index();
        let id = project.alloc_frame_id();
        let frame = project
            .timeline
            .frame(index)
            .map(|f| f.duplicate_as(id))
            .unwrap_or_else(|| Frame::blank(id, &project.canvas, Rgb::BLACK));
        AddFrameCommand {
            index: index + 1,
            frame,
            selected_before,
        }
    }
}

impl Command for AddFrameCommand {
    fn label(&self) -> &str {
        "Add frame"
    }

    fn apply(&self, project: &mut Project) {
        project.timeline.insert(self.index, self.frame.clone());
        let _ = project.timeline.select(self.index);
    }

    fn revert(&self, project: &mut Project) {
        let _ = project.timeline.remove(self.index);
        let target = self
            .selected_before
            .min(project.timeline.len().saturating_sub(1));
        let _ = project.timeline.select(target);
    }
}

pub struct DeleteFrameCommand {
    pub index: usize,
    pub frame: Frame,
    pub selected_before: usize,
}

impl DeleteFrameCommand {
    pub fn new(project: &Project, index: usize) -> Result<DeleteFrameCommand, DomainError> {
        if project.timeline.len() <= 1 {
            return Err(DomainError::CannotRemoveLastFrame);
        }
        let frame = project
            .timeline
            .frame(index)
            .cloned()
            .ok_or(DomainError::IndexOutOfRange)?;
        Ok(DeleteFrameCommand {
            index,
            frame,
            selected_before: project.timeline.selected_index(),
        })
    }
}

impl Command for DeleteFrameCommand {
    fn label(&self) -> &str {
        "Delete frame"
    }

    fn apply(&self, project: &mut Project) {
        let _ = project.timeline.remove(self.index);
        let target = self.index.min(project.timeline.len().saturating_sub(1));
        let _ = project.timeline.select(target);
    }

    fn revert(&self, project: &mut Project) {
        project.timeline.insert(self.index, self.frame.clone());
        let _ = project.timeline.select(self.selected_before);
    }
}

pub struct MoveFrameCommand {
    pub from: usize,
    pub to: usize,
    pub selected_before: usize,
}

impl Command for MoveFrameCommand {
    fn label(&self) -> &str {
        "Move frame"
    }

    fn apply(&self, project: &mut Project) {
        let _ = project.timeline.move_frame(self.from, self.to);
        let _ = project.timeline.select(self.to);
    }

    fn revert(&self, project: &mut Project) {
        let _ = project.timeline.move_frame(self.to, self.from);
        let _ = project.timeline.select(self.selected_before);
    }
}

pub struct SetPlaybackSettingsCommand {
    pub before_fps: Fps,
    pub after_fps: Fps,
    pub before_looping: bool,
    pub after_looping: bool,
}

impl Command for SetPlaybackSettingsCommand {
    fn label(&self) -> &str {
        "Change playback settings"
    }

    fn apply(&self, project: &mut Project) {
        project.timeline.fps = self.after_fps;
        project.timeline.looping = self.after_looping;
    }

    fn revert(&self, project: &mut Project) {
        project.timeline.fps = self.before_fps;
        project.timeline.looping = self.before_looping;
    }
}

pub struct AddLayerCommand {
    pub frame_id: FrameId,
    pub index: usize,
    pub layer: Layer,
    pub active_before: usize,
}

impl AddLayerCommand {
    pub fn blank_above(
        project: &mut Project,
        frame_id: FrameId,
    ) -> Result<AddLayerCommand, DomainError> {
        let fidx = project
            .timeline
            .index_of(frame_id)
            .ok_or(DomainError::IndexOutOfRange)?;
        let frame = project.timeline.frame_mut(fidx).unwrap();
        if frame.layers().len() >= Frame::MAX_LAYERS {
            return Err(DomainError::TooManyLayers);
        }
        let active_before = frame.active_index();
        let layer_id = frame.alloc_layer_id();
        let name = format!("Layer {}", layer_id.0);
        let layer = Layer::blank(layer_id, name, &project.canvas);
        Ok(AddLayerCommand {
            frame_id,
            index: active_before + 1,
            layer,
            active_before,
        })
    }

    pub fn duplicate(
        project: &mut Project,
        frame_id: FrameId,
        index: usize,
    ) -> Result<AddLayerCommand, DomainError> {
        let fidx = project
            .timeline
            .index_of(frame_id)
            .ok_or(DomainError::IndexOutOfRange)?;
        let frame = project.timeline.frame_mut(fidx).unwrap();
        if frame.layers().len() >= Frame::MAX_LAYERS {
            return Err(DomainError::TooManyLayers);
        }
        let active_before = frame.active_index();
        let source_layer = frame
            .layer(index)
            .ok_or(DomainError::IndexOutOfRange)?
            .clone();
        let layer_id = frame.alloc_layer_id();
        let mut new_layer = source_layer;
        new_layer.id = layer_id;
        new_layer.name = format!("{} copy", new_layer.name);
        Ok(AddLayerCommand {
            frame_id,
            index: index + 1,
            layer: new_layer,
            active_before,
        })
    }

    pub fn with_layer(
        project: &Project,
        frame_id: FrameId,
        layer: Layer,
    ) -> Result<AddLayerCommand, DomainError> {
        let fidx = project
            .timeline
            .index_of(frame_id)
            .ok_or(DomainError::IndexOutOfRange)?;
        let frame = project.timeline.frame(fidx).unwrap();
        if frame.layers().len() >= Frame::MAX_LAYERS {
            return Err(DomainError::TooManyLayers);
        }
        let active_before = frame.active_index();
        Ok(AddLayerCommand {
            frame_id,
            index: active_before + 1,
            layer,
            active_before,
        })
    }
}

impl Command for AddLayerCommand {
    fn label(&self) -> &str {
        "Add layer"
    }

    fn apply(&self, project: &mut Project) {
        if let Some(fidx) = project.timeline.index_of(self.frame_id) {
            let _ = project.timeline.select(fidx);
            if let Some(frame) = project.timeline.frame_mut(fidx) {
                let _ = frame.insert_layer(self.index, self.layer.clone());
                let _ = frame.select_layer(self.index);
            }
        }
    }

    fn revert(&self, project: &mut Project) {
        if let Some(fidx) = project.timeline.index_of(self.frame_id) {
            let _ = project.timeline.select(fidx);
            if let Some(frame) = project.timeline.frame_mut(fidx) {
                let _ = frame.remove_layer(self.index);
                let target = self
                    .active_before
                    .min(frame.layers().len().saturating_sub(1));
                let _ = frame.select_layer(target);
            }
        }
    }
}

pub struct DeleteLayerCommand {
    pub frame_id: FrameId,
    pub index: usize,
    pub layer: Layer,
    pub active_before: usize,
}

impl DeleteLayerCommand {
    pub fn new(
        project: &Project,
        frame_id: FrameId,
        index: usize,
    ) -> Result<DeleteLayerCommand, DomainError> {
        let fidx = project
            .timeline
            .index_of(frame_id)
            .ok_or(DomainError::IndexOutOfRange)?;
        let frame = project.timeline.frame(fidx).unwrap();
        if frame.layers().len() <= 1 {
            return Err(DomainError::CannotRemoveLastLayer);
        }
        let layer = frame
            .layer(index)
            .cloned()
            .ok_or(DomainError::IndexOutOfRange)?;
        Ok(DeleteLayerCommand {
            frame_id,
            index,
            layer,
            active_before: frame.active_index(),
        })
    }
}

impl Command for DeleteLayerCommand {
    fn label(&self) -> &str {
        "Delete layer"
    }

    fn apply(&self, project: &mut Project) {
        if let Some(fidx) = project.timeline.index_of(self.frame_id) {
            let _ = project.timeline.select(fidx);
            if let Some(frame) = project.timeline.frame_mut(fidx) {
                let _ = frame.remove_layer(self.index);
                let target = self.index.min(frame.layers().len().saturating_sub(1));
                let _ = frame.select_layer(target);
            }
        }
    }

    fn revert(&self, project: &mut Project) {
        if let Some(fidx) = project.timeline.index_of(self.frame_id) {
            let _ = project.timeline.select(fidx);
            if let Some(frame) = project.timeline.frame_mut(fidx) {
                let _ = frame.insert_layer(self.index, self.layer.clone());
                let _ = frame.select_layer(self.active_before);
            }
        }
    }
}

pub struct MoveLayerCommand {
    pub frame_id: FrameId,
    pub from: usize,
    pub to: usize,
    pub active_before: usize,
}

impl Command for MoveLayerCommand {
    fn label(&self) -> &str {
        "Move layer"
    }

    fn apply(&self, project: &mut Project) {
        if let Some(fidx) = project.timeline.index_of(self.frame_id) {
            let _ = project.timeline.select(fidx);
            if let Some(frame) = project.timeline.frame_mut(fidx) {
                let _ = frame.move_layer(self.from, self.to);
                let _ = frame.select_layer(self.to);
            }
        }
    }

    fn revert(&self, project: &mut Project) {
        if let Some(fidx) = project.timeline.index_of(self.frame_id) {
            let _ = project.timeline.select(fidx);
            if let Some(frame) = project.timeline.frame_mut(fidx) {
                let _ = frame.move_layer(self.to, self.from);
                let _ = frame.select_layer(self.active_before);
            }
        }
    }
}

pub struct SetLayerPropsCommand {
    pub frame_id: FrameId,
    pub layer_id: LayerId,
    pub before: LayerProps,
    pub after: LayerProps,
}

impl SetLayerPropsCommand {
    pub fn new(
        project: &Project,
        frame_id: FrameId,
        layer_id: LayerId,
        after: LayerProps,
    ) -> Option<SetLayerPropsCommand> {
        let fidx = project.timeline.index_of(frame_id)?;
        let frame = project.timeline.frame(fidx)?;
        let lidx = frame.index_of_layer(layer_id)?;
        let before = frame.layer(lidx)?.props();
        if before == after {
            None
        } else {
            Some(SetLayerPropsCommand {
                frame_id,
                layer_id,
                before,
                after,
            })
        }
    }
}

impl Command for SetLayerPropsCommand {
    fn label(&self) -> &str {
        "Set layer properties"
    }

    fn apply(&self, project: &mut Project) {
        if let Some(fidx) = project.timeline.index_of(self.frame_id) {
            if let Some(frame) = project.timeline.frame_mut(fidx) {
                frame.set_layer_props(self.layer_id, self.after.clone());
            }
        }
    }

    fn revert(&self, project: &mut Project) {
        if let Some(fidx) = project.timeline.index_of(self.frame_id) {
            if let Some(frame) = project.timeline.frame_mut(fidx) {
                frame.set_layer_props(self.layer_id, self.before.clone());
            }
        }
    }
}

pub struct ResizeCanvasCommand {
    pub before: CanvasSpec,
    pub after: CanvasSpec,
}

impl ResizeCanvasCommand {
    pub fn new(project: &Project, after: CanvasSpec) -> Option<ResizeCanvasCommand> {
        if project.canvas == after {
            None
        } else {
            Some(ResizeCanvasCommand {
                before: project.canvas,
                after,
            })
        }
    }
}

impl Command for ResizeCanvasCommand {
    fn label(&self) -> &str {
        "Resize canvas"
    }

    fn apply(&self, project: &mut Project) {
        project.resize_canvas(self.after);
    }

    fn revert(&self, project: &mut Project) {
        project.resize_canvas(self.before);
    }
}

pub struct CompositeCommand {
    pub label: String,
    pub children: Vec<Box<dyn Command>>,
}

impl CompositeCommand {
    pub fn new(label: &str, children: Vec<Box<dyn Command>>) -> Self {
        Self {
            label: label.to_string(),
            children,
        }
    }
}

impl Command for CompositeCommand {
    fn label(&self) -> &str {
        &self.label
    }

    fn apply(&self, project: &mut Project) {
        for child in &self.children {
            child.apply(project);
        }
    }

    fn revert(&self, project: &mut Project) {
        for child in self.children.iter().rev() {
            child.revert(project);
        }
    }
}
