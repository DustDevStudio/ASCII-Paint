use crate::model::{CanvasSpec, CellCoord, CellRect};

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum SelectionMode {
    Replace,
    Add,
    Subtract,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct EdgeSegment {
    pub from: (u16, u16),
    pub to: (u16, u16),
}

impl EdgeSegment {
    pub const fn new(from: (u16, u16), to: (u16, u16)) -> Self {
        Self { from, to }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Selection {
    cols: u16,
    rows: u16,
    bits: Vec<u64>,
    count: usize,
    revision: u64,
}

impl Selection {
    pub fn new(spec: &CanvasSpec) -> Selection {
        let total = spec.cols as usize * spec.rows as usize;
        let num_words = (total + 63) / 64;
        Selection {
            cols: spec.cols,
            rows: spec.rows,
            bits: vec![0; num_words],
            count: 0,
            revision: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn count(&self) -> usize {
        self.count
    }

    pub fn cols(&self) -> u16 {
        self.cols
    }

    pub fn rows(&self) -> u16 {
        self.rows
    }

    pub fn contains(&self, c: CellCoord) -> bool {
        if c.col >= self.cols || c.row >= self.rows {
            return false;
        }
        let idx = c.row as usize * self.cols as usize + c.col as usize;
        let word = idx / 64;
        let bit = idx % 64;
        (self.bits[word] & (1u64 << bit)) != 0
    }

    pub fn permits(&self, c: CellCoord) -> bool {
        self.is_empty() || self.contains(c)
    }

    pub fn bounds(&self) -> Option<CellRect> {
        if self.is_empty() {
            return None;
        }
        let mut min_col = self.cols;
        let mut max_col = 0;
        let mut min_row = self.rows;
        let mut max_row = 0;

        for r in 0..self.rows {
            for c in 0..self.cols {
                let coord = CellCoord::new(c, r);
                if self.contains(coord) {
                    min_col = min_col.min(c);
                    max_col = max_col.max(c);
                    min_row = min_row.min(r);
                    max_row = max_row.max(r);
                }
            }
        }

        Some(CellRect {
            min: CellCoord::new(min_col, min_row),
            max: CellCoord::new(max_col, max_row),
        })
    }

    pub fn apply(&mut self, cells: &[CellCoord], mode: SelectionMode) {
        match mode {
            SelectionMode::Replace => {
                self.clear_internal();
                for &c in cells {
                    self.set_bit(c, true);
                }
            }
            SelectionMode::Add => {
                for &c in cells {
                    self.set_bit(c, true);
                }
            }
            SelectionMode::Subtract => {
                for &c in cells {
                    self.set_bit(c, false);
                }
            }
        }
        self.touch();
    }

    pub fn select_all(&mut self) {
        let total = self.cols as usize * self.rows as usize;
        for word in self.bits.iter_mut() {
            *word = u64::MAX;
        }
        let remainder = total % 64;
        if remainder != 0 {
            if let Some(last) = self.bits.last_mut() {
                *last &= (1u64 << remainder) - 1;
            }
        }
        self.count = total;
        self.touch();
    }

    pub fn clear(&mut self) {
        self.clear_internal();
        self.touch();
    }

    pub fn invert(&mut self) {
        if self.is_empty() {
            return;
        }
        let total = self.cols as usize * self.rows as usize;
        let mut new_count = 0;
        for (i, word) in self.bits.iter_mut().enumerate() {
            *word = !*word;
            let bits_in_word = if (i + 1) * 64 <= total {
                64
            } else {
                total % 64
            };
            if bits_in_word < 64 {
                *word &= (1u64 << bits_in_word) - 1;
            }
            new_count += word.count_ones() as usize;
        }
        self.count = new_count;
        self.touch();
    }

    pub fn translate(&mut self, offset: (i32, i32)) -> bool {
        if self.is_empty() || (offset.0 == 0 && offset.1 == 0) {
            return true;
        }
        let bounds = match self.bounds() {
            Some(b) => b,
            None => return true,
        };

        let min_c = bounds.min.col as i32 + offset.0;
        let max_c = bounds.max.col as i32 + offset.0;
        let min_r = bounds.min.row as i32 + offset.1;
        let max_r = bounds.max.row as i32 + offset.1;

        if min_c < 0 || max_c >= self.cols as i32 || min_r < 0 || max_r >= self.rows as i32 {
            return false;
        }

        let mut new_sel = Selection::new(&CanvasSpec {
            cols: self.cols,
            rows: self.rows,
            cell_w: 1,
            cell_h: 1,
        });

        for r in 0..self.rows {
            for c in 0..self.cols {
                let coord = CellCoord::new(c, r);
                if self.contains(coord) {
                    let new_c = (c as i32 + offset.0) as u16;
                    let new_r = (r as i32 + offset.1) as u16;
                    new_sel.set_bit(CellCoord::new(new_c, new_r), true);
                }
            }
        }

        self.bits = new_sel.bits;
        self.count = new_sel.count;
        self.touch();
        true
    }

    pub fn restore(&mut self, other: &Selection) {
        self.cols = other.cols;
        self.rows = other.rows;
        self.bits = other.bits.clone();
        self.count = other.count;
        self.touch();
    }

    pub fn resize(&mut self, spec: &CanvasSpec) {
        let mut new_sel = Selection::new(spec);
        for r in 0..self.rows.min(spec.rows) {
            for c in 0..self.cols.min(spec.cols) {
                let coord = CellCoord::new(c, r);
                if self.contains(coord) {
                    new_sel.set_bit(coord, true);
                }
            }
        }
        self.cols = spec.cols;
        self.rows = spec.rows;
        self.bits = new_sel.bits;
        self.count = new_sel.count;
        self.touch();
    }

    fn clear_internal(&mut self) {
        for word in self.bits.iter_mut() {
            *word = 0;
        }
        self.count = 0;
    }

    fn set_bit(&mut self, c: CellCoord, value: bool) {
        if c.col >= self.cols || c.row >= self.rows {
            return;
        }
        let idx = c.row as usize * self.cols as usize + c.col as usize;
        let word = idx / 64;
        let bit = idx % 64;
        let mask = 1u64 << bit;
        let was_set = (self.bits[word] & mask) != 0;

        if value && !was_set {
            self.bits[word] |= mask;
            self.count += 1;
        } else if !value && was_set {
            self.bits[word] &= !mask;
            self.count = self.count.saturating_sub(1);
        }
    }

    pub fn boundary_edges(&self) -> Vec<EdgeSegment> {
        let mut edges = Vec::new();
        if self.is_empty() {
            return edges;
        }

        for r in 0..self.rows {
            for c in 0..self.cols {
                let coord = CellCoord::new(c, r);
                if self.contains(coord) {
                    if r == 0 || !self.contains(CellCoord::new(c, r - 1)) {
                        edges.push(EdgeSegment::new((c, r), (c + 1, r)));
                    }
                    if r + 1 == self.rows || !self.contains(CellCoord::new(c, r + 1)) {
                        edges.push(EdgeSegment::new((c, r + 1), (c + 1, r + 1)));
                    }
                    if c == 0 || !self.contains(CellCoord::new(c - 1, r)) {
                        edges.push(EdgeSegment::new((c, r), (c, r + 1)));
                    }
                    if c + 1 == self.cols || !self.contains(CellCoord::new(c + 1, r)) {
                        edges.push(EdgeSegment::new((c + 1, r), (c + 1, r + 1)));
                    }
                }
            }
        }

        edges
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }
}
