//! HTTP + WebSocket adapter (protocol §1): `/api/health`, the `/ws` socket,
//! the embedded web client, and the security controls (same-origin check,
//! name/client validation, socket cap, frame size and message-rate limits,
//! security headers).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::extract::rejection::QueryRejection;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue, Request, StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use base64::Engine;
use demo_core::protocol::{
    CLOSE_POLICY, ClientMsg, HealthInfo, MAX_FRAME, MAX_SOCKETS, PING_INTERVAL_SECS, PROTOCOL_VERSION, RATE_BURST, RATE_PER_SEC,
    ServerMsg, clean_name, valid_client_id,
};
use futures_util::{SinkExt, StreamExt};
use rust_embed::RustEmbed;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::sync::{mpsc, oneshot};

use crate::hub::{Cmd, HubHandle, OUTBOUND_DEPTH, Outbound};

#[derive(RustEmbed)]
#[folder = "web-dist/"]
struct Assets;

#[derive(Clone)]
pub struct AppState {
    hub: HubHandle,
    tls: bool,
    csp: Arc<str>,
    sockets: Arc<AtomicUsize>,
}

impl AppState {
    pub fn new(hub: HubHandle, tls: bool) -> AppState {
        AppState { hub, tls, csp: Arc::from(build_csp()), sockets: Arc::new(AtomicUsize::new(0)) }
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/ws", get(ws_upgrade))
        .fallback(get(static_file))
        .layer(middleware::from_fn_with_state(state.clone(), security_headers))
        .with_state(state)
}

// ---- security headers ----------------------------------------------------------

/// CSP with the hashes of any inline `<script>` in index.html (trunk's WASM loader).
fn build_csp() -> String {
    let mut hashes = String::new();
    if let Some(f) = Assets::get("index.html") {
        let html = String::from_utf8_lossy(&f.data);
        let mut rest = html.as_ref();
        while let Some(start) = rest.find("<script") {
            let after = &rest[start..];
            let Some(open_end) = after.find('>') else { break };
            let tag = &after[..open_end];
            let body_start = open_end + 1;
            let Some(close) = after[body_start..].find("</script>") else { break };
            let body = &after[body_start..body_start + close];
            if !tag.contains(" src=") && !body.trim().is_empty() {
                let digest = Sha256::digest(body.as_bytes());
                hashes.push_str(&format!(" 'sha256-{}'", base64::engine::general_purpose::STANDARD.encode(digest)));
            }
            rest = &after[body_start + close..];
        }
    }
    format!(
        "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'{hashes}; style-src 'self' 'unsafe-inline'; \
         connect-src 'self'; img-src 'self' data:; font-src 'self'; frame-ancestors 'none'; base-uri 'none'; \
         form-action 'self'; object-src 'none'"
    )
}

async fn security_headers(State(st): State<AppState>, req: Request<Body>, next: Next) -> Response {
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("same-origin"));
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    if let Ok(v) = HeaderValue::from_str(&st.csp) {
        h.insert(header::CONTENT_SECURITY_POLICY, v);
    }
    if st.tls {
        h.insert(header::STRICT_TRANSPORT_SECURITY, HeaderValue::from_static("max-age=31536000"));
    }
    res
}

/// Same-origin check: an `Origin` header, if present, must match `Host`.
fn origin_ok(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN) else { return true };
    let (Ok(origin), Some(Ok(host))) = (origin.to_str(), headers.get(header::HOST).map(|h| h.to_str())) else {
        return false;
    };
    origin == format!("http://{host}") || origin == format!("https://{host}")
}

fn err(status: StatusCode, msg: &str) -> Response {
    (status, Json(serde_json::json!({ "error": msg }))).into_response()
}

// ---- handlers ------------------------------------------------------------------------

async fn health() -> Json<HealthInfo> {
    Json(HealthInfo { ok: true, implementation: "rust".into(), version: demo_core::VERSION.into(), protocol: PROTOCOL_VERSION })
}

async fn static_file(uri: Uri, headers: HeaderMap) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    match Assets::get(path) {
        Some(f) => {
            let mime = f.metadata.mimetype().to_string();
            let cache = if path == "index.html" { "no-cache" } else { "public, max-age=3600" };
            let gzip_ok = headers
                .get(header::ACCEPT_ENCODING)
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| v.contains("gzip"));
            // a precompressed copy made by `make demo-web`, when the browser accepts it
            if let Some(gz) = gzip_ok.then(|| Assets::get(&format!("{path}.gz"))).flatten() {
                return (
                    [
                        (header::CONTENT_TYPE, mime),
                        (header::CACHE_CONTROL, cache.to_string()),
                        (header::CONTENT_ENCODING, "gzip".to_string()),
                        (header::VARY, "Accept-Encoding".to_string()),
                    ],
                    gz.data.into_owned(),
                )
                    .into_response();
            }
            (
                [(header::CONTENT_TYPE, mime), (header::CACHE_CONTROL, cache.to_string()), (header::VARY, "Accept-Encoding".to_string())],
                f.data.into_owned(),
            )
                .into_response()
        }
        // client-side routes fall back to the app shell
        None if !path.contains('.') => match Assets::get("index.html") {
            Some(f) => ([(header::CONTENT_TYPE, "text/html; charset=utf-8".to_string())], f.data.into_owned()).into_response(),
            None => StatusCode::NOT_FOUND.into_response(),
        },
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

#[derive(Deserialize)]
struct WsQuery {
    #[serde(default)]
    client: String,
    #[serde(default)]
    name: String,
}

