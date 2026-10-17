//! Protocol conformance suite (docs/pt/protocol.md).
//!
//! By default it starts an in-process server on a fresh in-memory database
//! seeded with the demo data. To check another implementation (the Go
//! server), point it at a running server whose database was freshly seeded
//! with `pt seed --password <PW>`:
//!
//!   PT_CONFORM_URL=http://127.0.0.1:8789 PT_CONFORM_PASSWORD=<PW> \
//!     cargo test -p pt-server --test conformance -- --test-threads=1
//!
//! Tests only touch rows they create, or fields they restore, so they can
//! share one server; they still run one at a time against external servers.

use std::net::SocketAddr;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use pt_core::model::*;
use pt_core::protocol::*;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::OnceCell;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

const PASSWORD: &str = "conformance-pass-1";

struct Target {
    addr: String, // host:port
    password: String,
}

static TARGET: OnceCell<Target> = OnceCell::const_new();
/// Undo is per user: tests acting as the same user must not interleave.
static ADMIN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn target() -> &'static Target {
    TARGET
        .get_or_init(|| async {
            if let Ok(url) = std::env::var("PT_CONFORM_URL") {
                let addr = url.trim_start_matches("http://").trim_end_matches('/').to_string();
                return Target { addr, password: std::env::var("PT_CONFORM_PASSWORD").expect("PT_CONFORM_PASSWORD") };
            }
            // In-process server on its own runtime thread so it outlives each test's runtime.
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async move {
                    let mut store = pt_server::Store::open_memory().unwrap();
                    pt_server::seed::seed(&mut store, PASSWORD).unwrap();
                    let app = pt_server::app(store, false).into_make_service_with_connect_info::<SocketAddr>();
                    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                    tx.send(l.local_addr().unwrap()).unwrap();
                    axum::serve(l, app).await.unwrap();
                });
            });
            let addr = rx.recv().unwrap();
            Target { addr: addr.to_string(), password: PASSWORD.into() }
        })
        .await
}

/// Minimal HTTP/1.1 client (enough for the JSON API; no TLS).
async fn http(method: &str, path: &str, headers: &[(&str, &str)], body: Option<&str>) -> (u16, Vec<(String, String)>, String) {
    let t = target().await;
    let mut s = TcpStream::connect(&t.addr).await.unwrap();
    let body = body.unwrap_or("");
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nContent-Length: {}\r\n", t.addr, body.len());
    if !body.is_empty() {
        req.push_str("Content-Type: application/json\r\n");
    }
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    req.push_str(body);
    s.write_all(req.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).await.unwrap();
    let text = String::from_utf8_lossy(&buf).to_string();
    let (head, rest) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let mut lines = head.lines();
    let status: u16 = lines.next().unwrap().split_whitespace().nth(1).unwrap().parse().unwrap();
    let hdrs = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_lowercase(), v.trim().to_string()))
        .collect();
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

async fn login(user: &str) -> String {
    let pw = target().await.password.clone();
    let (status, headers, body) =
        http("POST", "/api/login", &[], Some(&json!({"username": user, "password": pw, "client": "conform"}).to_string())).await;
    assert_eq!(status, 200, "login {user}: {body}");
    assert!(
        headers.iter().any(|(k, v)| k == "set-cookie" && v.contains("HttpOnly") && v.contains("SameSite=Strict")),
        "session cookie flags"
    );
    let r: LoginResponse = serde_json::from_str(&body).unwrap();
    assert_eq!(r.user.username, user);
    r.token
}

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>;

struct Client {
    ws: Ws,
    req: u64,
    me: User,
    conn: u64,
    data: Tables,
}

impl Client {
    async fn connect(user: &str) -> Client {
        let token = login(user).await;
        Client::with_token(&token).await
    }

