use crate::{Error, Result};
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::{
    io::Write,
    path::{Component, Path, PathBuf},
};

#[derive(Clone, Debug)]
pub struct Cache {
    directory: PathBuf,
}
impl Cache {
    /// Resolving a cache does not create files; writes create the directory lazily.
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }
    pub fn resolve() -> Self {
        let dev = std::env::var("FETCH_MIX_DEV")
            .is_ok_and(|s| matches!(s.trim().to_lowercase().as_str(), "1" | "true"));
        Self::new(if dev || Path::new(".tmp").is_dir() {
            PathBuf::from(".tmp")
        } else {
            dirs::cache_dir()
                .unwrap_or_else(|| PathBuf::from(".tmp"))
                .join("fetch-mix")
        })
    }
    pub fn dir(&self) -> &Path {
        &self.directory
    }
    fn path(&self, key: &str) -> Result<PathBuf> {
        let mut parts = Path::new(key).components();
        if !matches!(parts.next(), Some(Component::Normal(_))) || parts.next().is_some() {
            return Err(Error::Invalid("cache key must be one filename".into()));
        }
        Ok(self.directory.join(key))
    }
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        serde_json::from_slice(&std::fs::read(self.path(key).ok()?).ok()?).ok()
    }
    pub fn put<T: Serialize + ?Sized>(&self, key: &str, value: &T) -> Result<()> {
        atomic_write(&self.path(key)?, &serde_json::to_vec_pretty(value)?)
    }
    pub fn delete(&self, key: &str) -> Result<()> {
        match std::fs::remove_file(self.path(key)?) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
    pub fn hash(input: &str) -> String {
        format!("{:x}", Sha256::digest(input.as_bytes()))[..16].to_owned()
    }
}
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|e| e.error)?;
    Ok(())
}
