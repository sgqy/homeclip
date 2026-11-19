use std::{
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
    sync::Arc,
};

use tokio::sync::{Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard, Semaphore, watch};

use crate::db::Database;

#[derive(Clone, Debug)]
pub(crate) struct AppState {
    pub(crate) db: Database,
    pub(crate) directory: Arc<PathBuf>,
    pub(crate) changes: watch::Sender<()>,
    pub(crate) stopping: watch::Sender<bool>,
    pub(crate) transfers: Arc<Semaphore>,
    pub(crate) hashes: Arc<Semaphore>,
    locks: Arc<Vec<RwLock<()>>>,
    chunk_locks: Arc<Vec<Mutex<()>>>,
}

impl AppState {
    pub(crate) fn open(directory: &Path) -> anyhow::Result<Self> {
        std::fs::create_dir_all(directory.join("files"))?;
        let db = Database::open(&directory.join("homeclip.sqlite3"))?;
        Ok(Self {
            db,
            directory: Arc::new(directory.to_path_buf()),
            changes: watch::channel(()).0,
            stopping: watch::channel(false).0,
            transfers: Arc::new(Semaphore::new(8)),
            hashes: Arc::new(Semaphore::new(1)),
            locks: Arc::new((0..64).map(|_| RwLock::new(())).collect()),
            chunk_locks: Arc::new((0..256).map(|_| Mutex::new(())).collect()),
        })
    }

    pub(crate) fn file_path(&self, id: &str) -> PathBuf {
        // Callers obtain IDs from the database or generate a UUID locally.
        self.directory.join("files").join(id)
    }

    fn file_hash(id: &str) -> usize {
        let mut hash = DefaultHasher::new();
        id.hash(&mut hash);
        hash.finish() as usize
    }

    pub(crate) async fn file_lock(&self, id: &str) -> RwLockWriteGuard<'_, ()> {
        self.locks[Self::file_hash(id) % self.locks.len()]
            .write()
            .await
    }

    pub(crate) async fn file_read_lock(&self, id: &str) -> RwLockReadGuard<'_, ()> {
        self.locks[Self::file_hash(id) % self.locks.len()]
            .read()
            .await
    }

    pub(crate) async fn chunk_lock(&self, id: &str, index: u64) -> MutexGuard<'_, ()> {
        let slot = Self::file_hash(id).wrapping_add(index as usize) % self.chunk_locks.len();
        self.chunk_locks[slot].lock().await
    }
}
