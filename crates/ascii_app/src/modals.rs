use std::path::PathBuf;

use ascii_core::model::{CanvasSpec, Fps, Rgb};
use ascii_io::storage::RecentEntry;

use crate::types::{
    AppAction, ColorPickTarget, FileDialogs, NewProjectParams, SaveFormat, SaveRequest,
    Services, UnsavedChoice,
};

pub enum ModalOutcome {
    Open,
    Cancelled,
    Accepted(AppAction),
}

pub trait ModalView {
    fn title(&self) -> &str;
    fn ui(&mut self, ui: &mut egui::Ui, services: &Services) -> ModalOutcome;
}

pub enum Modal {
    NewProject(NewProjectModal),
    RecentProjects(RecentProjectsModal),
    ColorPicker(ColorPickerModal),
    Save(SaveModal),
    ConfirmUnsaved(ConfirmUnsavedModal),
    ExportProgress(ExportProgressModal),
    Error(ErrorModal),
}

impl Modal {
    pub fn ui(&mut self, ui: &mut egui::Ui, services: &Services) -> ModalOutcome {
        match self {
            Modal::NewProject(m) => m.ui(ui, services),
            Modal::RecentProjects(m) => m.ui(ui, services),
            Modal::ColorPicker(m) => m.ui(ui, services),
            Modal::Save(m) => m.ui(ui, services),
            Modal::ConfirmUnsaved(m) => m.ui(ui, services),
            Modal::ExportProgress(m) => m.ui(ui, services),
            Modal::Error(m) => m.ui(ui, services),
        }
    }
}

pub struct NewProjectModal {
    pub name: String,
    pub folder: PathBuf,
    pub background: Rgb,
    pub fps: Fps,
    pub canvas: CanvasSpec,
    pub error: Option<String>,
}

impl NewProjectModal {
    pub fn new(default_folder: PathBuf) -> Self {
        Self {
            name: "Untitled".to_string(),
            folder: default_folder,
            background: Rgb::BLACK,
            fps: Fps(12),
            canvas: CanvasSpec::default(),
            error: None,
        }
    }

    pub fn validate(&self) -> Result<NewProjectParams, &'static str> {
        let trimmed = self.name.trim();
        if trimmed.is_empty() {
            return Err("Project name cannot be empty");
        }
        if trimmed.len() > 64 {
            return Err("Project name cannot exceed 64 characters");
        }
        if !self.folder.exists() {
            return Err("Selected folder does not exist");
        }
        Ok(NewProjectParams {
            name: trimmed.to_string(),
            folder: self.folder.clone(),
            background: self.background,
            fps: self.fps,
        })
    }
}

impl ModalView for NewProjectModal {
    fn title(&self) -> &str {
        "Create New Project"
    }

    fn ui(&mut self, ui: &mut egui::Ui, _services: &Services) -> ModalOutcome {
        let mut outcome = ModalOutcome::Open;

        ui.heading("New Project");
        ui.add_space(8.0);

        egui::Grid::new("new_project_grid")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label("Name:");
                ui.text_edit_singleline(&mut self.name);
                ui.end_row();

                ui.label("Location:");
                ui.horizontal(|ui| {
                    let folder_str = self.folder.to_string_lossy();
                    ui.label(folder_str);
                    if ui.button("Browse...").clicked() {
                        if let Some(f) = FileDialogs::pick_folder(&self.folder) {
                            self.folder = f;
                        }
                    }
                });
                ui.end_row();

                ui.label("Background:");
                ui.horizontal(|ui| {
                    let mut color = [self.background.r, self.background.g, self.background.b];
                    if ui.color_edit_button_srgb(&mut color).changed() {
                        self.background = Rgb::new(color[0], color[1], color[2]);
                    }
                    ui.label(self.background.to_hex());
                });
                ui.end_row();

                ui.label("Frame Rate (FPS):");
                let mut fps_val = self.fps.0 as i32;
                if ui
                    .add(egui::DragValue::new(&mut fps_val).range(1..=60))
                    .changed()
                {
                    if let Ok(f) = Fps::new(fps_val as u8) {
                        self.fps = f;
                    }
                }
                ui.end_row();

                ui.label("Canvas:");
                ui.label(format!(
                    "{} x {} cells ({} x {} px)",
                    self.canvas.cols,
                    self.canvas.rows,
                    self.canvas.pixel_width(),
                    self.canvas.pixel_height()
                ));
                ui.end_row();
            });

        let validation = self.validate();
        if let Err(msg) = validation {
            ui.add_space(4.0);
            ui.colored_label(egui::Color32::RED, msg);
        } else if let Some(ref err) = self.error {
            ui.add_space(4.0);
            ui.colored_label(egui::Color32::RED, err);
        }

        ui.add_space(16.0);
        ui.horizontal(|ui| {
            let can_create = validation.is_ok();
            if ui
                .add_enabled(can_create, egui::Button::new("Create"))
                .clicked()
            {
                if let Ok(params) = validation {
                    outcome = ModalOutcome::Accepted(AppAction::CreateProject(params));
                }
            }

            if ui.button("Cancel").clicked() {
                outcome = ModalOutcome::Cancelled;
            }
        });

        outcome
    }
}

