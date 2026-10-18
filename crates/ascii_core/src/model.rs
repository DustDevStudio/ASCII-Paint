use std::time::Duration;
use thiserror::Error;

#[derive(Error, Debug, PartialEq, Eq, Clone)]
pub enum ColorParseError {
    #[error("Invalid color format")]
    InvalidFormat,
    #[error("Invalid hex digits")]
    InvalidHex,
}

#[derive(Error, Debug, PartialEq, Eq, Clone)]
pub enum DomainError {
    #[error("Index out of range")]
    IndexOutOfRange,
    #[error("Cannot remove the last frame")]
    CannotRemoveLastFrame,
    #[error("Invalid FPS value")]
    InvalidFps,
    #[error("Invalid canvas specification")]
    InvalidCanvas,
    #[error("Invalid project name")]
    InvalidName,
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const BLACK: Rgb = Rgb { r: 0, g: 0, b: 0 };
    pub const WHITE: Rgb = Rgb { r: 255, g: 255, b: 255 };

    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub fn from_hex(s: &str) -> Result<Rgb, ColorParseError> {
        let s = s.trim();
        let s = s.strip_prefix('#').unwrap_or(s);
        if s.len() != 6 {
            return Err(ColorParseError::InvalidFormat);
        }
        let r = u8::from_str_radix(&s[0..2], 16).map_err(|_| ColorParseError::InvalidHex)?;
        let g = u8::from_str_radix(&s[2..4], 16).map_err(|_| ColorParseError::InvalidHex)?;
        let b = u8::from_str_radix(&s[4..6], 16).map_err(|_| ColorParseError::InvalidHex)?;
        Ok(Rgb { r, g, b })
    }

