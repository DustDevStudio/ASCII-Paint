use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use ascii_core::model::Project;
use ascii_io::export::{Clock, ExportJobHandle, ExportMessage, ExportService, FfmpegLocator};
use ascii_io::storage::{ProjectRepository, RecentStore};
use ascii_render::{GlyphSource, TtfGlyphSource};

use crate::editor::EditorScreen;
use crate::modals::{
    ColorPickerModal, ConfirmUnsavedModal, ErrorModal, ExportProgressModal, Modal, ModalOutcome,
    ModalView, NewProjectModal, RecentProjectsModal, SaveModal,
};
use crate::types::{
    AppAction, ColorPickTarget, FileDialogs, PendingAction, SaveFormat,
    Services, UnsavedChoice,
};
use crate::welcome::WelcomeScreen;

pub enum AppScreen {
    Welcome(WelcomeScreen),
    Editor(Box<EditorScreen>),
}

pub struct AsciiPaintApp {
    screen: AppScreen,
    modal: Option<Modal>,
    services: Services,
    export_job: Option<ExportJobHandle>,
    pending: Option<PendingAction>,
}

impl AsciiPaintApp {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        let glyphs: Arc<dyn GlyphSource> = Arc::new(
            TtfGlyphSource::load_default(8, 15).expect("failed to load embedded default font"),
        );

        let services = Services {
            recent: RecentStore::new(),
            projects: ProjectRepository::new(),
            exporter: ExportService::new(),
            glyphs,
            dialogs: FileDialogs,
        };

        // Dark theme default
        cc.egui_ctx.set_visuals(egui::Visuals::dark());

