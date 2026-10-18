use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use ascii_core::history::{
    AddFrameCommand, Command, CompositeCommand, DeleteFrameCommand, History, MoveFrameCommand,
    SetFrameBackgroundCommand, SetPlaybackSettingsCommand,
};
use ascii_core::model::{CellCoord, Fps, FrameId, Project, Rgb};
use ascii_core::selection::Selection;
use ascii_core::tools::{
    Brush, CellOperation, ClearOperation, PaintChannel, PatchBuilder, ToolContext, ToolEvent,
    ToolId, ToolRegistry,
};
use ascii_render::GlyphSource;

use crate::canvas_view::CanvasView;
use crate::playback::PlaybackController;
use crate::thumbnail_cache::ThumbnailCache;
use crate::types::{AppAction, ColorPickTarget, Services};
use crate::viewport::Viewport;

pub struct EditorSession {
    pub project: Project,
    pub path: PathBuf,
    pub history: History,
    pub selection: Selection,
    pub tool_registry: ToolRegistry,
    pub brush: Brush,
    pub viewport: Viewport,
    pub playback: PlaybackController,
    pub thumbnail_cache: ThumbnailCache,
    pub show_grid: bool,
    pub toast: Option<(String, f64)>,
}

impl EditorSession {
    pub fn new(project: Project, path: PathBuf) -> Self {
        let selection = Selection::new(&project.canvas);
        let brush = Brush::default();
        let tool_registry = ToolRegistry::new();
        let viewport = Viewport::new();
        let history = History::new();
        let playback = PlaybackController::new();
        let thumbnail_cache = ThumbnailCache::new();

        Self {
            project,
            path,
            history,
            selection,
            tool_registry,
            brush,
            viewport,
            playback,
            thumbnail_cache,
            show_grid: true,
            toast: None,
        }
    }

    pub fn set_toast(&mut self, msg: impl Into<String>, now: f64) {
        self.toast = Some((msg.into(), now + 3.0));
    }

    pub fn add_frame(&mut self) {
        self.playback.pause();
        let idx = self.project.timeline.selected_index();
        let cmd = AddFrameCommand::blank_after(&mut self.project, idx);
        self.history.execute(Box::new(cmd), &mut self.project);
    }

    pub fn duplicate_frame(&mut self) {
        self.playback.pause();
        let idx = self.project.timeline.selected_index();
        let cmd = AddFrameCommand::duplicate(&mut self.project, idx);
        self.history.execute(Box::new(cmd), &mut self.project);
    }

    pub fn delete_frame(&mut self) {
        self.playback.pause();
        let idx = self.project.timeline.selected_index();
        if let Ok(cmd) = DeleteFrameCommand::new(&self.project, idx) {
            self.history.execute(Box::new(cmd), &mut self.project);
        }
    }

    #[allow(dead_code)]
    pub fn move_frame(&mut self, from: usize, to: usize) {
        if from != to {
            let selected_before = self.project.timeline.selected_index();
            let cmd = MoveFrameCommand {
                from,
                to,
                selected_before,
            };
            self.history.execute(Box::new(cmd), &mut self.project);
        }
    }

    pub fn select_frame(&mut self, i: usize) {
        self.playback.pause();
        let _ = self.project.timeline.select(i);
    }

    pub fn set_frame_background(&mut self, frame_id: FrameId, rgb: Rgb, all_frames: bool) {
        if !all_frames {
            if let Some(cmd) =
                SetFrameBackgroundCommand::new(&self.project, frame_id, rgb)
            {
                self.history.execute(Box::new(cmd), &mut self.project);
            }
        } else {
            let mut commands: Vec<Box<dyn Command>> = Vec::new();
            for f in self.project.timeline.frames() {
                if f.background != rgb {
                    if let Some(c) = SetFrameBackgroundCommand::new(&self.project, f.id, rgb) {
                        commands.push(Box::new(c));
                    }
                }
            }
            if !commands.is_empty() {
                let composite =
                    CompositeCommand::new("Set background (all frames)", commands);
                self.history.execute(Box::new(composite), &mut self.project);
            }
        }
    }

    pub fn set_fps(&mut self, fps: Fps) {
        if self.project.timeline.fps != fps {
            let before_fps = self.project.timeline.fps;
            let before_looping = self.project.timeline.looping;
            let cmd = SetPlaybackSettingsCommand {
                before_fps,
                after_fps: fps,
                before_looping,
                after_looping: before_looping,
            };
            self.history.execute(Box::new(cmd), &mut self.project);
        }
    }

