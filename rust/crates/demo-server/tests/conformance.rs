//! Protocol conformance suite (docs/demo/protocol.md).
//!
//! By default it starts an in-process server on a fresh in-memory database.
//! To check another implementation (the Go server) or a running binary,
//! point it at a plain-http server:
//!
//!   DEMO_CONFORM_URL=http://127.0.0.1:8790 \
//!     cargo test -p demo-server --test conformance
//!
//! The suite changes and resets control values on that server. Tests run
//! one at a time (a shared lock) because they share every key; they only
//! assume their own connections exist, so other clients may be connected.

use std::collections::BTreeMap;
use std::time::Duration;

use demo_core::dataset::peer_color;
use demo_core::keys::{self, KEYS};
use demo_core::protocol::*;
use demo_core::{Entry, Values};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, MutexGuard, OnceCell};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

static TARGET: OnceCell<String> = OnceCell::const_new();
static LOCK: Mutex<()> = Mutex::const_new(());

/// host:port of the server under test.
async fn target() -> &'static str {
    TARGET
        .get_or_init(|| async {
            if let Ok(url) = std::env::var("DEMO_CONFORM_URL") {
                return url.trim_start_matches("http://").trim_end_matches('/').to_string();
            }
            // In-process server on its own runtime thread so it outlives each test's runtime.
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async move {
                    let store = demo_server::Store::open_memory().unwrap();
                    let app = demo_server::app(store, false).unwrap();
                    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                    tx.send(l.local_addr().unwrap()).unwrap();
                    axum::serve(l, app).await.unwrap();
                });
            });
            rx.recv().unwrap().to_string()
        })
        .await
}

async fn lock() -> MutexGuard<'static, ()> {
    LOCK.lock().await
}

/// Minimal HTTP/1.1 GET (enough for health and static files; no TLS).
async fn get(path: &str) -> (u16, Vec<(String, String)>, String) {
    let addr = target().await;
    let mut s = TcpStream::connect(addr).await.unwrap();
    let req = format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    s.write_all(req.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).await.unwrap();
    let text = String::from_utf8_lossy(&buf).to_string();
    let (head, rest) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let mut lines = head.lines();
    let status: u16 = lines.next().unwrap().split_whitespace().nth(1).unwrap().parse().unwrap();
    let hdrs = lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_lowercase(), v.trim().to_string())).collect();
    // de-chunk if needed
    let body = if head.to_lowercase().contains("transfer-encoding: chunked") {
        let mut out = String::new();
        let mut r = rest;
        while let Some((size, tail)) = r.split_once("\r\n") {
            let n = usize::from_str_radix(size.trim(), 16).unwrap_or(0);
            if n == 0 {
                break;
            }
            out.push_str(&tail[..n]);
            r = &tail[n + 2..];
        }
        out
    } else {
        rest.to_string()
    };
    (status, hdrs, body)
}

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>;

/// Percent-encode a query value (names may contain spaces).
fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-_.".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

async fn ws_url(query: &str) -> String {
    format!("ws://{}/ws?{query}", target().await)
}

/// The handshake error (e.g. "HTTP error: 400 Bad Request") for a refused upgrade.
async fn refused(query: &str, origin: Option<&str>) -> String {
    let mut req = ws_url(query).await.into_client_request().unwrap();
    if let Some(o) = origin {
        req.headers_mut().insert("Origin", o.parse().unwrap());
    }
    match tokio_tungstenite::connect_async(req).await {
        Ok(_) => panic!("upgrade with {query} / {origin:?} should be refused"),
        Err(e) => e.to_string(),
    }
}

struct Client {
    ws: Ws,
    req: u64,
    conn: u64,
    name: String,
    welcome: ServerMsg,
    /// Our view of every key's value, kept current from `changed` by the core store.
    values: Values,
    /// The latest version we know per key (welcome, changed, our own acks).
    versions: BTreeMap<String, u64>,
}

