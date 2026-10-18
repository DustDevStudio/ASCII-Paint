mod app;
mod atlas;
mod canvas_view;
mod editor;
mod modals;
mod playback;
mod renderer;
mod thumbnail_cache;
mod types;
mod viewport;
mod welcome;

use app::AsciiPaintApp;

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("AsciiPaint")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        "AsciiPaint",
        native_options,
        Box::new(|cc| Ok(Box::new(AsciiPaintApp::new(cc)))),
    )
}
