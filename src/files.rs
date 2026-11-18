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
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tower_http::services::ServeFile;
use uuid::Uuid;

use crate::{
    db,
    error::{ApiError, Result},
    state::AppState,
};

/// Bounds each retry and request, without limiting total file size.
pub(crate) const CHUNK_SIZE: u64 = 8 * 1024 * 1024;

#[derive(Debug, Deserialize)]
pub(crate) struct NewUpload {
    name: String,
    size: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct Upload {
    id: String,
    name: String,
    size: u64,
    mime: String,
    received: u64,
    sha256: Option<String>,
}

async fn upload(state: &AppState, id: String) -> Result<Upload> {
    state
        .db
        .call(move |db| {
            db.query_row(
                "SELECT id, name, size, mime, received, sha256 FROM files WHERE id = ?1",
                [id],
                |r| {
                    Ok(Upload {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        size: r.get(2)?,
                        mime: r.get(3)?,
                        received: r.get(4)?,
                        sha256: r.get(5)?,
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
    let id = Uuid::new_v4().to_string();
    let path = state.file_path(&id);
    tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .await?;
    let key = id.clone();
    let mime = mime_guess::from_path(&input.name)
        .first_or_octet_stream()
        .to_string();
    if let Err(error) = state
        .db
        .call(move |db| {
            db.execute(
                "INSERT INTO files(id, name, size, mime, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![key, input.name, input.size, mime, db::now()],
            )?;
            Ok(())
        })
        .await
    {
        let _ = tokio::fs::remove_file(path).await;
        return Err(error);
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
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .open(state.file_path(&current.id))
        .await?;
    // Remove bytes from a request interrupted before its SQLite offset was committed.
    file.set_len(offset).await?;
    file.seek(std::io::SeekFrom::Start(offset)).await?;
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
        file.sync_data().await?;
        Ok(())
    }
    .await;
    if let Err(error) = write_result {
        file.set_len(offset).await?;
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
    let key = id.clone();
    state
        .db
        .call(move |db| {
            db.execute(
                "UPDATE files SET sha256 = ?1 WHERE id = ?2",
                params![hash, key],
            )?;
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
    let current = upload(&state, id.clone()).await?;
    let key = id.clone();
    state
        .db
        .call(move |db| {
            let tx = db.transaction()?;
            let changed = tx.execute(
                "DELETE FROM files WHERE id = ?1 AND message_id IS NULL",
                [&key],
            )?;
            if changed != 1 {
                return Err(ApiError::conflict("文件已属于一条消息"));
            }
            tx.execute("INSERT OR IGNORE INTO garbage(id) VALUES (?1)", [key])?;
            tx.commit()?;
            Ok(())
        })
        .await?;
    remove_garbage_file(&state, &current.id).await?;
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
    let mut response = ServeFile::new_with_mime(state.file_path(&file.id), &mime)
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
