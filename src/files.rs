use std::io::{BufReader, Read};

use axum::{
    Json,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, Request, StatusCode, header},
    response::Response,
};
use futures_util::StreamExt;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncSeekExt, AsyncWriteExt, BufWriter};
use tower_http::services::ServeFile;
use uuid::Uuid;

use crate::{
    db,
    error::{ApiError, Result},
    state::AppState,
};

/// Bounds each retry and request, without limiting total file size.
pub(crate) const CHUNK_SIZE: u64 = 8 * 1024 * 1024;
pub(crate) const WRITE_BUFFER_SIZE: usize = 1024 * 1024;

#[derive(Debug, Deserialize)]
pub(crate) struct NewUpload {
    name: String,
    size: u64,
    sha256: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct Upload {
    id: String,
    name: String,
    pub(crate) size: u64,
    mime: String,
    pub(crate) received: u64,
    uploaded: u64,
    pub(crate) sha256: Option<String>,
    #[serde(skip)]
    expected_sha256: Option<String>,
    #[serde(skip)]
    blob_id: Option<String>,
}

pub(crate) async fn upload(state: &AppState, id: String) -> Result<Upload> {
    state
        .db
        .call(move |db| {
            db.query_row(
                "SELECT id, name, size, mime, received, sha256,
                    received + COALESCE((SELECT SUM(length) FROM upload_parts WHERE file_id = files.id), 0),
                    expected_sha256, blob_id
                 FROM files WHERE id = ?1",
                [id],
                |r| {
                    Ok(Upload {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        size: r.get(2)?,
                        mime: r.get(3)?,
                        received: r.get(4)?,
                        sha256: r.get(5)?,
                        uploaded: r.get(6)?,
                        expected_sha256: r.get(7)?,
                        blob_id: r.get(8)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(ApiError::missing)
        })
        .await
}

pub(crate) async fn create(
    State(state): State<AppState>,
    Json(input): Json<NewUpload>,
) -> Result<Json<Upload>> {
    if input.name.is_empty() || input.name.len() > 4096 || input.size > i64::MAX as u64 {
        return Err(ApiError::bad("文件名或文件大小无效"));
    }
    if input.sha256.len() != 64 || !input.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ApiError::bad("需要完整的 SHA-256 十六进制摘要"));
    }
    let id = Uuid::new_v4().to_string();
    let key = id.clone();
    let mime = mime_guess::from_path(&input.name)
        .first_or_octet_stream()
        .to_string();
    let reused = state
        .db
        .call(move |db| {
            let tx = db.transaction()?;
            let hash = input.sha256.to_ascii_lowercase();
            let blob_id: Option<String> = tx.query_row(
                "SELECT id FROM blobs WHERE sha256 = ?1 AND size = ?2", params![hash, input.size], |r| r.get(0),
            ).optional()?;
            tx.execute(
                "INSERT INTO files(id, name, size, mime, created_at, expected_sha256, blob_id, sha256, received)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![key, input.name, input.size, mime, db::now(), hash, blob_id,
                    blob_id.as_ref().map(|_| &hash), if blob_id.is_some() { input.size } else { 0 }],
            )?;
            tx.commit()?;
            Ok(blob_id.is_some())
        })
        .await?;
    if !reused
        && let Err(error) = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(state.file_path(&id))
            .await
    {
        let key = id.clone();
        state
            .db
            .call(move |db| {
                db.execute("DELETE FROM files WHERE id = ?1", [key])?;
                Ok(())
            })
            .await?;
        return Err(error.into());
    }
    Ok(Json(upload(&state, id).await?))
}

pub(crate) async fn status(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Upload>> {
    Ok(Json(upload(&state, id).await?))
}

pub(crate) async fn append(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Body,
) -> Result<Json<Upload>> {
    tokio::spawn(append_inner(state, id, headers, body)).await?
}

async fn append_inner(
    state: AppState,
    id: String,
    headers: HeaderMap,
    body: Body,
) -> Result<Json<Upload>> {
    let offset = headers
        .get("upload-offset")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.parse::<u64>().ok())
        .ok_or_else(|| ApiError::bad("缺少有效的 Upload-Offset"))?;
    let _slot = state
        .transfers
        .acquire()
        .await
        .map_err(ApiError::internal)?;
    let _lock = state.file_lock(&id).await;
    let current = upload(&state, id.clone()).await?;
    if current.sha256.is_some() {
        return Err(ApiError::conflict("文件已完成上传"));
    }
    if offset != current.received {
        return Err(ApiError::conflict("上传位置已变化，请重新查询进度"));
    }
    let key = id.clone();
    let has_parts = state
        .db
        .call(move |db| {
            Ok(db.query_row(
                "SELECT EXISTS(SELECT 1 FROM upload_parts WHERE file_id = ?1)",
                [key],
                |r| r.get::<_, bool>(0),
            )?)
        })
        .await?;
    if has_parts {
        return Err(ApiError::conflict("存在并发分片，请使用分片接口继续上传"));
    }
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .open(state.file_path(&current.id))
        .await?;
    // Remove bytes from a request interrupted before its SQLite offset was committed.
    file.set_len(offset).await?;
    file.seek(std::io::SeekFrom::Start(offset)).await?;
    file.set_max_buf_size(WRITE_BUFFER_SIZE);
    let mut file = BufWriter::with_capacity(WRITE_BUFFER_SIZE, file);
    let mut stream = body.into_data_stream();
    let mut written = 0u64;
    let write_result: Result<()> = async {
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(ApiError::internal)?;
            written += chunk.len() as u64;
            if written > CHUNK_SIZE || written > current.size - offset {
                return Err(ApiError::bad("分片超过 8 MiB 或超出文件末尾"));
            }
            file.write_all(&chunk).await?;
        }
        if written == 0 {
            return Err(ApiError::bad("分片为空"));
        }
        file.flush().await?;
        file.get_ref().sync_data().await?;
        Ok(())
    }
    .await;
    if let Err(error) = write_result {
        // Finish any pending blocking write before truncating or releasing the lock.
        let _ = file.flush().await;
        file.get_ref().set_len(offset).await?;
        return Err(error);
    }
    let key = id.clone();
    state
        .db
        .call(move |db| {
            db.execute(
                "UPDATE files SET received = ?1 WHERE id = ?2",
                params![offset + written, key],
            )?;
            Ok(())
        })
        .await?;
    Ok(Json(upload(&state, id.clone()).await?))
}

pub(crate) async fn complete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Upload>> {
    tokio::spawn(complete_inner(state, id)).await?
}

async fn complete_inner(state: AppState, id: String) -> Result<Json<Upload>> {
    let _lock = state.file_lock(&id).await;
    let current = upload(&state, id.clone()).await?;
    if current.sha256.is_some() {
        return Ok(Json(current));
    }
    if current.received != current.size {
        return Err(ApiError::conflict("文件还未传完"));
    }
    // One hash worker bounds disk contention and keeps CPU work off the async executor.
    let permit = state
        .hashes
        .clone()
        .acquire_owned()
        .await
        .map_err(ApiError::internal)?;
    let path = state.file_path(&current.id);
    let hash = tokio::task::spawn_blocking(move || -> Result<String> {
        let _permit = permit;
        let mut input = BufReader::with_capacity(256 * 1024, std::fs::File::open(path)?);
        if input.get_ref().metadata()?.len() != current.size {
            return Err(ApiError::conflict("磁盘文件长度不一致，请重试上传"));
        }
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let size = input.read(&mut buffer)?;
            if size == 0 {
                break;
            }
            hasher.update(&buffer[..size]);
        }
        Ok(format!("{:x}", hasher.finalize()))
    })
    .await??;
    if current
        .expected_sha256
        .as_ref()
        .is_some_and(|expected| expected != &hash)
    {
        let key = id.clone();
        state
            .db
            .call(move |db| {
                let tx = db.transaction()?;
                tx.execute("UPDATE files SET received = 0 WHERE id = ?1", [&key])?;
                tx.execute("DELETE FROM upload_parts WHERE file_id = ?1", [key])?;
                tx.commit()?;
                Ok(())
            })
            .await?;
        let file = tokio::fs::OpenOptions::new()
            .write(true)
            .open(state.file_path(&id))
            .await?;
        file.set_len(0).await?;
        return Err(ApiError::unprocessable(
            "SHA-256 不一致，文件未通过校验，请重新上传",
        ));
    }
    let key = id.clone();
    state
        .db
        .call(move |db| {
            let tx = db.transaction()?;
            tx.execute(
                "INSERT OR IGNORE INTO blobs(id, sha256, size) VALUES (?1, ?2, ?3)",
                params![key, hash, current.size],
            )?;
            let blob_id: String = tx.query_row(
                "SELECT id FROM blobs WHERE sha256 = ?1 AND size = ?2",
                params![hash, current.size],
                |r| r.get(0),
            )?;
            tx.execute(
                "UPDATE files SET sha256 = ?1, blob_id = ?2 WHERE id = ?3",
                params![hash, blob_id, key],
            )?;
            if blob_id != key {
                tx.execute("INSERT OR IGNORE INTO garbage(id) VALUES (?1)", [key])?;
            }
            tx.commit()?;
            Ok(())
        })
        .await?;
    Ok(Json(upload(&state, id.clone()).await?))
}

