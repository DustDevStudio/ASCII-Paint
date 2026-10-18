use crate::model::{
    Cell, CellCoord, DomainError, Fps, Frame, FrameId, Project, Rgb,
};

pub trait Command: Send + Sync {
    fn label(&self) -> &str;
    fn apply(&self, project: &mut Project);
    fn revert(&self, project: &mut Project);
}

pub struct History {
    undo_stack: Vec<Box<dyn Command>>,
    redo_stack: Vec<Box<dyn Command>>,
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

    pub fn record_applied(&mut self, cmd: Box<dyn Command>) {
        if let Some(m) = self.clean_marker {
            if m > self.undo_stack.len() {
                self.clean_marker = None;
            }
        }
        self.redo_stack.clear();
        self.undo_stack.push(cmd);

        if self.undo_stack.len() > self.limit {
            self.undo_stack.remove(0);
            self.clean_marker = self.clean_marker.and_then(|m| m.checked_sub(1));
        }
    }

    pub fn undo(&mut self, project: &mut Project) -> Option<String> {
        if let Some(cmd) = self.undo_stack.pop() {
            cmd.revert(project);
            let label = cmd.label().to_string();
            self.redo_stack.push(cmd);
            Some(label)
        } else {
            None
        }
    }

    pub fn redo(&mut self, project: &mut Project) -> Option<String> {
        if let Some(cmd) = self.redo_stack.pop() {
            cmd.apply(project);
            let label = cmd.label().to_string();
            self.undo_stack.push(cmd);
            Some(label)
        } else {
            None
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo_stack.last().map(|c| c.label())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo_stack.last().map(|c| c.label())
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
pub struct CellChange {
    pub coord: CellCoord,
    pub before: Cell,
    pub after: Cell,
}

pub struct CellPatchCommand {
    pub frame_id: FrameId,
    pub changes: Vec<CellChange>,
    pub label: String,
}

impl Command for CellPatchCommand {
    fn label(&self) -> &str {
        &self.label
    }

    fn apply(&self, project: &mut Project) {
        if let Some(idx) = project.timeline.index_of(self.frame_id) {
            if let Some(frame) = project.timeline.frame_mut(idx) {
                for ch in &self.changes {
                    frame.set_cell(ch.coord, ch.after);
                }
            }
            let _ = project.timeline.select(idx);
        }
    }

    fn revert(&self, project: &mut Project) {
        if let Some(idx) = project.timeline.index_of(self.frame_id) {
            if let Some(frame) = project.timeline.frame_mut(idx) {
                for ch in self.changes.iter().rev() {
                    frame.set_cell(ch.coord, ch.before);
                }
            }
            let _ = project.timeline.select(idx);
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
        let idx = project.timeline.index_of(frame_id)?;
        let frame = project.timeline.frame(idx)?;
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
        if let Some(idx) = project.timeline.index_of(self.frame_id) {
            if let Some(frame) = project.timeline.frame_mut(idx) {
                frame.set_background(self.after);
            }
            let _ = project.timeline.select(idx);
        }
    }

    fn revert(&self, project: &mut Project) {
        if let Some(idx) = project.timeline.index_of(self.frame_id) {
            if let Some(frame) = project.timeline.frame_mut(idx) {
                frame.set_background(self.before);
            }
            let _ = project.timeline.select(idx);
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
        let canvas = project.canvas;
        let bg = project
            .timeline
            .frame(index)
            .map(|f| f.background)
            .unwrap_or(Rgb::BLACK);
        let id = project.alloc_frame_id();
        let frame = Frame::blank(id, &canvas, bg);
        let selected_before = project.timeline.selected_index();
        AddFrameCommand {
            index: index + 1,
            frame,
            selected_before,
        }
    }

    pub fn duplicate(project: &mut Project, index: usize) -> AddFrameCommand {
        let src = project.timeline.frame(index).cloned().unwrap_or_else(|| {
            let id = project.alloc_frame_id();
            Frame::blank(id, &project.canvas, Rgb::BLACK)
        });
        let new_id = project.alloc_frame_id();
        let frame = src.duplicate_as(new_id);
        let selected_before = project.timeline.selected_index();
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
        let sel = self
            .selected_before
            .min(project.timeline.len().saturating_sub(1));
        let _ = project.timeline.select(sel);
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
            .ok_or(DomainError::IndexOutOfRange)?
            .clone();
        let selected_before = project.timeline.selected_index();
        Ok(DeleteFrameCommand {
            index,
            frame,
            selected_before,
        })
    }
}

impl Command for DeleteFrameCommand {
    fn label(&self) -> &str {
        "Delete frame"
    }

    fn apply(&self, project: &mut Project) {
        let _ = project.timeline.remove(self.index);
        let sel = self.index.min(project.timeline.len().saturating_sub(1));
        let _ = project.timeline.select(sel);
    }

    fn revert(&self, project: &mut Project) {
        project.timeline.insert(self.index, self.frame.clone());
        let sel = self
            .selected_before
            .min(project.timeline.len().saturating_sub(1));
        let _ = project.timeline.select(sel);
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
        let sel = self
            .selected_before
            .min(project.timeline.len().saturating_sub(1));
        let _ = project.timeline.select(sel);
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

pub struct CompositeCommand {
    pub label: String,
    pub children: Vec<Box<dyn Command>>,
}

impl CompositeCommand {
    pub fn new(label: impl Into<String>, children: Vec<Box<dyn Command>>) -> Self {
        Self {
            label: label.into(),
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