    async fn with_token(token: &str) -> Client {
        let t = target().await;
        let mut req = format!("ws://{}/ws?client=conform", t.addr).into_client_request().unwrap();
        req.headers_mut().insert("Authorization", format!("Bearer {token}").parse().unwrap());
        let (ws, _) = tokio_tungstenite::connect_async(req).await.unwrap();
        let mut c = Client { ws, req: 0, me: dummy_user(), conn: 0, data: Tables::default() };
        match c.recv().await {
            ServerMsg::Welcome { me, conn, protocol, .. } => {
                assert_eq!(protocol, PROTOCOL_VERSION);
                c.me = me;
                c.conn = conn;
            }
            other => panic!("expected welcome, got {other:?}"),
        }
        match c.recv().await {
            ServerMsg::Snapshot { data } => c.data = data,
            other => panic!("expected snapshot, got {other:?}"),
        }
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
                return serde_json::from_str(t.as_str()).unwrap_or_else(|e| panic!("bad server message {t}: {e}"));
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
        self.ws.send(Message::Text(serde_json::to_string(msg).unwrap().into())).await.unwrap();
    }

    async fn call(&mut self, make: impl FnOnce(u64) -> ClientMsg) -> ResultMsg {
        self.req += 1;
        let req = self.req;
        self.send(&make(req)).await;
        self.wait(|m| match m {
            ServerMsg::Result(r) if r.req == req => Some(r.clone()),
            _ => None,
        })
        .await
    }

    async fn mutate(&mut self, ops: Vec<Op>) -> ResultMsg {
        self.call(|req| ClientMsg::Mutate { req, ops }).await
    }

    /// The `changes` event for a result's group (other tests share the server).
    async fn changes(&mut self, r: &ResultMsg) -> Changes {
        let group = r.group.expect("result has a group");
        self.wait(|m| match m {
            ServerMsg::Changes(c) if c.group == Some(group) => Some(c.clone()),
            _ => None,
        })
        .await
    }

    fn task_of_project(&self, code: &str) -> Task {
        let p = self.data.projects.iter().find(|p| p.code == code).unwrap();
        self.data.tasks.iter().find(|t| t.project_id == p.id).unwrap().clone()
    }
}

fn dummy_user() -> User {
    User { id: 0, username: String::new(), display_name: String::new(), role: Role::Viewer, active: true }
}

fn task_create(project_id: i64, title: &str) -> Op {
    let mut values = serde_json::Map::new();
    values.insert("project_id".into(), json!(project_id));
    values.insert("title".into(), json!(title));
    Op::Create { entity: Entity::Task, reference: Some("t".into()), values }
}

// ---- HTTP -----------------------------------------------------------------------

#[tokio::test]
async fn health_and_headers() {
    let (status, headers, body) = http("GET", "/api/health", &[], None).await;
    assert_eq!(status, 200);
    let h: HealthInfo = serde_json::from_str(&body).unwrap();
    assert!(h.ok);
    assert_eq!(h.protocol, 1);
    let get = |k: &str| headers.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    assert_eq!(get("x-content-type-options").as_deref(), Some("nosniff"));
    assert!(get("content-security-policy").unwrap().contains("frame-ancestors 'none'"));
}