    pub fn set_looping(&mut self, looping: bool) {
        if self.project.timeline.looping != looping {
            let before_fps = self.project.timeline.fps;
            let before_looping = self.project.timeline.looping;
            let cmd = SetPlaybackSettingsCommand {
                before_fps,
                after_fps: before_fps,
                before_looping,
                after_looping: looping,
            };
            self.history.execute(Box::new(cmd), &mut self.project);
        }
    }

    pub fn undo(&mut self, now: f64) {
        self.playback.pause();
        {
            let mut ctx = ToolContext {
                project: &mut self.project,
                selection: &mut self.selection,
                brush: &self.brush,
                history: &mut self.history,
            };
            self.tool_registry.dispatch(ToolEvent::Cancel, &mut ctx);
        }
        if let Some(label) = self.history.undo(&mut self.project) {
            self.set_toast(format!("Undid: {}", label), now);
        }
    }

    pub fn redo(&mut self, now: f64) {
        self.playback.pause();
        {
            let mut ctx = ToolContext {
                project: &mut self.project,
                selection: &mut self.selection,
                brush: &self.brush,
                history: &mut self.history,
            };
            self.tool_registry.dispatch(ToolEvent::Cancel, &mut ctx);
        }
        if let Some(label) = self.history.redo(&mut self.project) {
            self.set_toast(format!("Redid: {}", label), now);
        }
    }

    pub fn delete_selection_contents(&mut self) {
        if self.selection.is_empty() {
            return;
        }

        let frame_id = self.project.timeline.selected_frame().id;
        let mut builder = PatchBuilder::new(frame_id);

        for r in 0..self.project.canvas.rows {
            for c in 0..self.project.canvas.cols {
                let coord = CellCoord::new(c, r);
                if self.selection.contains(coord) {
                    let old = self
                        .project
                        .timeline
                        .selected_frame()
                        .grid
                        .get(coord)
                        .copied()
                        .unwrap_or_default();
                    let new_cell = ClearOperation.apply(old, &self.brush);
                    builder.write(&mut self.project, coord, new_cell);
                }
            }
        }

        if let Some(cmd) = builder.finish("Clear selection") {
            self.history.record_applied(Box::new(cmd));
        }
    }

    pub fn toggle_playback(&mut self, now: Instant) {
        if self.playback.is_playing() {
            self.playback.pause();
        } else if self.project.timeline.len() > 1 {
            let mut ctx = ToolContext {
                project: &mut self.project,
                selection: &mut self.selection,
                brush: &self.brush,
                history: &mut self.history,
            };
            self.tool_registry.dispatch(ToolEvent::Cancel, &mut ctx);
            self.playback.play(now);
        }
    }
}

pub struct EditorScreen {
    pub session: EditorSession,
    canvas_view: CanvasView,
    #[allow(dead_code)]
    dragged_frame: Option<usize>,
}

