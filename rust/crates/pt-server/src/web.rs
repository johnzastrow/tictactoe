//! HTTP + WebSocket adapter (protocol §1–2): login/logout/me/health, the
//! `/ws` socket, the embedded web client, and the security controls
//! (session auth, same-origin checks, login throttling, frame size and
//! message-rate limits, security headers).

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, DefaultBodyLimit, Query, State};
use axum::http::{HeaderMap, HeaderValue, Request, StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use pt_core::model::{Role, User};
use pt_core::protocol::{
    ClientMsg, ErrorBody, HealthInfo, LoginRequest, LoginResponse, MAX_FRAME, PROTOCOL_VERSION, ResultMsg, ServerMsg,
    valid_client_id,
};
use rust_embed::RustEmbed;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;

use crate::auth;
use crate::hub::{Cmd, HubHandle, OUTBOUND_DEPTH, Outbound};

#[derive(RustEmbed)]
#[folder = "web-dist/"]
struct Assets;

pub const COOKIE: &str = "pt_session";

#[derive(Clone)]
pub struct AppState {
    hub: HubHandle,
    tls: bool,
    csp: Arc<str>,
    limiter: Arc<Mutex<Limiter>>,
}

impl AppState {
    pub fn new(hub: HubHandle, tls: bool) -> AppState {
        AppState {
            hub,
            tls,
            csp: Arc::from(build_csp()),
            limiter: Arc::new(Mutex::new(Limiter::default())),
        }
    }
}

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/api/health", get(health))
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/api/me", get(me))
        .layer(DefaultBodyLimit::max(4096));
    Router::new()
        .merge(api)
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
         img-src 'self' data:; connect-src 'self'; font-src 'self'; frame-ancestors 'none'; base-uri 'none'; \
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
        h.insert(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=31536000"),
        );
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

// ---- auth helpers ----------------------------------------------------------------

fn session_token(headers: &HeaderMap) -> Option<String> {
    if let Some(v) = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) {
        if let Some(t) = v.strip_prefix("Bearer ") {
            return Some(t.trim().to_string());
        }
    }
    let cookies = headers.get_all(header::COOKIE);
    for c in cookies.iter().filter_map(|v| v.to_str().ok()) {
        for part in c.split(';') {
            if let Some(t) = part.trim().strip_prefix(&format!("{COOKIE}=")) {
                return Some(t.to_string());
            }
        }
    }
    None
}

async fn authed(st: &AppState, headers: &HeaderMap) -> Option<(User, String)> {
    let token = session_token(headers)?;
    if token.is_empty() || token.len() > 128 {
        return None;
    }
    let hash = auth::token_hash(&token);
    st.hub.session(hash.clone()).await.map(|u| (u, hash))
}

fn cookie(st: &AppState, token: &str, max_age: i64) -> HeaderValue {
    let secure = if st.tls { "; Secure" } else { "" };
    HeaderValue::from_str(&format!("{COOKIE}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}{secure}"))
        .unwrap_or_else(|_| HeaderValue::from_static(""))
}

// ---- login throttling --------------------------------------------------------------

#[derive(Default)]
pub struct Limiter {
    failures_by_ip: HashMap<IpAddr, Vec<Instant>>,
    failures_by_user: HashMap<String, Vec<Instant>>,
}

impl Limiter {
    /// False if this IP (20 failures / minute) or username (5 failures /
    /// 15 minutes) is over its limit. Successful logins never count.
    fn allow(&mut self, ip: IpAddr, user: &str) -> bool {
        let now = Instant::now();
        let by_ip = self.failures_by_ip.entry(ip).or_default();
        by_ip.retain(|t| now.duration_since(*t) < Duration::from_secs(60));
        let by_user = self.failures_by_user.entry(user.to_string()).or_default();
        by_user.retain(|t| now.duration_since(*t) < Duration::from_secs(15 * 60));
        let ok = by_ip.len() < 20 && by_user.len() < 5;
        // bound memory if someone sprays usernames or addresses
        if self.failures_by_user.len() > 10_000 {
            self.failures_by_user.retain(|_, v| !v.is_empty());
        }
        if self.failures_by_ip.len() > 10_000 {
            self.failures_by_ip.retain(|_, v| !v.is_empty());
        }
        ok
    }
    fn failed(&mut self, ip: IpAddr, user: &str) {
        self.failures_by_ip.entry(ip).or_default().push(Instant::now());
        self.failures_by_user.entry(user.to_string()).or_default().push(Instant::now());
    }
    fn succeeded(&mut self, user: &str) {
        self.failures_by_user.remove(user);
    }
}

