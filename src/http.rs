use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{
        sse::{Event as SseEvent, KeepAlive, Sse},
        IntoResponse, Json,
    },
    routing::{get, post},
    Router,
};
use futures::{Stream, StreamExt as _};
use serde::{Deserialize, Serialize};
use zamsync_core::{NodeId, SequenceNumber};
use zamsync_storage::{EncryptionKey, PayloadSchema};

use crate::util::open_engine;

// ---------------------------------------------------------------------------
// Shared state -- all fields must be Send + Sync.
// EncryptionKey wraps a ChaCha20 cipher that may not be Sync, so we store
// the raw 32-byte key and reconstruct per request.
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct HttpState {
    data_dir: Arc<PathBuf>,
    raw_key: Option<[u8; 32]>,
    node_id: NodeId,
    schema: PayloadSchema,
}

impl HttpState {
    fn enc_key(&self) -> Option<EncryptionKey> {
        self.raw_key.map(EncryptionKey::from_bytes)
    }
}

pub struct HttpConfig {
    pub bind_addr: String,
    pub data_dir: PathBuf,
    pub enc_key: Option<EncryptionKey>,
    pub node_id: NodeId,
    pub schema: PayloadSchema,
}

// ---------------------------------------------------------------------------
// Entry point -- runs in a dedicated OS thread with its own tokio runtime.
// ---------------------------------------------------------------------------

pub fn spawn(config: HttpConfig) -> std::thread::JoinHandle<()> {
    let raw_key = config.enc_key.as_ref().map(EncryptionKey::raw_bytes);
    let bind_addr = config.bind_addr.clone();

    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio rt");

        let state = Arc::new(HttpState {
            data_dir: Arc::new(config.data_dir),
            raw_key,
            node_id: config.node_id,
            schema: config.schema,
        });

        let app = Router::new()
            .route("/health", get(health))
            .route("/submit", post(submit))
            .route("/events", get(events))
            .route("/events/stream", get(events_stream))
            .with_state(state);

        rt.block_on(async {
            let listener = tokio::net::TcpListener::bind(&bind_addr)
                .await
                .unwrap_or_else(|e| panic!("http bind {bind_addr}: {e}"));
            println!("[http] listening on http://{bind_addr}");
            axum::serve(listener, app).await.expect("http serve");
        });
    })
}

// ---------------------------------------------------------------------------
// Wire types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    node_id: String,
    events: usize,
}

#[derive(Deserialize)]
struct SubmitRequest {
    #[serde(default = "default_event_type")]
    event_type: u32,
    payload: serde_json::Value,
}

fn default_event_type() -> u32 {
    1
}

#[derive(Serialize)]
struct SubmitResponse {
    seq: u64,
    node_id: String,
}

#[derive(Deserialize)]
struct SinceQuery {
    since: Option<u64>,
}