impl Client {
    async fn connect(name: &str) -> Client {
        let url = ws_url(&format!("client=conform&name={}", enc(name))).await;
        let mut req = url.into_client_request().unwrap();
        // a browser-like same-origin header must be accepted
        let origin = format!("http://{}", target().await);
        req.headers_mut().insert("Origin", origin.parse().unwrap());
        let (ws, _) = tokio_tungstenite::connect_async(req).await.unwrap();
        let mut c = Client { ws, req: 1000, conn: 0, name: String::new(), welcome: ServerMsg::Pong, values: Values::new(), versions: BTreeMap::new() };
        let first = c.recv().await;
        match &first {
            ServerMsg::Welcome { protocol, you, .. } => {
                assert_eq!(*protocol, PROTOCOL_VERSION);
                c.conn = you.conn;
                c.name = you.name.clone();
            }
            other => panic!("expected welcome first, got {other:?}"),
        }
        c.welcome = first;
        c
    }

    async fn recv(&mut self) -> ServerMsg {
        loop {
            let msg = tokio::time::timeout(Duration::from_secs(5), self.ws.next())
                .await
                .expect("timed out waiting for a message")
                .expect("socket closed")
                .expect("socket error");
            if let Message::Text(t) = msg {
                let m: ServerMsg = serde_json::from_str(t.as_str()).unwrap_or_else(|e| panic!("bad server message {t}: {e}"));
                match &m {
                    ServerMsg::Welcome { values, .. } => {
                        self.versions = values.iter().map(|(k, e)| (k.clone(), e.version)).collect();
                    }
                    ServerMsg::Changed { key, version, .. } => {
                        let v = self.versions.entry(key.clone()).or_default();
                        *v = (*v).max(*version);
                    }
                    _ => {}
                }
                self.values.handle(m.clone());
                return m;
            }
        }
    }

    /// Wait for a message matching `pred`, skipping others (presence etc.).
    async fn wait<T>(&mut self, mut pred: impl FnMut(&ServerMsg) -> Option<T>) -> T {
        loop {
            let m = self.recv().await;
            if let Some(v) = pred(&m) {
                return v;
            }
        }
    }

    async fn send(&mut self, msg: &ClientMsg) {
        self.send_text(&serde_json::to_string(msg).unwrap()).await;
    }

    async fn send_text(&mut self, text: &str) {
        self.ws.send(Message::Text(text.to_string().into())).await.unwrap();
    }

    async fn call(&mut self, make: impl FnOnce(u64) -> ClientMsg) -> Ack {
        self.req += 1;
        let req = self.req;
        self.send(&make(req)).await;
        self.wait(|m| match m {
            ServerMsg::Ack(a) if a.req == req => Some(a.clone()),
            _ => None,
        })
        .await
    }

    fn version(&self, key: &str) -> u64 {
        self.versions.get(key).copied().unwrap_or(0)
    }

    /// `set` based on the version we hold.
    async fn set(&mut self, key: &str, value: Value) -> Ack {
        let base = self.version(key);
        self.set_raw(key, base, value, false).await
    }

    async fn set_raw(&mut self, key: &str, base: u64, value: Value, force: bool) -> Ack {
        let k = key.to_string();
        let ack = self.call(|req| ClientMsg::Set { req, key: k, base, value, force }).await;
        if let (true, Some(v)) = (ack.ok, ack.version) {
            let cur = self.versions.entry(key.to_string()).or_default();
            *cur = (*cur).max(v);
        }
        ack
    }

    /// `reset`; on success, also reads the `changed` of every reset key
    /// (they reach the sender too), so later sets use fresh versions.
    async fn reset(&mut self, keys: &[&str]) -> Ack {
        let list: Vec<String> = keys.iter().map(|k| k.to_string()).collect();
        let ack = self.call(|req| ClientMsg::Reset { req, keys: list }).await;
        if ack.ok {
            let mut left: std::collections::BTreeSet<String> =
                if keys.is_empty() { KEYS.iter().map(|k| k.key.to_string()).collect() } else { keys.iter().map(|k| k.to_string()).collect() };
            while !left.is_empty() {
                let k = self
                    .wait(|m| match m {
                        ServerMsg::Changed { key, .. } => Some(key.clone()),
                        _ => None,
                    })
                    .await;
                left.remove(&k);
            }
        }
        ack
    }

    /// The next `changed` for `key`.
    async fn changed(&mut self, key: &str) -> (Value, u64, String, u64, String) {
        self.wait(|m| match m {
            ServerMsg::Changed { key: k, value, version, by, conn, at } if k == key => {
                Some((value.clone(), *version, by.clone(), *conn, at.clone()))
            }
            _ => None,
        })
        .await
    }