pub struct RecentRow {
    pub entry: RecentEntry,
    pub exists: bool,
}

pub struct RecentProjectsModal {
    pub rows: Vec<RecentRow>,
    pub selected: Option<usize>,
}

impl RecentProjectsModal {
    pub fn new(entries: &[RecentEntry]) -> Self {
        let rows = entries
            .iter()
            .map(|e| RecentRow {
                exists: e.path.exists(),
                entry: e.clone(),
            })
            .collect();
        Self {
            rows,
            selected: None,
        }
    }
}

impl ModalView for RecentProjectsModal {
    fn title(&self) -> &str {
        "Recent Projects"
    }

    fn ui(&mut self, ui: &mut egui::Ui, _services: &Services) -> ModalOutcome {
        let mut outcome = ModalOutcome::Open;

        ui.heading("Recent Projects");
        ui.add_space(8.0);

        if self.rows.is_empty() {
            ui.label("No recent projects yet.");
        } else {
            egui::ScrollArea::vertical()
                .max_height(250.0)
                .show(ui, |ui| {
                    for (i, row) in self.rows.iter().enumerate() {
                        let is_selected = self.selected == Some(i);
                        let name = if row.exists {
                            row.entry.name.clone()
                        } else {
                            format!("{} (missing)", row.entry.name)
                        };

                        let resp = ui.selectable_label(is_selected, name);
                        if resp.clicked() {
                            self.selected = Some(i);
                        }
                        if resp.double_clicked() && row.exists {
                            outcome = ModalOutcome::Accepted(AppAction::OpenProjectFile(
                                row.entry.path.clone(),
                            ));
                        }
                    }
                });
        }

        ui.add_space(16.0);
        ui.horizontal(|ui| {
            let can_open = self
                .selected
                .and_then(|idx| self.rows.get(idx))
                .map(|r| r.exists)
                .unwrap_or(false);

            if ui.add_enabled(can_open, egui::Button::new("Open")).clicked() {
                if let Some(idx) = self.selected {
                    outcome = ModalOutcome::Accepted(AppAction::OpenProjectFile(
                        self.rows[idx].entry.path.clone(),
                    ));
                }
            }

            if ui.button("Browse...").clicked() {
                if let Some(path) = FileDialogs::pick_file(&["aspr"]) {
                    outcome = ModalOutcome::Accepted(AppAction::OpenProjectFile(path));
                }
            }

            if let Some(idx) = self.selected {
                if ui.button("Remove from list").clicked() {
                    outcome = ModalOutcome::Accepted(AppAction::RemoveRecent(
                        self.rows[idx].entry.path.clone(),
                    ));
                }
            }

            if !self.rows.is_empty() && ui.button("Clear list").clicked() {
                outcome = ModalOutcome::Accepted(AppAction::ClearRecent);
            }

            if ui.button("Close").clicked() {
                outcome = ModalOutcome::Cancelled;
            }
        });

        outcome
    }
}

pub struct ColorPickerModal {
    pub target: ColorPickTarget,
    pub original: Option<Rgb>,
    pub working: Option<Rgb>,
    pub allow_transparent: bool,
    pub apply_to_all: bool,
    hex_input: String,
    hex_error: bool,
}

impl ColorPickerModal {
    pub const PRESETS: [Rgb; 16] = [
        Rgb::new(0, 0, 0),
        Rgb::new(255, 255, 255),
        Rgb::new(128, 128, 128),
        Rgb::new(192, 192, 192),
        Rgb::new(128, 0, 0),
        Rgb::new(255, 0, 0),
        Rgb::new(128, 128, 0),
        Rgb::new(255, 255, 0),
        Rgb::new(0, 128, 0),
        Rgb::new(0, 255, 0),
        Rgb::new(0, 128, 128),
        Rgb::new(0, 255, 255),
        Rgb::new(0, 0, 128),
        Rgb::new(0, 0, 255),
        Rgb::new(128, 0, 128),
        Rgb::new(255, 0, 255),
    ];

    pub fn new(
        target: ColorPickTarget,
        original: Option<Rgb>,
        allow_transparent: bool,
        apply_to_all: bool,
    ) -> Self {
        let hex = original
            .map(|c| c.to_hex())
            .unwrap_or_else(|| "#000000".to_string());
        Self {
            target,
            original,
            working: original,
            allow_transparent,
            apply_to_all,
            hex_input: hex,
            hex_error: false,
        }
    }
}