// ---- handlers ------------------------------------------------------------------------

async fn health() -> Json<HealthInfo> {
    Json(HealthInfo {
        ok: true,
        implementation: "rust".into(),
        version: pt_core::VERSION.into(),
        protocol: PROTOCOL_VERSION,
    })
}

async fn login(
    State(st): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Result<Json<LoginRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    if !origin_ok(&headers) {
        return err(StatusCode::FORBIDDEN, "cross-origin request refused");
    }
    let Ok(Json(req)) = body else {
        return err(StatusCode::BAD_REQUEST, "expected {\"username\", \"password\"}");
    };
    let username = req.username.trim().to_lowercase();
    if username.is_empty() || username.len() > 64 || req.password.len() > 256 {
        return err(StatusCode::UNAUTHORIZED, "invalid credentials");
    }
    if !st.limiter.lock().map(|mut l| l.allow(addr.ip(), &username)).unwrap_or(false) {
        tracing::warn!(ip = %addr.ip(), user = %username, "login throttled");
        return err(StatusCode::TOO_MANY_REQUESTS, "too many attempts");
    }
    let found = st.hub.login_lookup(username.clone()).await;
    let password = req.password.clone();
    // Verify off the async threads; unknown users still pay for a hash.
    let (user, ok) = {
        let hash = found.as_ref().map(|(_, h)| h.clone()).unwrap_or_else(|| auth::dummy_hash().to_string());
        let ok = tokio::task::spawn_blocking(move || auth::verify_password(&password, &hash))
            .await
            .unwrap_or(false);
        (found.map(|(u, _)| u), ok)
    };
    let user = match user {
        Some(u) if ok && u.active => u,
        _ => {
            if let Ok(mut l) = st.limiter.lock() {
                l.failed(addr.ip(), &username);
            }
            tracing::info!(ip = %addr.ip(), user = %username, "login failed");
            return err(StatusCode::UNAUTHORIZED, "invalid credentials");
        }
    };
    if let Ok(mut l) = st.limiter.lock() {
        l.succeeded(&username);
    }
    let token = auth::new_token();
    let client = if valid_client_id(&req.client) { req.client.clone() } else { String::new() };
    if !st.hub.create_session(auth::token_hash(&token), user.id, client).await {
        return err(StatusCode::INTERNAL_SERVER_ERROR, "internal error");
    }
    tracing::info!(ip = %addr.ip(), user = %user.username, "login");
    let mut res = Json(LoginResponse { token: token.clone(), user }).into_response();
    res.headers_mut().insert(header::SET_COOKIE, cookie(&st, &token, auth::SESSION_TTL_SECS));
    res
}

async fn logout(State(st): State<AppState>, headers: HeaderMap) -> Response {
    if !origin_ok(&headers) {
        return err(StatusCode::FORBIDDEN, "cross-origin request refused");
    }
    if let Some(token) = session_token(&headers) {
        st.hub.send(Cmd::Logout { token_hash: auth::token_hash(&token) });
    }
    let mut res = StatusCode::NO_CONTENT.into_response();
    res.headers_mut().insert(header::SET_COOKIE, cookie(&st, "", 0));
    res
}

async fn me(State(st): State<AppState>, headers: HeaderMap) -> Response {
    match authed(&st, &headers).await {
        Some((u, _)) => Json(u).into_response(),
        None => err(StatusCode::UNAUTHORIZED, "not logged in"),
    }
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
            // a precompressed copy made by `make web`, when the browser accepts it
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
            ([(header::CONTENT_TYPE, mime), (header::CACHE_CONTROL, cache.to_string()), (header::VARY, "Accept-Encoding".to_string())], f.data.into_owned()).into_response()
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
}

