use std::fs;
use std::io;
use std::path::Path;
use thiserror::Error;

use ascii_core::model::{CellBuffer, CellCoord, InkCell, Rgba};
use crate::format::{AsciiCodec, AsciiImage, FormatCodec, FormatError};

#[derive(Error, Debug)]
pub enum ImportError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    #[error("File not found")]
    NotFound,
    #[error("Unsupported file type: {0}")]
    UnsupportedType(String),
    #[error("File is empty")]
    Empty,
    #[error("File exceeds maximum size ({0})")]
    TooLarge(String),
    #[error("Format error: {0}")]
    Format(#[from] FormatError),
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ImportKind {
    PlainText,
    AsciiImage,
}

#[derive(Clone, Debug)]
pub enum ImportSource {
    Text(ParsedText),
    Image(AsciiImage),
}

#[derive(Clone, Debug)]
pub struct ParsedText {
    pub lines: Vec<Vec<char>>,
    pub width: u32,
    pub height: u32,
    pub replaced_invalid: usize,
    pub expanded_tabs: usize,
}

pub struct TextImporter;

impl TextImporter {
    pub const MAX_DIM: u32 = 4096;
    pub const TAB_WIDTH: u32 = 4;

    pub fn parse(bytes: &[u8]) -> Result<ParsedText, ImportError> {
        let bytes = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
            &bytes[3..]
        } else {
            bytes
        };

        let mut replaced_invalid = 0;
        let mut expanded_tabs = 0;
        let decoded = match std::str::from_utf8(bytes) {
            Ok(s) => s.to_string(),
            Err(_) => {
                let mut res = String::new();
                let mut rest = bytes;
                while !rest.is_empty() {
                    match std::str::from_utf8(rest) {
                        Ok(valid) => {
                            res.push_str(valid);
                            break;
                        }
                        Err(err) => {
                            let (valid, after) = rest.split_at(err.valid_up_to());
                            res.push_str(std::str::from_utf8(valid).unwrap_or(""));
                            res.push('\u{FFFD}');
                            replaced_invalid += 1;
                            let error_len = err.error_len().unwrap_or(1);
                            rest = &after[error_len..];
                        }
                    }
                }
                res
            }
        };

        let raw_lines: Vec<&str> = decoded
            .split("\r\n")
            .flat_map(|line| line.split('\n'))
            .flat_map(|line| line.split('\r'))
            .collect();

        // Drop trailing empty lines
        let mut last_non_empty = raw_lines.len();
        while last_non_empty > 0 && raw_lines[last_non_empty - 1].is_empty() {
            last_non_empty -= 1;
        }

        if last_non_empty == 0 {
            return Err(ImportError::Empty);
        }

        let mut rows = Vec::with_capacity(last_non_empty);
        let mut max_width = 0;

        for &line in &raw_lines[..last_non_empty] {
            let mut row = Vec::new();
            let mut col = 0;

            for ch in line.chars() {
                if ch == '\t' {
                    let n = Self::TAB_WIDTH - (col % Self::TAB_WIDTH);
                    for _ in 0..n {
                        row.push(' ');
                    }
                    col += n;
                    expanded_tabs += 1;
                } else if ch <= '\u{001F}' || ch == '\u{007F}' {
                    row.push(' ');
                    col += 1;
                } else {
                    row.push(ch);
                    col += 1;
                }
            }

            max_width = max_width.max(row.len());
            rows.push(row);
        }

        if max_width == 0 {
            return Err(ImportError::Empty);
        }

        let width = max_width as u32;
        let height = rows.len() as u32;

        if width > Self::MAX_DIM || height > Self::MAX_DIM {
            return Err(ImportError::TooLarge("4096 x 4096 cells".to_string()));
        }

        Ok(ParsedText {
            lines: rows,
            width,
            height,
            replaced_invalid,
            expanded_tabs,
        })
    }
}

#[derive(Clone, Debug)]
pub struct ImportedFile {
    pub kind: ImportKind,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub warnings: Vec<String>,
    source: ImportSource,
}

impl ImportedFile {
    pub fn to_buffer(&self, ink: Rgba) -> CellBuffer {
        let width = self.width.min(u16::MAX as u32) as u16;
        let height = self.height.min(u16::MAX as u32) as u16;
        let total = width as usize * height as usize;

        let mut ink_cells = vec![InkCell::EMPTY; total];
        let mut fill_cells = vec![Rgba::TRANSPARENT; total];

        match &self.source {
            ImportSource::Text(text) => {
                for r in 0..height {
                    if let Some(line) = text.lines.get(r as usize) {
                        for c in 0..width {
                            if let Some(&ch) = line.get(c as usize) {
                                let idx = r as usize * width as usize + c as usize;
                                if ch != ' ' {
                                    ink_cells[idx] = InkCell {
                                        glyph: ch,
                                        color: ink,
                                    }
                                    .normalized();
                                }
                            }
                        }
                    }
                }
            }
            ImportSource::Image(image) => {
                for (coord, flat_cell) in image.frame.grid.iter() {
                    if coord.col < width && coord.row < height {
                        let idx = coord.row as usize * width as usize + coord.col as usize;
                        if flat_cell.glyph != ' ' {
                            ink_cells[idx] = InkCell {
                                glyph: flat_cell.glyph,
                                color: flat_cell.ink,
                            }
                            .normalized();
                        }
                        if flat_cell.fill.a > 0 {
                            fill_cells[idx] = flat_cell.fill.normalized();
                        }
                    }
                }
            }
        }

        CellBuffer::from_parts(
            CellCoord::new(0, 0),
            width,
            height,
            ink_cells,
            fill_cells,
        )
    }
}

pub struct ImportService;

impl ImportService {
    pub const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024; // 16 MiB

    pub fn new() -> Self {
        Self
    }

    pub fn load(&self, path: &Path) -> Result<ImportedFile, ImportError> {
        if !path.exists() {
            return Err(ImportError::NotFound);
        }

        let meta = fs::metadata(path)?;
        if meta.len() > Self::MAX_FILE_BYTES {
            return Err(ImportError::TooLarge("16 MiB".to_string()));
        }

        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Imported")
            .chars()
            .take(64)
            .collect::<String>();

        let bytes = fs::read(path)?;

        match ext.as_str() {
            "txt" => {
                let parsed = TextImporter::parse(&bytes)?;
                let mut warnings = Vec::new();
                if parsed.replaced_invalid > 0 {
                    warnings.push(format!(
                        "{} invalid UTF-8 sequences were replaced",
                        parsed.replaced_invalid
                    ));
                }
                if parsed.expanded_tabs > 0 {
                    warnings.push(format!(
                        "{} tabs were expanded to spaces",
                        parsed.expanded_tabs
                    ));
                }

                Ok(ImportedFile {
                    kind: ImportKind::PlainText,
                    name,
                    width: parsed.width,
                    height: parsed.height,
                    warnings,
                    source: ImportSource::Text(parsed),
                })
            }
            "ascii" => {
                let codec = AsciiCodec;
                let image = codec.decode(&bytes)?;
                let width = image.canvas.cols as u32;
                let height = image.canvas.rows as u32;

                Ok(ImportedFile {
                    kind: ImportKind::AsciiImage,
                    name,
                    width,
                    height,
                    warnings: Vec::new(),
                    source: ImportSource::Image(image),
                })
            }
            _ => Err(ImportError::UnsupportedType(ext)),
        }
    }
}

impl Default for ImportService {
    fn default() -> Self {
        Self::new()
    }
}