impl ModalView for ColorPickerModal {
    fn title(&self) -> &str {
        match self.target {
            ColorPickTarget::PaintColor => "Choose Paint Color",
            ColorPickTarget::EraseColor => "Choose Erase Color",
            ColorPickTarget::FrameBackground(_) => "Choose Frame Background",
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _services: &Services) -> ModalOutcome {
        let mut outcome = ModalOutcome::Open;

        ui.heading(self.title());
        ui.add_space(8.0);

        if self.allow_transparent {
            let mut is_transparent = self.working.is_none();
            if ui.checkbox(&mut is_transparent, "Transparent").changed() {
                if is_transparent {
                    self.working = None;
                } else {
                    self.working = Some(self.original.unwrap_or(Rgb::BLACK));
                }
            }
        }

        if let Some(rgb) = self.working {
            let mut color = [rgb.r, rgb.g, rgb.b];
            if ui.color_edit_button_srgb(&mut color).changed() {
                self.working = Some(Rgb::new(color[0], color[1], color[2]));
                self.hex_input = self.working.unwrap().to_hex();
                self.hex_error = false;
            }

            ui.horizontal(|ui| {
                ui.label("Hex:");
                let text_resp = ui.text_edit_singleline(&mut self.hex_input);
                if text_resp.changed() {
                    match Rgb::from_hex(&self.hex_input) {
                        Ok(parsed) => {
                            self.working = Some(parsed);
                            self.hex_error = false;
                        }
                        Err(_) => {
                            self.hex_error = true;
                        }
                    }
                }
                if self.hex_error {
                    ui.colored_label(egui::Color32::RED, "Invalid hex");
                }
            });

            ui.add_space(8.0);
            ui.label("Presets:");
            ui.horizontal_wrapped(|ui| {
                for preset in &Self::PRESETS {
                    let mut col = [preset.r, preset.g, preset.b];
                    if ui.color_edit_button_srgb(&mut col).clicked() {
                        self.working = Some(*preset);
                        self.hex_input = preset.to_hex();
                        self.hex_error = false;
                    }
                }
            });
        }

        if matches!(self.target, ColorPickTarget::FrameBackground(_)) {
            ui.add_space(8.0);
            ui.checkbox(&mut self.apply_to_all, "Apply to all frames");
        }

        ui.add_space(16.0);
        ui.horizontal(|ui| {
            if ui.button("OK").clicked() {
                outcome = ModalOutcome::Accepted(AppAction::ApplyColor(
                    self.target,
                    self.working,
                    self.apply_to_all,
                ));
            }
            if ui.button("Cancel").clicked() {
                outcome = ModalOutcome::Cancelled;
            }
        });

        outcome
    }
}

pub struct SaveModal {
    pub format: SaveFormat,
    pub file_name: String,
    pub folder: PathBuf,
    pub ffmpeg_available: bool,
    #[allow(dead_code)]
    pub error: Option<String>,
}

impl SaveModal {
    pub fn new(project_name: &str, folder: PathBuf, ffmpeg_available: bool) -> Self {
        Self {
            format: SaveFormat::ProjectAspr,
            file_name: project_name.to_string(),
            folder,
            ffmpeg_available,
            error: None,
        }
    }

    pub fn validate(&self) -> Result<SaveRequest, &'static str> {
        let trimmed = self.file_name.trim();
        if trimmed.is_empty() {
            return Err("File name cannot be empty");
        }
        if !self.folder.exists() {
            return Err("Folder does not exist");
        }
        if self.format == SaveFormat::AnimationMp4 && !self.ffmpeg_available {
            return Err("ffmpeg is required for MP4 export");
        }

        let mut path = self.folder.join(trimmed);
        path.set_extension(self.format.extension());

        Ok(SaveRequest {
            format: self.format,
            path,
        })
    }
}

impl ModalView for SaveModal {
    fn title(&self) -> &str {
        "Save / Export"
    }

