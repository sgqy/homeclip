use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use crate::error::{ApiError, Result};

#[derive(Clone, Debug)]
pub(crate) struct Database(Arc<Mutex<Connection>>);

impl Database {
    pub(crate) fn open(path: &Path) -> anyhow::Result<Self> {
        let db = Connection::open(path)?;
        db.busy_timeout(Duration::from_secs(5))?;
        db.execute_batch(include_str!("schema.sql"))?;
        Ok(Self(Arc::new(Mutex::new(db))))
    }

    // SQLite work stays on the blocking pool; no database lock crosses an await.
    pub(crate) async fn call<T, F>(&self, operation: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T> + Send + 'static,
    {
        let db = self.clone();
        tokio::task::spawn_blocking(move || {
            operation(&mut db.0.lock().expect("SQLite connection lock poisoned"))
        })
        .await?
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Attachment {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) size: u64,
    pub(crate) mime: String,
    pub(crate) sha256: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Message {
    pub(crate) id: i64,
    pub(crate) client_id: String,
    pub(crate) text: String,
    pub(crate) created_at: u64,
    pub(crate) updated_at: u64,
    pub(crate) reply_to: Option<i64>,
    pub(crate) reply_excerpt: Option<String>,
    pub(crate) attachments: Vec<Attachment>,
}

#[derive(Debug, Serialize)]
pub(crate) struct Change {
    pub(crate) id: i64,
    pub(crate) kind: String,
    pub(crate) message_id: i64,
}

pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub(crate) fn message(db: &Connection, id: i64) -> Result<Message> {
    let mut message = db
        .query_row(
            "SELECT m.id, m.client_id, m.text, m.created_at, m.updated_at, m.reply_to,
                substr(p.text, 1, 120) FROM messages m
         LEFT JOIN messages p ON p.id = m.reply_to WHERE m.id = ?1",
            [id],
            |r| {
                Ok(Message {
                    id: r.get(0)?,
                    client_id: r.get(1)?,
                    text: r.get(2)?,
                    created_at: r.get(3)?,
                    updated_at: r.get(4)?,
                    reply_to: r.get(5)?,
                    reply_excerpt: r.get(6)?,
                    attachments: Vec::new(),
                })
            },
        )
        .optional()?
        .ok_or_else(ApiError::missing)?;
    message.attachments = db
        .prepare(
            "SELECT id, name, size, mime, sha256 FROM files WHERE message_id = ?1 ORDER BY rowid",
        )?
        .query_map([id], |r| {
            Ok(Attachment {
                id: r.get(0)?,
                name: r.get(1)?,
                size: r.get(2)?,
                mime: r.get(3)?,
                sha256: r.get(4)?,
            })
        })?
        .collect::<std::result::Result<_, _>>()?;
    Ok(message)
}

pub(crate) fn record_change(db: &Connection, kind: &str, id: i64) -> Result<()> {
    db.execute(
        "INSERT INTO events(kind, message_id) VALUES (?1, ?2)",
        params![kind, id],
    )?;
    Ok(())
}
