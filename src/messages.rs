use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::{
    db::{self, Message},
    error::{ApiError, Result},
    state::AppState,
};

#[derive(Debug, Deserialize, Default)]
pub(crate) struct PageQuery {
    before: Option<i64>,
    limit: Option<u32>,
    #[serde(default)]
    q: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct Page {
    messages: Vec<Message>,
    next_before: Option<i64>,
    cursor: i64,
}

pub(crate) async fn list(
    State(state): State<AppState>,
    Query(query): Query<PageQuery>,
) -> Result<Json<Page>> {
    let page = state
        .db
        .call(move |db| {
            let limit = query.limit.unwrap_or(40).clamp(1, 100) as usize;
            let mut ids = db
                .prepare(
                    "SELECT m.id FROM messages m WHERE (?1 IS NULL OR m.id < ?1)
             AND (?2 = '' OR instr(lower(m.text), lower(?2)) > 0 OR EXISTS
               (SELECT 1 FROM message_files mf JOIN files f ON f.id = mf.file_id
                WHERE mf.message_id = m.id AND instr(lower(f.name), lower(?2)) > 0))
             ORDER BY m.id DESC LIMIT ?3",
                )?
                .query_map(params![query.before, query.q, limit + 1], |r| {
                    r.get::<_, i64>(0)
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            let has_more = ids.len() > limit;
            ids.truncate(limit);
            let next_before = if has_more { ids.last().copied() } else { None };
            let messages = ids
                .into_iter()
                .map(|id| db::message(db, id))
                .collect::<Result<Vec<_>>>()?;
            let cursor =
                db.query_row("SELECT COALESCE(MAX(id), 0) FROM events", [], |r| r.get(0))?;
            Ok(Page {
                messages,
                next_before,
                cursor,
            })
        })
        .await?;
    Ok(Json(page))
}

pub(crate) async fn get(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Message>> {
    Ok(Json(state.db.call(move |db| db::message(db, id)).await?))
}

#[derive(Debug, Deserialize)]
pub(crate) struct NewMessage {
    client_id: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    attachments: Vec<String>,
    reply_to: Option<i64>,
}

pub(crate) async fn create(
    State(state): State<AppState>,
    Json(input): Json<NewMessage>,
) -> Result<Json<Message>> {
    if input.client_id.is_empty() || input.client_id.len() > 128 {
        return Err(ApiError::bad("消息标识无效"));
    }
    if input.text.trim().is_empty() && input.attachments.is_empty() {
        return Err(ApiError::bad("写点文字或添加文件后再发送"));
    }
    let message = state.db.call(move |db| {
        let tx = db.transaction()?;
        if let Some(id) = tx.query_row("SELECT id FROM messages WHERE client_id = ?1", [&input.client_id], |r| r.get(0)).optional()? {
            return db::message(&tx, id);
        }
        if let Some(reply) = input.reply_to {
            db::message(&tx, reply)?;
        }
        let now = db::now();
        tx.execute("INSERT INTO messages(client_id, text, created_at, updated_at, reply_to) VALUES (?1, ?2, ?3, ?3, ?4)",
            params![input.client_id, input.text, now, input.reply_to])?;
        let id = tx.last_insert_rowid();
        for (position, attachment) in input.attachments.into_iter().enumerate() {
            let changed = tx.execute("INSERT OR IGNORE INTO message_files(message_id, file_id, position)
                SELECT ?1, id, ?3 FROM files WHERE id = ?2 AND sha256 IS NOT NULL", params![id, attachment, position as i64])?;
            if changed != 1 { return Err(ApiError::conflict("附件尚未上传完成、已删除或在本消息中重复")); }
        }
        db::record_change(&tx, "upsert", id)?;
        let message = db::message(&tx, id)?;
        tx.commit()?;
        Ok(message)
    }).await?;
    state.changes.send_replace(());
    Ok(Json(message))
}

#[derive(Debug, Deserialize)]
pub(crate) struct EditMessage {
    text: String,
}

pub(crate) async fn edit(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(input): Json<EditMessage>,
) -> Result<Json<Message>> {
    let message = state
        .db
        .call(move |db| {
            let tx = db.transaction()?;
            let old = db::message(&tx, id)?;
            if input.text.trim().is_empty() && old.attachments.is_empty() {
                return Err(ApiError::bad("消息不能为空"));
            }
            tx.execute(
                "UPDATE messages SET text = ?1, updated_at = ?2 WHERE id = ?3",
                params![input.text, db::now(), id],
            )?;
            db::record_change(&tx, "upsert", id)?;
            let message = db::message(&tx, id)?;
            tx.commit()?;
            Ok(message)
        })
        .await?;
    state.changes.send_replace(());
    Ok(Json(message))
}

pub(crate) async fn delete(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode> {
    let files = state
        .db
        .call(move |db| {
            let tx = db.transaction()?;
            let message = db::message(&tx, id)?;
            tx.execute("DELETE FROM messages WHERE id = ?1", [id])?;
            for file in &message.attachments {
                db::remove_unused_file(&tx, &file.id)?;
            }
            db::record_change(&tx, "delete", id)?;
            tx.commit()?;
            Ok(message.attachments)
        })
        .await?;
    state.changes.send_replace(());
    if !files.is_empty() {
        crate::files::collect_garbage(&state).await?;
    }
    Ok(StatusCode::NO_CONTENT)
}