    /// The `changed` for `key` that brings it to `version` (skipping older ones).
    async fn changed_to(&mut self, key: &str, version: u64) -> Value {
        self.wait(|m| match m {
            ServerMsg::Changed { key: k, value, version: v, .. } if k == key && *v >= version => {
                assert_eq!(*v, version, "{key}: expected version {version}");
                Some(value.clone())
            }
            _ => None,
        })
        .await
    }

    /// The next presence list satisfying `pred`.
    async fn presence(&mut self, mut pred: impl FnMut(&[Peer]) -> bool) -> Vec<Peer> {
        self.wait(|m| match m {
            ServerMsg::Presence { peers } if pred(peers) => Some(peers.clone()),
            _ => None,
        })
        .await
    }

    /// Read until the server closes; returns the close code.
    async fn close_code(&mut self) -> Option<u16> {
        loop {
            match tokio::time::timeout(Duration::from_secs(5), self.ws.next()).await.expect("timed out waiting for close") {
                Some(Ok(Message::Close(frame))) => return frame.map(|f| u16::from(f.code)),
                Some(Ok(_)) => continue,
                Some(Err(_)) | None => return None,
            }
        }
    }
}

fn is_timestamp(s: &str) -> bool {
    demo_core::date::parse_rfc3339(s).is_some() && s.len() == 20 && s.ends_with('Z')
}

// ---- HTTP -----------------------------------------------------------------------

#[tokio::test]
async fn health_and_headers() {
    let (status, headers, body) = get("/api/health").await;
    assert_eq!(status, 200);
    let h: HealthInfo = serde_json::from_str(&body).unwrap();
    assert!(h.ok);
    assert!(h.implementation == "rust" || h.implementation == "go", "impl {}", h.implementation);
    assert_eq!(h.version, "0.5.0");
    assert_eq!(h.protocol, 1);
    let hdr = |k: &str| headers.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    assert_eq!(hdr("x-content-type-options").as_deref(), Some("nosniff"));
    assert_eq!(hdr("referrer-policy").as_deref(), Some("same-origin"));
    assert_eq!(hdr("x-frame-options").as_deref(), Some("DENY"));
    let csp = hdr("content-security-policy").expect("CSP header");
    for part in ["default-src 'self'", "'wasm-unsafe-eval'", "connect-src 'self'", "frame-ancestors 'none'"] {
        assert!(csp.contains(part), "CSP lacks {part}: {csp}");
    }
    // the web client (or its placeholder) at /
    let (status, headers, body) = get("/").await;
    assert_eq!(status, 200);
    assert!(headers.iter().any(|(k, v)| k == "content-type" && v.starts_with("text/html")));
    assert!(body.to_lowercase().contains("<!doctype html"));
}

// ---- handshake rules ------------------------------------------------------------

#[tokio::test]
async fn bad_name_or_client_is_400() {
    let long = "a".repeat(25);
    for q in [
        "client=conform".to_string(),
        "name=alice".to_string(),
        "client=conform&name=".to_string(),
        "client=conform&name=1abc".to_string(),
        "client=conform&name=%5Fx".to_string(),
        format!("client=conform&name={long}"),
        "client=conform&name=al%21ce".to_string(),
        "client=UPPER&name=alice".to_string(),
        "client=&name=alice".to_string(),
        format!("client={long}&name=alice"),
        "client=a_b&name=alice".to_string(),
    ] {
        let e = refused(&q, None).await;
        assert!(e.contains("400"), "{q}: expected 400, got {e}");
    }
}

#[tokio::test]
async fn foreign_origin_is_403() {
    for origin in ["http://evil.example", "https://evil.example", "null"] {
        let e = refused("client=conform&name=alice", Some(origin)).await;
        assert!(e.contains("403"), "{origin}: expected 403, got {e}");
    }
}

#[tokio::test]
async fn name_is_trimmed_and_may_contain_spaces() {
    let _g = lock().await;
    let c = Client::connect("  Conform Bot ").await;
    assert_eq!(c.name, "Conform Bot");
}

// ---- welcome --------------------------------------------------------------------