impl EditorScreen {
    pub fn new(project: Project, path: PathBuf, glyphs: Arc<dyn GlyphSource>, ctx: &egui::Context) -> Self {
        let session = EditorSession::new(project, path);
        let canvas_view = CanvasView::new(ctx, glyphs);
        Self {
            session,
            canvas_view,
            dragged_frame: None,
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, _services: &Services) -> Vec<AppAction> {
        let mut actions = Vec::new();
        let now_sec = ui.input(|i| i.time);

        // Keyboard shortcuts
        ui.input(|i| {
            let ctrl = i.modifiers.ctrl || i.modifiers.command;
            if ctrl && i.key_pressed(egui::Key::S) {
                actions.push(AppAction::QuickSave);
            }
            if ctrl && !i.modifiers.shift && i.key_pressed(egui::Key::Z) {
                self.session.undo(now_sec);
            }
            if (ctrl && i.key_pressed(egui::Key::Y)) || (ctrl && i.modifiers.shift && i.key_pressed(egui::Key::Z)) {
                self.session.redo(now_sec);
            }
            if ctrl && i.key_pressed(egui::Key::A) {
                self.session.selection.select_all();
            }
            if ctrl && i.key_pressed(egui::Key::D) {
                if !self.session.tool_registry.active().is_busy() {
                    self.session.selection.clear();
                }
            }
            if i.key_pressed(egui::Key::Delete) && self.session.tool_registry.active_id() != ToolId::EditCell {
                self.session.delete_selection_contents();
            }
        });

        // Top MenuBar
        egui::TopBottomPanel::top("menu_bar").show_inside(ui, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("New Project...").clicked() {
                        actions.push(AppAction::OpenNewProjectModal);
                        ui.close_menu();
                    }
                    if ui.button("Open Recent...").clicked() {
                        actions.push(AppAction::OpenRecentModal);
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Save (Ctrl+S)").clicked() {
                        actions.push(AppAction::QuickSave);
                        ui.close_menu();
                    }
                    if ui.button("Save As... / Export...").clicked() {
                        actions.push(AppAction::OpenSaveModal);
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Close Project").clicked() {
                        actions.push(AppAction::RequestCloseProject);
                        ui.close_menu();
                    }
                });

                ui.menu_button("Edit", |ui| {
                    if ui.add_enabled(self.session.history.can_undo(), egui::Button::new("Undo (Ctrl+Z)")).clicked() {
                        self.session.undo(now_sec);
                        ui.close_menu();
                    }
                    if ui.add_enabled(self.session.history.can_redo(), egui::Button::new("Redo (Ctrl+Y)")).clicked() {
                        self.session.redo(now_sec);
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Select All (Ctrl+A)").clicked() {
                        self.session.selection.select_all();
                        ui.close_menu();
                    }
                    if ui.button("Deselect (Ctrl+D)").clicked() {
                        self.session.selection.clear();
                        ui.close_menu();
                    }
                    if ui.button("Delete Selection Contents (Delete)").clicked() {
                        self.session.delete_selection_contents();
                        ui.close_menu();
                    }
                });

                ui.menu_button("View", |ui| {
                    if ui.button("Zoom In (Ctrl+=)").clicked() {
                        self.session.viewport.zoom_step(1, ui.clip_rect());
                        ui.close_menu();
                    }
                    if ui.button("Zoom Out (Ctrl+-)").clicked() {
                        self.session.viewport.zoom_step(-1, ui.clip_rect());
                        ui.close_menu();
                    }
                    if ui.button("Fit to Window (Ctrl+0)").clicked() {
                        self.session.viewport.fit(ui.clip_rect(), &self.session.project.canvas);
                        ui.close_menu();
                    }
                    if ui.button("Actual Size 100% (Ctrl+1)").clicked() {
                        self.session.viewport.actual_size(ui.clip_rect(), &self.session.project.canvas);
                        ui.close_menu();
                    }
                    ui.separator();
                    ui.checkbox(&mut self.session.show_grid, "Show Grid");
                });
            });
        });

        // Bottom StatusBar
        egui::TopBottomPanel::bottom("status_bar").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                if let Some(hover) = self.canvas_view.hover {
                    ui.label(format!("Cell: ({}, {})", hover.col, hover.row));
                } else {
                    ui.label("Cell: -");
                }
                ui.separator();
                ui.label(format!(
                    "Canvas: {}x{} cells ({}x{} px)",
                    self.session.project.canvas.cols,
                    self.session.project.canvas.rows,
                    self.session.project.canvas.pixel_width(),
                    self.session.project.canvas.pixel_height()
                ));
                ui.separator();
                ui.label(format!("Zoom: {}%", (self.session.viewport.zoom * 100.0).round()));
                if self.session.history.is_dirty() {
                    ui.colored_label(egui::Color32::YELLOW, "[Unsaved *]");
                }

                if let Some((ref msg, expires)) = self.session.toast {
                    if now_sec < expires {
                        ui.separator();
                        ui.colored_label(egui::Color32::LIGHT_GREEN, msg);
                    }
                }
            });
        });

        // Left ToolPanel
        egui::SidePanel::left("tool_panel")
            .resizable(false)
            .default_width(120.0)
            .show_inside(ui, |ui| {
                ui.heading("Tools");
                ui.add_space(4.0);

                let tools = [
                    (ToolId::EditCell, "Edit (T)"),
                    (ToolId::PaintCell, "Paint (B)"),
                    (ToolId::ClearCell, "Clear (E)"),
                    (ToolId::FillCell, "Fill (G)"),
                    (ToolId::SelectRect, "Square (R)"),
                    (ToolId::SelectEllipse, "Ellipse (O)"),
                    (ToolId::SelectPolygon, "Polygon (P)"),
                ];

                for (id, label) in tools {
                    let is_active = self.session.tool_registry.active_id() == id;
                    if ui.selectable_label(is_active, label).clicked() {
                        let mut ctx = ToolContext {
                            project: &mut self.session.project,
                            selection: &mut self.session.selection,
                            brush: &self.session.brush,
                            history: &mut self.session.history,
                        };
                        self.session.tool_registry.switch_to(id, &mut ctx);
                    }
                }

                ui.separator();
                ui.label("Target Channel:");
                ui.horizontal(|ui| {
                    let is_fg = self.session.brush.channel == PaintChannel::Foreground;
                    if ui.selectable_label(is_fg, "FG").clicked() {
                        self.session.brush.channel = PaintChannel::Foreground;
                    }
                    let is_bg = self.session.brush.channel == PaintChannel::Background;
                    if ui.selectable_label(is_bg, "BG").clicked() {
                        self.session.brush.channel = PaintChannel::Background;
                    }
                });

                ui.separator();
                ui.label("Paint Color:");
                let mut paint_rgb = [
                    self.session.brush.paint.r,
                    self.session.brush.paint.g,
                    self.session.brush.paint.b,
                ];
                ui.horizontal(|ui| {
                    if ui.color_edit_button_srgb(&mut paint_rgb).clicked() {
                        actions.push(AppAction::OpenColorPicker(ColorPickTarget::PaintColor));
                    }
                    ui.label(self.session.brush.paint.to_hex());
                });

                ui.add_space(4.0);
                ui.label("Erase Color:");
                ui.horizontal(|ui| {
                    let mut erase_rgb = self
                        .session
                        .brush
                        .erase
                        .map(|c| [c.r, c.g, c.b])
                        .unwrap_or([0, 0, 0]);
                    if ui.color_edit_button_srgb(&mut erase_rgb).clicked() {
                        actions.push(AppAction::OpenColorPicker(ColorPickTarget::EraseColor));
                    }
                    if let Some(c) = self.session.brush.erase {
                        ui.label(c.to_hex());
                    } else {
                        ui.label("None");
                    }
                });
            });

        // Right FramePanel
        egui::SidePanel::right("frame_panel")
            .resizable(false)
            .default_width(180.0)
            .show_inside(ui, |ui| {
                ui.heading("Frames");
                ui.add_space(4.0);

                let is_playing = self.session.playback.is_playing();

                // Frame list
                let len = self.session.project.timeline.len();
                let selected = self.session.project.timeline.selected_index();

                egui::ScrollArea::vertical()
                    .max_height(260.0)
                    .show(ui, |ui| {
                        for i in 0..len {
                            let frame = self.session.project.timeline.frame(i).unwrap().clone();
                            let is_selected = i == selected;
                            let thumb_id = self.session.thumbnail_cache.get(ui.ctx(), &frame).id();

                            ui.horizontal(|ui| {
                                let label = format!("#{}", i + 1);
                                if ui.selectable_label(is_selected, label).clicked() {
                                    self.session.select_frame(i);
                                }
                                ui.image((thumb_id, egui::vec2(40.0, 20.0)));
                            });
                        }
                    });

                ui.separator();
                ui.add_enabled_ui(!is_playing, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("+ Add").clicked() {
                            self.session.add_frame();
                        }
                        if ui.button("Duplicate").clicked() {
                            self.session.duplicate_frame();
                        }
                        let can_delete = len > 1;
                        if ui.add_enabled(can_delete, egui::Button::new("Delete")).clicked() {
                            self.session.delete_frame();
                        }
                    });

                    ui.separator();
                    ui.label("Frame Background:");
                    let current_bg = self.session.project.timeline.selected_frame().background;
                    let mut bg_rgb = [current_bg.r, current_bg.g, current_bg.b];
                    ui.horizontal(|ui| {
                        let selected_id = self.session.project.timeline.selected_frame().id;
                        if ui.color_edit_button_srgb(&mut bg_rgb).clicked() {
                            actions.push(AppAction::OpenColorPicker(
                                ColorPickTarget::FrameBackground(selected_id),
                            ));
                        }
                        ui.label(current_bg.to_hex());
                    });
                });

                ui.separator();
                ui.label("Playback:");
                let mut fps_val = self.session.project.timeline.fps.0 as i32;
                ui.add_enabled_ui(!is_playing, |ui| {
                    if ui
                        .add(egui::DragValue::new(&mut fps_val).range(1..=60).prefix("FPS: "))
                        .changed()
                    {
                        if let Ok(f) = Fps::new(fps_val as u8) {
                            self.session.set_fps(f);
                        }
                    }
                });

                ui.horizontal(|ui| {
                    let play_label = if is_playing { "Pause" } else { "Play" };
                    let can_play = len > 1 || is_playing;
                    if ui.add_enabled(can_play, egui::Button::new(play_label)).clicked() {
                        self.session.toggle_playback(Instant::now());
                    }

                    let mut looping = self.session.project.timeline.looping;
                    if ui.checkbox(&mut looping, "Loop").changed() {
                        self.session.set_looping(looping);
                    }
                });
            });

        // Center: CanvasView
        egui::CentralPanel::default().show_inside(ui, |ui| {
            self.canvas_view.ui(ui, &mut self.session);
        });

        actions
    }
}
