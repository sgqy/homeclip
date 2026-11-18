use std::{convert::Infallible, time::Duration};

use axum::{
    extract::{Query, State},
    http::HeaderMap,
    response::{
        Sse,
        sse::{Event, KeepAlive},
    },
};
use futures_util::Stream;
use serde::Deserialize;

use crate::{db::Change, state::AppState};

#[derive(Debug, Deserialize)]
pub(crate) struct EventQuery {
    after: Option<i64>,
}

pub(crate) async fn subscribe(
    State(state): State<AppState>,
    Query(query): Query<EventQuery>,
    headers: HeaderMap,
) -> Sse<impl Stream<Item = std::result::Result<Event, Infallible>>> {
    let mut cursor = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<i64>().ok())
        .or(query.after)
        .unwrap_or(0)
        .max(0);
    // Subscribe before reading SQLite so a concurrent commit cannot be missed.
    let mut wake = state.changes.subscribe();
    let mut stopping = state.stopping.subscribe();
    let stream = async_stream::stream! {
        loop {
            if *stopping.borrow() { break; }
            let result = state.db.call(move |db| {
                let latest: i64 = db.query_row("SELECT COALESCE(MAX(id), 0) FROM events", [], |r| r.get(0))?;
                let changes = db.prepare("SELECT id, kind, message_id FROM events WHERE id > ?1 ORDER BY id LIMIT 100")?
                    .query_map([cursor], |r| Ok(Change { id: r.get(0)?, kind: r.get(1)?, message_id: r.get(2)? }))?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                Ok((latest, changes))
            }).await;
            match result {
                Ok((latest, _)) if cursor > latest => {
                    cursor = latest;
                    yield Ok(Event::default().event("reset").id(cursor.to_string()).data("{}"));
                }
                Ok((_, changes)) if !changes.is_empty() => {
                    for change in changes {
                        cursor = change.id;
                        if let Ok(event) = Event::default().event("change").id(cursor.to_string()).json_data(change) {
                            yield Ok(event);
                        }
                    }
                    continue;
                }
                Ok(_) => {},
                Err(error) => {
                    tracing::warn!(?error, "event stream interrupted");
                    break;
                }
            }
            tokio::select! {
                result = wake.changed() => { if result.is_err() { break; } },
                _ = stopping.changed() => break,
            }
        }
    };
    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("heartbeat"),
    )
}