#[tokio::test]
async fn login_rejects_bad_credentials() {
    let (s, _, body) = http("POST", "/api/login", &[], Some(r#"{"username":"alice","password":"wrong-password"}"#)).await;
    assert_eq!(s, 401);
    assert!(body.contains("invalid credentials"));
    let (s, _, body2) = http("POST", "/api/login", &[], Some(r#"{"username":"no-such-user","password":"wrong-password"}"#)).await;
    assert_eq!(s, 401);
    assert_eq!(body, body2, "same message for unknown users");
    let (s, _, _) = http("POST", "/api/login", &[], Some("not json")).await;
    assert_eq!(s, 400);
    let (s, _, _) = http("GET", "/api/me", &[], None).await;
    assert_eq!(s, 401);
}

#[tokio::test]
async fn login_throttles_failures() {
    let body = r#"{"username":"throttle-probe","password":"wrong-password"}"#;
    let mut statuses = vec![];
    for _ in 0..6 {
        statuses.push(http("POST", "/api/login", &[], Some(body)).await.0);
    }
    assert_eq!(&statuses[..5], &[401; 5]);
    assert_eq!(statuses[5], 429);
}

#[tokio::test]
async fn me_cookie_and_logout() {
    let token = login("bob").await;
    let cookie = format!("pt_session={token}");
    let (s, _, body) = http("GET", "/api/me", &[("Cookie", &cookie)], None).await;
    assert_eq!(s, 200, "{body}");
    assert!(body.contains("\"bob\""));
    let auth = format!("Bearer {token}");
    let (s, _, _) = http("POST", "/api/logout", &[("Authorization", &auth)], None).await;
    assert_eq!(s, 204);
    let (s, _, _) = http("GET", "/api/me", &[("Cookie", &cookie)], None).await;
    assert_eq!(s, 401);
}

#[tokio::test]
async fn ws_requires_auth_and_same_origin() {
    let t = target().await;
    let req = format!("ws://{}/ws?client=conform", t.addr).into_client_request().unwrap();
    assert!(tokio_tungstenite::connect_async(req).await.is_err(), "no session must be refused");
    let token = login("carol").await;
    let mut req = format!("ws://{}/ws", t.addr).into_client_request().unwrap();
    req.headers_mut().insert("Authorization", format!("Bearer {token}").parse().unwrap());
    req.headers_mut().insert("Origin", "http://evil.example".parse().unwrap());
    let err = tokio_tungstenite::connect_async(req).await.unwrap_err().to_string();
    assert!(err.contains("403"), "cross-origin must be 403: {err}");
    let (s, _, _) = http("POST", "/api/login", &[("Origin", "http://evil.example")], Some(r#"{"username":"carol","password":"x"}"#)).await;
    assert_eq!(s, 403);
}

// ---- WebSocket ------------------------------------------------------------------

#[tokio::test]
async fn snapshot_contents() {
    let c = Client::connect("alice").await;
    assert_eq!(c.me.username, "alice");
    assert_eq!(c.me.role, Role::Member);
    assert!(c.data.projects.len() >= 50);
    assert!(c.data.tasks.len() >= 500);
    assert_eq!(c.data.users.len(), 10);
    assert_eq!(c.data.programs.len(), 8);
    assert_eq!(c.data.templates.len(), 5);
    assert!(c.data.custom_fields.len() >= 5);
    assert!(!c.data.activity.is_empty() && c.data.activity.len() <= 500);
    assert!(c.data.tasks.iter().any(|t| !t.deps.is_empty()));
    assert!(c.data.projects.iter().filter(|p| !p.code.starts_with("CNF")).all(|p| p.cf.len() >= 3));
}

#[tokio::test]
async fn set_conflict_and_broadcast() {
    let mut a = Client::connect("alice").await;
    let mut b = Client::connect("dave").await;
    let t = a.task_of_project("NWR-002");
    let old = Value::from(t.title.clone());
    let r = a.mutate(vec![Op::set(Entity::Task, t.id, "title", old.clone(), json!("Renamed by conformance"))]).await;
    assert!(r.ok && r.conflicts.is_empty() && r.group.is_some(), "{r:?}");
    for c in [&mut a, &mut b] {
        let ch = c.changes(&r).await;
        assert_eq!(ch.upserts.tasks[0].title, "Renamed by conformance");
        assert_eq!(ch.activity[0].field, "title");
        assert_eq!(ch.activity[0].new, json!("Renamed by conformance"));
    }
    // b still believes the old title: conflict, nothing applied
    let r = b.mutate(vec![Op::set(Entity::Task, t.id, "title", old.clone(), json!("Lost update"))]).await;
    assert!(r.ok && r.group.is_none());
    assert_eq!(r.conflicts.len(), 1);
    assert_eq!(r.conflicts[0].current, json!("Renamed by conformance"));
    assert_eq!(r.conflicts[0].by, Some(a.me.id));
    // force overrides
    let r = b
        .mutate(vec![Op::Set { entity: Entity::Task, id: t.id, field: "title".into(), base: Value::Null, value: old, force: true }])
        .await;
    assert!(r.ok && r.group.is_some());
}

#[tokio::test]
async fn validation_is_atomic() {
    let mut c = Client::connect("erin").await;
    let t = c.task_of_project("FAB-001");
    let r = c
        .mutate(vec![
            Op::set(Entity::Task, t.id, "phase", json!(t.phase), json!("Changed")),
            Op::set(Entity::Task, t.id, "status", json!(t.status.as_str()), json!("nonsense")),
        ])
        .await;
    assert!(!r.ok);
    let e = r.error.unwrap();
    assert_eq!(e.code, "invalid");
    assert_eq!(e.index, Some(1));
    // the first op was rolled back: setting from the original base still works
    let r = c.mutate(vec![Op::set(Entity::Task, t.id, "phase", json!(t.phase), json!(t.phase.clone() + "!"))]).await;
    assert!(r.ok && r.conflicts.is_empty(), "{r:?}");
    let r = c.mutate(vec![Op::set(Entity::Task, t.id, "phase", json!(t.phase.clone() + "!"), json!(t.phase))]).await;
    assert!(r.ok);
    // cross-field: due before start
    let r = c
        .mutate(vec![Op::set(Entity::Task, t.id, "due_date", json!(t.due_date), json!("1999-01-01"))])
        .await;
    assert_eq!(r.error.unwrap().code, "invalid");
    // unknown field
    let r = c.mutate(vec![Op::set(Entity::Task, t.id, "password", json!(null), json!("x"))]).await;
    assert!(!r.ok);
}

#[tokio::test]
async fn create_delete_undo_redo() {
    let mut c = Client::connect("frank").await;
    let pid = c.data.projects.iter().find(|p| p.code == "GIS-003").unwrap().id;
    let r = c.mutate(vec![task_create(pid, "Conformance task")]).await;
    assert!(r.ok, "{r:?}");
    let id = r.created["t"];
    let ch = c.changes(&r).await;
    let created = ch.upserts.tasks.iter().find(|t| t.id == id).unwrap();
    assert!(created.seq > 1);
    assert_eq!(created.status, TaskStatus::Todo);

    let r = c.mutate(vec![Op::Delete { entity: Entity::Task, id }]).await;
    assert!(r.ok);
    let ch = c.changes(&r).await;
    assert_eq!(ch.deletes.tasks, vec![id]);

    let r = c.call(|req| ClientMsg::Undo { req }).await;
    assert!(r.ok, "{r:?}");
    let ch = c.changes(&r).await;
    assert!(ch.upserts.tasks.iter().any(|t| t.id == id && t.title == "Conformance task"));
    assert!(ch.activity.iter().all(|a| a.state == ActState::Undone));

    let r = c.call(|req| ClientMsg::Redo { req }).await;
    assert!(r.ok, "{r:?}");
    let ch = c.changes(&r).await;
    assert_eq!(ch.deletes.tasks, vec![id]);

    // undo twice: the delete, then the create
    let r = c.call(|req| ClientMsg::Undo { req }).await;
    assert!(r.ok);
    c.changes(&r).await;
    let r = c.call(|req| ClientMsg::Undo { req }).await;
    assert!(r.ok);
    let ch = c.changes(&r).await;
    assert_eq!(ch.deletes.tasks, vec![id]);
}

#[tokio::test]
async fn undo_conflicts_when_someone_else_changed_it() {
    let mut a = Client::connect("grace").await;
    let mut b = Client::connect("heidi").await;
    let t = a.task_of_project("RND-002");
    let r = a.mutate(vec![Op::set(Entity::Task, t.id, "est_hours", json!(t.est_hours), json!(77))]).await;
    assert!(r.ok);
    let r = b.mutate(vec![Op::set(Entity::Task, t.id, "est_hours", json!(77), json!(78))]).await;
    assert!(r.ok && r.conflicts.is_empty());
    let r = a.call(|req| ClientMsg::Undo { req }).await;
    assert!(!r.ok);
    assert_eq!(r.error.unwrap().code, "conflict");
    assert!(pt_core::fields::values_equal(&r.conflicts[0].current, &json!(78)), "{:?}", r.conflicts);
    // put it back
    let r = b.mutate(vec![Op::set(Entity::Task, t.id, "est_hours", json!(78), json!(t.est_hours))]).await;
    assert!(r.ok);
    let r = b.call(|req| ClientMsg::Redo { req }).await;
    assert_eq!(r.error.unwrap().code, "nothing_to_redo");
}

#[tokio::test]
async fn authorization() {
    let mut v = Client::connect("victor").await;
    let t = v.task_of_project("MKT-001");
    let r = v.mutate(vec![Op::set(Entity::Task, t.id, "title", json!(t.title), json!("nope"))]).await;
    assert_eq!(r.error.unwrap().code, "forbidden");
    let mut m = Client::connect("bob").await;
    let mut values = serde_json::Map::new();
    values.insert("name".into(), json!("Sneaky program"));
    let r = m.mutate(vec![Op::Create { entity: Entity::Program, reference: None, values }]).await;
    assert_eq!(r.error.unwrap().code, "forbidden");
    let uid = m.data.users.iter().find(|u| u.username == "carol").unwrap().id;
    let r = m.mutate(vec![Op::set(Entity::User, uid, "role", json!("member"), json!("admin"))]).await;
    assert_eq!(r.error.unwrap().code, "forbidden");
    let r = m.mutate(vec![Op::Delete { entity: Entity::User, id: uid }]).await;
    assert_eq!(r.error.unwrap().code, "forbidden");
}

#[tokio::test]
async fn deps_reject_cycles_and_other_projects() {
    let mut c = Client::connect("alice").await;
    let p = c.data.projects.iter().find(|p| p.code == "CTH-004").unwrap().id;
    let ts: Vec<Task> = c.data.tasks.iter().filter(|t| t.project_id == p).cloned().collect();
    let (a, b) = (&ts[0], &ts[1]);
    // b depends on a (seeded chain may already do so); make a depend on b → cycle
    let r = c.mutate(vec![Op::set(Entity::Task, b.id, "deps", json!(b.deps), json!([a.id]))]).await;
    assert!(r.ok, "{r:?}");
    let r = c.mutate(vec![Op::set(Entity::Task, a.id, "deps", json!(a.deps), json!([b.id]))]).await;
    assert_eq!(r.error.unwrap().code, "invalid");
    let other = c.data.tasks.iter().find(|t| t.project_id != p).unwrap().id;
    let r = c.mutate(vec![Op::set(Entity::Task, a.id, "deps", json!(a.deps), json!([other]))]).await;
    assert_eq!(r.error.unwrap().code, "invalid");
    let r = c.mutate(vec![Op::set(Entity::Task, b.id, "deps", json!([a.id]), json!(b.deps))]).await;
    assert!(r.ok);
}

#[tokio::test]
async fn project_from_template() {
    let _admin = ADMIN.lock().await;
    let mut c = Client::connect("admin").await;
    let tpl = c.data.templates.iter().find(|t| t.name == "Data Migration").unwrap().id;
    let n = c.data.template_tasks.iter().filter(|t| t.template_id == tpl).count();
    let mut values = serde_json::Map::new();
    values.insert("code".into(), json!("cnf-001"));
    values.insert("name".into(), json!("Conformance project"));
    values.insert("template_id".into(), json!(tpl));
    values.insert("start_date".into(), json!("2027-01-04"));
    values.insert("tags".into(), json!(["Pilot", "data"]));
    values.insert("cf.3".into(), json!("North"));
    let r = c.mutate(vec![Op::Create { entity: Entity::Project, reference: Some("p".into()), values }]).await;
    assert!(r.ok, "{r:?}");
    let pid = r.created["p"];
    let ch = c.changes(&r).await;
    let p = ch.upserts.projects.iter().find(|p| p.id == pid).unwrap();
    assert_eq!(p.code, "CNF-001");
    assert_eq!(p.tags, vec!["data", "pilot"]);
    assert_eq!(p.cf.get("3"), Some(&json!("North")));
    let tasks: Vec<&Task> = ch.upserts.tasks.iter().filter(|t| t.project_id == pid).collect();
    assert_eq!(tasks.len(), n);
    assert_eq!(tasks[0].start_date.as_deref(), Some("2027-01-04"));
    assert!(tasks.iter().any(|t| !t.deps.is_empty()), "phases are chained");
    // duplicate code
    let mut values = serde_json::Map::new();
    values.insert("code".into(), json!("CNF-001"));
    values.insert("name".into(), json!("dup"));
    let r = c.mutate(vec![Op::Create { entity: Entity::Project, reference: None, values }]).await;
    assert_eq!(r.error.unwrap().code, "invalid");
    // clean up (undo the create)
    let r = c.call(|req| ClientMsg::Undo { req }).await;
    assert!(r.ok, "{r:?}");
    let ch = c.changes(&r).await;
    assert!(ch.deletes.projects.contains(&pid));
}

#[tokio::test]
async fn custom_fields_admin_flow() {
    let _admin = ADMIN.lock().await;
    let mut c = Client::connect("admin").await;
    let mut values = serde_json::Map::new();
    values.insert("entity".into(), json!("task"));
    values.insert("name".into(), json!("Conformance size"));
    values.insert("kind".into(), json!("enum"));
    values.insert("options".into(), json!(["S", "M", "L"]));
    let r = c.mutate(vec![Op::Create { entity: Entity::CustomField, reference: Some("f".into()), values }]).await;
    assert!(r.ok, "{r:?}");
    let fid = r.created["f"];
    c.changes(&r).await;
    let t = c.task_of_project("ITX-001");
    let key = format!("cf.{fid}");
    let r = c.mutate(vec![Op::set(Entity::Task, t.id, &key, Value::Null, json!("M"))]).await;
    assert!(r.ok && r.conflicts.is_empty(), "{r:?}");
    let ch = c.changes(&r).await;
    assert_eq!(ch.upserts.tasks[0].cf.get(&fid.to_string()), Some(&json!("M")));
    let r = c.mutate(vec![Op::set(Entity::Task, t.id, &key, json!("M"), json!("XL"))]).await;
    assert_eq!(r.error.unwrap().code, "invalid");
    // delete the field: rows lose the value
    let r = c.mutate(vec![Op::Delete { entity: Entity::CustomField, id: fid }]).await;
    assert!(r.ok);
    let ch = c.changes(&r).await;
    assert_eq!(ch.deletes.custom_fields, vec![fid]);
    assert!(ch.upserts.tasks.iter().any(|x| x.id == t.id && !x.cf.contains_key(&fid.to_string())));
}

#[tokio::test]
async fn presence_and_focus() {
    let mut a = Client::connect("carol").await;
    let mut b = Client::connect("dave").await;
    let t = a.task_of_project("FAC-001");
    a.send(&ClientMsg::Focus { view: Some("tasks".into()), entity: Some(Entity::Task), id: Some(t.id), field: Some("status".into()) })
        .await;
    let a_conn = a.conn;
    let peer = b
        .wait(|m| match m {
            ServerMsg::Presence { peers } => peers
                .iter()
                .find(|p| p.conn == a_conn && p.focus.is_some())
                .cloned(),
            _ => None,
        })
        .await;
    assert_eq!(peer.client, "conform");
    assert_eq!(peer.focus.unwrap().id, t.id);
    assert_eq!(peer.view.as_deref(), Some("tasks"));
    a.send(&ClientMsg::Ping).await;
    a.wait(|m| (m == &ServerMsg::Pong).then_some(())).await;
}

#[tokio::test]
async fn set_password_rules() {
    let mut c = Client::connect("erin").await;
    let me = c.me.id;
    let r = c
        .call(|req| ClientMsg::SetPassword { req, user_id: me, password: "new-password-1".into(), old_password: Some("wrong".into()) })
        .await;
    assert_eq!(r.error.unwrap().code, "invalid");
    let other = c.data.users.iter().find(|u| u.username == "frank").unwrap().id;
    let r = c
        .call(|req| ClientMsg::SetPassword { req, user_id: other, password: "new-password-1".into(), old_password: None })
        .await;
    assert_eq!(r.error.unwrap().code, "forbidden");
}

#[tokio::test]
async fn bad_frames_get_errors() {
    let mut c = Client::connect("bob").await;
    c.ws.send(Message::Text("{\"type\":\"nope\"}".into())).await.unwrap();
    let code = c
        .wait(|m| match m {
            ServerMsg::Error { code, .. } => Some(code.clone()),
            _ => None,
        })
        .await;
    assert_eq!(code, "bad_request");
    // still usable
    c.send(&ClientMsg::Ping).await;
    c.wait(|m| (m == &ServerMsg::Pong).then_some(())).await;
}
