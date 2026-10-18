# ASCII-Paint

A desktop ASCII art and animation editor built in Rust with `eframe`/`egui`.

## Architecture

The project is organized as a Cargo workspace with four crates:

- **`ascii_core`** (lib): Pure domain logic, data models, canvas and cell structures, selection system, rasterization and flood-fill algorithms, drawing tools, and command-based undo/redo history. Free of UI, IO, filesystem, and time dependencies.
- **`ascii_render`** (lib): Glyph source abstraction, TTF glyph rasterizer using `fontdue`, frame compositing (`FrameRasterizer`), and frame thumbnail generator (`ThumbnailBuilder`).
- **`ascii_io`** (lib): Binary format container codec (`.ascii`, `.aanim`, `.aspr`) with little-endian encoding, zstd compression, and CRC-32 integrity checking; atomic filesystem writer; project repository; recent projects store; and export service (PNG, ASCII, Aanim, and asynchronous MP4 video via `ffmpeg`).
- **`ascii_app`** (bin): GUI application built with `eframe`/`egui`, including application shell, welcome screen, modals, canvas view, viewport controller (zoom/pan), glyph atlas, playback controller, frame timeline, and drawing tools.

## File Formats

All formats use a unified 20-byte little-endian container header with zstd compression and CRC-32 validation:

- **`.aspr`**: Complete AsciiPaint project including metadata, canvas spec, timeline frames, playback settings, and background colors.
- **`.ascii`**: Single-frame ASCII image export with canvas header and frame block.
- **`.aanim`**: Multi-frame ASCII animation export with canvas header, animation header, and sequence of frame blocks.
- **`.png`**: High-resolution raster image export of the current frame.
- **`.mp4`**: H.264 video export using `ffmpeg`.

## Building and Running

Prerequisites:
- Rust toolchain (2021 edition or later)
- `ffmpeg` (optional, for MP4 video export)

### Run Tests

```bash
cargo test --workspace
```

### Run Application

```bash
cargo run --bin ascii_app
```