#[derive(Serialize, Clone)]
struct EventJson {
    seq: u64,
    node_id: String,
    event_type: u32,
    payload: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn to_event_json(e: &zamsync_core::Event) -> EventJson {
    let payload = serde_json::from_slice(&e.payload)
        .unwrap_or_else(|_| serde_json::Value::String(base64_encode(&e.payload)));
    EventJson {
        seq: e.seq.0,
        node_id: format!("{:08x}", e.origin_node.0),
        event_type: e.event_type,
        payload,
    }
}

fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = chunk.get(1).copied().unwrap_or(0) as usize;
        let b2 = chunk.get(2).copied().unwrap_or(0) as usize;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(CHARS[(n >> 18) & 63] as char);
        out.push(CHARS[(n >> 12) & 63] as char);
        out.push(if chunk.len() > 1 { CHARS[(n >> 6) & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { CHARS[n & 63] as char } else { '=' });
    }
    out
}

struct AppError(String);

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        (StatusCode::INTERNAL_SERVER_ERROR, self.0).into_response()
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn health(State(s): State<Arc<HttpState>>) -> Json<HealthResponse> {
    let count = tokio::task::spawn_blocking({
        let s = s.clone();
        move || {
            open_engine(&s.data_dir, s.node_id, s.enc_key(), PayloadSchema::None)
                .map(|e| e.state().count)
                .unwrap_or(0)
        }
    })
    .await
    .unwrap_or(0);

    Json(HealthResponse {
        status: "ok",
        node_id: format!("{:08x}", s.node_id.0),
        events: count,
    })
}

async fn submit(
    State(s): State<Arc<HttpState>>,
    Json(body): Json<SubmitRequest>,
) -> Result<Json<SubmitResponse>, AppError> {
    let payload_bytes = serde_json::to_vec(&body.payload)
        .map_err(|e| AppError(e.to_string()))?;
    let event_type = body.event_type;
    let node_id = s.node_id;

    let seq = tokio::task::spawn_blocking({
        let s = s.clone();
        move || -> Result<SequenceNumber, String> {
            let mut engine = open_engine(&s.data_dir, s.node_id, s.enc_key(), s.schema.clone())
                .map_err(|e| e.to_string())?;
            let seq = engine
                .submit(event_type, payload_bytes)
                .map_err(|e| e.to_string())?;
            engine.sync().map_err(|e| e.to_string())?;
            Ok(seq)
        }
    })
    .await
    .map_err(|e| AppError(e.to_string()))?
    .map_err(AppError)?;

    Ok(Json(SubmitResponse {
        seq: seq.0,
        node_id: format!("{:08x}", node_id.0),
    }))
}

async fn events(
    State(s): State<Arc<HttpState>>,
    Query(q): Query<SinceQuery>,
) -> Result<Json<Vec<EventJson>>, AppError> {
    let since = q.since.unwrap_or(0);

    let evts = tokio::task::spawn_blocking({
        let s = s.clone();
        move || -> Result<Vec<EventJson>, String> {
            let engine =
                open_engine(&s.data_dir, s.node_id, s.enc_key(), PayloadSchema::None)
                    .map_err(|e| e.to_string())?;
            let evts = engine
                .scan_events()
                .map_err(|e| e.to_string())?
                .filter_map(|r| r.ok())
                .filter(|e| e.seq.0 >= since)
                .map(|e| to_event_json(&e))
                .collect();
            Ok(evts)
        }
    })
    .await
    .map_err(|e| AppError(e.to_string()))?
    .map_err(AppError)?;

    Ok(Json(evts))
}

// SSE endpoint: polls every 500ms and streams new events as they appear.
// Uses `unfold` so `last_seq` is properly threaded through each poll iteration.
async fn events_stream(
    State(s): State<Arc<HttpState>>,
    Query(q): Query<SinceQuery>,
) -> Sse<impl Stream<Item = Result<SseEvent, Infallible>>> {
    let initial = q.since.unwrap_or(0);

    let stream = futures::stream::unfold(initial, move |last_seq| {
        let s = s.clone();
        async move {
            tokio::time::sleep(Duration::from_millis(500)).await;

            let current = last_seq;
            let evts: Vec<EventJson> = tokio::task::spawn_blocking(move || {
                let engine =
                    open_engine(&s.data_dir, s.node_id, s.enc_key(), PayloadSchema::None)
                        .ok()?;
                let evts: Vec<EventJson> = engine
                    .scan_events()
                    .ok()?
                    .filter_map(|r| r.ok())
                    .filter(|e| e.seq.0 > current)
                    .map(|e| to_event_json(&e))
                    .collect();
                Some(evts)
            })
            .await
            .ok()
            .flatten()
            .unwrap_or_default();

            let new_last = evts.iter().map(|e| e.seq).max().unwrap_or(last_seq);
            Some((evts, new_last))
        }
    })
    .flat_map(|evts: Vec<EventJson>| {
        tokio_stream::iter(evts.into_iter().map(|e| {
            let data = serde_json::to_string(&e).unwrap_or_default();
            Ok::<_, Infallible>(SseEvent::default().data(data))
        }))
    });

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    )
}
