use axum::{
    Router,
    body::{Body, Bytes},
    http::{Request, StatusCode},
    response::Response,
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use tower::ServiceExt;

use crate::{app, state::AppState};

struct Harness {
    router: Router,
    state: AppState,
    directory: TempDir,
}

impl Harness {
    fn new() -> Self {
        let directory = TempDir::new().unwrap();
        let state = AppState::open(directory.path()).unwrap();
        let router = app::router(state.clone(), directory.path());
        Self {
            router,
            state,
            directory,
        }
    }

    async fn request(&self, method: &str, path: &str, input: Value) -> Response {
        self.router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("content-type", "application/json")
                    .body(Body::from(input.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    async fn create_upload(&self, name: &str, data: &[u8]) -> String {
        let response = self
            .request("POST", "/api/uploads", json!({"name": name, "size": data.len(), "sha256": format!("{:x}", Sha256::digest(data))}))
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        json_body(response).await["id"].as_str().unwrap().into()
    }

    async fn append(&self, id: &str, offset: u64, data: impl Into<Body>) -> Response {
        self.router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/api/uploads/{id}"))
                    .header("upload-offset", offset)
                    .body(data.into())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    async fn part(&self, id: &str, offset: u64, data: impl Into<Body>) -> Response {
        self.router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/uploads/{id}/chunks/{offset}"))
                    .body(data.into())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    async fn ready_file(&self, name: &str, data: &'static str) -> String {
        let id = self.create_upload(name, data.as_bytes()).await;
        if !data.is_empty() {
            assert_eq!(self.append(&id, 0, data).await.status(), StatusCode::OK);
        }
        let response = self
            .request("POST", &format!("/api/uploads/{id}/complete"), json!(null))
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        id
    }
}

async fn json_body(response: Response) -> Value {
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}

#[tokio::test]
async fn upload_resume_hash_range_and_download_keep_original_bytes() {
    let h = Harness::new();
    let id = h.create_upload("家庭.txt", b"hello world").await;
    assert_eq!(h.append(&id, 0, "hello ").await.status(), StatusCode::OK);
    assert_eq!(
        h.append(&id, 0, "hello ").await.status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        h.request("POST", &format!("/api/uploads/{id}/complete"), json!(null))
            .await
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(h.append(&id, 6, "world").await.status(), StatusCode::OK);
    let completed = json_body(
        h.request("POST", &format!("/api/uploads/{id}/complete"), json!(null))
            .await,
    )
    .await;
    assert_eq!(
        completed["sha256"],
        format!("{:x}", Sha256::digest(b"hello world"))
    );
    let completed_again = json_body(
        h.request("POST", &format!("/api/uploads/{id}/complete"), json!(null))
            .await,
    )
    .await;
    assert_eq!(completed, completed_again, "completion must be idempotent");
    for (range, expected) in [
        ("bytes=6-10", "world"),
        ("bytes=-5", "world"),
        ("bytes=0-4", "hello"),
    ] {
        let response = h
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/files/{id}/content"))
                    .header("range", range)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert!(response.headers().contains_key("content-range"));
        assert_eq!(
            response.into_body().collect().await.unwrap().to_bytes(),
            expected
        );
    }
    let response = h
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/files/{id}/content"))
                .header("range", "bytes=99-100")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    let response = h
        .request(
            "GET",
            &format!("/api/files/{id}/content?download=1"),
            json!(null),
        )
        .await;
    assert!(
        response.headers()["content-disposition"]
            .to_str()
            .unwrap()
            .starts_with("attachment; filename*=UTF-8''")
    );
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        "hello world"
    );
    let response = h
        .request("HEAD", &format!("/api/files/{id}/content"), json!(null))
        .await;
    assert_eq!(response.headers()["content-length"], "11");
    assert!(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .is_empty()
    );
}

#[tokio::test]
async fn interrupted_chunk_is_retried_from_committed_offset_after_restart() {
    let mut h = Harness::new();
    let id = h.create_upload("restart.txt", b"abcdef").await;
    assert_eq!(h.append(&id, 0, "abc").await.status(), StatusCode::OK);
    // Simulate bytes reaching disk just before a process dies, without committing the offset.
    std::fs::write(h.state.file_path(&id), b"abcXX").unwrap();
    h.state = AppState::open(h.directory.path()).unwrap();
    h.router = app::router(h.state.clone(), h.directory.path());
    let status = json_body(
        h.request("GET", &format!("/api/uploads/{id}"), json!(null))
            .await,
    )
    .await;
    assert_eq!(status["received"], 3);
    assert_eq!(h.append(&id, 3, "def").await.status(), StatusCode::OK);
    let done = json_body(
        h.request("POST", &format!("/api/uploads/{id}/complete"), json!(null))
            .await,
    )
    .await;
    assert_eq!(done["sha256"], format!("{:x}", Sha256::digest(b"abcdef")));
    assert_eq!(std::fs::read(h.state.file_path(&id)).unwrap(), b"abcdef");
}

#[tokio::test]
async fn broken_or_oversized_request_does_not_advance_upload() {
    let h = Harness::new();
    let id = h.create_upload("small.bin", b"abcd").await;
    assert_eq!(
        h.append(&id, 0, "too long").await.status(),
        StatusCode::BAD_REQUEST
    );
    let stream = futures_util::stream::iter([
        Ok(Bytes::from_static(b"ab")),
        Err(std::io::Error::other("connection lost")),
    ]);
    assert_eq!(
        h.append(&id, 0, Body::from_stream(stream)).await.status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    let status = json_body(
        h.request("GET", &format!("/api/uploads/{id}"), json!(null))
            .await,
    )
    .await;
    assert_eq!(status["received"], 0);
    assert_eq!(std::fs::metadata(h.state.file_path(&id)).unwrap().len(), 0);
    assert_eq!(h.append(&id, 0, "abcd").await.status(), StatusCode::OK);
}

#[tokio::test]
async fn chunk_limit_does_not_limit_total_size() {
    let h = Harness::new();
    let size = crate::files::CHUNK_SIZE as usize;
    let mut original = vec![7; size];
    original.push(8);
    let id = h.create_upload("large.bin", &original).await;
    assert_eq!(
        h.append(&id, 0, vec![7u8; size + 1]).await.status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        h.append(&id, 0, vec![7u8; size]).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        h.append(&id, size as u64, vec![8u8]).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        h.request("POST", &format!("/api/uploads/{id}/complete"), json!(null))
            .await
            .status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn messages_are_atomic_idempotent_editable_and_searchable() {
    let h = Harness::new();
    let file = h.ready_file("notes.txt", "attachment").await;
    let input = json!({"client_id":"first", "text":"  原始文字\n", "attachments":[file]});
    let created = json_body(h.request("POST", "/api/messages", input.clone()).await).await;
    let retry = json_body(h.request("POST", "/api/messages", input).await).await;
    assert_eq!(created, retry);
    assert_eq!(created["text"], "  原始文字\n");
    assert_eq!(created["attachments"][0]["name"], "notes.txt");
    let invalid = h
        .request(
            "POST",
            "/api/messages",
            json!({"client_id":"second", "attachments":[file, "missing"]}),
        )
        .await;
    assert_eq!(invalid.status(), StatusCode::CONFLICT);
    let page = json_body(h.request("GET", "/api/messages?q=notes", json!(null)).await).await;
    assert_eq!(
        page["messages"].as_array().unwrap().len(),
        1,
        "failed attachment claims must roll back their message"
    );
    let id = created["id"].as_i64().unwrap();
    let edited = json_body(
        h.request(
            "PATCH",
            &format!("/api/messages/{id}"),
            json!({"text":"edited"}),
        )
        .await,
    )
    .await;
    assert_eq!(edited["text"], "edited");
    assert_eq!(edited["attachments"], created["attachments"]);
}

#[tokio::test]
async fn reply_pagination_and_delete_are_consistent() {
    let h = Harness::new();
    let file = h.ready_file("gone.txt", "file").await;
    let first = json_body(
        h.request(
            "POST",
            "/api/messages",
            json!({"client_id":"1", "text":"parent", "attachments":[file]}),
        )
        .await,
    )
    .await;
    let parent_id = first["id"].as_i64().unwrap();
    let second = json_body(
        h.request(
            "POST",
            "/api/messages",
            json!({"client_id":"2", "text":"reply", "reply_to":parent_id}),
        )
        .await,
    )
    .await;
    assert_eq!(second["reply_excerpt"], "parent");
    let page = json_body(h.request("GET", "/api/messages?limit=1", json!(null)).await).await;
    assert_eq!(page["messages"][0]["id"], second["id"]);
    let next = page["next_before"].as_i64().unwrap();
    let older = json_body(
        h.request(
            "GET",
            &format!("/api/messages?before={next}&limit=1"),
            json!(null),
        )
        .await,
    )
    .await;
    assert_eq!(older["messages"][0]["id"], first["id"]);
    assert!(older["next_before"].is_null());
    assert_eq!(
        h.request("DELETE", &format!("/api/messages/{parent_id}"), json!(null))
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    assert!(!h.state.file_path(&file).exists());
    let reply = json_body(
        h.request(
            "GET",
            &format!("/api/messages/{}", second["id"]),
            json!(null),
        )
        .await,
    )
    .await;
    assert!(reply["reply_to"].is_null());
    assert_eq!(
        h.request("GET", &format!("/api/files/{file}/content"), json!(null))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn sse_replays_committed_changes_using_last_event_id() {
    let h = Harness::new();
    h.request(
        "POST",
        "/api/messages",
        json!({"client_id":"1", "text":"a"}),
    )
    .await;
    let page = json_body(h.request("GET", "/api/messages", json!(null)).await).await;
    let cursor = page["cursor"].as_i64().unwrap();
    h.request(
        "POST",
        "/api/messages",
        json!({"client_id":"2", "text":"b"}),
    )
    .await;
    let response = h
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/events?after=0")
                .header("last-event-id", cursor)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.headers()["content-type"], "text/event-stream");
    let mut body = response.into_body();
    let frame = tokio::time::timeout(std::time::Duration::from_secs(2), body.frame())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let text = String::from_utf8(frame.into_data().unwrap().to_vec()).unwrap();
    assert!(text.contains("event: change"), "{text}");
    assert!(text.contains(&format!("id: {}", cursor + 1)), "{text}");
    assert!(text.contains("\"message_id\":2"), "{text}");
}

#[tokio::test]
async fn empty_files_and_upload_cancellation_work() {
    let h = Harness::new();
    let id = h.ready_file("empty.txt", "").await;
    let status = json_body(
        h.request("GET", &format!("/api/uploads/{id}"), json!(null))
            .await,
    )
    .await;
    assert_eq!(status["sha256"], format!("{:x}", Sha256::digest(b"")));
    assert_eq!(
        h.request("DELETE", &format!("/api/uploads/{id}"), json!(null))
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    assert!(!h.state.file_path(&id).exists());
    assert_eq!(
        h.request("GET", &format!("/api/uploads/{id}"), json!(null))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        h.request(
            "POST",
            "/api/messages",
            json!({"client_id":"empty", "text":" "})
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn active_event_stream_closes_when_the_server_stops() {
    let h = Harness::new();
    let response = h.request("GET", "/api/events", json!(null)).await;
    let mut body = response.into_body();
    h.state.stopping.send_replace(true);
    let frame = tokio::time::timeout(std::time::Duration::from_secs(1), body.frame())
        .await
        .expect("SSE must finish before graceful shutdown waits for connections");
    assert!(frame.is_none());
}

#[tokio::test]
async fn chunks_of_one_file_transfer_concurrently_without_overwriting_committed_bytes() {
    let h = Harness::new();
    let size = crate::files::CHUNK_SIZE as usize;
    let mut original = vec![7; size];
    original.extend_from_slice(b"end");
    let id = h.create_upload("parallel.bin", &original).await;
    let (started, running) = tokio::sync::oneshot::channel();
    let (release, proceed) = tokio::sync::oneshot::channel();
    let slow = Body::from_stream(async_stream::stream! {
        yield Ok::<_, std::io::Error>(Bytes::from(vec![7; 64 * 1024]));
        started.send(()).unwrap();
        proceed.await.unwrap();
        yield Ok(Bytes::from(vec![7; size - 64 * 1024]));
    });
    let (head, ()) = tokio::join!(h.part(&id, 0, slow), async {
        running.await.unwrap();
        let tail = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            h.part(&id, size as u64, "end"),
        )
        .await
        .expect("another part must finish while the first request is still streaming");
        assert_eq!(tail.status(), StatusCode::OK);
        let progress = json_body(tail).await;
        assert_eq!(progress["received"], 0);
        assert_eq!(progress["uploaded"], 3);
        assert_eq!(
            h.part(&id, size as u64, "BAD").await.status(),
            StatusCode::OK
        );
        release.send(()).unwrap();
    });
    assert_eq!(head.status(), StatusCode::OK);
    assert_eq!(json_body(head).await["received"], size + 3);
    let completed = json_body(
        h.request("POST", &format!("/api/uploads/{id}/complete"), json!(null))
            .await,
    )
    .await;
    assert_eq!(
        completed["sha256"],
        format!("{:x}", Sha256::digest(&original))
    );
    assert_eq!(std::fs::read(h.state.file_path(&id)).unwrap(), original);
}

#[tokio::test]
async fn parallel_gaps_survive_restart_and_bad_parts_do_not_destroy_other_ranges() {
    let mut h = Harness::new();
    let size = crate::files::CHUNK_SIZE as usize;
    let mut original = vec![2; size];
    original.extend_from_slice(b"end");
    let id = h.create_upload("resume.bin", &original).await;
    assert_eq!(
        h.part(&id, size as u64, "end").await.status(),
        StatusCode::OK
    );
    h.state = AppState::open(h.directory.path()).unwrap();
    h.router = app::router(h.state.clone(), h.directory.path());
    assert_eq!(
        h.request("POST", &format!("/api/uploads/{id}/complete"), json!(null))
            .await
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(h.append(&id, 0, "abc").await.status(), StatusCode::CONFLICT);
    assert_eq!(h.part(&id, 1, "abc").await.status(), StatusCode::CONFLICT);
    assert_eq!(
        h.part(&id, (size + 3) as u64, "a").await.status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        h.part(&id, 0, vec![1; size + 1]).await.status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        h.part(&id, 0, "short").await.status(),
        StatusCode::BAD_REQUEST
    );
    let broken = futures_util::stream::iter([
        Ok(Bytes::from(vec![1; 2 * 1024 * 1024])),
        Err(std::io::Error::other("disconnected")),
    ]);
    assert_eq!(
        h.part(&id, 0, Body::from_stream(broken)).await.status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    let progress = json_body(
        h.request("GET", &format!("/api/uploads/{id}"), json!(null))
            .await,
    )
    .await;
    assert_eq!(progress["received"], 0);
    assert_eq!(progress["uploaded"], 3);
    assert_eq!(h.part(&id, 0, vec![2; size]).await.status(), StatusCode::OK);
    let bytes = std::fs::read(h.state.file_path(&id)).unwrap();
    assert_eq!(&bytes[..size], vec![2; size]);
    assert_eq!(&bytes[size..], b"end");
    let remaining = h
        .state
        .db
        .call(move |db| {
            Ok(db.query_row("SELECT COUNT(*) FROM upload_parts", [], |r| {
                r.get::<_, u64>(0)
            })?)
        })
        .await
        .unwrap();
    assert_eq!(
        remaining, 0,
        "merged ranges must not accumulate for large files"
    );
}

#[tokio::test]
async fn parallel_upload_can_resume_an_old_partial_chunk_and_cancel_cleans_progress() {
    let h = Harness::new();
    let size = crate::files::CHUNK_SIZE as usize;
    let mut original = vec![2; size + 1];
    original[..3].copy_from_slice(b"abc");
    original[size] = b'z';
    let id = h.create_upload("legacy.bin", &original).await;
    assert_eq!(h.append(&id, 0, "abc").await.status(), StatusCode::OK);
    assert_eq!(h.part(&id, size as u64, "z").await.status(), StatusCode::OK);
    assert_eq!(
        h.part(&id, 3, vec![2; size - 3]).await.status(),
        StatusCode::OK
    );
    let completed = h
        .request("POST", &format!("/api/uploads/{id}/complete"), json!(null))
        .await;
    assert_eq!(completed.status(), StatusCode::OK);
    let bytes = std::fs::read(h.state.file_path(&id)).unwrap();
    assert_eq!(&bytes[..3], b"abc");
    assert_eq!(&bytes[3..size], vec![2; size - 3]);
    assert_eq!(bytes[size], b'z');

    let cancelled = h.create_upload("cancel.bin", &vec![3; size + 1]).await;
    assert_eq!(
        h.part(&cancelled, size as u64, "z").await.status(),
        StatusCode::OK
    );
    assert_eq!(
        h.request("DELETE", &format!("/api/uploads/{cancelled}"), json!(null))
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    assert!(!h.state.file_path(&cancelled).exists());
    let remaining = h
        .state
        .db
        .call(move |db| {
            Ok(db.query_row("SELECT COUNT(*) FROM upload_parts", [], |r| {
                r.get::<_, u64>(0)
            })?)
        })
        .await
        .unwrap();
    assert_eq!(remaining, 0);
}

#[tokio::test]
async fn hash_mismatch_cannot_be_published_and_can_be_retried() {
    let h = Harness::new();
    assert_eq!(
        h.request(
            "POST",
            "/api/uploads",
            json!({"name":"bad", "size":4, "sha256":"invalid"})
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    let id = h.create_upload("checked.bin", b"good").await;
    assert_eq!(h.part(&id, 0, "evil").await.status(), StatusCode::OK);
    let rejected = h
        .request("POST", &format!("/api/uploads/{id}/complete"), json!(null))
        .await;
    assert_eq!(rejected.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let progress = json_body(
        h.request("GET", &format!("/api/uploads/{id}"), json!(null))
            .await,
    )
    .await;
    assert!(progress["sha256"].is_null());
    assert_eq!(progress["received"], 0);
    assert_eq!(
        h.request("GET", &format!("/api/files/{id}/content"), json!(null))
            .await
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        h.request(
            "POST",
            "/api/messages",
            json!({"client_id":"bad", "attachments":[id]})
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(h.part(&id, 0, "good").await.status(), StatusCode::OK);
    let complete = h
        .request("POST", &format!("/api/uploads/{id}/complete"), json!(null))
        .await;
    assert_eq!(complete.status(), StatusCode::OK);
    assert_eq!(
        json_body(complete).await["sha256"],
        format!("{:x}", Sha256::digest(b"good"))
    );
    let other = h.create_upload("different.bin", b"evil").await;
    assert!(
        json_body(
            h.request("GET", &format!("/api/uploads/{other}"), json!(null))
                .await
        )
        .await["sha256"]
            .is_null()
    );
}

#[tokio::test]
async fn deduplicated_files_keep_names_and_live_until_the_last_message_or_draft_releases_them() {
    let h = Harness::new();
    let original = h.ready_file("original.txt", "shared bytes").await;
    let alias = h.create_upload("renamed.txt", b"shared bytes").await;
    let reused = json_body(
        h.request("GET", &format!("/api/uploads/{alias}"), json!(null))
            .await,
    )
    .await;
    assert!(reused["sha256"].is_string());
    assert_eq!(reused["name"], "renamed.txt");
    assert_eq!(reused["received"], 12);
    assert!(
        !h.state.file_path(&alias).exists(),
        "instant upload must not create another copy"
    );
    let other = h.ready_file("other.txt", "different").await;
    let one = json_body(
        h.request(
            "POST",
            "/api/messages",
            json!({"client_id":"one", "attachments":[original]}),
        )
        .await,
    )
    .await;
    let two = json_body(
        h.request(
            "POST",
            "/api/messages",
            json!({"client_id":"two", "attachments":[original, other]}),
        )
        .await,
    )
    .await;
    assert_eq!(two["attachments"].as_array().unwrap().len(), 2);
    assert_eq!(one["attachments"][0]["id"], two["attachments"][0]["id"]);
    let three = json_body(
        h.request(
            "POST",
            "/api/messages",
            json!({"client_id":"three", "attachments":[alias]}),
        )
        .await,
    )
    .await;
    assert_eq!(
        h.request("DELETE", &format!("/api/uploads/{original}"), json!(null))
            .await
            .status(),
        StatusCode::CONFLICT
    );
    for message in [&one, &two] {
        assert_eq!(
            h.request(
                "DELETE",
                &format!("/api/messages/{}", message["id"]),
                json!(null)
            )
            .await
            .status(),
            StatusCode::NO_CONTENT
        );
        let download = h
            .request("GET", &format!("/api/files/{alias}/content"), json!(null))
            .await;
        assert_eq!(download.status(), StatusCode::OK);
        assert_eq!(
            download.into_body().collect().await.unwrap().to_bytes(),
            "shared bytes"
        );
    }
    let pending = h.create_upload("pending.txt", b"shared bytes").await;
    h.request(
        "DELETE",
        &format!("/api/messages/{}", three["id"]),
        json!(null),
    )
    .await;
    assert!(
        h.state.file_path(&original).exists(),
        "an unsent instant-upload draft still owns its content"
    );
    let download = h
        .request(
            "GET",
            &format!("/api/files/{pending}/content?download=1"),
            json!(null),
        )
        .await;
    assert_eq!(download.status(), StatusCode::OK);
    assert!(
        download.headers()["content-disposition"]
            .to_str()
            .unwrap()
            .contains("pending%2Etxt")
    );
    assert_eq!(
        h.request("DELETE", &format!("/api/uploads/{pending}"), json!(null))
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    assert!(!h.state.file_path(&original).exists());
    assert!(!h.state.file_path(&other).exists());
}

#[tokio::test]
async fn simultaneous_identical_uploads_converge_on_one_blob() {
    let h = Harness::new();
    let first = h.create_upload("first.txt", b"identical").await;
    let second = h.create_upload("second.txt", b"identical").await;
    let (a, b) = tokio::join!(
        h.part(&first, 0, "identical"),
        h.part(&second, 0, "identical")
    );
    assert_eq!(a.status(), StatusCode::OK);
    assert_eq!(b.status(), StatusCode::OK);
    let first_path = format!("/api/uploads/{first}/complete");
    let second_path = format!("/api/uploads/{second}/complete");
    let (a, b) = tokio::join!(
        h.request("POST", &first_path, json!(null)),
        h.request("POST", &second_path, json!(null))
    );
    assert_eq!(a.status(), StatusCode::OK);
    assert_eq!(b.status(), StatusCode::OK);
    crate::files::collect_garbage(&h.state).await.unwrap();
    let count = h
        .state
        .db
        .call(|db| Ok(db.query_row("SELECT COUNT(*) FROM blobs", [], |r| r.get::<_, u64>(0))?))
        .await
        .unwrap();
    assert_eq!(count, 1);
    assert_eq!(
        std::fs::read_dir(h.directory.path().join("files"))
            .unwrap()
            .count(),
        1
    );
    h.request("DELETE", &format!("/api/uploads/{first}"), json!(null))
        .await;
    let download = h
        .request("GET", &format!("/api/files/{second}/content"), json!(null))
        .await;
    assert_eq!(download.status(), StatusCode::OK);
    assert_eq!(
        download.into_body().collect().await.unwrap().to_bytes(),
        "identical"
    );
    h.request("DELETE", &format!("/api/uploads/{second}"), json!(null))
        .await;
    assert_eq!(
        std::fs::read_dir(h.directory.path().join("files"))
            .unwrap()
            .count(),
        0
    );

    let empty = h.ready_file("empty.txt", "").await;
    let another = h.create_upload("another-empty.txt", b"").await;
    let status = json_body(
        h.request("GET", &format!("/api/uploads/{another}"), json!(null))
            .await,
    )
    .await;
    assert_eq!(status["sha256"], format!("{:x}", Sha256::digest(b"")));
    assert!(h.state.file_path(&empty).exists());
    assert!(!h.state.file_path(&another).exists());
}

#[tokio::test]
async fn version_two_migration_preserves_urls_messages_pending_ranges_and_deduplicates_old_bytes() {
    let directory = TempDir::new().unwrap();
    std::fs::create_dir(directory.path().join("files")).unwrap();
    let hash = format!("{:x}", Sha256::digest(b"shared"));
    {
        let db = rusqlite::Connection::open(directory.path().join("homeclip.sqlite3")).unwrap();
        db.execute_batch(include_str!("fixtures/schema_v2.sql"))
            .unwrap();
        db.execute_batch("INSERT INTO messages(id, client_id, text, created_at, updated_at) VALUES (1, 'one', 'first', 0, 0), (2, 'two', 'second', 0, 0);").unwrap();
        for (id, name, message) in [("a", "one.txt", 1), ("b", "two.txt", 2)] {
            db.execute("INSERT INTO files(id, name, size, mime, received, sha256, created_at, message_id) VALUES (?1, ?2, 6, 'text/plain', 6, ?3, 0, ?4)", rusqlite::params![id, name, hash, message]).unwrap();
            std::fs::write(directory.path().join("files").join(id), b"shared").unwrap();
        }
        let size = crate::files::CHUNK_SIZE;
        db.execute("INSERT INTO files(id, name, size, mime, created_at) VALUES ('pending', 'partial.bin', ?1, 'application/octet-stream', 0)", [size + 3]).unwrap();
        db.execute(
            "INSERT INTO upload_parts(file_id, offset, length) VALUES ('pending', ?1, 3)",
            [size],
        )
        .unwrap();
        let mut partial = vec![0; size as usize];
        partial.extend_from_slice(b"end");
        std::fs::write(directory.path().join("files/pending"), partial).unwrap();
    }
    let state = AppState::open(directory.path()).unwrap();
    let h = Harness {
        router: app::router(state.clone(), directory.path()),
        state,
        directory,
    };
    crate::files::collect_garbage(&h.state).await.unwrap();
    assert!(!h.state.file_path("b").exists());
    for (id, name) in [(1, "one.txt"), (2, "two.txt")] {
        let message = json_body(
            h.request("GET", &format!("/api/messages/{id}"), json!(null))
                .await,
        )
        .await;
        assert_eq!(message["attachments"][0]["name"], name);
    }
    h.request("DELETE", "/api/messages/1", json!(null)).await;
    assert_eq!(
        h.request("GET", "/api/files/b/content", json!(null))
            .await
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes(),
        "shared"
    );
    let progress = json_body(h.request("GET", "/api/uploads/pending", json!(null)).await).await;
    assert_eq!(progress["received"], 0);
    assert_eq!(progress["uploaded"], 3);
    assert_eq!(
        h.part("pending", 0, vec![7; crate::files::CHUNK_SIZE as usize])
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        h.request("POST", "/api/uploads/pending/complete", json!(null))
            .await
            .status(),
        StatusCode::OK
    );
    let reopened = AppState::open(h.directory.path()).unwrap();
    let violations = reopened
        .db
        .call(|db| {
            Ok(
                db.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| {
                    r.get::<_, u64>(0)
                })?,
            )
        })
        .await
        .unwrap();
    assert_eq!(violations, 0);
}