#[tokio::test]
async fn welcome_has_every_key_and_defaults_after_reset() {
    let _g = lock().await;
    let mut a = Client::connect("wanda").await;
    let ack = a.reset(&[]).await;
    assert!(ack.ok, "reset all: {ack:?}");
    // one changed per key reached the sender too (read by `reset`)
    for k in KEYS {
        assert_eq!(a.values.value(k.key), keys::default_value(k.key).as_ref(), "{}", k.key);
        assert_eq!(a.values.entry(k.key).unwrap().by, "wanda");
    }
    let b = Client::connect("walter").await;
    let ServerMsg::Welcome { server, you, values, .. } = &b.welcome else { unreachable!() };
    assert!(server.implementation == "rust" || server.implementation == "go");
    assert_eq!(server.version, "0.5.0");
    assert_eq!(you.name, "walter");
    assert_eq!(you.color, peer_color(you.conn));
    assert!(you.conn > a.conn, "conn ids increase");
    assert_eq!(values.len(), KEYS.len(), "welcome must carry exactly every key");
    for k in KEYS {
        let e: &Entry = values.get(k.key).unwrap_or_else(|| panic!("welcome lacks {}", k.key));
        assert_eq!(Some(&e.value), keys::default_value(k.key).as_ref(), "{} default", k.key);
        assert!(e.version >= 1, "{} version", k.key);
        assert!(is_timestamp(&e.at), "{} at {:?}", k.key, e.at);
    }
}

#[tokio::test]
async fn presence_follows_welcome() {
    let _g = lock().await;
    let mut a = Client::connect("pia").await;
    let me = a.conn;
    let peers = a.presence(|p| p.iter().any(|x| x.conn == me)).await;
    let mine = peers.iter().find(|p| p.conn == me).unwrap();
    assert_eq!((mine.name.as_str(), mine.client.as_str()), ("pia", "conform"));
    assert_eq!(mine.color, peer_color(me));
}

// ---- set ------------------------------------------------------------------------

#[tokio::test]
async fn set_acks_then_broadcasts_to_everyone() {
    let _g = lock().await;
    let mut a = Client::connect("alice").await;
    let mut b = Client::connect("bob").await;
    let before = a.version("select.status");
    let ack = a.set("select.status", json!("done")).await;
    assert!(ack.ok, "{ack:?}");
    assert_eq!(ack.version, Some(before + 1));
    let (v, ver, by, conn, at) = a.changed("select.status").await;
    assert_eq!((v, ver, by.as_str(), conn), (json!("done"), before + 1, "alice", a.conn));
    assert!(is_timestamp(&at), "at {at}");
    let (v, ver, by, conn, _) = b.changed("select.status").await;
    assert_eq!((v, ver, by.as_str(), conn), (json!("done"), before + 1, "alice", a.conn));

    // the server normalises before storing and broadcasting
    let ack = b.set("text.code", json!("  abc-123 ")).await;
    assert!(ack.ok, "{ack:?}");
    assert_eq!(a.changed("text.code").await.0, json!("ABC-123"));
    let ack = b.set("tags.tags", json!(["Zed", "alpha", "zed"])).await;
    assert!(ack.ok, "{ack:?}");
    assert_eq!(a.changed("tags.tags").await.0, json!(["alpha", "zed"]));
    let ack = b.set("number.hours", json!(7.256)).await;
    assert!(ack.ok);
    assert_eq!(a.changed("number.hours").await.0.as_f64(), Some(7.26));
    let ack = b.set("choice.channels", json!(["slack", "sms", "email"])).await;
    assert!(ack.ok);
    assert_eq!(a.changed("choice.channels").await.0, json!(["email", "sms", "slack"]));
    let ack = b.set("text.email", json!(" Ada@Example.COM")).await;
    assert!(ack.ok);
    assert_eq!(a.changed("text.email").await.0, json!("ada@example.com"));

    // the same value again still bumps the version
    let v1 = b.version("select.status");
    let ack = b.set("select.status", json!("done")).await;
    assert_eq!(ack.version, Some(v1 + 1));
    assert!(b.reset(&["select.status", "text.code", "tags.tags", "number.hours", "choice.channels", "text.email"]).await.ok);
}