async fn ws_upgrade(
    State(st): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> Response {
    if !origin_ok(&headers) {
        return err(StatusCode::FORBIDDEN, "cross-origin WebSocket refused");
    }
    let Some((user, session)) = authed(&st, &headers).await else {
        return err(StatusCode::UNAUTHORIZED, "not logged in");
    };
    let client = if valid_client_id(&q.client) { q.client } else { "unknown".into() };
    ws.max_message_size(MAX_FRAME)
        .max_frame_size(MAX_FRAME)
        .on_upgrade(move |socket| run_socket(st, socket, user, client, session))
}

/// Token bucket: 50 msg/s sustained, burst 100 (protocol §1).
struct Bucket {
    tokens: f64,
    last: Instant,
}

impl Bucket {
    fn take(&mut self) -> bool {
        let now = Instant::now();
        self.tokens = (self.tokens + now.duration_since(self.last).as_secs_f64() * 50.0).min(100.0);
        self.last = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

async fn run_socket(st: AppState, socket: WebSocket, user: User, client: String, session: String) {
    let (out_tx, mut out_rx) = mpsc::channel::<Outbound>(OUTBOUND_DEPTH);
    let Some(conn) = st.hub.connect(user, client, session, out_tx.clone()).await else {
        return;
    };
    let (mut sink, mut stream) = socket.split();

    // Writer: hub output + keepalive pings. Ends when the hub drops our sender.
    let mut writer = tokio::spawn(async move {
        let mut ping = tokio::time::interval(Duration::from_secs(30));
        ping.tick().await;
        loop {
            tokio::select! {
                msg = out_rx.recv() => match msg {
                    Some(Outbound::Text(t)) => {
                        if sink.send(Message::Text(t.as_ref().into())).await.is_err() { break; }
                    }
                    None => {
                        let _ = sink.send(Message::Close(Some(CloseFrame { code: 1008, reason: "session ended".into() }))).await;
                        break;
                    }
                },
                _ = ping.tick() => {
                    if sink.send(Message::Ping(Vec::new().into())).await.is_err() { break; }
                }
            }
        }
    });
    drop(out_tx); // only the hub holds a sender now

    let mut bucket = Bucket { tokens: 100.0, last: Instant::now() };
    let close_reason: Option<&str> = loop {
        let msg = tokio::select! {
            m = stream.next() => m,
            _ = &mut writer => break None,
        };
        let Some(Ok(msg)) = msg else { break None };
        let text = match msg {
            Message::Text(t) => t,
            Message::Close(_) => break None,
            Message::Binary(_) => break Some("binary frames are not supported"),
            _ => continue,
        };
        if !bucket.take() {
            break Some("rate limit exceeded");
        }
        match serde_json::from_str::<ClientMsg>(text.as_str()) {
            Ok(ClientMsg::SetPassword { req, user_id, password, old_password }) => {
                set_password(&st, conn, req, user_id, password, old_password).await;
            }
            Ok(msg) => st.hub.send(Cmd::Client { conn, msg }),
            Err(e) => {
                let m = ServerMsg::Error { code: "bad_request".into(), message: format!("unreadable message: {e}") };
                st.hub.send(Cmd::Reply { conn, msg: m });
            }
        }
    };
    st.hub.send(Cmd::Disconnect { conn });
    if let Some(reason) = close_reason {
        tracing::warn!(conn, reason, "closing socket");
    }
    writer.abort();
}

/// set_password (protocol §5): authorise, verify the old password and hash
/// the new one here, off the hub thread; the hub re-checks and stores it.
async fn set_password(st: &AppState, conn: u64, req: u64, target: i64, password: String, old: Option<String>) {
    let fail = |e: ErrorBody| {
        let res = ResultMsg { req, ok: false, error: Some(e), ..Default::default() };
        st.hub.send(Cmd::Reply { conn, msg: ServerMsg::Result(res) });
    };
    let Some((actor, hash)) = st.hub.password_info(conn, target).await else { return };
    if actor.id != target && actor.role != Role::Admin {
        return fail(ErrorBody::new("forbidden", "you can only change your own password"));
    }
    let Some(hash) = hash else {
        return fail(ErrorBody::new("not_found", "no such user"));
    };
    if !(8..=256).contains(&password.chars().count()) {
        return fail(ErrorBody::new("invalid", "password must be 8–256 characters"));
    }
    if actor.role != Role::Admin {
        let old = old.unwrap_or_default();
        let ok = tokio::task::spawn_blocking(move || auth::verify_password(&old, &hash)).await.unwrap_or(false);
        if !ok {
            return fail(ErrorBody::new("invalid", "current password is wrong"));
        }
    }
    match tokio::task::spawn_blocking(move || auth::hash_password(&password)).await {
        Ok(Ok(h)) => st.hub.send(Cmd::SetPasswordHash { conn, req, target, hash: h }),
        _ => fail(ErrorBody::new("internal", "internal error")),
    }
}
