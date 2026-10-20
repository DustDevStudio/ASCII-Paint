use thiserror::Error;

#[derive(Error, Debug, PartialEq, Eq, Clone)]
pub enum BrushError {
    #[error("Invalid brush dimensions (must be 1..=8)")]
    InvalidDimensions,
    #[error("Invalid brush name (must be 1..=32 characters)")]
    InvalidName,
    #[error("Invalid brush anchor (must be inside stamp on a non-empty cell)")]
    InvalidAnchor,
    #[error("Empty stamp (must contain at least one glyph)")]
    EmptyStamp,
    #[error("Duplicate brush name")]
    DuplicateName,
    #[error("Built-in brushes are read-only")]
    BuiltinReadOnly,
    #[error("Brush not found")]
    NotFound,
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct BrushId(pub u32);

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Stamp {
    pub width: u8,
    pub height: u8,
    pub cells: Vec<Option<char>>,
    pub anchor: (u8, u8),
}

impl Stamp {
    pub fn new_single(ch: char) -> Self {
        Stamp {
            width: 1,
            height: 1,
            cells: vec![Some(ch)],
            anchor: (0, 0),
        }
    }

    pub fn new(width: u8, height: u8, cells: Vec<Option<char>>, anchor: (u8, u8)) -> Self {
        Stamp {
            width,
            height,
            cells,
            anchor,
        }
    }

    pub fn glyph_at(&self, col: u8, row: u8) -> Option<char> {
        if col < self.width && row < self.height {
            let idx = row as usize * self.width as usize + col as usize;
            self.cells.get(idx).copied().flatten()
        } else {
            None
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Brush {
    pub id: BrushId,
    pub name: String,
    pub stamp: Stamp,
}

impl Brush {
    pub fn new_1x1(id: BrushId, name: &str, ch: char) -> Self {
        Brush {
            id,
            name: name.to_string(),
            stamp: Stamp::new_single(ch),
        }
    }

    pub fn validate(&self) -> Result<(), BrushError> {
        let name_len = self.name.trim().chars().count();
        if name_len == 0 || name_len > 32 {
            return Err(BrushError::InvalidName);
        }

        if self.stamp.width == 0
            || self.stamp.width > 8
            || self.stamp.height == 0
            || self.stamp.height > 8
        {
            return Err(BrushError::InvalidDimensions);
        }

        let expected_len = self.stamp.width as usize * self.stamp.height as usize;
        if self.stamp.cells.len() != expected_len {
            return Err(BrushError::InvalidDimensions);
        }

        if !self.stamp.cells.iter().any(|c| c.is_some()) {
            return Err(BrushError::EmptyStamp);
        }

        let (ax, ay) = self.stamp.anchor;
        if ax >= self.stamp.width || ay >= self.stamp.height {
            return Err(BrushError::InvalidAnchor);
        }

        if self.stamp.glyph_at(ax, ay).is_none() {
            return Err(BrushError::InvalidAnchor);
        }

        Ok(())
    }

    pub fn fill_glyph(&self) -> char {
        let (ax, ay) = self.stamp.anchor;
        self.stamp.glyph_at(ax, ay).unwrap_or('#')
    }
}

#[derive(Clone, Debug)]
pub struct BrushLibrary {
    brushes: Vec<Brush>,
    next_custom_id: u32,
}

impl BrushLibrary {
    pub const DEFAULT_ID: BrushId = BrushId(7); // '#'

    pub fn with_builtins() -> BrushLibrary {
        let builtins = vec![
            Brush::new_1x1(BrushId(1), "Dot", '.'),
            Brush::new_1x1(BrushId(2), "Colon", ':'),
            Brush::new_1x1(BrushId(3), "Dash", '-'),
            Brush::new_1x1(BrushId(4), "Equal", '='),
            Brush::new_1x1(BrushId(5), "Plus", '+'),
            Brush::new_1x1(BrushId(6), "Asterisk", '*'),
            Brush::new_1x1(BrushId(7), "Hash", '#'),
            Brush::new_1x1(BrushId(8), "Percent", '%'),
            Brush::new_1x1(BrushId(9), "At", '@'),
            Brush::new_1x1(BrushId(10), "Block full", '\u{2588}'),
            Brush::new_1x1(BrushId(11), "Block dark", '\u{2593}'),
            Brush::new_1x1(BrushId(12), "Block med", '\u{2592}'),
            Brush::new_1x1(BrushId(13), "Block light", '\u{2591}'),
        ];

        BrushLibrary {
            brushes: builtins,
            next_custom_id: 1000,
        }
    }

    pub fn default_id() -> BrushId {
        Self::DEFAULT_ID
    }

    pub fn get(&self, id: BrushId) -> Option<&Brush> {
        self.brushes.iter().find(|b| b.id == id)
    }

    pub fn get_mut(&mut self, id: BrushId) -> Option<&mut Brush> {
        self.brushes.iter_mut().find(|b| b.id == id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Brush> {
        self.brushes.iter()
    }

    pub fn upsert(&mut self, mut b: Brush) -> Result<BrushId, BrushError> {
        b.validate()?;

        for existing in &self.brushes {
            if existing.id != b.id && existing.name.eq_ignore_ascii_case(&b.name) {
                return Err(BrushError::DuplicateName);
            }
        }

        if b.id.0 < 1000 && b.id.0 != 0 {
            if let Some(pos) = self.brushes.iter().position(|x| x.id == b.id) {
                if pos < 13 {
                    return Err(BrushError::BuiltinReadOnly);
                }
            }
        }

        if b.id.0 == 0 || b.id.0 < 1000 {
            b.id = BrushId(self.next_custom_id);
            self.next_custom_id = self.next_custom_id.saturating_add(1);
        } else if b.id.0 >= self.next_custom_id {
            self.next_custom_id = b.id.0 + 1;
        }

        if let Some(pos) = self.brushes.iter().position(|x| x.id == b.id) {
            self.brushes[pos] = b.clone();
        } else {
            self.brushes.push(b.clone());
        }

        Ok(b.id)
    }

    pub fn remove(&mut self, id: BrushId) -> Result<(), BrushError> {
        if id.0 < 1000 {
            return Err(BrushError::BuiltinReadOnly);
        }
        let pos = self
            .brushes
            .iter()
            .position(|x| x.id == id)
            .ok_or(BrushError::NotFound)?;
        self.brushes.remove(pos);
        Ok(())
    }
}

impl Default for BrushLibrary {
    fn default() -> Self {
        Self::with_builtins()
    }
}
