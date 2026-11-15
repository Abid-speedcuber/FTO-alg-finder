//! Publish completed cache files atomically; cancellation leaves the previous file intact.
use std::{
    io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT_FILE: AtomicU64 = AtomicU64::new(0);
pub(crate) struct CacheWrite {
    temporary: PathBuf,
    destination: PathBuf,
}
impl CacheWrite {
    pub(crate) fn new(path: &Path) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut name = path.as_os_str().to_os_string();
        name.push(format!(
            ".{}.{}.building",
            std::process::id(),
            NEXT_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        Ok(Self {
            temporary: PathBuf::from(name),
            destination: path.to_owned(),
        })
    }
    pub(crate) fn path(&self) -> &Path {
        &self.temporary
    }
    pub(crate) fn commit(self) -> io::Result<()> {
        std::fs::rename(&self.temporary, &self.destination)
    }
}
impl Drop for CacheWrite {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.temporary);
    }
}
