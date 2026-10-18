use ascii_core::model::{CanvasSpec, Cell, Fps, Frame, FrameId, Grid, Project, ProjectMeta, Rgb, Timeline};
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
    const MAX_UNCOMPRESSED_LEN: usize = 512 * 1024 * 1024; // 512 MiB

    pub fn wrap(kind: FileKind, payload: &[u8]) -> Result<Vec<u8>, FormatError> {
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

        // 3. version == 1
        let version = u16::from_le_bytes([bytes[4], bytes[5]]);
        if version != 1 {
            return Err(FormatError::UnsupportedVersion { found: version });
        }

        // 4. flags & 0xFFFE == 0
        let flags = u16::from_le_bytes([bytes[6], bytes[7]]);
        if (flags & 0xFFFE) != 0 {
            return Err(FormatError::ReservedFlags { flags });
        }

        let uncompressed_len = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
        let stored_len = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
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
            // Limit decompression to uncompressed_len
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

pub struct FrameBlockCodec;

impl FrameBlockCodec {
    pub fn write(out: &mut Vec<u8>, frame: &Frame) {
        // 1. background [u8; 3]
        out.push(frame.background.r);
        out.push(frame.background.g);
        out.push(frame.background.b);

        // 2. glyph_plane: [u32; cols*rows] LE
        for (_, cell) in frame.grid.iter() {
            let scalar = cell.glyph as u32;
            out.extend_from_slice(&scalar.to_le_bytes());
        }

        // 3. fg_plane: [u8; 3 * cols*rows]
        for (_, cell) in frame.grid.iter() {
            out.push(cell.fg.r);
            out.push(cell.fg.g);
            out.push(cell.fg.b);
        }

        // 4. bg_plane: [u8; 4 * cols*rows] (RGBA: A=0 or A=255)
        for (_, cell) in frame.grid.iter() {
            match cell.bg {
                Some(c) => {
                    out.push(c.r);
                    out.push(c.g);
                    out.push(c.b);
                    out.push(255);
                }
                None => {
                    out.push(0);
                    out.push(0);
                    out.push(0);
                    out.push(0);
                }
            }
        }
    }

    pub fn read(
        r: &mut ByteReader,
        canvas: &CanvasSpec,
        id: FrameId,
    ) -> Result<Frame, FormatError> {
        let count = canvas.cell_count();
        let expected_bytes = 3 + 11 * count;
        if r.remaining() < expected_bytes {
            return Err(FormatError::Truncated);
        }

        let bg_r = r.u8()?;
        let bg_g = r.u8()?;
        let bg_b = r.u8()?;
        let background = Rgb::new(bg_r, bg_g, bg_b);

        let mut glyphs = Vec::with_capacity(count);
        for _ in 0..count {
            let val = r.u32()?;
            let ch = char::from_u32(val).unwrap_or('\u{FFFD}');
            glyphs.push(ch);
        }

        let mut fgs = Vec::with_capacity(count);
        for _ in 0..count {
            let cr = r.u8()?;
            let cg = r.u8()?;
            let cb = r.u8()?;
            fgs.push(Rgb::new(cr, cg, cb));
        }

        let mut bgs = Vec::with_capacity(count);
        for _ in 0..count {
            let cr = r.u8()?;
            let cg = r.u8()?;
            let cb = r.u8()?;
            let ca = r.u8()?;
            if ca == 0 {
                bgs.push(None);
            } else if ca == 255 {
                bgs.push(Some(Rgb::new(cr, cg, cb)));
            } else {
                return Err(FormatError::InvalidField {
                    name: "bg_plane",
                    reason: "alpha must be 0 or 255".to_string(),
                });
            }
        }

        let mut cells = Vec::with_capacity(count);
        for i in 0..count {
            cells.push(Cell {
                glyph: glyphs[i],
                fg: fgs[i],
                bg: bgs[i],
            });
        }

        let grid = Grid::new(canvas.cols, canvas.rows, cells).map_err(|_| {
            FormatError::InvalidField {
                name: "grid",
                reason: "grid size mismatch".to_string(),
            }
        })?;

        Ok(Frame::from_parts(id, background, grid))
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
    pub frame: Frame,
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

        // FrameBlock
        FrameBlockCodec::write(&mut payload, &value.frame);

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

        if !(1..=1024).contains(&cols) {
            return Err(FormatError::InvalidField {
                name: "cols",
                reason: "must be 1..=1024".to_string(),
            });
        }
        if !(1..=1024).contains(&rows) {
            return Err(FormatError::InvalidField {
                name: "rows",
                reason: "must be 1..=1024".to_string(),
            });
        }
        if !(1..=64).contains(&cell_w) {
            return Err(FormatError::InvalidField {
                name: "cell_w",
                reason: "must be 1..=64".to_string(),
            });
        }
        if !(1..=64).contains(&cell_h) {
            return Err(FormatError::InvalidField {
                name: "cell_h",
                reason: "must be 1..=64".to_string(),
            });
        }

        let canvas = CanvasSpec {
            cols,
            rows,
            cell_w,
            cell_h,
        };

        let frame = FrameBlockCodec::read(&mut r, &canvas, FrameId(1))?;
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AsciiAnimation {
    pub canvas: CanvasSpec,
    pub font_name: String,
    pub fps: Fps,
    pub looping: bool,
    pub frames: Vec<Frame>,
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
        let flags = if value.looping { 1u8 } else { 0u8 };
        payload.push(flags);
        payload.extend_from_slice(&0u16.to_le_bytes()); // reserved
        payload.extend_from_slice(&(value.frames.len() as u32).to_le_bytes());

        // FrameBlocks
        for f in &value.frames {
            FrameBlockCodec::write(&mut payload, f);
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
            reason: "must be 1..=60".to_string(),
        })?;

        let flags = r.u8()?;
        if (flags & 0xFE) != 0 {
            return Err(FormatError::InvalidField {
                name: "flags",
                reason: "reserved bits set".to_string(),
            });
        }
        let looping = (flags & 1) != 0;

        let reserved = r.u16()?;
        if reserved != 0 {
            return Err(FormatError::InvalidField {
                name: "reserved",
                reason: "must be 0".to_string(),
            });
        }

        let frame_count = r.u32()? as usize;
        if !(1..=10000).contains(&frame_count) {
            return Err(FormatError::InvalidField {
                name: "frame_count",
                reason: "must be 1..=10000".to_string(),
            });
        }

        let mut frames = Vec::with_capacity(frame_count);
        for i in 0..frame_count {
            let frame = FrameBlockCodec::read(&mut r, &canvas, FrameId((i + 1) as u64))?;
            frames.push(frame);
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

    fn encode(&self, project: &Project) -> Result<Vec<u8>, FormatError> {
        let mut payload = Vec::new();

        // 1. CanvasHeader
        payload.extend_from_slice(&project.canvas.cols.to_le_bytes());
        payload.extend_from_slice(&project.canvas.rows.to_le_bytes());
        payload.push(project.canvas.cell_w);
        payload.push(project.canvas.cell_h);
        let font_bytes = b"default";
        payload.extend_from_slice(&(font_bytes.len() as u16).to_le_bytes());
        payload.extend_from_slice(font_bytes);

        // 2. ProjectMeta
        let name_bytes = project.meta.name.as_bytes();
        payload.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        payload.extend_from_slice(name_bytes);
        payload.extend_from_slice(&project.meta.created_at.to_le_bytes());
        payload.extend_from_slice(&project.meta.modified_at.to_le_bytes());
        let ver_bytes = project.meta.app_version.as_bytes();
        payload.extend_from_slice(&(ver_bytes.len() as u16).to_le_bytes());
        payload.extend_from_slice(ver_bytes);

        // 3. ProjectHeader (12 bytes)
        payload.push(project.timeline.fps.0);
        let flags = if project.timeline.looping { 1u8 } else { 0u8 };
        payload.push(flags);
        payload.extend_from_slice(&0u16.to_le_bytes()); // reserved
        payload.extend_from_slice(&(project.timeline.selected_index() as u32).to_le_bytes());
        payload.extend_from_slice(&(project.timeline.len() as u32).to_le_bytes());

        // 4. FrameBlocks
        for f in project.timeline.frames() {
            FrameBlockCodec::write(&mut payload, f);
        }

        ContainerCodec::wrap(FileKind::Aspr, &payload)
    }

    fn decode(&self, bytes: &[u8]) -> Result<Project, FormatError> {
        let payload = ContainerCodec::unwrap(FileKind::Aspr, bytes)?;
        let mut r = ByteReader::new(&payload);

        // 1. CanvasHeader
        let cols = r.u16()?;
        let rows = r.u16()?;
        let cell_w = r.u8()?;
        let cell_h = r.u8()?;
        let _font_name = r.string()?;

        if !(1..=1024).contains(&cols) {
            return Err(FormatError::InvalidField {
                name: "cols",
                reason: "must be 1..=1024".to_string(),
            });
        }
        if !(1..=1024).contains(&rows) {
            return Err(FormatError::InvalidField {
                name: "rows",
                reason: "must be 1..=1024".to_string(),
            });
        }
        if !(1..=64).contains(&cell_w) {
            return Err(FormatError::InvalidField {
                name: "cell_w",
                reason: "must be 1..=64".to_string(),
            });
        }
        if !(1..=64).contains(&cell_h) {
            return Err(FormatError::InvalidField {
                name: "cell_h",
                reason: "must be 1..=64".to_string(),
            });
        }

        let canvas = CanvasSpec {
            cols,
            rows,
            cell_w,
            cell_h,
        };

        // 2. ProjectMeta
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

        // 3. ProjectHeader
        let fps_val = r.u8()?;
        let fps = Fps::new(fps_val).map_err(|_| FormatError::InvalidField {
            name: "fps",
            reason: "must be 1..=60".to_string(),
        })?;

        let flags = r.u8()?;
        if (flags & 0xFE) != 0 {
            return Err(FormatError::InvalidField {
                name: "flags",
                reason: "reserved bits set".to_string(),
            });
        }
        let looping = (flags & 1) != 0;

        let reserved = r.u16()?;
        if reserved != 0 {
            return Err(FormatError::InvalidField {
                name: "reserved",
                reason: "must be 0".to_string(),
            });
        }

        let selected_index = r.u32()? as usize;
        let frame_count = r.u32()? as usize;
        if !(1..=10000).contains(&frame_count) {
            return Err(FormatError::InvalidField {
                name: "frame_count",
                reason: "must be 1..=10000".to_string(),
            });
        }
        if selected_index >= frame_count {
            return Err(FormatError::InvalidField {
                name: "selected_index",
                reason: "must be < frame_count".to_string(),
            });
        }

        // 4. FrameBlocks
        let mut frames = Vec::with_capacity(frame_count);
        for i in 0..frame_count {
            let frame = FrameBlockCodec::read(&mut r, &canvas, FrameId((i + 1) as u64))?;
            frames.push(frame);
        }

        if r.remaining() != 0 {
            return Err(FormatError::InvalidField {
                name: "payload",
                reason: "trailing bytes".to_string(),
            });
        }

        let timeline = Timeline::from_frames(frames, selected_index, fps, looping)
            .map_err(|_| FormatError::InvalidField {
                name: "timeline",
                reason: "invalid timeline state".to_string(),
            })?;

        Ok(Project::from_parts(
            meta,
            canvas,
            timeline,
            (frame_count + 1) as u64,
        ))
    }
}