        Self {
            screen: AppScreen::Welcome(WelcomeScreen::new()),
            modal: None,
            services,
            export_job: None,
            pending: None,
        }
    }

    fn poll_export_job(&mut self) {
        if let Some(ref job) = self.export_job {
            let messages = job.poll();
            for msg in messages {
                match msg {
                    ExportMessage::Progress(done, total) => {
                        if let Some(Modal::ExportProgress(ref mut m)) = self.modal {
                            m.done = done;
                            m.total = total;
                        }
                    }
                    ExportMessage::Finished(path) => {
                        self.modal = None;
                        self.export_job = None;
                        if let AppScreen::Editor(ref mut ed) = self.screen {
                            ed.session.set_toast(
                                format!("Exported {}", path.file_name().unwrap().to_string_lossy()),
                                0.0,
                            );
                        }
                        return;
                    }
                    ExportMessage::Failed(err) => {
                        self.export_job = None;
                        self.modal = Some(Modal::Error(ErrorModal::new(
                            "Export Failed",
                            err,
                        )));
                        return;
                    }
                    ExportMessage::Cancelled => {
                        self.export_job = None;
                        self.modal = None;
                        return;
                    }
                }
            }
        }
    }

    fn handle_action(&mut self, ctx: &egui::Context, action: AppAction) {
        match action {
            AppAction::OpenNewProjectModal => {
                let default_folder = dirs::document_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join("AsciiPaint");
                self.modal = Some(Modal::NewProject(NewProjectModal::new(default_folder)));
            }
            AppAction::OpenRecentModal => {
                self.modal = Some(Modal::RecentProjects(RecentProjectsModal::new(
                    self.services.recent.entries(),
                )));
            }
            AppAction::CreateProject(params) => {
                let now = Clock::now_unix();
                let project = Project::new(params.name.clone(), params.background, params.fps, now);
                let path = params.folder.join(format!("{}.aspr", params.name));

                match self.services.projects.save(&path, &project) {
                    Ok(()) => {
                        self.services.recent.touch(path.clone(), params.name, now);
                        let ed = EditorScreen::new(
                            project,
                            path,
                            Arc::clone(&self.services.glyphs),
                            ctx,
                        );
                        self.screen = AppScreen::Editor(Box::new(ed));
                        self.modal = None;
                    }
                    Err(e) => {
                        if let Some(Modal::NewProject(ref mut m)) = self.modal {
                            m.error = Some(format!("Failed to save project: {}", e));
                        }
                    }
                }
            }
            AppAction::OpenProjectFile(path) => {
                if let AppScreen::Editor(ref ed) = self.screen {
                    if ed.session.history.is_dirty() {
                        self.pending = Some(PendingAction::OpenProjectFile(path));
                        self.modal = Some(Modal::ConfirmUnsaved(ConfirmUnsavedModal::new(
                            &ed.session.project.meta.name,
                        )));
                        return;
                    }
                }

                match self.services.projects.load(&path) {
                    Ok(project) => {
                        let name = project.meta.name.clone();
                        let now = Clock::now_unix();
                        self.services.recent.touch(path.clone(), name, now);
                        let ed = EditorScreen::new(
                            project,
                            path,
                            Arc::clone(&self.services.glyphs),
                            ctx,
                        );
                        self.screen = AppScreen::Editor(Box::new(ed));
                        self.modal = None;
                    }
                    Err(e) => {
                        self.modal = Some(Modal::Error(ErrorModal::new(
                            "Cannot Open Project",
                            format!("Failed to open {}: {}", path.display(), e),
                        )));
                    }
                }
            }
            AppAction::RemoveRecent(path) => {
                self.services.recent.remove(&path);
                self.modal = Some(Modal::RecentProjects(RecentProjectsModal::new(
                    self.services.recent.entries(),
                )));
            }
            AppAction::ClearRecent => {
                self.services.recent.clear();
                self.modal = Some(Modal::RecentProjects(RecentProjectsModal::new(
                    self.services.recent.entries(),
                )));
            }
            AppAction::RequestCloseProject => {
                if let AppScreen::Editor(ref ed) = self.screen {
                    if ed.session.history.is_dirty() {
                        self.pending = Some(PendingAction::CloseProject);
                        self.modal = Some(Modal::ConfirmUnsaved(ConfirmUnsavedModal::new(
                            &ed.session.project.meta.name,
                        )));
                        return;
                    }
                }
                self.screen = AppScreen::Welcome(WelcomeScreen::new());
                self.modal = None;
            }
            AppAction::OpenSaveModal => {
                if let AppScreen::Editor(ref ed) = self.screen {
                    let folder = ed
                        .session
                        .path
                        .parent()
                        .map(PathBuf::from)
                        .unwrap_or_else(|| PathBuf::from("."));
                    let ffmpeg_available = FfmpegLocator::find().is_some();
                    self.modal = Some(Modal::Save(SaveModal::new(
                        &ed.session.project.meta.name,
                        folder,
                        ffmpeg_available,
                    )));
                }
            }
            AppAction::QuickSave => {
                if let AppScreen::Editor(ref mut ed) = self.screen {
                    let now = Clock::now_unix();
                    ed.session.project.touch_modified(now);
                    match self
                        .services
                        .projects
                        .save(&ed.session.path, &ed.session.project)
                    {
                        Ok(()) => {
                            ed.session.history.mark_clean();
                            let now_f64 = ctx.input(|i| i.time);
                            ed.session.set_toast("Saved project", now_f64);
                        }
                        Err(e) => {
                            self.modal = Some(Modal::Error(ErrorModal::new(
                                "Cannot Save Project",
                                e.to_string(),
                            )));
                        }
                    }
                }
            }
            AppAction::SaveAs(req) => {
                if let AppScreen::Editor(ref mut ed) = self.screen {
                    let now_f64 = ctx.input(|i| i.time);
                    match req.format {
                        SaveFormat::ProjectAspr => {
                            let now = Clock::now_unix();
                            ed.session.project.touch_modified(now);
                            match self.services.projects.save(&req.path, &ed.session.project) {
                                Ok(()) => {
                                    ed.session.path = req.path.clone();
                                    ed.session.history.mark_clean();
                                    self.services.recent.touch(
                                        req.path,
                                        ed.session.project.meta.name.clone(),
                                        now,
                                    );
                                    ed.session.set_toast("Saved project", now_f64);
                                    self.modal = None;
                                }
                                Err(e) => {
                                    self.modal = Some(Modal::Error(ErrorModal::new(
                                        "Save Failed",
                                        e.to_string(),
                                    )));
                                }
                            }
                        }
                        SaveFormat::FramePng => {
                            match self.services.exporter.export_frame_png(
                                &ed.session.project,
                                &req.path,
                                self.services.glyphs.as_ref(),
                            ) {
                                Ok(()) => {
                                    ed.session.set_toast("Saved PNG", now_f64);
                                    self.modal = None;
                                }
                                Err(e) => {
                                    self.modal = Some(Modal::Error(ErrorModal::new(
                                        "PNG Export Failed",
                                        e.to_string(),
                                    )));
                                }
                            }
                        }
                        SaveFormat::FrameAscii => {
                            match self.services.exporter.export_frame_ascii(
                                &ed.session.project,
                                &req.path,
                                self.services.glyphs.as_ref(),
                            ) {
                                Ok(()) => {
                                    ed.session.set_toast("Saved ASCII", now_f64);
                                    self.modal = None;
                                }
                                Err(e) => {
                                    self.modal = Some(Modal::Error(ErrorModal::new(
                                        "ASCII Export Failed",
                                        e.to_string(),
                                    )));
                                }
                            }
                        }
                        SaveFormat::AnimationAanim => {
                            match self.services.exporter.export_animation_aanim(
                                &ed.session.project,
                                &req.path,
                                self.services.glyphs.as_ref(),
                            ) {
                                Ok(()) => {
                                    ed.session.set_toast("Saved Aanim", now_f64);
                                    self.modal = None;
                                }
                                Err(e) => {
                                    self.modal = Some(Modal::Error(ErrorModal::new(
                                        "Aanim Export Failed",
                                        e.to_string(),
                                    )));
                                }
                            }
                        }
                        SaveFormat::AnimationMp4 => {
                            if let Some(ffmpeg) = FfmpegLocator::find() {
                                let total = ed.session.project.timeline.len() as u32;
                                let job = self.services.exporter.start_mp4(
                                    &ed.session.project,
                                    req.path,
                                    Arc::clone(&self.services.glyphs),
                                    ffmpeg,
                                );
                                self.export_job = Some(job);
                                self.modal = Some(Modal::ExportProgress(ExportProgressModal::new(total)));
                            } else {
                                self.modal = Some(Modal::Error(ErrorModal::new(
                                    "ffmpeg Not Found",
                                    "Please install ffmpeg to export MP4 videos.",
                                )));
                            }
                        }
                    }
                }
            }
            AppAction::OpenColorPicker(target) => {
                if let AppScreen::Editor(ref ed) = self.screen {
                    let original = match target {
                        ColorPickTarget::PaintColor => Some(ed.session.brush.paint),
                        ColorPickTarget::EraseColor => ed.session.brush.erase,
                        ColorPickTarget::FrameBackground(id) => {
                            let idx = ed.session.project.timeline.index_of(id);
                            idx.and_then(|i| ed.session.project.timeline.frame(i))
                                .map(|f| f.background)
                        }
                    };
                    let allow_transparent = target == ColorPickTarget::EraseColor;
                    self.modal = Some(Modal::ColorPicker(ColorPickerModal::new(
                        target,
                        original,
                        allow_transparent,
                        false,
                    )));
                }
            }
            AppAction::ApplyColor(target, color, apply_to_all) => {
                if let AppScreen::Editor(ref mut ed) = self.screen {
                    match target {
                        ColorPickTarget::PaintColor => {
                            if let Some(c) = color {
                                ed.session.brush.paint = c;
                            }
                        }
                        ColorPickTarget::EraseColor => {
                            ed.session.brush.erase = color;
                        }
                        ColorPickTarget::FrameBackground(id) => {
                            if let Some(c) = color {
                                ed.session.set_frame_background(id, c, apply_to_all);
                            }
                        }
                    }
                }
                self.modal = None;
            }
            AppAction::ConfirmUnsaved(choice) => {
                match choice {
                    UnsavedChoice::Save => {
                        self.handle_action(ctx, AppAction::QuickSave);
                        let pending = self.pending.take();
                        self.continue_pending(ctx, pending);
                    }
                    UnsavedChoice::Discard => {
                        let pending = self.pending.take();
                        self.continue_pending(ctx, pending);
                    }
                    UnsavedChoice::Cancel => {
                        self.pending = None;
                        self.modal = None;
                    }
                }
            }
            AppAction::CancelExport => {
                if let Some(ref job) = self.export_job {
                    job.request_cancel();
                }
            }
            AppAction::CloseModal => {
                self.modal = None;
            }
        }
    }

    fn continue_pending(&mut self, ctx: &egui::Context, pending: Option<PendingAction>) {
        self.modal = None;
        match pending {
            Some(PendingAction::CloseProject) => {
                self.screen = AppScreen::Welcome(WelcomeScreen::new());
            }
            Some(PendingAction::QuitApplication) => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Some(PendingAction::OpenProjectFile(path)) => {
                self.handle_action(ctx, AppAction::OpenProjectFile(path));
            }
            None => {}
        }
    }
}

