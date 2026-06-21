use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    extract::{rejection::JsonRejection, Query, State},
    http::{header, StatusCode},
    response::{
        sse::{Event as SseEvent, KeepAlive, Sse},
        IntoResponse, Json, Response,
    },
    routing::{get, post},
    Router,
};
use futures::{Stream, StreamExt as _};
use serde::{Deserialize, Serialize};
use zamsync_core::{ports::StateStore, Event, NodeId, SequenceNumber, ZamError};
use zamsync_storage::{EncryptionKey, PayloadSchema, ZamEngine};

use crate::util::{format_date, open_engine};

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct HttpState {
    data_dir: Arc<PathBuf>,
    raw_key: Option<[u8; 32]>,
    node_id: NodeId,
    schema: PayloadSchema,
    started_at: Instant,
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
// Entry point
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
            started_at: Instant::now(),
        });

        let app = build_router(state);

        rt.block_on(async {
            let listener = tokio::net::TcpListener::bind(&bind_addr)
                .await
                .unwrap_or_else(|e| panic!("http bind {bind_addr}: {e}"));
            println!("[http] listening on http://{bind_addr}");
            axum::serve(listener, app).await.expect("http serve");
        });
    })
}

fn build_router(state: Arc<HttpState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/submit", post(submit))
        .route("/events", get(events))
        .route("/events/stream", get(events_stream))
        .route("/ui", get(dashboard))
        .route("/ui/data", get(ui_data))
        .with_state(state)
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

#[derive(Serialize)]
struct UiData {
    node_id: String,
    events: usize,
    wal_size_bytes: u64,
    uptime_seconds: u64,
    oldest_event: Option<String>,
    newest_event: Option<String>,
}

// WAL scan state for /ui/data: collects count + oldest/newest timestamps in one pass.
#[derive(Default)]
struct DashState {
    count: usize,
    oldest_ms: Option<u64>,
    newest_ms: Option<u64>,
}

impl StateStore for DashState {
    fn apply_event(&mut self, _seq: SequenceNumber, event: &Event) -> zamsync_core::ZamResult<()> {
        self.count += 1;
        let phys = event.hlc.physical;
        self.oldest_ms = Some(self.oldest_ms.map_or(phys, |o| o.min(phys)));
        self.newest_ms = Some(self.newest_ms.map_or(phys, |n| n.max(phys)));
        Ok(())
    }
    fn last_applied_seq(&self) -> Option<SequenceNumber> {
        None
    }
}

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Typed HTTP API errors with stable machine-readable codes.
///
/// Codes are additive -- new codes may be added; existing codes are never renamed.
#[derive(Debug)]
enum AppError {
    /// 400 -- request body is not valid JSON or Content-Type is missing.
    InvalidJson(String),
    /// 422 -- body is valid JSON but fails the configured --schema validation.
    SchemaViolation(String),
    /// 503 -- WAL file unavailable (disk full, corrupt, permission error).
    Unavailable(String),
    /// 500 -- unexpected internal error.
    Internal(String),
}

#[derive(Serialize)]
struct ErrorBody {
    error: &'static str,
    message: String,
}

impl AppError {
    fn status(&self) -> StatusCode {
        match self {
            Self::InvalidJson(_) => StatusCode::BAD_REQUEST,
            Self::SchemaViolation(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::Unavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn code(&self) -> &'static str {
        match self {
            Self::InvalidJson(_) => "INVALID_JSON",
            Self::SchemaViolation(_) => "SCHEMA_VIOLATION",
            Self::Unavailable(_) => "WAL_UNAVAILABLE",
            Self::Internal(_) => "INTERNAL_ERROR",
        }
    }

    fn message(&self) -> &str {
        match self {
            Self::InvalidJson(m)
            | Self::SchemaViolation(m)
            | Self::Unavailable(m)
            | Self::Internal(m) => m,
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (
            self.status(),
            Json(ErrorBody {
                error: self.code(),
                message: self.message().to_owned(),
            }),
        )
            .into_response()
    }
}

fn zam_to_app(e: ZamError) -> AppError {
    let msg = e.to_string();
    match e {
        ZamError::Validation(m) => AppError::SchemaViolation(m),
        ZamError::Io(_) | ZamError::Corruption(_) | ZamError::Storage(_) => {
            AppError::Unavailable(msg)
        }
        _ => AppError::Internal(msg),
    }
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
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = chunk.get(1).copied().unwrap_or(0) as usize;
        let b2 = chunk.get(2).copied().unwrap_or(0) as usize;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(CHARS[(n >> 18) & 63] as char);
        out.push(CHARS[(n >> 12) & 63] as char);
        out.push(if chunk.len() > 1 {
            CHARS[(n >> 6) & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            CHARS[n & 63] as char
        } else {
            '='
        });
    }
    out
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
    body: Result<Json<SubmitRequest>, JsonRejection>,
) -> Result<Json<SubmitResponse>, AppError> {
    let Json(body) = body.map_err(|e| AppError::InvalidJson(e.to_string()))?;
    let payload_bytes =
        serde_json::to_vec(&body.payload).map_err(|e| AppError::Internal(e.to_string()))?;
    let event_type = body.event_type;
    let node_id = s.node_id;

    let seq = tokio::task::spawn_blocking({
        let s = s.clone();
        move || -> Result<SequenceNumber, AppError> {
            let mut engine = open_engine(&s.data_dir, s.node_id, s.enc_key(), s.schema.clone())
                .map_err(|e| AppError::Unavailable(e.to_string()))?;
            let seq = engine
                .submit(event_type, payload_bytes)
                .map_err(zam_to_app)?;
            engine
                .sync()
                .map_err(|e| AppError::Unavailable(e.to_string()))?;
            Ok(seq)
        }
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

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
        move || -> Result<Vec<EventJson>, AppError> {
            let engine = open_engine(&s.data_dir, s.node_id, s.enc_key(), PayloadSchema::None)
                .map_err(|e| AppError::Unavailable(e.to_string()))?;
            let evts = engine
                .scan_events()
                .map_err(|e| AppError::Unavailable(e.to_string()))?
                .filter_map(|r| r.ok())
                .filter(|e| e.seq.0 >= since)
                .map(|e| to_event_json(&e))
                .collect();
            Ok(evts)
        }
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    Ok(Json(evts))
}

// SSE endpoint: polls every 500ms and streams new events as they appear.
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
                    open_engine(&s.data_dir, s.node_id, s.enc_key(), PayloadSchema::None).ok()?;
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

// Serves the embedded status dashboard with security headers.
async fn dashboard() -> Response {
    let html = include_str!("dashboard.html");
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (
                header::HeaderName::from_static("content-security-policy"),
                // Inline styles + scripts are ours; no external origins allowed.
                "default-src 'self'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; connect-src 'self'; img-src 'none'",
            ),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (header::X_FRAME_OPTIONS, "SAMEORIGIN"),
            (
                header::HeaderName::from_static("referrer-policy"),
                "no-referrer",
            ),
        ],
        html,
    )
        .into_response()
}

// Returns aggregate node stats consumed by the dashboard.
async fn ui_data(State(s): State<Arc<HttpState>>) -> Result<Json<UiData>, AppError> {
    let node_id = format!("{:08x}", s.node_id.0);
    let uptime_seconds = s.started_at.elapsed().as_secs();

    let (events, wal_size_bytes, oldest_ms, newest_ms) = tokio::task::spawn_blocking({
        let s = s.clone();
        move || -> Result<(usize, u64, Option<u64>, Option<u64>), AppError> {
            let engine: zamsync_storage::ZamEngine<
                zamsync_storage::WalEventStore,
                zamsync_storage::FilePeerStore,
                DashState,
            > = match s.raw_key {
                Some(key) => ZamEngine::open_wal_encrypted(
                    &*s.data_dir,
                    s.node_id,
                    DashState::default(),
                    EncryptionKey::from_bytes(key),
                )
                .map_err(|e| AppError::Unavailable(e.to_string()))?,
                None => ZamEngine::open_wal(&*s.data_dir, s.node_id, DashState::default())
                    .map_err(|e| AppError::Unavailable(e.to_string()))?,
            };
            let st = engine.state();
            let wal_size = engine.wal_byte_size();
            Ok((st.count, wal_size, st.oldest_ms, st.newest_ms))
        }
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))??;

    Ok(Json(UiData {
        node_id,
        events,
        wal_size_bytes,
        uptime_seconds,
        oldest_event: oldest_ms.map(format_date),
        newest_event: newest_ms.map(format_date),
    }))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use tower::ServiceExt as _;

    fn make_state() -> (Arc<HttpState>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let state = Arc::new(HttpState {
            data_dir: Arc::new(dir.path().to_path_buf()),
            raw_key: None,
            node_id: NodeId(1),
            schema: PayloadSchema::None,
            started_at: Instant::now(),
        });
        (state, dir) // return dir so it stays alive for the test
    }

    // ---- Dashboard tests ----

    #[tokio::test]
    async fn ui_returns_html_200() {
        let (state, _dir) = make_state();
        let app = build_router(state);
        let resp = app
            .oneshot(Request::builder().uri("/ui").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(
            ct.contains("text/html"),
            "content-type must be text/html, got {ct}"
        );
    }

    #[tokio::test]
    async fn ui_has_security_headers() {
        let (state, _dir) = make_state();
        let app = build_router(state);
        let resp = app
            .oneshot(Request::builder().uri("/ui").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let headers = resp.headers();
        assert!(
            headers.contains_key("content-security-policy"),
            "missing Content-Security-Policy"
        );
        assert!(
            headers.contains_key(header::X_CONTENT_TYPE_OPTIONS),
            "missing X-Content-Type-Options"
        );
        assert!(
            headers.contains_key(header::X_FRAME_OPTIONS),
            "missing X-Frame-Options"
        );
    }

    #[tokio::test]
    async fn ui_data_returns_valid_json() {
        let (state, _dir) = make_state();
        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/ui/data")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(ct.contains("application/json"), "got: {ct}");
        let body = axum::body::to_bytes(resp.into_body(), 64 * 1024)
            .await
            .unwrap();
        let data: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(data["node_id"].is_string(), "node_id must be a string");
        assert!(data["events"].is_number(), "events must be a number");
        assert!(
            data["wal_size_bytes"].is_number(),
            "wal_size_bytes must be a number"
        );
        assert!(
            data["uptime_seconds"].is_number(),
            "uptime_seconds must be a number"
        );
    }

    #[tokio::test]
    async fn ui_data_node_id_matches_state() {
        let (state, _dir) = make_state();
        let expected = format!("{:08x}", state.node_id.0);
        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/ui/data")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(resp.into_body(), 64 * 1024)
            .await
            .unwrap();
        let data: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(data["node_id"].as_str().unwrap(), expected);
    }

    #[tokio::test]
    async fn health_returns_ok() {
        let (state, _dir) = make_state();
        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 64 * 1024)
            .await
            .unwrap();
        let data: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(data["status"].as_str().unwrap(), "ok");
    }

    // ---- Error code tests ----

    #[tokio::test]
    async fn submit_invalid_json_returns_400() {
        let (state, _dir) = make_state();
        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/submit")
                    .header("content-type", "application/json")
                    .body(Body::from("not json!!!"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(resp.into_body(), 64 * 1024)
            .await
            .unwrap();
        let data: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(data["error"].as_str().unwrap(), "INVALID_JSON");
        assert!(data["message"].is_string());
    }

    #[tokio::test]
    async fn submit_missing_content_type_returns_400() {
        let (state, _dir) = make_state();
        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/submit")
                    .body(Body::from(r#"{"payload":{}}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(resp.into_body(), 64 * 1024)
            .await
            .unwrap();
        let data: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(data["error"].as_str().unwrap(), "INVALID_JSON");
    }

    #[tokio::test]
    async fn submit_schema_violation_returns_422() {
        let dir = tempfile::tempdir().unwrap();
        let state = Arc::new(HttpState {
            data_dir: Arc::new(dir.path().to_path_buf()),
            raw_key: None,
            node_id: NodeId(1),
            schema: PayloadSchema::JsonRequired(vec!["patient_id".to_string()]),
            started_at: Instant::now(),
        });
        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/submit")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"payload":{"name":"John"}}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body = axum::body::to_bytes(resp.into_body(), 64 * 1024)
            .await
            .unwrap();
        let data: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(data["error"].as_str().unwrap(), "SCHEMA_VIOLATION");
        assert!(
            data["message"].as_str().unwrap().contains("patient_id"),
            "message should mention the missing field, got: {}",
            data["message"]
        );
    }

    #[tokio::test]
    async fn submit_valid_payload_returns_200() {
        let (state, _dir) = make_state();
        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/submit")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"payload":{"x":1}}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 64 * 1024)
            .await
            .unwrap();
        let data: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(data["seq"].is_number());
        assert!(data["node_id"].is_string());
    }

    #[tokio::test]
    async fn error_response_always_has_error_and_message_fields() {
        let (state, _dir) = make_state();
        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/submit")
                    .header("content-type", "application/json")
                    .body(Body::from("{bad}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(resp.into_body(), 64 * 1024)
            .await
            .unwrap();
        let data: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(data["error"].is_string(), "error field must be present");
        assert!(data["message"].is_string(), "message field must be present");
    }

    #[tokio::test]
    async fn submit_with_all_fields_returns_200() {
        let (state, _dir) = make_state();
        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/submit")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"event_type":2,"payload":{"patient_id":"P-999","type":"discharge"}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 64 * 1024)
            .await
            .unwrap();
        let data: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(data["seq"].is_number(), "seq must be present");
        assert!(data["node_id"].is_string(), "node_id must be present");
    }
}
