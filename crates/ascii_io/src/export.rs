use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use thiserror::Error;

use ascii_core::model::{CanvasSpec, Fps, Frame, Project};
use ascii_render::{FrameRasterizer, GlyphSource};
use crate::format::{AanimCodec, AsciiAnimation, AsciiCodec, AsciiImage, FormatCodec, FormatError};
use crate::storage::AtomicWriter;

#[derive(Error, Debug)]
pub enum ExportError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    #[error("Image encoding error: {0}")]
    Image(String),
    #[error("Format error: {0}")]
    Format(#[from] FormatError),
    #[error("ffmpeg was not found")]
    FfmpegNotFound,
    #[error("ffmpeg failed with code {status}: {stderr_tail}")]
    FfmpegFailed { status: i32, stderr_tail: String },
    #[error("Export was cancelled")]
    Cancelled,
}

#[derive(Clone, Debug)]
pub enum ExportMessage {
    Progress(u32, u32),
    Finished(PathBuf),
    Failed(String),
    Cancelled,
}

pub struct ExportJobHandle {
    rx: Receiver<ExportMessage>,
    cancel: Arc<AtomicBool>,
    _join: Option<JoinHandle<()>>,
}

impl ExportJobHandle {
    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn poll(&self) -> Vec<ExportMessage> {
        let mut messages = Vec::new();
        while let Ok(msg) = self.rx.try_recv() {
            messages.push(msg);
        }
        messages
    }
}

pub struct FfmpegLocator;

impl FfmpegLocator {
    pub fn find() -> Option<PathBuf> {
        // 1. Check next to current executable
        if let Ok(mut exe) = std::env::current_exe() {
            exe.pop();
            let candidate = exe.join(if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" });
            if candidate.is_file() {
                return Some(candidate);
            }
        }

        // 2. Search PATH
        if let Ok(paths) = std::env::var("PATH") {
            let separator = if cfg!(windows) { ';' } else { ':' };
            let exe_name = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
            for dir in paths.split(separator) {
                let candidate = Path::new(dir).join(exe_name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }

        None
    }
}

pub struct Clock;

impl Clock {
    pub fn now_unix() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }
}

pub struct Mp4Worker {
    frames: Vec<Frame>,
    canvas: CanvasSpec,
    fps: Fps,
    out: PathBuf,
    ffmpeg: PathBuf,
    glyphs: Arc<dyn GlyphSource>,
}

impl Mp4Worker {
    pub fn run(self, tx: Sender<ExportMessage>, cancel: Arc<AtomicBool>) {
        let part_file = self.out.with_extension(format!(
            "{}.part",
            self.out.extension().and_then(|e| e.to_str()).unwrap_or("mp4")
        ));

        let width = self.canvas.pixel_width();
        let height = self.canvas.pixel_height();
        let fps = self.fps.0;

        let spawn_res = Command::new(&self.ffmpeg)
            .args([
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgba",
                "-s",
                &format!("{}x{}", width, height),
                "-framerate",
                &fps.to_string(),
                "-i",
                "pipe:0",
                "-vf",
                "pad=ceil(iw/2)*2:ceil(ih/2)*2",
                "-c:v",
                "libx264",
                "-preset",
                "medium",
                "-crf",
                "18",
                "-pix_fmt",
                "yuv420p",
                "-movflags",
                "+faststart",
                "-f",
                "mp4",
            ])
            .arg(&part_file)
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();

        let mut child: Child = match spawn_res {
            Ok(c) => c,
            Err(e) => {
                let _ = tx.send(ExportMessage::Failed(e.to_string()));
                return;
            }
        };

        let mut stdin = match child.stdin.take() {
            Some(s) => s,
            None => {
                let _ = child.kill();
                let _ = tx.send(ExportMessage::Failed("Cannot open ffmpeg stdin".to_string()));
                return;
            }
        };

        // Drain stderr in background thread
        let mut stderr = child.stderr.take();
        let stderr_thread = thread::spawn(move || {
            let mut tail = Vec::new();
            if let Some(mut err) = stderr.take() {
                let mut buf = [0u8; 1024];
                while let Ok(n) = err.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    tail.extend_from_slice(&buf[..n]);
                    if tail.len() > 4096 {
                        tail.drain(0..tail.len() - 4096);
                    }
                }
            }
            String::from_utf8_lossy(&tail).to_string()
        });

        let total = self.frames.len() as u32;

        for (i, frame) in self.frames.iter().enumerate() {
            if cancel.load(Ordering::SeqCst) {
                let _ = child.kill();
                drop(stdin);
                let _ = std::fs::remove_file(&part_file);
                let _ = tx.send(ExportMessage::Cancelled);
                let _ = stderr_thread.join();
                return;
            }

            let buf = FrameRasterizer::render(frame, &self.canvas, self.glyphs.as_ref());
            if let Err(e) = stdin.write_all(&buf.pixels) {
                let _ = child.kill();
                drop(stdin);
                let _ = std::fs::remove_file(&part_file);
                let _ = tx.send(ExportMessage::Failed(format!("Pipe write error: {}", e)));
                let _ = stderr_thread.join();
                return;
            }

            let _ = tx.send(ExportMessage::Progress(i as u32 + 1, total));
        }

        drop(stdin);

        let status = match child.wait() {
            Ok(s) => s,
            Err(e) => {
                let _ = std::fs::remove_file(&part_file);
                let _ = tx.send(ExportMessage::Failed(e.to_string()));
                let _ = stderr_thread.join();
                return;
            }
        };

        let stderr_tail = stderr_thread.join().unwrap_or_default();

        if status.success() {
            if let Err(e) = std::fs::rename(&part_file, &self.out) {
                let _ = tx.send(ExportMessage::Failed(e.to_string()));
            } else {
                let _ = tx.send(ExportMessage::Finished(self.out));
            }
        } else {
            let _ = std::fs::remove_file(&part_file);
            let code = status.code().unwrap_or(-1);
            let _ = tx.send(ExportMessage::Failed(format!(
                "ffmpeg exited with {}: {}",
                code, stderr_tail
            )));
        }
    }
}

pub struct ExportService;

impl ExportService {
    pub fn new() -> Self {
        Self
    }