/// Holds one of the [`MAX_SOCKETS`] slots; dropping it frees the slot, also
/// when an upgrade never completes.
struct Slot(Arc<AtomicUsize>);

impl Slot {
    fn take(count: &Arc<AtomicUsize>) -> Option<Slot> {
        let slot = Slot(count.clone());
        // over the cap: dropping `slot` gives the increment back
        (count.fetch_add(1, Ordering::SeqCst) < MAX_SOCKETS).then_some(slot)
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

async fn ws_upgrade(
    State(st): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<WsQuery>, QueryRejection>,
    ws: WebSocketUpgrade,
) -> Response {
    if !origin_ok(&headers) {
        return err(StatusCode::FORBIDDEN, "cross-origin WebSocket refused");
    }
    let Ok(Query(q)) = query else {
        return err(StatusCode::BAD_REQUEST, "bad query");
    };
    if !valid_client_id(&q.client) {
        return err(StatusCode::BAD_REQUEST, "client must match [a-z0-9-]{1,24}");
    }
    let Some(name) = clean_name(&q.name) else {
        return err(StatusCode::BAD_REQUEST, "name must be 1–24 letters, digits, space, _ . - starting with a letter");
    };
    let Some(slot) = Slot::take(&st.sockets) else {
        return err(StatusCode::SERVICE_UNAVAILABLE, "too many connections");
    };
    // Frames over MAX_FRAME are refused by us (close 1008) rather than by the
    // codec, so the codec limit sits above ours.
    ws.max_message_size(MAX_FRAME * 4)
        .max_frame_size(MAX_FRAME * 4)
        .on_upgrade(move |socket| run_socket(st, socket, name, q.client, slot))
}

/// Token bucket: 20 msg/s sustained, burst 40 (protocol §1).
struct Bucket {
    tokens: f64,
    last: Instant,
}

impl Bucket {
    fn take(&mut self) -> bool {
        let now = Instant::now();
        self.tokens = (self.tokens + now.duration_since(self.last).as_secs_f64() * RATE_PER_SEC).min(RATE_BURST);
        self.last = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

async fn run_socket(st: AppState, socket: WebSocket, name: String, client: String, _slot: Slot) {
    let (out_tx, mut out_rx) = mpsc::channel::<Outbound>(OUTBOUND_DEPTH);
    let Some(conn) = st.hub.connect(name, client, out_tx).await else {
        return;
    };
    let (mut sink, mut stream) = socket.split();
    let (close_tx, mut close_rx) = oneshot::channel::<(u16, &'static str)>();

    // Writer: hub output, keepalive pings, and the close frame for a policy
    // violation. Ends when the hub drops our sender (slow client).
    let mut writer = tokio::spawn(async move {
        let mut ping = tokio::time::interval(Duration::from_secs(PING_INTERVAL_SECS));
        ping.tick().await;
        loop {
            tokio::select! {
                biased;
                close = &mut close_rx => {
                    if let Ok((code, reason)) = close {
                        let _ = sink.send(Message::Close(Some(CloseFrame { code, reason: reason.into() }))).await;
                    }
                    break;
                }
                msg = out_rx.recv() => match msg {
                    Some(Outbound::Text(t)) => {
                        if sink.send(Message::Text(t.as_ref().into())).await.is_err() { break; }
                    }
                    None => {
                        let _ = sink.send(Message::Close(Some(CloseFrame { code: CLOSE_POLICY, reason: "too slow".into() }))).await;
                        break;
                    }
                },
                _ = ping.tick() => {
                    if sink.send(Message::Ping(Vec::new().into())).await.is_err() { break; }
                }
            }
        }
    });

    let mut bucket = Bucket { tokens: RATE_BURST, last: Instant::now() };
    let violation: Option<(u16, &'static str)> = loop {
        let msg = tokio::select! {
            m = stream.next() => m,
            _ = &mut writer => break None,
        };
        let Some(Ok(msg)) = msg else { break None };
        let text = match msg {
            Message::Text(t) => t,
            Message::Close(_) => break None,
            Message::Binary(_) => break Some((1003, "text frames only")),
            _ => continue, // ping/pong are handled by the codec
        };
        if text.len() > MAX_FRAME {
            break Some((CLOSE_POLICY, "message too big"));
        }
        if !bucket.take() {
            break Some((CLOSE_POLICY, "rate limit exceeded"));
        }
        match serde_json::from_str::<ClientMsg>(text.as_str()) {
            Ok(msg) => st.hub.send(Cmd::Client { conn, msg }),
            Err(_) => st.hub.send(Cmd::Reply { conn, msg: ServerMsg::Error { message: "bad message".into() } }),
        }
    };
    if let Some((code, reason)) = violation {
        tracing::warn!(conn, reason, "closing socket");
        let _ = close_tx.send((code, reason));
        // let the writer flush the close frame, but don't wait forever
        let _ = tokio::time::timeout(Duration::from_secs(2), &mut writer).await;
        // Drain what the client already sent until its close reply: dropping
        // the socket with unread input makes TCP reset the connection, and
        // the client could lose our close frame (and its code).
        let _ = tokio::time::timeout(Duration::from_secs(1), async {
            while let Some(Ok(m)) = stream.next().await {
                if matches!(m, Message::Close(_)) {
                    break;
                }
            }
        })
        .await;
    }
    writer.abort();
    st.hub.send(Cmd::Disconnect { conn });
}