impl eframe::App for AsciiPaintApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 1. Poll export job
        self.poll_export_job();

        // 2. Playback tick if in Editor
        if let AppScreen::Editor(ref mut ed) = self.screen {
            let now = Instant::now();
            if let Some(duration) = ed.session.playback.tick(now, &mut ed.session.project.timeline) {
                ctx.request_repaint_after(duration);
            }
        }

        // Window title
        let title = match self.screen {
            AppScreen::Welcome(_) => "AsciiPaint".to_string(),
            AppScreen::Editor(ref ed) => {
                let dirty = if ed.session.history.is_dirty() { " *" } else { "" };
                format!("AsciiPaint - {}{}", ed.session.project.meta.name, dirty)
            }
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));

        // 3. Screen UI
        let mut actions = Vec::new();

        // If a modal is open, disable screen interaction
        let modal_open = self.modal.is_some();

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_enabled_ui(!modal_open, |ui| {
                match self.screen {
                    AppScreen::Welcome(ref mut w) => {
                        actions.extend(w.ui(ui, &self.services));
                    }
                    AppScreen::Editor(ref mut ed) => {
                        actions.extend(ed.ui(ui, &self.services));
                    }
                }
            });
        });

        // 4. Modal UI
        if let Some(ref mut m) = self.modal {
            let mut outcome = ModalOutcome::Open;
            let window_title = match m {
                Modal::NewProject(_) => "New Project",
                Modal::RecentProjects(_) => "Recent Projects",
                Modal::ColorPicker(c) => c.title(),
                Modal::Save(_) => "Save / Export",
                Modal::ConfirmUnsaved(_) => "Confirm Save",
                Modal::ExportProgress(_) => "Export Progress",
                Modal::Error(_) => "Error",
            };

            egui::Window::new(window_title)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    outcome = m.ui(ui, &self.services);
                });

            match outcome {
                ModalOutcome::Accepted(action) => {
                    self.handle_action(ctx, action);
                }
                ModalOutcome::Cancelled => {
                    self.modal = None;
                }
                ModalOutcome::Open => {}
            }
        }

        // 5. Handle screen actions
        for action in actions {
            self.handle_action(ctx, action);
        }
    }
}
