//! Concurrent writes to disjoint ranges of the original file.

use axum::{
    Json,
    body::Body,
    extract::{Path, State},
};
use futures_util::StreamExt;
use rusqlite::{OptionalExtension, params};
use tokio::io::{AsyncSeekExt, AsyncWriteExt, BufWriter};

use crate::{
    error::{ApiError, Result},
    files::{CHUNK_SIZE, Upload, WRITE_BUFFER_SIZE, upload},
    state::AppState,
};

// Leave HTTP/1.1 connections available for SSE and message requests in the browser.
pub(crate) const UPLOAD_CONCURRENCY: usize = 4;

pub(crate) async fn put(
    State(state): State<AppState>,
    Path((id, offset)): Path<(String, u64)>,
    body: Body,
) -> Result<Json<Upload>> {
    // A dropped HTTP response must not release locks while a blocking disk write is active.
    tokio::spawn(write_part(state, id, offset, body)).await?
}

async fn write_part(state: AppState, id: String, offset: u64, body: Body) -> Result<Json<Upload>> {
    let _slot = state
        .transfers
        .acquire()
        .await
        .map_err(ApiError::internal)?;
    let _file_lock = state.file_read_lock(&id).await;
    let _chunk_lock = state.chunk_lock(&id, offset / CHUNK_SIZE).await;
    let current = upload(&state, id.clone()).await?;
    if offset >= current.size {
        return Err(ApiError::bad("分片起点超出文件末尾"));
    }
    let length = (CHUNK_SIZE - offset % CHUNK_SIZE).min(current.size - offset);
    let key = id.clone();
    let committed = offset + length <= current.received
        || state
            .db
            .call(move |db| {
                Ok(db.query_row(
                    "SELECT EXISTS(SELECT 1 FROM upload_parts WHERE file_id = ?1 AND offset = ?2)",
                    params![key, offset],
                    |r| r.get::<_, bool>(0),
                )?)
            })
            .await?;
    if !committed
        && (current.sha256.is_some()
            || offset < current.received
            || (!offset.is_multiple_of(CHUNK_SIZE) && offset != current.received))
    {
        return Err(ApiError::conflict("分片位置无效，请重新查询进度"));
    }

    let mut stream = body.into_data_stream();
    // Lost acknowledgements may cause retries. Consume them without touching committed bytes.
    if committed {
        let mut received = 0u64;
        while let Some(chunk) = stream.next().await {
            received += chunk.map_err(ApiError::internal)?.len() as u64;
            if received > length {
                return Err(ApiError::bad("分片长度不符"));
            }
        }
        if received != length {
            return Err(ApiError::bad("分片长度不符"));
        }
        return Ok(Json(upload(&state, id.clone()).await?));
    }

    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .open(state.file_path(&id))
        .await?;
    file.seek(std::io::SeekFrom::Start(offset)).await?;
    file.set_max_buf_size(WRITE_BUFFER_SIZE);
    let mut file = BufWriter::with_capacity(WRITE_BUFFER_SIZE, file);
    let result: Result<()> = async {
        let mut written = 0u64;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(ApiError::internal)?;
            written += chunk.len() as u64;
            if written > length {
                return Err(ApiError::bad("分片长度不符"));
            }
            file.write_all(&chunk).await?;
        }
        if written != length {
            return Err(ApiError::bad("分片长度不符"));
        }
        file.flush().await?;
        file.get_ref().sync_data().await?;
        Ok(())
    }
    .await;
    if let Err(error) = result {
        // Wait for pending writes, but never truncate another concurrent part.
        // Uncommitted bytes are overwritten on retry and cannot pass completion.
        let _ = file.flush().await;
        return Err(error);
    }
    commit_part(&state, id.clone(), offset, length).await?;
    Ok(Json(upload(&state, id.clone()).await?))
}

async fn commit_part(state: &AppState, id: String, offset: u64, length: u64) -> Result<()> {
    state
        .db
        .call(move |db| {
            let tx = db.transaction()?;
            tx.execute(
                "INSERT INTO upload_parts(file_id, offset, length) VALUES (?1, ?2, ?3)",
                params![id, offset, length],
            )?;
            let mut received: u64 =
                tx.query_row("SELECT received FROM files WHERE id = ?1", [&id], |r| {
                    r.get(0)
                })?;
            while let Some(length) = tx
                .query_row(
                    "SELECT length FROM upload_parts WHERE file_id = ?1 AND offset = ?2",
                    params![id, received],
                    |r| r.get::<_, u64>(0),
                )
                .optional()?
            {
                tx.execute(
                    "DELETE FROM upload_parts WHERE file_id = ?1 AND offset = ?2",
                    params![id, received],
                )?;
                received += length;
            }
            tx.execute(
                "UPDATE files SET received = ?1 WHERE id = ?2",
                params![received, id],
            )?;
            tx.commit()?;
            Ok(())
        })
        .await
}