    pub fn to_hex(&self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    pub fn to_rgba(&self, alpha: u8) -> [u8; 4] {
        [self.r, self.g, self.b, alpha]
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Default)]
pub struct CellCoord {
    pub col: u16,
    pub row: u16,
}

impl CellCoord {
    pub const fn new(col: u16, row: u16) -> Self {
        Self { col, row }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct CellRect {
    pub min: CellCoord,
    pub max: CellCoord,
}

impl CellRect {
    pub fn from_corners(a: CellCoord, b: CellCoord) -> CellRect {
        CellRect {
            min: CellCoord {
                col: a.col.min(b.col),
                row: a.row.min(b.row),
            },
            max: CellCoord {
                col: a.col.max(b.col),
                row: a.row.max(b.row),
            },
        }
    }

    pub fn width(&self) -> u16 {
        self.max.col.saturating_sub(self.min.col) + 1
    }

    pub fn height(&self) -> u16 {
        self.max.row.saturating_sub(self.min.row) + 1
    }

    pub fn contains(&self, c: CellCoord) -> bool {
        c.col >= self.min.col && c.col <= self.max.col && c.row >= self.min.row && c.row <= self.max.row
    }

    pub fn iter(&self) -> impl Iterator<Item = CellCoord> {
        let min_col = self.min.col;
        let max_col = self.max.col;
        let min_row = self.min.row;
        let max_row = self.max.row;

        (min_row..=max_row).flat_map(move |r| (min_col..=max_col).map(move |c| CellCoord::new(c, r)))
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct CanvasSpec {
    pub cols: u16,
    pub rows: u16,
    pub cell_w: u8,
    pub cell_h: u8,
}

impl CanvasSpec {
    pub const DEFAULT_COLS: u16 = 240;
    pub const DEFAULT_ROWS: u16 = 72;
    pub const DEFAULT_CELL_W: u8 = 8;
    pub const DEFAULT_CELL_H: u8 = 15;

    pub fn pixel_width(&self) -> u32 {
        self.cols as u32 * self.cell_w as u32
    }

    pub fn pixel_height(&self) -> u32 {
        self.rows as u32 * self.cell_h as u32
    }

    pub fn cell_count(&self) -> usize {
        self.cols as usize * self.rows as usize
    }

    pub fn contains(&self, c: CellCoord) -> bool {
        c.col < self.cols && c.row < self.rows
    }

    pub fn clamp(&self, col: i32, row: i32) -> CellCoord {
        let max_col = self.cols.saturating_sub(1) as i32;
        let max_row = self.rows.saturating_sub(1) as i32;
        CellCoord {
            col: col.clamp(0, max_col) as u16,
            row: row.clamp(0, max_row) as u16,
        }
    }
}

impl Default for CanvasSpec {
    fn default() -> Self {
        Self {
            cols: Self::DEFAULT_COLS,
            rows: Self::DEFAULT_ROWS,
            cell_w: Self::DEFAULT_CELL_W,
            cell_h: Self::DEFAULT_CELL_H,
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Cell {
    pub glyph: char,
    pub fg: Rgb,
    pub bg: Option<Rgb>,
}

impl Cell {
    pub const EMPTY: Cell = Cell {
        glyph: ' ',
        fg: Rgb::WHITE,
        bg: None,
    };

    pub fn is_blank(&self) -> bool {
        self.glyph == ' ' && self.bg.is_none()
    }
}

impl Default for Cell {
    fn default() -> Self {
        Self::EMPTY
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Grid {
    cols: u16,
    rows: u16,
    cells: Vec<Cell>,
}

impl Grid {
    pub fn blank(spec: &CanvasSpec) -> Grid {
        let count = spec.cell_count();
        Grid {
            cols: spec.cols,
            rows: spec.rows,
            cells: vec![Cell::EMPTY; count],
        }
    }

    pub fn new(cols: u16, rows: u16, cells: Vec<Cell>) -> Result<Grid, DomainError> {
        if cells.len() != cols as usize * rows as usize {
            return Err(DomainError::InvalidCanvas);
        }
        Ok(Grid { cols, rows, cells })
    }

    pub fn cols(&self) -> u16 {
        self.cols
    }

    pub fn rows(&self) -> u16 {
        self.rows
    }

    pub fn get(&self, c: CellCoord) -> Option<&Cell> {
        if c.col >= self.cols || c.row >= self.rows {
            None
        } else {
            let idx = self.index(c);
            self.cells.get(idx)
        }
    }

    pub fn set(&mut self, c: CellCoord, cell: Cell) -> Option<Cell> {
        if c.col >= self.cols || c.row >= self.rows {
            None
        } else {
            let idx = self.index(c);
            let prev = self.cells[idx];
            self.cells[idx] = cell;
            Some(prev)
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (CellCoord, &Cell)> {
        let cols = self.cols;
        self.cells.iter().enumerate().map(move |(idx, cell)| {
            let col = (idx % cols as usize) as u16;
            let row = (idx / cols as usize) as u16;
            (CellCoord::new(col, row), cell)
        })
    }

    pub fn raw_cells(&self) -> &[Cell] {
        &self.cells
    }

    fn index(&self, c: CellCoord) -> usize {
        c.row as usize * self.cols as usize + c.col as usize
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct FrameId(pub u64);

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Frame {
    pub id: FrameId,
    pub background: Rgb,
    pub grid: Grid,
    revision: u64,
}

impl Frame {
    pub fn blank(id: FrameId, spec: &CanvasSpec, background: Rgb) -> Frame {
        Frame {
            id,
            background,
            grid: Grid::blank(spec),
            revision: 0,
        }
    }

    pub fn from_parts(id: FrameId, background: Rgb, grid: Grid) -> Frame {
        Frame {
            id,
            background,
            grid,
            revision: 0,
        }
    }

    pub fn duplicate_as(&self, new_id: FrameId) -> Frame {
        Frame {
            id: new_id,
            background: self.background,
            grid: self.grid.clone(),
            revision: 0,
        }
    }

    pub fn set_cell(&mut self, c: CellCoord, cell: Cell) -> Option<Cell> {
        let prev = self.grid.set(c, cell)?;
        self.touch();
        Some(prev)
    }

    pub fn set_background(&mut self, bg: Rgb) -> Rgb {
        let prev = self.background;
        self.background = bg;
        self.touch();
        prev
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Fps(pub u8);

impl Fps {
    pub const MIN: u8 = 1;
    pub const MAX: u8 = 60;
    pub const DEFAULT: Fps = Fps(12);

    pub fn new(v: u8) -> Result<Fps, DomainError> {
        if (Self::MIN..=Self::MAX).contains(&v) {
            Ok(Fps(v))
        } else {
            Err(DomainError::InvalidFps)
        }
    }

    pub fn frame_duration(&self) -> Duration {
        Duration::from_secs_f64(1.0 / self.0 as f64)
    }
}

impl Default for Fps {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Timeline {
    frames: Vec<Frame>,
    selected: usize,
    pub fps: Fps,
    pub looping: bool,
}

impl Timeline {
    pub fn new(initial_frame: Frame, fps: Fps) -> Self {
        Timeline {
            frames: vec![initial_frame],
            selected: 0,
            fps,
            looping: true,
        }
    }

    pub fn from_frames(frames: Vec<Frame>, selected: usize, fps: Fps, looping: bool) -> Result<Self, DomainError> {
        if frames.is_empty() {
            return Err(DomainError::CannotRemoveLastFrame);
        }
        if selected >= frames.len() {
            return Err(DomainError::IndexOutOfRange);
        }
        Ok(Timeline {
            frames,
            selected,
            fps,
            looping,
        })
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub fn frame(&self, i: usize) -> Option<&Frame> {
        self.frames.get(i)
    }

    pub fn frame_mut(&mut self, i: usize) -> Option<&mut Frame> {
        self.frames.get_mut(i)
    }

    pub fn selected_index(&self) -> usize {
        self.selected
    }

    pub fn selected_frame(&self) -> &Frame {
        &self.frames[self.selected]
    }

    pub fn selected_frame_mut(&mut self) -> &mut Frame {
        &mut self.frames[self.selected]
    }

    pub fn select(&mut self, i: usize) -> Result<(), DomainError> {
        if i < self.frames.len() {
            self.selected = i;
            Ok(())
        } else {
            Err(DomainError::IndexOutOfRange)
        }
    }

    pub fn index_of(&self, id: FrameId) -> Option<usize> {
        self.frames.iter().position(|f| f.id == id)
    }

    pub fn insert(&mut self, i: usize, f: Frame) {
        let idx = i.min(self.frames.len());
        self.frames.insert(idx, f);
    }

    pub fn remove(&mut self, i: usize) -> Result<Frame, DomainError> {
        if self.frames.len() <= 1 {
            return Err(DomainError::CannotRemoveLastFrame);
        }
        if i >= self.frames.len() {
            return Err(DomainError::IndexOutOfRange);
        }
        let removed = self.frames.remove(i);
        if self.selected >= self.frames.len() {
            self.selected = self.frames.len() - 1;
        }
        Ok(removed)
    }

    pub fn move_frame(&mut self, from: usize, to: usize) -> Result<(), DomainError> {
        if from >= self.frames.len() {
            return Err(DomainError::IndexOutOfRange);
        }
        let frame = self.frames.remove(from);
        let dest = to.min(self.frames.len());
        self.frames.insert(dest, frame);
        Ok(())
    }

    pub fn frames(&self) -> &[Frame] {
        &self.frames
    }

    pub fn frames_mut(&mut self) -> &mut [Frame] {
        &mut self.frames
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ProjectMeta {
    pub name: String,
    pub created_at: i64,
    pub modified_at: i64,
    pub app_version: String,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Project {
    pub meta: ProjectMeta,
    pub canvas: CanvasSpec,
    pub timeline: Timeline,
    next_frame_id: u64,
}

impl Project {
    pub fn new(name: String, background: Rgb, fps: Fps, now: i64) -> Project {
        let canvas = CanvasSpec::default();
        let frame = Frame::blank(FrameId(1), &canvas, background);
        let timeline = Timeline::new(frame, fps);
        let meta = ProjectMeta {
            name,
            created_at: now,
            modified_at: now,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
        };
        Project {
            meta,
            canvas,
            timeline,
            next_frame_id: 2,
        }
    }

    pub fn from_parts(
        meta: ProjectMeta,
        canvas: CanvasSpec,
        timeline: Timeline,
        next_frame_id: u64,
    ) -> Project {
        Project {
            meta,
            canvas,
            timeline,
            next_frame_id,
        }
    }

    pub fn alloc_frame_id(&mut self) -> FrameId {
        let id = self.next_frame_id;
        self.next_frame_id = self.next_frame_id.saturating_add(1);
        FrameId(id)
    }

    pub fn touch_modified(&mut self, now: i64) {
        self.meta.modified_at = now;
    }

    pub fn next_frame_id(&self) -> u64 {
        self.next_frame_id
    }
}