    fn ui(&mut self, ui: &mut egui::Ui, _services: &Services) -> ModalOutcome {
        let mut outcome = ModalOutcome::Open;

        ui.heading("Save / Export");
        ui.add_space(8.0);

        egui::Grid::new("save_modal_grid")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label("Format:");
                egui::ComboBox::from_id_salt("save_format_combo")
                    .selected_text(self.format.label())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.format,
                            SaveFormat::ProjectAspr,
                            SaveFormat::ProjectAspr.label(),
                        );
                        ui.selectable_value(
                            &mut self.format,
                            SaveFormat::FramePng,
                            SaveFormat::FramePng.label(),
                        );
                        ui.selectable_value(
                            &mut self.format,
                            SaveFormat::FrameAscii,
                            SaveFormat::FrameAscii.label(),
                        );
                        ui.selectable_value(
                            &mut self.format,
                            SaveFormat::AnimationAanim,
                            SaveFormat::AnimationAanim.label(),
                        );
                        ui.add_enabled_ui(self.ffmpeg_available, |ui| {
                            ui.selectable_value(
                                &mut self.format,
                                SaveFormat::AnimationMp4,
                                SaveFormat::AnimationMp4.label(),
                            );
                        });
                    });
                ui.end_row();

                ui.label("File Name:");
                ui.text_edit_singleline(&mut self.file_name);
                ui.end_row();

                ui.label("Folder:");
                ui.horizontal(|ui| {
                    ui.label(self.folder.to_string_lossy());
                    if ui.button("Browse...").clicked() {
                        if let Some(f) = FileDialogs::pick_folder(&self.folder) {
                            self.folder = f;
                        }
                    }
                });
                ui.end_row();
            });

        let validation = self.validate();
        if let Err(msg) = validation {
            ui.add_space(4.0);
            ui.colored_label(egui::Color32::RED, msg);
        }

        ui.add_space(16.0);
        ui.horizontal(|ui| {
            let can_save = validation.is_ok();
            if ui.add_enabled(can_save, egui::Button::new("Save")).clicked() {
                if let Ok(req) = validation {
                    outcome = ModalOutcome::Accepted(AppAction::SaveAs(req));
                }
            }
            if ui.button("Cancel").clicked() {
                outcome = ModalOutcome::Cancelled;
            }
        });

        outcome
    }
}

pub struct ConfirmUnsavedModal {
    pub project_name: String,
}

impl ConfirmUnsavedModal {
    pub fn new(project_name: &str) -> Self {
        Self {
            project_name: project_name.to_string(),
        }
    }
}

impl ModalView for ConfirmUnsavedModal {
    fn title(&self) -> &str {
        "Unsaved Changes"
    }

    fn ui(&mut self, ui: &mut egui::Ui, _services: &Services) -> ModalOutcome {
        let mut outcome = ModalOutcome::Open;

        ui.heading("Save Changes?");
        ui.add_space(8.0);
        ui.label(format!(
            "Project '{}' has unsaved changes. Do you want to save before closing?",
            self.project_name
        ));

        ui.add_space(16.0);
        ui.horizontal(|ui| {
            if ui.button("Save").clicked() {
                outcome = ModalOutcome::Accepted(AppAction::ConfirmUnsaved(UnsavedChoice::Save));
            }
            if ui.button("Discard").clicked() {
                outcome = ModalOutcome::Accepted(AppAction::ConfirmUnsaved(UnsavedChoice::Discard));
            }
            if ui.button("Cancel").clicked() {
                outcome = ModalOutcome::Accepted(AppAction::ConfirmUnsaved(UnsavedChoice::Cancel));
            }
        });

        outcome
    }
}

pub struct ExportProgressModal {
    pub done: u32,
    pub total: u32,
    pub cancelling: bool,
}

impl ExportProgressModal {
    pub fn new(total: u32) -> Self {
        Self {
            done: 0,
            total,
            cancelling: false,
        }
    }
}

impl ModalView for ExportProgressModal {
    fn title(&self) -> &str {
        "Exporting Video"
    }

    fn ui(&mut self, ui: &mut egui::Ui, _services: &Services) -> ModalOutcome {
        let mut outcome = ModalOutcome::Open;

        ui.heading("Exporting MP4");
        ui.add_space(8.0);

        let frac = if self.total > 0 {
            self.done as f32 / self.total as f32
        } else {
            0.0
        };

        ui.add(egui::ProgressBar::new(frac).text(format!("Frame {} / {}", self.done, self.total)));

        if self.cancelling {
            ui.add_space(4.0);
            ui.colored_label(egui::Color32::YELLOW, "Cancelling export...");
        }

        ui.add_space(16.0);
        if ui
            .add_enabled(!self.cancelling, egui::Button::new("Cancel"))
            .clicked()
        {
            self.cancelling = true;
            outcome = ModalOutcome::Accepted(AppAction::CancelExport);
        }

        outcome
    }
}

pub struct ErrorModal {
    pub heading: String,
    pub details: String,
}

impl ErrorModal {
    pub fn new(heading: impl Into<String>, details: impl Into<String>) -> Self {
        Self {
            heading: heading.into(),
            details: details.into(),
        }
    }
}

impl ModalView for ErrorModal {
    fn title(&self) -> &str {
        "Error"
    }

    fn ui(&mut self, ui: &mut egui::Ui, _services: &Services) -> ModalOutcome {
        let mut outcome = ModalOutcome::Open;

        ui.heading(&self.heading);
        ui.add_space(8.0);
        ui.colored_label(egui::Color32::RED, &self.details);

        ui.add_space(16.0);
        if ui.button("OK").clicked() {
            outcome = ModalOutcome::Cancelled;
        }

        outcome
    }
}
