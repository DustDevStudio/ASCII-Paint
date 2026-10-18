use crate::types::{AppAction, FileDialogs, Services};

pub struct WelcomeScreen;

impl WelcomeScreen {
    pub fn new() -> Self {
        Self
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, _services: &Services) -> Vec<AppAction> {
        let mut actions = Vec::new();

        ui.vertical_centered(|ui| {
            ui.add_space(80.0);
            ui.heading(
                egui::RichText::new("AsciiPaint")
                    .size(48.0)
                    .color(egui::Color32::from_rgb(100, 200, 255)),
            );
            ui.label(
                egui::RichText::new("ASCII Art & Animation Studio")
                    .size(18.0)
                    .color(egui::Color32::GRAY),
            );

            ui.add_space(40.0);

            let btn_size = egui::vec2(220.0, 44.0);

            if ui
                .add_sized(
                    btn_size,
                    egui::Button::new(egui::RichText::new("+ Create New Project").size(16.0)),
                )
                .clicked()
            {
                actions.push(AppAction::OpenNewProjectModal);
            }

            ui.add_space(12.0);

            if ui
                .add_sized(
                    btn_size,
                    egui::Button::new(egui::RichText::new("Open Recent Project").size(16.0)),
                )
                .clicked()
            {
                actions.push(AppAction::OpenRecentModal);
            }

            ui.add_space(12.0);

            if ui
                .add_sized(
                    btn_size,
                    egui::Button::new(egui::RichText::new("Browse Project File (.aspr)...").size(16.0)),
                )
                .clicked()
            {
                if let Some(path) = FileDialogs::pick_file(&["aspr"]) {
                    actions.push(AppAction::OpenProjectFile(path));
                }
            }
        });

        actions
    }
}

impl Default for WelcomeScreen {
    fn default() -> Self {
        Self::new()
    }
}
