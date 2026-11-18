use std::{
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
    sync::Arc,
};

use tokio::sync::{Mutex, MutexGuard, Semaphore, watch};

use crate::db::Database;

#[derive(Clone, Debug)]
pub(crate) struct AppState {
    pub(crate) db: Database,
    pub(crate) directory: Arc<PathBuf>,
    pub(crate) changes: watch::Sender<()>,
    pub(crate) transfers: Arc<Semaphore>,
    pub(crate) hashes: Arc<Semaphore>,
    locks: Arc<Vec<Mutex<()>>>,
}

impl AppState {
    pub(crate) fn open(directory: &Path) -> anyhow::Result<Self> {
        std::fs::create_dir_all(directory.join("files"))?;
        let db = Database::open(&directory.join("homeclip.sqlite3"))?;
        Ok(Self {
            db,
            directory: Arc::new(directory.to_path_buf()),
            changes: watch::channel(()).0,
            transfers: Arc::new(Semaphore::new(4)),
            hashes: Arc::new(Semaphore::new(1)),
            locks: Arc::new((0..64).map(|_| Mutex::new(())).collect()),
        })
    }

    pub(crate) fn file_path(&self, id: &str) -> PathBuf {
        // Callers obtain IDs from the database or generate a UUID locally.
        self.directory.join("files").join(id)
    }

    pub(crate) async fn file_lock(&self, id: &str) -> MutexGuard<'_, ()> {
        let mut hash = DefaultHasher::new();
        id.hash(&mut hash);
        self.locks[hash.finish() as usize % self.locks.len()]
            .lock()
            .await
    }
}
