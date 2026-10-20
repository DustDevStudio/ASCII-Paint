use ascii_core::compose::{FlatCell, FlatFrame};
use ascii_core::model::{
    CanvasSpec, Fps, Frame, FrameId, Grid, InkCell, Layer, LayerId,
    Project, ProjectMeta, Rgb, Rgba, Timeline,
};
use thiserror::Error;

#[derive(Error, Debug, PartialEq, Eq, Clone)]
pub enum FormatError {
    #[error("File is truncated")]
    Truncated,
    #[error("Bad magic bytes")]
    BadMagic,
    #[error("Unsupported version: {found}")]
    UnsupportedVersion { found: u16 },
    #[error("Reserved flags set: {flags:#x}")]
    ReservedFlags { flags: u16 },
    #[error("CRC-32 checksum mismatch")]
    CrcMismatch,
    #[error("Length mismatch")]
    LengthMismatch,
    #[error("Payload exceeds maximum size")]
    TooLarge,
    #[error("Invalid field '{name}': {reason}")]
    InvalidField { name: &'static str, reason: String },
    #[error("Compression error: {0}")]
    Compression(String),
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum FileKind {
    Ascii,
    Aanim,
    Aspr,
}

impl FileKind {
    pub const fn magic(&self) -> [u8; 4] {
        match self {
            FileKind::Ascii => *b"ASCI",
            FileKind::Aanim => *b"AANM",
            FileKind::Aspr => *b"ASPR",
        }
    }

    pub const fn extension(&self) -> &'static str {
        match self {
            FileKind::Ascii => "ascii",
            FileKind::Aanim => "aanim",
            FileKind::Aspr => "aspr",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContainerHeader {
    pub magic: [u8; 4],
    pub version: u16,
    pub flags: u16,
    pub uncompressed_len: u32,
    pub stored_len: u32,
    pub crc32: u32,
}

pub struct ContainerCodec;

impl ContainerCodec {
    pub const MAX_UNCOMPRESSED_LEN: usize = 512 * 1024 * 1024; // 512 MiB

    pub fn wrap(kind: FileKind, payload: &[u8]) -> Result<Vec<u8>, FormatError> {
        if payload.len() > Self::MAX_UNCOMPRESSED_LEN {
            return Err(FormatError::TooLarge);
        }

        let crc = crc32fast::hash(payload);
        let uncompressed_len = payload.len() as u32;

        let compressed = zstd::encode_all(payload, 3)
            .map_err(|e| FormatError::Compression(e.to_string()))?;

        let (flags, body) = if compressed.len() < payload.len() {
            (1u16, compressed)
        } else {
            (0u16, payload.to_vec())
        };

        let stored_len = body.len() as u32;
        let mut out = Vec::with_capacity(20 + body.len());

        out.extend_from_slice(&kind.magic());
        out.extend_from_slice(&1u16.to_le_bytes()); // version = 1
        out.extend_from_slice(&flags.to_le_bytes());
        out.extend_from_slice(&uncompressed_len.to_le_bytes());
        out.extend_from_slice(&stored_len.to_le_bytes());
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&body);

        Ok(out)
    }

    pub fn unwrap(kind: FileKind, bytes: &[u8]) -> Result<Vec<u8>, FormatError> {
        // 1. bytes.len() >= 20
        if bytes.len() < 20 {
            return Err(FormatError::Truncated);
        }

        // 2. magic == expected
        let magic = [bytes[0], bytes[1], bytes[2], bytes[3]];
        if magic != kind.magic() {
            return Err(FormatError::BadMagic);
        }

        // 3. version <= 1 (version 0 also rejected)
        let version = u16::from_le_bytes([bytes[4], bytes[5]]);
        if version != 1 {
            return Err(FormatError::UnsupportedVersion { found: version });
        }

        // 4. flags & 0xFFFE == 0
        let flags = u16::from_le_bytes([bytes[6], bytes[7]]);
        if (flags & 0xFFFE) != 0 {
            return Err(FormatError::ReservedFlags { flags });
        }

        let uncompressed_len =
            u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
        let stored_len =
            u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
        let crc = u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);

        // 5. bytes.len() == 20 + stored_len
        if bytes.len() < 20 + stored_len {
            return Err(FormatError::Truncated);
        }
        if bytes.len() > 20 + stored_len {
            return Err(FormatError::LengthMismatch);
        }

        // 6. uncompressed_len <= 512 MiB
        if uncompressed_len > Self::MAX_UNCOMPRESSED_LEN {
            return Err(FormatError::LengthMismatch);
        }

        let body = &bytes[20..20 + stored_len];

        // 7. Decode body
        let payload = if (flags & 1) != 0 {
            let mut cursor = std::io::Cursor::new(body);
            let mut decoder = zstd::Decoder::new(&mut cursor)
                .map_err(|e| FormatError::Compression(e.to_string()))?;
            let mut decompressed = Vec::with_capacity(uncompressed_len);
            use std::io::Read;
            let mut take = (&mut decoder).take(uncompressed_len as u64 + 1);
            take.read_to_end(&mut decompressed)
                .map_err(|e| FormatError::Compression(e.to_string()))?;
            decompressed
        } else {
            body.to_vec()
        };

        // 8. payload.len() == uncompressed_len
        if payload.len() != uncompressed_len {
            return Err(FormatError::LengthMismatch);
        }

        // 9. crc32 matches
        if crc32fast::hash(&payload) != crc {
            return Err(FormatError::CrcMismatch);
        }

        Ok(payload)
    }
}

pub struct ByteReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> ByteReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn u8(&mut self) -> Result<u8, FormatError> {
        if self.pos < self.data.len() {
            let val = self.data[self.pos];
            self.pos += 1;
            Ok(val)
        } else {
            Err(FormatError::Truncated)
        }
    }

    pub fn u16(&mut self) -> Result<u16, FormatError> {
        let b = self.bytes(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    pub fn u32(&mut self) -> Result<u32, FormatError> {
        let b = self.bytes(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn i64(&mut self) -> Result<i64, FormatError> {
        let b = self.bytes(8)?;
        Ok(i64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    pub fn string(&mut self) -> Result<String, FormatError> {
        let len = self.u16()? as usize;
        let b = self.bytes(len)?;
        String::from_utf8(b.to_vec()).map_err(|e| FormatError::InvalidField {
            name: "string",
            reason: e.to_string(),
        })
    }

    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8], FormatError> {
        if self.pos + n <= self.data.len() {
            let slice = &self.data[self.pos..self.pos + n];
            self.pos += n;
            Ok(slice)
        } else {
            Err(FormatError::Truncated)
        }
    }

    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }
}

pub struct FlatFrameBlockCodec;

impl FlatFrameBlockCodec {
    pub fn write(out: &mut Vec<u8>, flat: &FlatFrame) {
        // 1. background [u8; 3]
        out.push(flat.background.r);
        out.push(flat.background.g);
        out.push(flat.background.b);

        // 2. glyph_plane: [u32; cols*rows] LE
        for (_, cell) in flat.grid.iter() {
            let scalar = cell.glyph as u32;
            out.extend_from_slice(&scalar.to_le_bytes());
        }

        // 3. ink_plane: [u8; 4 * cols*rows] (RGBA)
        for (_, cell) in flat.grid.iter() {
            let norm_ink = if cell.glyph == ' ' {
                Rgba::TRANSPARENT
            } else {
                cell.ink.normalized()
            };
            out.push(norm_ink.r);
            out.push(norm_ink.g);
            out.push(norm_ink.b);
            out.push(norm_ink.a);
        }

        // 4. fill_plane: [u8; 4 * cols*rows] (RGBA)
        for (_, cell) in flat.grid.iter() {
            let norm_fill = cell.fill.normalized();
            out.push(norm_fill.r);
            out.push(norm_fill.g);
            out.push(norm_fill.b);
            out.push(norm_fill.a);
        }
    }

    pub fn read(r: &mut ByteReader, canvas: &CanvasSpec) -> Result<FlatFrame, FormatError> {
        let total = canvas.cell_count();
        let expected_bytes = 3 + 12 * total;
        if r.remaining() < expected_bytes {
            return Err(FormatError::Truncated);
        }

        let bg_r = r.u8()?;
        let bg_g = r.u8()?;
        let bg_b = r.u8()?;
        let background = Rgb::new(bg_r, bg_g, bg_b);

        let mut glyphs = Vec::with_capacity(total);
        for _ in 0..total {
            let val = r.u32()?;
            let ch = char::from_u32(val).unwrap_or('\u{FFFD}');
            glyphs.push(ch);
        }

        let mut inks = Vec::with_capacity(total);
        for i in 0..total {
            let ir = r.u8()?;
            let ig = r.u8()?;
            let ib = r.u8()?;
            let ia = r.u8()?;
            let rgba = if glyphs[i] == ' ' || ia == 0 {
                Rgba::TRANSPARENT
            } else {
                Rgba::new(ir, ig, ib, ia).normalized()
            };
            inks.push(rgba);
        }

        let mut fills = Vec::with_capacity(total);
        for _ in 0..total {
            let fr = r.u8()?;
            let fg = r.u8()?;
            let fb = r.u8()?;
            let fa = r.u8()?;
            let rgba = if fa == 0 {
                Rgba::TRANSPARENT
            } else {
                Rgba::new(fr, fg, fb, fa).normalized()
            };
            fills.push(rgba);
        }

        let mut cells = Vec::with_capacity(total);
        for i in 0..total {
            cells.push(FlatCell {
                glyph: glyphs[i],
                ink: inks[i],
                fill: fills[i],
            });
        }

        let grid = Grid::from_cells(canvas.cols, canvas.rows, cells)
            .map_err(|_| FormatError::InvalidField {
                name: "grid",
                reason: "Dimension mismatch".to_string(),
            })?;

        Ok(FlatFrame { background, grid })
    }
}

pub struct LayeredFrameBlockCodec;

impl LayeredFrameBlockCodec {
    pub fn write(out: &mut Vec<u8>, frame: &Frame) {
        // FrameHeader: 7 bytes
        out.push(frame.background.r);
        out.push(frame.background.g);
        out.push(frame.background.b);
        out.extend_from_slice(&(frame.active_index() as u16).to_le_bytes());
        out.extend_from_slice(&(frame.layers().len() as u16).to_le_bytes());

        // LayerBlocks
        for layer in frame.layers() {
            let name_bytes = layer.name.as_bytes();
            out.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
            out.extend_from_slice(name_bytes);
            let flags: u8 = if layer.visible { 1 } else { 0 };
            out.push(flags);
            out.push(layer.opacity);

            // glyph_plane
            for (_, cell) in layer.ink.iter() {
                let scalar = cell.glyph as u32;
                out.extend_from_slice(&scalar.to_le_bytes());
            }

            // ink_plane
            for (_, cell) in layer.ink.iter() {
                let norm = cell.color.normalized();
                out.push(norm.r);
                out.push(norm.g);
                out.push(norm.b);
                out.push(norm.a);
            }

            // fill_plane
            for (_, cell) in layer.fill.iter() {
                let norm = cell.normalized();
                out.push(norm.r);
                out.push(norm.g);
                out.push(norm.b);
                out.push(norm.a);
            }
        }
    }

    pub fn read(
        r: &mut ByteReader,
        canvas: &CanvasSpec,
        id: FrameId,
    ) -> Result<Frame, FormatError> {
        let total = canvas.cell_count();
        if r.remaining() < 7 {
            return Err(FormatError::Truncated);
        }

        let bg_r = r.u8()?;
        let bg_g = r.u8()?;
        let bg_b = r.u8()?;
        let background = Rgb::new(bg_r, bg_g, bg_b);

        let active_layer = r.u16()? as usize;
        let layer_count = r.u16()? as usize;

        if layer_count == 0 || layer_count > Frame::MAX_LAYERS {
            return Err(FormatError::InvalidField {
                name: "layer_count",
                reason: format!("Must be 1..=64, got {}", layer_count),
            });
        }
        if active_layer >= layer_count {
            return Err(FormatError::InvalidField {
                name: "active_layer",
                reason: format!("Must be < layer_count, got {}", active_layer),
            });
        }

        let mut layers = Vec::with_capacity(layer_count);
        for l_idx in 0..layer_count {
            let name_raw = r.string().unwrap_or_else(|_| format!("Layer {}", l_idx + 1));
            let name = if name_raw.trim().is_empty() {
                format!("Layer {}", l_idx + 1)
            } else {
                name_raw
            };

            let flags = r.u8()?;
            if (flags & 0xFE) != 0 {
                return Err(FormatError::InvalidField {
                    name: "layer.flags",
                    reason: "reserved bits set".to_string(),
                });
            }
            let visible = (flags & 1) != 0;
            let opacity = r.u8()?;

            let mut glyphs = Vec::with_capacity(total);
            for _ in 0..total {
                let val = r.u32()?;
                let ch = char::from_u32(val).unwrap_or('\u{FFFD}');
                glyphs.push(ch);
            }

            let mut ink_cells = Vec::with_capacity(total);
            for i in 0..total {
                let ir = r.u8()?;
                let ig = r.u8()?;
                let ib = r.u8()?;
                let ia = r.u8()?;
                let color = if glyphs[i] == ' ' || ia == 0 {
                    Rgba::TRANSPARENT
                } else {
                    Rgba::new(ir, ig, ib, ia).normalized()
                };
                ink_cells.push(InkCell {
                    glyph: glyphs[i],
                    color,
                });
            }

            let mut fill_cells = Vec::with_capacity(total);
            for _ in 0..total {
                let fr = r.u8()?;
                let fg = r.u8()?;
                let fb = r.u8()?;
                let fa = r.u8()?;
                let rgba = if fa == 0 {
                    Rgba::TRANSPARENT
                } else {
                    Rgba::new(fr, fg, fb, fa).normalized()
                };
                fill_cells.push(rgba);
            }

            let ink_grid = Grid::from_cells(canvas.cols, canvas.rows, ink_cells)
                .map_err(|_| FormatError::InvalidField {
                    name: "ink",
                    reason: "Dimension mismatch".to_string(),
                })?;
            let fill_grid = Grid::from_cells(canvas.cols, canvas.rows, fill_cells)
                .map_err(|_| FormatError::InvalidField {
                    name: "fill",
                    reason: "Dimension mismatch".to_string(),
                })?;

            layers.push(Layer {
                id: LayerId((l_idx + 1) as u32),
                name,
                visible,
                opacity,
                ink: ink_grid,
                fill: fill_grid,
            });
        }

        Frame::from_raw_parts(
            id,
            background,
            layers,
            active_layer,
            (layer_count + 1) as u32,
        )
        .map_err(|e| FormatError::InvalidField {
            name: "frame",
            reason: format!("{:?}", e),
        })
    }
}

pub trait FormatCodec<T> {
    fn kind(&self) -> FileKind;
    fn encode(&self, value: &T) -> Result<Vec<u8>, FormatError>;
    fn decode(&self, bytes: &[u8]) -> Result<T, FormatError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AsciiImage {
    pub canvas: CanvasSpec,
    pub font_name: String,
    pub frame: FlatFrame,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AsciiAnimation {
    pub canvas: CanvasSpec,
    pub font_name: String,
    pub fps: Fps,
    pub looping: bool,
    pub frames: Vec<FlatFrame>,
}

pub struct AsciiCodec;

impl FormatCodec<AsciiImage> for AsciiCodec {
    fn kind(&self) -> FileKind {
        FileKind::Ascii
    }

    fn encode(&self, value: &AsciiImage) -> Result<Vec<u8>, FormatError> {
        let mut payload = Vec::new();

        // CanvasHeader
        payload.extend_from_slice(&value.canvas.cols.to_le_bytes());
        payload.extend_from_slice(&value.canvas.rows.to_le_bytes());
        payload.push(value.canvas.cell_w);
        payload.push(value.canvas.cell_h);
        let font_bytes = value.font_name.as_bytes();
        payload.extend_from_slice(&(font_bytes.len() as u16).to_le_bytes());
        payload.extend_from_slice(font_bytes);

        // FlatFrameBlock
        FlatFrameBlockCodec::write(&mut payload, &value.frame);

        ContainerCodec::wrap(FileKind::Ascii, &payload)
    }

    fn decode(&self, bytes: &[u8]) -> Result<AsciiImage, FormatError> {
        let payload = ContainerCodec::unwrap(FileKind::Ascii, bytes)?;
        let mut r = ByteReader::new(&payload);

        let cols = r.u16()?;
        let rows = r.u16()?;
        let cell_w = r.u8()?;
        let cell_h = r.u8()?;
        let font_name = r.string()?;

        let canvas = CanvasSpec {
            cols,
            rows,
            cell_w,
            cell_h,
        };
        let frame = FlatFrameBlockCodec::read(&mut r, &canvas)?;

        if r.remaining() != 0 {
            return Err(FormatError::InvalidField {
                name: "payload",
                reason: "trailing bytes".to_string(),
            });
        }

        Ok(AsciiImage {
            canvas,
            font_name,
            frame,
        })
    }
}

pub struct AanimCodec;

impl FormatCodec<AsciiAnimation> for AanimCodec {
    fn kind(&self) -> FileKind {
        FileKind::Aanim
    }

    fn encode(&self, value: &AsciiAnimation) -> Result<Vec<u8>, FormatError> {
        let mut payload = Vec::new();

        // CanvasHeader
        payload.extend_from_slice(&value.canvas.cols.to_le_bytes());
        payload.extend_from_slice(&value.canvas.rows.to_le_bytes());
        payload.push(value.canvas.cell_w);
        payload.push(value.canvas.cell_h);
        let font_bytes = value.font_name.as_bytes();
        payload.extend_from_slice(&(font_bytes.len() as u16).to_le_bytes());
        payload.extend_from_slice(font_bytes);

        // AnimHeader (8 bytes)
        payload.push(value.fps.0);
        let flags: u8 = if value.looping { 1 } else { 0 };
        payload.push(flags);
        payload.extend_from_slice(&0u16.to_le_bytes()); // reserved = 0
        payload.extend_from_slice(&(value.frames.len() as u32).to_le_bytes());

        // FlatFrameBlocks
        for f in &value.frames {
            FlatFrameBlockCodec::write(&mut payload, f);
        }

        ContainerCodec::wrap(FileKind::Aanim, &payload)
    }

    fn decode(&self, bytes: &[u8]) -> Result<AsciiAnimation, FormatError> {
        let payload = ContainerCodec::unwrap(FileKind::Aanim, bytes)?;
        let mut r = ByteReader::new(&payload);

        let cols = r.u16()?;
        let rows = r.u16()?;
        let cell_w = r.u8()?;
        let cell_h = r.u8()?;
        let font_name = r.string()?;

        let canvas = CanvasSpec {
            cols,
            rows,
            cell_w,
            cell_h,
        };

        let fps_val = r.u8()?;
        let fps = Fps::new(fps_val).map_err(|_| FormatError::InvalidField {
            name: "fps",
            reason: "Must be 1..=60".to_string(),
        })?;
        let flags = r.u8()?;
        if (flags & 0xFE) != 0 {
            return Err(FormatError::InvalidField {
                name: "anim.flags",
                reason: "reserved bits set".to_string(),
            });
        }
        let looping = (flags & 1) != 0;
        let reserved = r.u16()?;
        if reserved != 0 {
            return Err(FormatError::InvalidField {
                name: "anim.reserved",
                reason: "must be 0".to_string(),
            });
        }
        let frame_count = r.u32()? as usize;
        if frame_count == 0 || frame_count > 10000 {
            return Err(FormatError::InvalidField {
                name: "frame_count",
                reason: "Must be 1..=10000".to_string(),
            });
        }

        let mut frames = Vec::with_capacity(frame_count);
        for _ in 0..frame_count {
            let f = FlatFrameBlockCodec::read(&mut r, &canvas)?;
            frames.push(f);
        }

        if r.remaining() != 0 {
            return Err(FormatError::InvalidField {
                name: "payload",
                reason: "trailing bytes".to_string(),
            });
        }

        Ok(AsciiAnimation {
            canvas,
            font_name,
            fps,
            looping,
            frames,
        })
    }
}

pub struct AsprCodec;

impl FormatCodec<Project> for AsprCodec {
    fn kind(&self) -> FileKind {
        FileKind::Aspr
    }

    fn encode(&self, value: &Project) -> Result<Vec<u8>, FormatError> {
        let mut payload = Vec::new();

        // CanvasHeader
        payload.extend_from_slice(&value.canvas.cols.to_le_bytes());
        payload.extend_from_slice(&value.canvas.rows.to_le_bytes());
        payload.push(value.canvas.cell_w);
        payload.push(value.canvas.cell_h);
        let font_bytes = b"default";
        payload.extend_from_slice(&(font_bytes.len() as u16).to_le_bytes());
        payload.extend_from_slice(font_bytes);

        // ProjectMeta
        let name_bytes = value.meta.name.as_bytes();
        if name_bytes.is_empty() || name_bytes.len() > 1024 {
            return Err(FormatError::InvalidField {
                name: "meta.name",
                reason: "Length must be 1..=1024".to_string(),
            });
        }
        payload.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        payload.extend_from_slice(name_bytes);
        payload.extend_from_slice(&value.meta.created_at.to_le_bytes());
        payload.extend_from_slice(&value.meta.modified_at.to_le_bytes());
        let ver_bytes = value.meta.app_version.as_bytes();
        payload.extend_from_slice(&(ver_bytes.len() as u16).to_le_bytes());
        payload.extend_from_slice(ver_bytes);

        // ProjectHeader (12 bytes)
        payload.push(value.timeline.fps.0);
        let flags: u8 = if value.timeline.looping { 1 } else { 0 };
        payload.push(flags);
        payload.extend_from_slice(&0u16.to_le_bytes()); // reserved = 0
        payload.extend_from_slice(&(value.timeline.selected_index() as u32).to_le_bytes());
        payload.extend_from_slice(&(value.timeline.len() as u32).to_le_bytes());

        // LayeredFrameBlocks
        for f in value.timeline.frames() {
            LayeredFrameBlockCodec::write(&mut payload, f);
        }

        ContainerCodec::wrap(FileKind::Aspr, &payload)
    }

    fn decode(&self, bytes: &[u8]) -> Result<Project, FormatError> {
        let payload = ContainerCodec::unwrap(FileKind::Aspr, bytes)?;
        let mut r = ByteReader::new(&payload);

        // CanvasHeader
        let cols = r.u16()?;
        let rows = r.u16()?;
        let cell_w = r.u8()?;
        let cell_h = r.u8()?;
        let _font_name = r.string()?;

        let canvas = CanvasSpec {
            cols,
            rows,
            cell_w,
            cell_h,
        };

        // ProjectMeta
        let name = r.string()?;
        let created_at = r.i64()?;
        let modified_at = r.i64()?;
        let app_version = r.string()?;

        let meta = ProjectMeta {
            name,
            created_at,
            modified_at,
            app_version,
        };

        // ProjectHeader
        let fps_val = r.u8()?;
        let fps = Fps::new(fps_val).map_err(|_| FormatError::InvalidField {
            name: "fps",
            reason: "Must be 1..=60".to_string(),
        })?;
        let flags = r.u8()?;
        if (flags & 0xFE) != 0 {
            return Err(FormatError::InvalidField {
                name: "project.flags",
                reason: "reserved bits set".to_string(),
            });
        }
        let looping = (flags & 1) != 0;
        let reserved = r.u16()?;
        if reserved != 0 {
            return Err(FormatError::InvalidField {
                name: "project.reserved",
                reason: "must be 0".to_string(),
            });
        }
        let selected_index = r.u32()? as usize;
        let frame_count = r.u32()? as usize;

        if frame_count == 0 || frame_count > 10000 {
            return Err(FormatError::InvalidField {
                name: "frame_count",
                reason: "Must be 1..=10000".to_string(),
            });
        }
        if selected_index >= frame_count {
            return Err(FormatError::InvalidField {
                name: "selected_index",
                reason: format!("Must be < frame_count, got {}", selected_index),
            });
        }

        let mut frames = Vec::with_capacity(frame_count);
        for f_idx in 0..frame_count {
            let f = LayeredFrameBlockCodec::read(&mut r, &canvas, FrameId((f_idx + 1) as u64))?;
            frames.push(f);
        }

        if r.remaining() != 0 {
            return Err(FormatError::InvalidField {
                name: "payload",
                reason: "trailing bytes".to_string(),
            });
        }

        let first = frames.remove(0);
        let mut timeline = Timeline::new(first, fps);
        timeline.looping = looping;
        for f in frames {
            timeline.insert(timeline.len(), f);
        }
        let _ = timeline.select(selected_index);

        Ok(Project::from_raw_parts(
            meta,
            canvas,
            timeline,
            (frame_count + 1) as u64,
        ))
    }
}