#[tokio::test]
async fn stale_base_is_a_conflict_and_force_overrides() {
    let _g = lock().await;
    let mut a = Client::connect("alice").await;
    let mut b = Client::connect("bob").await;
    assert!(a.set("select.priority", json!("low")).await.ok);
    b.changed("select.priority").await;
    let cur = b.version("select.priority");
    // b edits from an older version
    let ack = b.set_raw("select.priority", cur - 1, json!("critical"), false).await;
    assert!(!ack.ok);
    assert!(ack.conflict, "{ack:?}");
    let current = ack.current.expect("conflict carries current");
    assert_eq!((current.value, current.version, current.by.as_str()), (json!("low"), cur, "alice"));
    // nothing changed: a fresh connection still sees "low" at the same version
    let c = Client::connect("carol").await;
    assert_eq!(c.values.str("select.priority"), "low");
    assert_eq!(c.version("select.priority"), cur);
    // force skips the check
    let ack = b.set_raw("select.priority", 0, json!("critical"), true).await;
    assert!(ack.ok, "{ack:?}");
    assert_eq!(ack.version, Some(cur + 1));
    assert_eq!(a.changed_to("select.priority", cur + 1).await, json!("critical"));
    // force does not skip validation
    let ack = b.set_raw("select.priority", 0, json!("urgent"), true).await;
    assert!(!ack.ok && !ack.conflict && ack.error.is_some(), "{ack:?}");
    assert!(b.reset(&["select.priority"]).await.ok);
}

#[tokio::test]
async fn invalid_values_are_refused() {
    let _g = lock().await;
    let mut a = Client::connect("vera").await;
    let ack = a.set_raw("no.such.key", 1, json!(1), false).await;
    assert!(!ack.ok && !ack.conflict);
    assert_eq!(ack.error.as_deref(), Some("unknown key"));
    let cases: Vec<(&str, Value)> = vec![
        ("text.name", json!("   ")),
        ("text.name", json!("x".repeat(81))),
        ("text.email", json!("not-an-email")),
        ("text.code", json!("AB-12")),
        ("number.hours", json!(1000.5)),
        ("number.hours", json!("12")),
        ("number.qty", json!(100)),
        ("number.qty", json!(2.5)),
        ("number.percent", json!(-1)),
        ("date.start", json!("2026-02-30")),
        ("date.due", json!("tomorrow")),
        ("select.status", json!("finished")),
        ("combo.owner", json!("mallory")),
        ("tags.tags", json!(["has space"])),
        ("tags.tags", json!((0..13).map(|i| format!("t{i}")).collect::<Vec<_>>())),
        ("textarea.notes", json!("x".repeat(5001))),
        ("choice.notify", json!("yes")),
        ("choice.channels", json!(["fax"])),
        ("choice.size", json!("xxl")),
        ("cascade.pick", json!({"program": "Tailspin Air", "project": "POS refresh", "phase": null})),
        ("cascade.pick", json!({"program": null, "project": "POS refresh", "phase": null})),
        ("grid.rows", json!([{"id": 1, "item": "a", "qty": 1, "price": 1, "status": "todo", "due": null},
                             {"id": 1, "item": "b", "qty": 1, "price": 1, "status": "todo", "due": null}])),
        ("grid.rows", json!([{"id": 1, "item": "a", "qty": 10000, "price": 1, "status": "todo", "due": null}])),
        ("menus.files", json!([{"name": "a", "starred": false}, {"name": "a", "starred": true}])),
        ("palette.last", json!("x".repeat(81))),
        ("palette.last", json!("x\u{1b}[31m")),
        ("text.name", json!("Ada\u{7}")),
        ("textarea.notes", json!("clear\u{1b}[2J")),
        ("dialog.profile", json!({"name": "Grace", "email": ""})),
        ("charts.series", json!([1, 2, 3])),
        ("charts.series", json!([101, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])),
        ("kanban.cards", json!([{"id": 1, "title": "x", "col": "backlog"}])),
        ("gantt.bars", json!([{"id": 1, "label": "x", "start": "2026-10-19", "days": 0}])),
    ];
    for (key, value) in cases {
        let before = a.version(key);
        let ack = a.set(key, value.clone()).await;
        assert!(!ack.ok, "{key} = {value} must be refused");
        assert!(!ack.conflict, "{key}: an invalid value is not a conflict");
        assert!(ack.error.as_deref().is_some_and(|e| !e.is_empty()), "{key}: needs an error message");
        assert_eq!(a.version(key), before);
    }
}