    pub fn export_frame_png(
        &self,
        project: &Project,
        path: &Path,
        glyphs: &dyn GlyphSource,
    ) -> Result<(), ExportError> {
        let frame = project.timeline.selected_frame();
        let buf = FrameRasterizer::render(frame, &project.canvas, glyphs);

        let mut png_bytes = Vec::new();
        let encoder = image::codecs::png::PngEncoder::new(&mut png_bytes);
        use image::ImageEncoder;
        encoder
            .write_image(
                &buf.pixels,
                buf.width,
                buf.height,
                image::ExtendedColorType::Rgba8,
            )
            .map_err(|e| ExportError::Image(e.to_string()))?;

        AtomicWriter::write(path, &png_bytes)?;
        Ok(())
    }

    pub fn export_frame_ascii(
        &self,
        project: &Project,
        path: &Path,
        glyphs: &dyn GlyphSource,
    ) -> Result<(), ExportError> {
        let image = AsciiImage {
            canvas: project.canvas,
            font_name: glyphs.font_name().to_string(),
            frame: project.timeline.selected_frame().clone(),
        };
        let codec = AsciiCodec;
        let bytes = codec.encode(&image)?;
        AtomicWriter::write(path, &bytes)?;
        Ok(())
    }

    pub fn export_animation_aanim(
        &self,
        project: &Project,
        path: &Path,
        glyphs: &dyn GlyphSource,
    ) -> Result<(), ExportError> {
        let anim = AsciiAnimation {
            canvas: project.canvas,
            font_name: glyphs.font_name().to_string(),
            fps: project.timeline.fps,
            looping: project.timeline.looping,
            frames: project.timeline.frames().to_vec(),
        };
        let codec = AanimCodec;
        let bytes = codec.encode(&anim)?;
        AtomicWriter::write(path, &bytes)?;
        Ok(())
    }

    pub fn start_mp4(
        &self,
        project: &Project,
        path: PathBuf,
        glyphs: Arc<dyn GlyphSource>,
        ffmpeg: PathBuf,
    ) -> ExportJobHandle {
        let (tx, rx) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_clone = Arc::clone(&cancel);

        let worker = Mp4Worker {
            frames: project.timeline.frames().to_vec(),
            canvas: project.canvas,
            fps: project.timeline.fps,
            out: path,
            ffmpeg,
            glyphs,
        };

        let handle = thread::spawn(move || {
            worker.run(tx, cancel_clone);
        });

        ExportJobHandle {
            rx,
            cancel,
            _join: Some(handle),
        }
    }
}

impl Default for ExportService {
    fn default() -> Self {
        Self::new()
    }
}
