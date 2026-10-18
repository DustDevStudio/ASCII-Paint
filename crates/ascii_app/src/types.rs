use std::path::{Path, PathBuf};
use std::sync::Arc;

use ascii_core::model::{Fps, FrameId, Rgb};
use ascii_io::export::ExportService;
use ascii_io::storage::{ProjectRepository, RecentStore};
use ascii_render::GlyphSource;

#[allow(dead_code)]
pub struct FileDialogs;

impl FileDialogs {
    pub fn pick_folder(start: &Path) -> Option<PathBuf> {
        rfd::FileDialog::new().set_directory(start).pick_folder()
    }

    pub fn pick_file(extensions: &[&str]) -> Option<PathBuf> {
        let mut dialog = rfd::FileDialog::new();
        if !extensions.is_empty() {
            dialog = dialog.add_filter("AsciiPaint Project", extensions);
        }
        dialog.pick_file()
    }
}

pub struct Services {
    pub recent: RecentStore,
    pub projects: ProjectRepository,
    pub exporter: ExportService,
    pub glyphs: Arc<dyn GlyphSource>,
    #[allow(dead_code)]
    pub dialogs: FileDialogs,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum PendingAction {
    CloseProject,
    QuitApplication,
    OpenProjectFile(PathBuf),
}

#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub enum AppAction {
    OpenNewProjectModal,
    OpenRecentModal,
    CreateProject(NewProjectParams),
    OpenProjectFile(PathBuf),
    RemoveRecent(PathBuf),
    ClearRecent,
    RequestCloseProject,
    OpenSaveModal,
    QuickSave,
    SaveAs(SaveRequest),
    OpenColorPicker(ColorPickTarget),
    ApplyColor(ColorPickTarget, Option<Rgb>, bool),
    ConfirmUnsaved(UnsavedChoice),
    CancelExport,
    CloseModal,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ColorPickTarget {
    PaintColor,
    EraseColor,
    FrameBackground(FrameId),
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum UnsavedChoice {
    Save,
    Discard,
    Cancel,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NewProjectParams {
    pub name: String,
    pub folder: PathBuf,
    pub background: Rgb,
    pub fps: Fps,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SaveRequest {
    pub format: SaveFormat,
    pub path: PathBuf,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum SaveFormat {
    ProjectAspr,
    FramePng,
    FrameAscii,
    AnimationAanim,
    AnimationMp4,
}

impl SaveFormat {
    pub fn extension(&self) -> &'static str {
        match self {
            SaveFormat::ProjectAspr => "aspr",
            SaveFormat::FramePng => "png",
            SaveFormat::FrameAscii => "ascii",
            SaveFormat::AnimationAanim => "aanim",
            SaveFormat::AnimationMp4 => "mp4",
        }
    }

    #[allow(dead_code)]
    pub fn scope(&self) -> SaveScope {
        match self {
            SaveFormat::ProjectAspr => SaveScope::Project,
            SaveFormat::FramePng | SaveFormat::FrameAscii => SaveScope::CurrentFrame,
            SaveFormat::AnimationAanim | SaveFormat::AnimationMp4 => SaveScope::AllFrames,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            SaveFormat::ProjectAspr => "AsciiPaint Project (.aspr)",
            SaveFormat::FramePng => "Current Frame PNG (.png)",
            SaveFormat::FrameAscii => "Current Frame Ascii (.ascii)",
            SaveFormat::AnimationAanim => "Ascii Animation (.aanim)",
            SaveFormat::AnimationMp4 => "H.264 Video (.mp4)",
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[allow(dead_code)]
pub enum SaveScope {
    Project,
    CurrentFrame,
    AllFrames,
}
