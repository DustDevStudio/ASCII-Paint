use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use thiserror::Error;

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
                    // Rename corrupt file to recent.json.bad
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