#[tokio::test]
async fn due_must_not_precede_start() {
    let _g = lock().await;
    let mut a = Client::connect("dora").await;
    assert!(a.reset(&["date.start", "date.due"]).await.ok);
    // defaults: start 2026-11-02, due 2026-11-20
    let ack = a.set("date.start", json!("2026-11-21")).await;
    assert!(!ack.ok && !ack.conflict, "start after due: {ack:?}");
    let ack = a.set("date.due", json!("2026-11-01")).await;
    assert!(!ack.ok && !ack.conflict, "due before start: {ack:?}");
    assert!(a.set("date.due", json!("2026-11-02")).await.ok, "equal dates are fine");
    // with due cleared, any start is fine
    assert!(a.set("date.due", Value::Null).await.ok);
    assert!(a.set("date.start", json!("2027-01-01")).await.ok);
    let ack = a.set("date.due", json!("2026-12-31")).await;
    assert!(!ack.ok, "{ack:?}");
    assert!(a.set("date.start", Value::Null).await.ok);
    assert!(a.set("date.due", json!("2026-12-31")).await.ok);
    assert!(a.reset(&["date.start", "date.due"]).await.ok);
}

// ---- reset ----------------------------------------------------------------------

#[tokio::test]
async fn reset_some_keys() {
    let _g = lock().await;
    let mut a = Client::connect("rita").await;
    let mut b = Client::connect("rob").await;
    assert!(a.set("number.qty", json!(9)).await.ok);
    assert!(a.set("choice.dark", json!(true)).await.ok);
    let (v_qty, v_dark) = (a.version("number.qty"), a.version("choice.dark"));
    let ack = a.reset(&["number.qty", "choice.dark"]).await;
    assert!(ack.ok, "{ack:?}");
    assert_eq!(b.changed_to("number.qty", v_qty + 1).await, json!(3));
    assert_eq!(b.changed_to("choice.dark", v_dark + 1).await, json!(false));
    // unknown keys: error, nothing reset
    assert!(a.set("number.qty", json!(5)).await.ok);
    let ack = a.reset(&["number.qty", "nope"]).await;
    assert!(!ack.ok && ack.error.is_some(), "{ack:?}");
    let c = Client::connect("rhea").await;
    assert_eq!(c.values.i64("number.qty"), 5);
    assert!(a.reset(&["number.qty"]).await.ok);
}

// ---- presence -------------------------------------------------------------------

#[tokio::test]
async fn presence_on_connect_focus_disconnect() {
    let _g = lock().await;
    let mut a = Client::connect("ann").await;
    let mut b = Client::connect("ben").await;
    let (ac, bc) = (a.conn, b.conn);
    let peers = a.presence(|p| p.iter().any(|x| x.conn == bc)).await;
    assert!(peers.iter().any(|x| x.conn == ac), "presence includes the receiver");
    let ben = peers.iter().find(|x| x.conn == bc).unwrap();
    assert_eq!((ben.name.as_str(), ben.client.as_str(), ben.color.as_str()), ("ben", "conform", peer_color(bc)));
    assert_eq!(ben.tab, None, "tab is null before focus");

    b.send(&ClientMsg::Focus { tab: "date".into() }).await;
    a.presence(|p| p.iter().any(|x| x.conn == bc && x.tab.as_deref() == Some("date"))).await;
    // b sees it too
    b.presence(|p| p.iter().any(|x| x.conn == bc && x.tab.as_deref() == Some("date"))).await;
    b.send(&ClientMsg::Focus { tab: "gantt".into() }).await;
    a.presence(|p| p.iter().any(|x| x.conn == bc && x.tab.as_deref() == Some("gantt"))).await;

    b.ws.close(None).await.unwrap();
    a.presence(|p| !p.iter().any(|x| x.conn == bc)).await;
}

// ---- toast ----------------------------------------------------------------------

#[tokio::test]
async fn toast_broadcast_and_rate_limit() {
    let _g = lock().await;
    let mut a = Client::connect("tom").await;
    let mut b = Client::connect("tess").await;
    let ack = a.call(|req| ClientMsg::Toast { req, text: "  Lunch is here ".into() }).await;
    assert!(ack.ok, "{ack:?}");
    let ac = a.conn;
    let got = b
        .wait(|m| match m {
            ServerMsg::Toast { text, by, conn } if *conn == ac => Some((text.clone(), by.clone())),
            _ => None,
        })
        .await;
    assert_eq!(got, ("Lunch is here".into(), "tom".into()));
    // a second toast within 2 s is dropped with an error ack
    let ack = a.call(|req| ClientMsg::Toast { req, text: "again".into() }).await;
    assert!(!ack.ok && ack.error.is_some(), "{ack:?}");
    // empty / too long text is refused
    let ack = b.call(|req| ClientMsg::Toast { req, text: "   ".into() }).await;
    assert!(!ack.ok, "{ack:?}");
    let mut c = Client::connect("tara").await;
    let ack = c.call(|req| ClientMsg::Toast { req, text: "x".repeat(121) }).await;
    assert!(!ack.ok, "{ack:?}");
    let ack = c.call(|req| ClientMsg::Toast { req, text: "bell\u{7}".into() }).await;
    assert!(!ack.ok, "control characters: {ack:?}");
}

