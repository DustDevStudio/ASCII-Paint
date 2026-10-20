use std::time::Duration;
use thiserror::Error;

use crate::selection::Selection;

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
    #[error("Cannot remove the last layer")]
    CannotRemoveLastLayer,
    #[error("Too many layers in frame")]
    TooManyLayers,
    #[error("Invalid FPS value")]
    InvalidFps,
    #[error("Invalid canvas specification")]
    InvalidCanvas,
    #[error("Invalid project name")]
    InvalidName,
    #[error("Invalid layer opacity")]
    InvalidOpacity,
}

#[inline]
pub fn div255(x: u32) -> u32 {
    (x + 127) / 255
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

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const TRANSPARENT: Rgba = Rgba { r: 0, g: 0, b: 0, a: 0 };
    pub const BLACK: Rgba = Rgba { r: 0, g: 0, b: 0, a: 255 };
    pub const WHITE: Rgba = Rgba { r: 255, g: 255, b: 255, a: 255 };

    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const fn opaque(c: Rgb) -> Self {
        Self { r: c.r, g: c.g, b: c.b, a: 255 }
    }

    pub fn from_hex(s: &str) -> Result<Rgba, ColorParseError> {
        let s = s.trim();
        let s = s.strip_prefix('#').unwrap_or(s);
        if s.len() == 6 {
            let rgb = Rgb::from_hex(s)?;
            Ok(Self::opaque(rgb))
        } else if s.len() == 8 {
            let r = u8::from_str_radix(&s[0..2], 16).map_err(|_| ColorParseError::InvalidHex)?;
            let g = u8::from_str_radix(&s[2..4], 16).map_err(|_| ColorParseError::InvalidHex)?;
            let b = u8::from_str_radix(&s[4..6], 16).map_err(|_| ColorParseError::InvalidHex)?;
            let a = u8::from_str_radix(&s[6..8], 16).map_err(|_| ColorParseError::InvalidHex)?;
            Ok(Rgba { r, g, b, a }.normalized())
        } else {
            Err(ColorParseError::InvalidFormat)
        }
    }

    pub fn to_hex(&self) -> String {
        if self.a == 255 {
            format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
        } else {
            format!("#{:02X}{:02X}{:02X}{:02X}", self.r, self.g, self.b, self.a)
        }
    }

    pub fn rgb(&self) -> Rgb {
        Rgb { r: self.r, g: self.g, b: self.b }
    }

    pub fn is_transparent(&self) -> bool {
        self.a == 0
    }

    pub fn normalized(&self) -> Rgba {
        if self.a == 0 {
            Self::TRANSPARENT
        } else {
            *self
        }
    }

    pub fn scaled_alpha(&self, opacity: u8) -> Rgba {
        let a_prime = div255(self.a as u32 * opacity as u32) as u8;
        Rgba {
            r: self.r,
            g: self.g,
            b: self.b,
            a: a_prime,
        }
        .normalized()
    }

    pub fn over(&self, below: Rgba) -> Rgba {
        let a_s = self.a as u32;
        let a_d = below.a as u32;

        let out_a = a_s + div255(a_d * (255 - a_s));
        if out_a == 0 {
            return Self::TRANSPARENT;
        }

        let denom = out_a * 255;
        let blend_channel = |c_s: u8, c_d: u8| -> u8 {
            let cs = c_s as u32;
            let cd = c_d as u32;
            let num = cs * a_s * 255 + cd * a_d * (255 - a_s) + denom / 2;
            (num / denom).min(255) as u8
        };

        Rgba {
            r: blend_channel(self.r, below.r),
            g: blend_channel(self.g, below.g),
            b: blend_channel(self.b, below.b),
            a: out_a.min(255) as u8,
        }
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportFit {
    Fits,
    TooBig {
        suggested: CanvasSpec,
        cut_if_kept: (u32, u32),
        cut_if_resized: (u32, u32),
    },
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
    pub const MAX_DIM: u16 = 1024;

    pub fn default() -> CanvasSpec {
        CanvasSpec {
            cols: Self::DEFAULT_COLS,
            rows: Self::DEFAULT_ROWS,
            cell_w: Self::DEFAULT_CELL_W,
            cell_h: Self::DEFAULT_CELL_H,
        }
    }

    pub fn with_size(cols: u16, rows: u16) -> Result<CanvasSpec, DomainError> {
        if cols == 0 || rows == 0 || cols > Self::MAX_DIM || rows > Self::MAX_DIM {
            return Err(DomainError::InvalidCanvas);
        }
        Ok(CanvasSpec {
            cols,
            rows,
            cell_w: Self::DEFAULT_CELL_W,
            cell_h: Self::DEFAULT_CELL_H,
        })
    }

    pub fn fit_content(&self, w: u32, h: u32) -> ImportFit {
        if w <= self.cols as u32 && h <= self.rows as u32 {
            ImportFit::Fits
        } else {
            let suggested_cols = (self.cols as u32).max(w).min(Self::MAX_DIM as u32) as u16;
            let suggested_rows = (self.rows as u32).max(h).min(Self::MAX_DIM as u32) as u16;
            let suggested = CanvasSpec {
                cols: suggested_cols,
                rows: suggested_rows,
                cell_w: self.cell_w,
                cell_h: self.cell_h,
            };
            let cut_if_kept = (
                w.saturating_sub(self.cols as u32),
                h.saturating_sub(self.rows as u32),
            );
            let cut_if_resized = (
                w.saturating_sub(suggested_cols as u32),
                h.saturating_sub(suggested_rows as u32),
            );
            ImportFit::TooBig {
                suggested,
                cut_if_kept,
                cut_if_resized,
            }
        }
    }

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
        let max_col = (self.cols.saturating_sub(1)) as i32;
        let max_row = (self.rows.saturating_sub(1)) as i32;
        CellCoord {
            col: col.clamp(0, max_col) as u16,
            row: row.clamp(0, max_row) as u16,
        }
    }
}

impl Default for CanvasSpec {
    fn default() -> Self {
        Self::default()
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct InkCell {
    pub glyph: char,
    pub color: Rgba,
}

impl InkCell {
    pub const EMPTY: InkCell = InkCell {
        glyph: ' ',
        color: Rgba::TRANSPARENT,
    };

    pub fn new(glyph: char, color: Rgba) -> Self {
        Self { glyph, color }.normalized()
    }

    pub fn is_empty(&self) -> bool {
        self.glyph == ' '
    }

    pub fn normalized(&self) -> InkCell {
        if self.glyph == ' ' {
            Self::EMPTY
        } else {
            InkCell {
                glyph: self.glyph,
                color: self.color.normalized(),
            }
        }
    }
}

impl Default for InkCell {
    fn default() -> Self {
        Self::EMPTY
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum Plane {
    Foreground,
    Background,
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct PlaneSet {
    pub foreground: bool,
    pub background: bool,
}

impl PlaneSet {
    pub const BOTH: PlaneSet = PlaneSet {
        foreground: true,
        background: true,
    };
    pub const FOREGROUND: PlaneSet = PlaneSet {
        foreground: true,
        background: false,
    };
    pub const BACKGROUND: PlaneSet = PlaneSet {
        foreground: false,
        background: true,
    };

    pub fn contains(&self, p: Plane) -> bool {
        match p {
            Plane::Foreground => self.foreground,
            Plane::Background => self.background,
        }
    }

    pub fn is_empty(&self) -> bool {
        !self.foreground && !self.background
    }

    pub fn iter(&self) -> impl Iterator<Item = Plane> {
        let mut planes = Vec::new();
        if self.foreground {
            planes.push(Plane::Foreground);
        }
        if self.background {
            planes.push(Plane::Background);
        }
        planes.into_iter()
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum PlaneValue {
    Ink(InkCell),
    Fill(Rgba),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Grid<T> {
    cols: u16,
    rows: u16,
    cells: Vec<T>,
}

impl<T: Clone> Grid<T> {
    pub fn filled(spec: &CanvasSpec, value: T) -> Grid<T> {
        let count = spec.cell_count();
        Grid {
            cols: spec.cols,
            rows: spec.rows,
            cells: vec![value; count],
        }
    }

    pub fn from_cells(cols: u16, rows: u16, cells: Vec<T>) -> Result<Grid<T>, DomainError> {
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

    pub fn cells(&self) -> &[T] {
        &self.cells
    }

    pub fn cells_mut(&mut self) -> &mut [T] {
        &mut self.cells
    }

    pub fn get(&self, c: CellCoord) -> Option<&T> {
        if c.col < self.cols && c.row < self.rows {
            Some(&self.cells[self.index(c)])
        } else {
            None
        }
    }

    pub fn set(&mut self, c: CellCoord, value: T) -> Option<T> {
        if c.col < self.cols && c.row < self.rows {
            let idx = self.index(c);
            let old = std::mem::replace(&mut self.cells[idx], value);
            Some(old)
        } else {
            None
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (CellCoord, &T)> {
        let cols = self.cols;
        self.cells.iter().enumerate().map(move |(i, v)| {
            let col = (i % cols as usize) as u16;
            let row = (i / cols as usize) as u16;
            (CellCoord::new(col, row), v)
        })
    }

    pub fn resized(&self, cols: u16, rows: u16, pad: T) -> Grid<T> {
        let mut new_cells = vec![pad; cols as usize * rows as usize];
        let copy_cols = self.cols.min(cols);
        let copy_rows = self.rows.min(rows);

        for r in 0..copy_rows {
            for c in 0..copy_cols {
                let old_idx = r as usize * self.cols as usize + c as usize;
                let new_idx = r as usize * cols as usize + c as usize;
                new_cells[new_idx] = self.cells[old_idx].clone();
            }
        }

        Grid {
            cols,
            rows,
            cells: new_cells,
        }
    }

    fn index(&self, c: CellCoord) -> usize {
        c.row as usize * self.cols as usize + c.col as usize
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct LayerId(pub u32);

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LayerProps {
    pub name: String,
    pub visible: bool,
    pub opacity: u8,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Layer {
    pub id: LayerId,
    pub name: String,
    pub visible: bool,
    pub opacity: u8,
    pub ink: Grid<InkCell>,
    pub fill: Grid<Rgba>,
}

impl Layer {
    pub fn blank(id: LayerId, name: String, spec: &CanvasSpec) -> Layer {
        Layer {
            id,
            name,
            visible: true,
            opacity: 255,
            ink: Grid::filled(spec, InkCell::EMPTY),
            fill: Grid::filled(spec, Rgba::TRANSPARENT),
        }
    }

    pub fn props(&self) -> LayerProps {
        LayerProps {
            name: self.name.clone(),
            visible: self.visible,
            opacity: self.opacity,
        }
    }

    pub fn value(&self, plane: Plane, c: CellCoord) -> Option<PlaneValue> {
        match plane {
            Plane::Foreground => self.ink.get(c).map(|&ic| PlaneValue::Ink(ic)),
            Plane::Background => self.fill.get(c).map(|&fc| PlaneValue::Fill(fc)),
        }
    }

    pub fn is_blank(&self) -> bool {
        self.ink.cells().iter().all(|c| c.is_empty())
            && self.fill.cells().iter().all(|c| c.is_transparent())
    }

    pub fn resized(&self, cols: u16, rows: u16) -> Layer {
        Layer {
            id: self.id,
            name: self.name.clone(),
            visible: self.visible,
            opacity: self.opacity,
            ink: self.ink.resized(cols, rows, InkCell::EMPTY),
            fill: self.fill.resized(cols, rows, Rgba::TRANSPARENT),
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct FrameId(pub u64);

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Frame {
    pub id: FrameId,
    pub background: Rgb,
    layers: Vec<Layer>,
    active: usize,
    next_layer_id: u32,
    revision: u64,
}

impl Frame {
    pub const MAX_LAYERS: usize = 64;

    pub fn blank(id: FrameId, spec: &CanvasSpec, background: Rgb) -> Frame {
        let first_layer = Layer::blank(LayerId(1), "Layer 1".to_string(), spec);
        Frame {
            id,
            background,
            layers: vec![first_layer],
            active: 0,
            next_layer_id: 2,
            revision: 0,
        }
    }

    pub fn duplicate_as(&self, new_id: FrameId) -> Frame {
        Frame {
            id: new_id,
            background: self.background,
            layers: self.layers.clone(),
            active: self.active,
            next_layer_id: self.next_layer_id,
            revision: 0,
        }
    }

    pub fn layers(&self) -> &[Layer] {
        &self.layers
    }

    pub fn layers_mut(&mut self) -> &mut [Layer] {
        &mut self.layers
    }

    pub fn layer(&self, i: usize) -> Option<&Layer> {
        self.layers.get(i)
    }

    pub fn layer_mut(&mut self, i: usize) -> Option<&mut Layer> {
        self.layers.get_mut(i)
    }

    pub fn index_of_layer(&self, id: LayerId) -> Option<usize> {
        self.layers.iter().position(|l| l.id == id)
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    pub fn active_layer(&self) -> &Layer {
        &self.layers[self.active]
    }

    pub fn active_layer_mut(&mut self) -> &mut Layer {
        &mut self.layers[self.active]
    }

    pub fn select_layer(&mut self, i: usize) -> Result<(), DomainError> {
        if i < self.layers.len() {
            self.active = i;
            Ok(())
        } else {
            Err(DomainError::IndexOutOfRange)
        }
    }

    pub fn alloc_layer_id(&mut self) -> LayerId {
        let id = LayerId(self.next_layer_id);
        self.next_layer_id = self.next_layer_id.saturating_add(1);
        id
    }

    pub fn insert_layer(&mut self, i: usize, l: Layer) -> Result<(), DomainError> {
        if self.layers.len() >= Self::MAX_LAYERS {
            return Err(DomainError::TooManyLayers);
        }
        if i > self.layers.len() {
            return Err(DomainError::IndexOutOfRange);
        }
        self.layers.insert(i, l);
        self.touch();
        Ok(())
    }

    pub fn remove_layer(&mut self, i: usize) -> Result<Layer, DomainError> {
        if self.layers.len() <= 1 {
            return Err(DomainError::CannotRemoveLastLayer);
        }
        if i >= self.layers.len() {
            return Err(DomainError::IndexOutOfRange);
        }
        let l = self.layers.remove(i);
        if self.active >= self.layers.len() {
            self.active = self.layers.len() - 1;
        }
        self.touch();
        Ok(l)
    }

    pub fn move_layer(&mut self, from: usize, to: usize) -> Result<(), DomainError> {
        if from >= self.layers.len() || to >= self.layers.len() {
            return Err(DomainError::IndexOutOfRange);
        }
        if from == to {
            return Ok(());
        }
        let l = self.layers.remove(from);
        self.layers.insert(to, l);
        self.touch();
        Ok(())
    }

    pub fn set_ink(&mut self, layer: LayerId, c: CellCoord, v: InkCell) -> Option<InkCell> {
        let idx = self.index_of_layer(layer)?;
        let norm = v.normalized();
        let old = self.layers[idx].ink.set(c, norm)?;
        self.touch();
        Some(old)
    }

    pub fn set_fill(&mut self, layer: LayerId, c: CellCoord, v: Rgba) -> Option<Rgba> {
        let idx = self.index_of_layer(layer)?;
        let norm = v.normalized();
        let old = self.layers[idx].fill.set(c, norm)?;
        self.touch();
        Some(old)
    }

    pub fn set_layer_props(&mut self, layer: LayerId, p: LayerProps) -> Option<LayerProps> {
        let idx = self.index_of_layer(layer)?;
        let l = &mut self.layers[idx];
        let old = l.props();
        l.name = p.name;
        l.visible = p.visible;
        l.opacity = p.opacity;
        self.touch();
        Some(old)
    }

    pub fn set_background(&mut self, bg: Rgb) -> Rgb {
        let old = self.background;
        self.background = bg;
        self.touch();
        old
    }

    pub fn resize_canvas(&mut self, cols: u16, rows: u16) {
        for l in &mut self.layers {
            *l = l.resized(cols, rows);
        }
        self.touch();
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn from_raw_parts(
        id: FrameId,
        background: Rgb,
        layers: Vec<Layer>,
        active: usize,
        next_layer_id: u32,
    ) -> Result<Frame, DomainError> {
        if layers.is_empty() || layers.len() > Self::MAX_LAYERS || active >= layers.len() {
            return Err(DomainError::InvalidCanvas);
        }
        Ok(Frame {
            id,
            background,
            layers,
            active,
            next_layer_id,
            revision: 0,
        })
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

#[derive(Clone, Debug)]
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
        let i = i.min(self.frames.len());
        self.frames.insert(i, f);
    }

    pub fn remove(&mut self, i: usize) -> Result<Frame, DomainError> {
        if self.frames.len() <= 1 {
            return Err(DomainError::CannotRemoveLastFrame);
        }
        if i >= self.frames.len() {
            return Err(DomainError::IndexOutOfRange);
        }
        let f = self.frames.remove(i);
        if self.selected >= self.frames.len() {
            self.selected = self.frames.len() - 1;
        }
        Ok(f)
    }

    pub fn move_frame(&mut self, from: usize, to: usize) -> Result<(), DomainError> {
        if from >= self.frames.len() || to >= self.frames.len() {
            return Err(DomainError::IndexOutOfRange);
        }
        if from == to {
            return Ok(());
        }
        let f = self.frames.remove(from);
        self.frames.insert(to, f);
        Ok(())
    }

    pub fn frames(&self) -> &[Frame] {
        &self.frames
    }

    pub fn frames_mut(&mut self) -> &mut [Frame] {
        &mut self.frames
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectMeta {
    pub name: String,
    pub created_at: i64,
    pub modified_at: i64,
    pub app_version: String,
}

#[derive(Clone, Debug)]
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

        Project {
            meta: ProjectMeta {
                name,
                created_at: now,
                modified_at: now,
                app_version: env!("CARGO_PKG_VERSION").to_string(),
            },
            canvas,
            timeline,
            next_frame_id: 2,
        }
    }

    pub fn alloc_frame_id(&mut self) -> FrameId {
        let id = FrameId(self.next_frame_id);
        self.next_frame_id = self.next_frame_id.saturating_add(1);
        id
    }

    pub fn resize_canvas(&mut self, new: CanvasSpec) -> CanvasSpec {
        let old = self.canvas;
        self.canvas = new;
        for f in self.timeline.frames_mut() {
            f.resize_canvas(new.cols, new.rows);
        }
        old
    }

    pub fn touch_modified(&mut self, now: i64) {
        self.meta.modified_at = now;
    }

    pub fn from_raw_parts(
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
}

use crate::compose::FlatFrame;

#[derive(Clone, Debug)]
pub struct CellBuffer {
    pub origin: CellCoord,
    pub width: u16,
    pub height: u16,
    ink: Vec<InkCell>,
    fill: Vec<Rgba>,
    mask: Vec<bool>,
}

impl CellBuffer {
    pub fn extract(layer: &Layer, sel: &Selection) -> CellBuffer {
        let bounds = sel.bounds().unwrap_or(CellRect {
            min: CellCoord::new(0, 0),
            max: CellCoord::new(0, 0),
        });
        let origin = bounds.min;
        let width = bounds.width();
        let height = bounds.height();
        let total = width as usize * height as usize;

        let mut ink = vec![InkCell::EMPTY; total];
        let mut fill = vec![Rgba::TRANSPARENT; total];
        let mut mask = vec![false; total];

        for r in 0..height {
            for c in 0..width {
                let coord = CellCoord::new(origin.col + c, origin.row + r);
                let idx = r as usize * width as usize + c as usize;
                if sel.contains(coord) {
                    mask[idx] = true;
                    if let Some(val) = layer.ink.get(coord) {
                        ink[idx] = *val;
                    }
                    if let Some(val) = layer.fill.get(coord) {
                        fill[idx] = *val;
                    }
                }
            }
        }

        CellBuffer {
            origin,
            width,
            height,
            ink,
            fill,
            mask,
        }
    }

    pub fn extract_flat(flat: &FlatFrame, sel: &Selection) -> CellBuffer {
        let bounds = sel.bounds().unwrap_or(CellRect {
            min: CellCoord::new(0, 0),
            max: CellCoord::new(0, 0),
        });
        let origin = bounds.min;
        let width = bounds.width();
        let height = bounds.height();
        let total = width as usize * height as usize;

        let mut ink = vec![InkCell::EMPTY; total];
        let mut fill = vec![Rgba::TRANSPARENT; total];
        let mut mask = vec![false; total];

        for r in 0..height {
            for c in 0..width {
                let coord = CellCoord::new(origin.col + c, origin.row + r);
                let idx = r as usize * width as usize + c as usize;
                if sel.contains(coord) {
                    mask[idx] = true;
                    if let Some(cell) = flat.grid.get(coord) {
                        ink[idx] = InkCell {
                            glyph: cell.glyph,
                            color: cell.ink,
                        }
                        .normalized();
                        fill[idx] = cell.fill.normalized();
                    }
                }
            }
        }

        CellBuffer {
            origin,
            width,
            height,
            ink,
            fill,
            mask,
        }
    }

    pub fn from_parts(
        origin: CellCoord,
        width: u16,
        height: u16,
        ink: Vec<InkCell>,
        fill: Vec<Rgba>,
    ) -> CellBuffer {
        let total = width as usize * height as usize;
        let mask = vec![true; total];
        CellBuffer {
            origin,
            width,
            height,
            ink,
            fill,
            mask,
        }
    }

    pub fn cells(&self) -> impl Iterator<Item = (CellCoord, &InkCell, &Rgba)> {
        let width = self.width;
        let origin = self.origin;
        self.mask.iter().enumerate().filter_map(move |(i, &is_set)| {
            if is_set {
                let c = (i % width as usize) as u16;
                let r = (i / width as usize) as u16;
                let coord = CellCoord::new(origin.col + c, origin.row + r);
                Some((coord, &self.ink[i], &self.fill[i]))
            } else {
                None
            }
        })
    }

    pub fn is_blank(&self) -> bool {
        self.cells().all(|(_, ink, fill)| ink.is_empty() && fill.is_transparent())
    }

    pub fn selected_cells(&self, offset: (i32, i32), spec: &CanvasSpec) -> Vec<CellCoord> {
        let mut result = Vec::new();
        for (i, &is_set) in self.mask.iter().enumerate() {
            if is_set {
                let local_c = (i % self.width as usize) as i32;
                let local_r = (i / self.width as usize) as i32;
                let target_c = self.origin.col as i32 + local_c + offset.0;
                let target_r = self.origin.row as i32 + local_r + offset.1;
                if target_c >= 0
                    && target_c < spec.cols as i32
                    && target_r >= 0
                    && target_r < spec.rows as i32
                {
                    result.push(CellCoord::new(target_c as u16, target_r as u16));
                }
            }
        }
        result
    }

    pub fn into_layer(&self, id: LayerId, name: String, spec: &CanvasSpec) -> Layer {
        let mut layer = Layer::blank(id, name, spec);
        for (c, ink, fill) in self.cells() {
            if spec.contains(c) {
                if !ink.is_empty() {
                    layer.ink.set(c, *ink);
                }
                if fill.a > 0 {
                    layer.fill.set(c, *fill);
                }
            }
        }
        layer
    }

    pub fn get_local(&self, col: u16, row: u16) -> Option<(&InkCell, &Rgba, bool)> {
        if col < self.width && row < self.height {
            let idx = row as usize * self.width as usize + col as usize;
            Some((&self.ink[idx], &self.fill[idx], self.mask[idx]))
        } else {
            None
        }
    }
}