pub(crate) async fn cancel(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    let _lock = state.file_lock(&id).await;
    upload(&state, id.clone()).await?;
    let key = id.clone();
    state
        .db
        .call(move |db| {
            let tx = db.transaction()?;
            if !db::remove_unused_file(&tx, &key)? {
                return Err(ApiError::conflict("文件仍被消息引用"));
            }
            tx.commit()?;
            Ok(())
        })
        .await?;
    collect_garbage(&state).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
pub(crate) struct FileQuery {
    download: Option<u8>,
}

pub(crate) async fn content(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<FileQuery>,
    request: Request<Body>,
) -> Result<Response> {
    let file = upload(&state, id).await?;
    if file.sha256.is_none() {
        return Err(ApiError::conflict("文件还未完成校验"));
    }
    let mime = file.mime.parse().map_err(ApiError::internal)?;
    let blob_id = file
        .blob_id
        .as_deref()
        .ok_or_else(|| ApiError::conflict("文件还未完成校验"))?;
    let mut response = ServeFile::new_with_mime(state.file_path(blob_id), &mime)
        .try_call(request)
        .await
        .map_err(ApiError::internal)?
        .map(Body::new);
    let disposition = if query.download == Some(1) {
        "attachment"
    } else {
        "inline"
    };
    let filename = utf8_percent_encode(&file.name, NON_ALPHANUMERIC);
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("{disposition}; filename*=UTF-8''{filename}"))
            .map_err(ApiError::internal)?,
    );
    Ok(response)
}

async fn remove_garbage_file(state: &AppState, id: &str) -> Result<()> {
    match tokio::fs::remove_file(state.file_path(id)).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let id = id.to_owned();
    state
        .db
        .call(move |db| {
            db.execute("DELETE FROM garbage WHERE id = ?1", [id])?;
            Ok(())
        })
        .await
}

pub(crate) async fn collect_garbage(state: &AppState) -> Result<()> {
    let ids = state
        .db
        .call(|db| {
            Ok(db
                .prepare("SELECT id FROM garbage LIMIT 100")?
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?)
        })
        .await?;
    for id in ids {
        if let Err(error) = remove_garbage_file(state, &id).await {
            tracing::warn!(?error, %id, "will retry attachment cleanup");
        }
    }
    Ok(())
}
