//! Local web front end: serves the embedded page and bridges WebSocket
//! clients to the hub.

use std::{
    future::Future,
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    Router,
    extract::{
        Request, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use ttt_core::{Difficulty, Player};

use crate::hub::{HubHandle, StateView};

const INDEX_HTML: &str = include_str!("../web/index.html");
const APP_JS: &str = include_str!("../web/app.js");
const STYLE_CSS: &str = include_str!("../web/style.css");

const MAX_MESSAGE_BYTES: usize = 4 * 1024;
const MAX_MSGS_PER_SEC: u32 = 20;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    Move { cell: usize },
    NewGame,
    SetAi { player: Option<Player> },
    SetDifficulty { difficulty: Difficulty },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    State(StateView),
    Error { message: String },
}

#[derive(Clone)]
struct AppState {
    hub: HubHandle,
    /// Exact `Host` values we answer to (DNS-rebinding defense).
    allowed_hosts: Arc<[String]>,
    /// Exact `Origin` values allowed to open a WebSocket (CSWSH defense).
    allowed_origins: Arc<[String]>,
}

/// Serve on an already-bound listener until `shutdown` resolves.
///
/// Taking a listener (rather than an address) lets callers bind first,
/// report errors like "port in use" before starting the TUI, and use port 0
/// in tests.
pub async fn serve(
    listener: TcpListener,
    hub: HubHandle,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let addr = listener.local_addr()?;
    // Refuse to run on anything but loopback: there is no authentication.
    if !addr.ip().is_loopback() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("refusing to serve on non-loopback address {addr}"),
        ));
    }
    let app = router(hub, addr);
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
}

fn router(hub: HubHandle, addr: SocketAddr) -> Router {
    let port = addr.port();
    let hosts: Vec<String> = vec![format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    let origins = hosts
        .iter()
        .map(|h| format!("http://{h}"))
        .collect::<Vec<_>>();
    let state = AppState {
        hub,
        allowed_hosts: hosts.into(),
        allowed_origins: origins.into(),
    };
    Router::new()
        .route(
            "/",
            get(|| async { asset(INDEX_HTML, "text/html; charset=utf-8") }),
        )
        .route(
            "/app.js",
            get(|| async { asset(APP_JS, "text/javascript; charset=utf-8") }),
        )
        .route(
            "/style.css",
            get(|| async { asset(STYLE_CSS, "text/css; charset=utf-8") }),
        )
        .route("/ws", get(ws_upgrade))
        .layer(middleware::from_fn_with_state(state.clone(), guard_host))
        .with_state(state)
}

fn asset(body: &'static str, content_type: &'static str) -> Response {
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        body,
    )
        .into_response()
}

/// Reject unexpected Host headers and add security headers to every response.
async fn guard_host(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let host_ok = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|h| state.allowed_hosts.iter().any(|a| a == h));
    if !host_ok {
        return (StatusCode::MISDIRECTED_REQUEST, "unexpected Host header").into_response();
    }
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    // 'self' in connect-src covers same-origin ws:// in current browsers.
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'",
        ),
    );
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    res
}

async fn ws_upgrade(
    State(state): State<AppState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    // Browsers always send Origin on WebSocket handshakes. Without this check
    // any web page you visit could connect to localhost and play moves.
    if let Some(origin) = headers.get(header::ORIGIN) {
        let ok = origin
            .to_str()
            .is_ok_and(|o| state.allowed_origins.iter().any(|a| a == o));
        if !ok {
            tracing::warn!(?origin, "rejected WebSocket from foreign origin");
            return (StatusCode::FORBIDDEN, "origin not allowed").into_response();
        }
    }
    ws.max_message_size(MAX_MESSAGE_BYTES)
        .max_frame_size(MAX_MESSAGE_BYTES)
        .on_upgrade(move |socket| client_session(socket, state.hub))
}

async fn client_session(socket: WebSocket, hub: HubHandle) {
    let _guard = hub.join().await;
    let mut updates = hub.subscribe();
    let (mut sink, mut stream) = socket.split();

    // Fixed one-second window; crude but enough to stop a runaway client.
    let mut window_start = Instant::now();
    let mut window_count = 0u32;

    // Send the current state immediately; mark it seen so we don't resend.
    let first = ServerMsg::State(updates.borrow_and_update().clone());
    if send(&mut sink, &first).await.is_err() {
        return;
    }

    loop {
        tokio::select! {
            changed = updates.changed() => {
                if changed.is_err() {
                    break; // hub stopped
                }
                let msg = ServerMsg::State(updates.borrow_and_update().clone());
                if send(&mut sink, &msg).await.is_err() {
                    break;
                }
            }
            incoming = stream.next() => {
                let Some(Ok(msg)) = incoming else { break };
                let text = match msg {
                    Message::Text(t) => t,
                    Message::Close(_) => break,
                    _ => continue, // ping/pong handled by axum; binary ignored
                };
                if window_start.elapsed() >= Duration::from_secs(1) {
                    window_start = Instant::now();
                    window_count = 0;
                }
                window_count += 1;
                if window_count > MAX_MSGS_PER_SEC {
                    tracing::warn!("closing WebSocket: rate limit exceeded");
                    break;
                }
                if let Err(message) = dispatch(&hub, text.as_str()).await {
                    let reply = ServerMsg::Error { message };
                    if send(&mut sink, &reply).await.is_err() {
                        break;
                    }
                }
            }
        }
    }
}

/// Parse and execute one client message. Errors are user-facing strings;
/// they never include internal details.
async fn dispatch(hub: &HubHandle, text: &str) -> Result<(), String> {
    let msg: ClientMsg = serde_json::from_str(text).map_err(|_| "malformed message".to_string())?;
    let result = match msg {
        ClientMsg::Move { cell } => hub.play(cell).await,
        ClientMsg::NewGame => hub.new_game().await,
        ClientMsg::SetAi { player } => hub.set_ai(player).await,
        ClientMsg::SetDifficulty { difficulty } => hub.set_difficulty(difficulty).await,
    };
    result.map_err(|e| e.to_string())
}

async fn send<S>(sink: &mut S, msg: &ServerMsg) -> Result<(), ()>
where
    S: SinkExt<Message> + Unpin,
{
    let json = serde_json::to_string(msg).map_err(|_| ())?;
    sink.send(Message::Text(json.into())).await.map_err(|_| ())
}