// ---- framing --------------------------------------------------------------------

#[tokio::test]
async fn ping_pong_and_bad_messages() {
    let _g = lock().await;
    let mut a = Client::connect("pat").await;
    a.send(&ClientMsg::Ping).await;
    a.wait(|m| matches!(m, ServerMsg::Pong).then_some(())).await;
    for bad in ["not json", r#"{"type":"explode"}"#, r#"{"type":"set","req":1}"#, "[]"] {
        a.send_text(bad).await;
        a.wait(|m| matches!(m, ServerMsg::Error { .. }).then_some(())).await;
    }
    // the socket stays open
    a.send(&ClientMsg::Ping).await;
    a.wait(|m| matches!(m, ServerMsg::Pong).then_some(())).await;
}

#[tokio::test]
async fn rate_limit_closes_with_1008() {
    let _g = lock().await;
    let mut a = Client::connect("rex").await;
    for _ in 0..120 {
        if a.ws.send(Message::Text(r#"{"type":"ping"}"#.into())).await.is_err() {
            break;
        }
    }
    assert_eq!(a.close_code().await, Some(1008));
}

#[tokio::test]
async fn oversized_frame_closes_with_1008() {
    let _g = lock().await;
    let mut a = Client::connect("ollie").await;
    let big = format!(r#"{{"type":"toast","req":1,"text":"{}"}}"#, "x".repeat(MAX_FRAME + 10));
    let _ = a.ws.send(Message::Text(big.into())).await;
    assert_eq!(a.close_code().await, Some(1008));
}

/// Values sent with `welcome` match what `changed` later reports.
#[tokio::test]
async fn welcome_reflects_changes() {
    let _g = lock().await;
    let mut a = Client::connect("wes").await;
    let rows = json!([{"id": 9, "item": "Pen", "qty": 2, "price": 1.5, "status": "todo", "due": "2026-11-30"}]);
    assert!(a.set("grid.rows", rows.clone()).await.ok);
    assert!(a.set("cascade.pick", json!({"program": "Contoso Health", "project": null, "phase": null})).await.ok);
    let b = Client::connect("wyn").await;
    let ServerMsg::Welcome { values, .. } = &b.welcome else { unreachable!() };
    let m: BTreeMap<&str, &Entry> = values.iter().map(|(k, v)| (k.as_str(), v)).collect();
    assert_eq!(m["grid.rows"].value, rows);
    assert_eq!(m["grid.rows"].by, "wes");
    assert_eq!(m["cascade.pick"].value, json!({"program": "Contoso Health", "project": null, "phase": null}));
    assert!(a.reset(&["grid.rows", "cascade.pick"]).await.ok);
}

#[tokio::test]
async fn socket_cap_is_503() {
    let _g = lock().await;
    let mut open = Vec::new();
    let mut refusal = None;
    for _ in 0..=MAX_SOCKETS {
        let req = ws_url("client=conform&name=cap").await.into_client_request().unwrap();
        match tokio_tungstenite::connect_async(req).await {
            Ok((ws, _)) => open.push(ws),
            Err(e) => {
                refusal = Some(e.to_string());
                break;
            }
        }
    }
    let e = refusal.expect("the 65th socket must be refused");
    assert!(e.contains("503"), "expected 503, got {e}");
    assert!(open.len() <= MAX_SOCKETS);
    // closing frees the slots again
    for mut ws in open {
        let _ = ws.close(None).await;
    }
    for _ in 0..50 {
        let req = ws_url("client=conform&name=cap").await.into_client_request().unwrap();
        if tokio_tungstenite::connect_async(req).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("slots were not freed after closing");
}
