use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use ascii_core::brush::{Brush, BrushError, BrushId, BrushLibrary, Stamp};
use ascii_core::model::Project;
use crate::format::{AsprCodec, FormatCodec, FormatError};

#[derive(Error, Debug)]
pub enum RepoError {
    #[error("File not found")]
    NotFound,
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    #[error("Format error: {0}")]
    Format(#[from] FormatError),
}

pub struct AtomicWriter;

impl AtomicWriter {
    pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let tmp_path = path.with_extension(format!(
            "{}.tmp",
            path.extension().and_then(|e| e.to_str()).unwrap_or("tmp")
        ));

        let write_res = (|| -> io::Result<()> {
            let mut file = File::create(&tmp_path)?;
            file.write_all(bytes)?;
            file.flush()?;
            file.sync_all()?;
            fs::rename(&tmp_path, path)?;
            Ok(())
        })();

        if write_res.is_err() {
            let _ = fs::remove_file(&tmp_path);
        }

        write_res
    }
}

pub struct ProjectRepository;

impl ProjectRepository {
    pub fn new() -> Self {
        Self
    }

    pub fn save(&self, path: &Path, project: &Project) -> Result<(), RepoError> {
        let codec = AsprCodec;
        let bytes = codec.encode(project)?;
        AtomicWriter::write(path, &bytes)?;
        Ok(())
    }

    pub fn load(&self, path: &Path) -> Result<Project, RepoError> {
        if !path.exists() {
            return Err(RepoError::NotFound);
        }
        let bytes = fs::read(path).map_err(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                RepoError::NotFound
            } else {
                RepoError::Io(e)
            }
        })?;
        let codec = AsprCodec;
        let project = codec.decode(&bytes)?;
        Ok(project)
    }

    pub fn exists(&self, path: &Path) -> bool {
        path.exists()
    }
}

impl Default for ProjectRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
pub struct RecentEntry {
    pub name: String,
    pub path: PathBuf,
    pub last_opened_unix: i64,
}

#[derive(Serialize, Deserialize, Debug)]
struct RecentData {
    version: u32,
    entries: Vec<RecentEntry>,
}

pub struct RecentStore {
    config_file: Option<PathBuf>,
    entries: Vec<RecentEntry>,
}

impl RecentStore {
    pub const MAX_ENTRIES: usize = 10;

    pub fn open_default() -> Self {
        Self::new()
    }

    pub fn new() -> Self {
        let config_file = dirs::config_dir().map(|d| d.join("AsciiPaint").join("recent.json"));
        let mut store = Self {
            config_file,
            entries: Vec::new(),
        };
        store.load();
        store
    }

    pub fn with_path(config_file: PathBuf) -> Self {
        let mut store = Self {
            config_file: Some(config_file),
            entries: Vec::new(),
        };
        store.load();
        store
    }

    fn load(&mut self) {
        let path = match self.config_file.as_ref() {
            Some(p) => p,
            None => return,
        };

        if !path.exists() {
            return;
        }

        match fs::read_to_string(path) {
            Ok(content) => match serde_json::from_str::<RecentData>(&content) {
                Ok(data) => {
                    self.entries = data.entries;
                    self.entries.truncate(Self::MAX_ENTRIES);
                }
                Err(_) => {
                    let bad_path = path.with_extension("json.bad");
                    let _ = fs::rename(path, bad_path);
                    self.entries.clear();
                }
            },
            Err(_) => {
                self.entries.clear();
            }
        }
    }

    pub fn save(&self) {
        let path = match self.config_file.as_ref() {
            Some(p) => p,
            None => return,
        };

        let data = RecentData {
            version: 1,
            entries: self.entries.clone(),
        };

        if let Ok(json) = serde_json::to_string_pretty(&data) {
            let _ = AtomicWriter::write(path, json.as_bytes());
        }
    }

    pub fn touch(&mut self, path: PathBuf, name: String, now: i64) {
        self.entries.retain(|e| e.path != path);
        self.entries.insert(
            0,
            RecentEntry {
                name,
                path,
                last_opened_unix: now,
            },
        );
        self.entries.truncate(Self::MAX_ENTRIES);
        self.save();
    }

    pub fn remove(&mut self, path: &Path) {
        self.entries.retain(|e| e.path != path);
        self.save();
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.save();
    }

    pub fn entries(&self) -> &[RecentEntry] {
        &self.entries
    }
}

impl Default for RecentStore {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct BrushRecord {
    pub id: u32,
    pub name: String,
    pub width: u8,
    pub height: u8,
    pub anchor: (u8, u8),
    pub cells: Vec<Option<String>>,
}

#[derive(Serialize, Deserialize, Debug)]
struct BrushData {
    version: u32,
    brushes: Vec<BrushRecord>,
}

#[derive(Error, Debug)]
pub enum BrushStoreError {
    #[error("Brush error: {0}")]
    Brush(#[from] BrushError),
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
}

pub struct BrushStore {
    file: Option<PathBuf>,
    library: BrushLibrary,
}

impl BrushStore {
    pub fn open_default() -> Self {
        let file = dirs::config_dir().map(|d| d.join("AsciiPaint").join("brushes.json"));
        let mut store = BrushStore {
            file,
            library: BrushLibrary::with_builtins(),
        };
        store.load();
        store
    }

    pub fn with_path(file: PathBuf) -> Self {
        let mut store = BrushStore {
            file: Some(file),
            library: BrushLibrary::with_builtins(),
        };
        store.load();
        store
    }

    fn load(&mut self) {
        let path = match self.file.as_ref() {
            Some(p) => p,
            None => return,
        };

        if !path.exists() {
            return;
        }

        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return,
        };

        let data: BrushData = match serde_json::from_str(&content) {
            Ok(d) => d,
            Err(_) => {
                let bad = path.with_extension("json.bad");
                let _ = fs::rename(path, bad);
                return;
            }
        };

        for rec in data.brushes {
            let cells = rec
                .cells
                .into_iter()
                .map(|s| s.and_then(|str_val| str_val.chars().next()))
                .collect();
            let brush = Brush {
                id: BrushId(rec.id),
                name: rec.name,
                stamp: Stamp::new(rec.width, rec.height, cells, rec.anchor),
            };
            let _ = self.library.upsert(brush);
        }
    }

    pub fn library(&self) -> &BrushLibrary {
        &self.library
    }

    pub fn upsert(&mut self, b: Brush) -> Result<BrushId, BrushStoreError> {
        let id = self.library.upsert(b)?;
        let _ = self.persist();
        Ok(id)
    }

    pub fn remove(&mut self, id: BrushId) -> Result<(), BrushStoreError> {
        self.library.remove(id)?;
        let _ = self.persist();
        Ok(())
    }

    fn persist(&self) -> io::Result<()> {
        let path = match self.file.as_ref() {
            Some(p) => p,
            None => return Ok(()),
        };

        let mut records = Vec::new();
        for b in self.library.iter() {
            if b.id.0 >= 1000 {
                let cells = b
                    .stamp
                    .cells
                    .iter()
                    .map(|c| c.map(|ch| ch.to_string()))
                    .collect();
                records.push(BrushRecord {
                    id: b.id.0,
                    name: b.name.clone(),
                    width: b.stamp.width,
                    height: b.stamp.height,
                    anchor: b.stamp.anchor,
                    cells,
                });
            }
        }

        let data = BrushData {
            version: 1,
            brushes: records,
        };

        if let Ok(json) = serde_json::to_string_pretty(&data) {
            AtomicWriter::write(path, json.as_bytes())?;
        }
        Ok(())
    }
}
