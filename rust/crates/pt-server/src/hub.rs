//! The hub: one thread that owns the store and every connection's outbound
//! queue. Commands arrive in one mailbox and run in order, so there are no
//! locks and no write races. Broadcasts are serialised once and shared.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::thread;

use pt_core::model::{Role, User};
use pt_core::protocol::{ClientMsg, ErrorBody, Focus, MAX_OPS, PROTOCOL_VERSION, Peer, ResultMsg, ServerInfo, ServerMsg};
use tokio::sync::{mpsc, oneshot};

use crate::store::{Outcome, Store};

/// Per-connection outbound queue depth. A client that falls this far behind
/// is disconnected (it reconnects and gets a fresh snapshot).
pub const OUTBOUND_DEPTH: usize = 512;

#[derive(Debug)]
pub enum Outbound {
    Text(Arc<str>),
}

#[allow(clippy::large_enum_variant)]
pub enum Cmd {
    LoginLookup {
        username: String,
        reply: oneshot::Sender<Option<(User, String)>>,
    },
    CreateSession {
        token_hash: String,
        user_id: i64,
        client: String,
        reply: oneshot::Sender<bool>,
    },
    Session {
        token_hash: String,
        reply: oneshot::Sender<Option<User>>,
    },
    Logout {
        token_hash: String,
    },
    Connect {
        user: User,
        client: String,
        session: String,
        out: mpsc::Sender<Outbound>,
        reply: oneshot::Sender<u64>,
    },
    Disconnect {
        conn: u64,
    },
    Client {
        conn: u64,
        msg: ClientMsg,
    },
    /// For set_password: the actor's fresh user row and the target's hash.
    PasswordInfo {
        conn: u64,
        target: i64,
        reply: oneshot::Sender<Option<(User, Option<String>)>>,
    },
    /// Send one message to one connection, in mailbox order.
    Reply {
        conn: u64,
        msg: ServerMsg,
    },
    SetPasswordHash {
        conn: u64,
        req: u64,
        target: i64,
        hash: String,
    },
}

#[derive(Clone)]
pub struct HubHandle {
    tx: mpsc::UnboundedSender<Cmd>,
}

impl HubHandle {
    pub fn send(&self, cmd: Cmd) {
        // The hub only stops at process exit; a failed send is harmless then.
        let _ = self.tx.send(cmd);
    }

    async fn ask<T>(&self, make: impl FnOnce(oneshot::Sender<T>) -> Cmd) -> Option<T> {
        let (tx, rx) = oneshot::channel();
        self.send(make(tx));
        rx.await.ok()
    }

    pub async fn login_lookup(&self, username: String) -> Option<(User, String)> {
        self.ask(|reply| Cmd::LoginLookup { username, reply }).await.flatten()
    }

    pub async fn create_session(&self, token_hash: String, user_id: i64, client: String) -> bool {
        self.ask(|reply| Cmd::CreateSession { token_hash, user_id, client, reply })
            .await
            .unwrap_or(false)
    }

    pub async fn session(&self, token_hash: String) -> Option<User> {
        self.ask(|reply| Cmd::Session { token_hash, reply }).await.flatten()
    }

    pub async fn connect(&self, user: User, client: String, session: String, out: mpsc::Sender<Outbound>) -> Option<u64> {
        self.ask(|reply| Cmd::Connect { user, client, session, out, reply }).await
    }

    pub async fn password_info(&self, conn: u64, target: i64) -> Option<(User, Option<String>)> {
        self.ask(|reply| Cmd::PasswordInfo { conn, target, reply }).await.flatten()
    }
}

struct Conn {
    user: User,
    client: String,
    session: String,
    view: Option<String>,
    focus: Option<Focus>,
    out: mpsc::Sender<Outbound>,
}

struct Hub {
    store: Store,
    info: ServerInfo,
    conns: BTreeMap<u64, Conn>,
    next_conn: u64,
}

pub fn spawn(store: Store, info: ServerInfo) -> HubHandle {
    let (tx, mut rx) = mpsc::unbounded_channel::<Cmd>();
    thread::Builder::new()
        .name("pt-hub".into())
        .spawn(move || {
            let mut hub = Hub {
                store,
                info,
                conns: BTreeMap::new(),
                next_conn: 1,
            };
            while let Some(cmd) = rx.blocking_recv() {
                hub.handle(cmd);
            }
        })
        .expect("spawn hub thread");
    HubHandle { tx }
}

fn json(msg: &ServerMsg) -> Arc<str> {
    // Serialising our own types cannot fail.
    Arc::from(serde_json::to_string(msg).unwrap_or_default())
}

impl Hub {
    fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::LoginLookup { username, reply } => {
                let _ = reply.send(self.store.user_for_login(&username).ok().flatten());
            }
            Cmd::CreateSession { token_hash, user_id, client, reply } => {
                let _ = reply.send(self.store.create_session(&token_hash, user_id, &client).is_ok());
            }
            Cmd::Session { token_hash, reply } => {
                let _ = reply.send(self.store.session_user(&token_hash).ok().flatten());
            }
            Cmd::Logout { token_hash } => {
                let _ = self.store.delete_session(&token_hash);
                let gone: Vec<u64> = self.conns.iter().filter(|(_, c)| c.session == token_hash).map(|(id, _)| *id).collect();
                for id in gone {
                    self.conns.remove(&id);
                }
                self.broadcast_presence();
            }
            Cmd::Connect { user, client, session, out, reply } => {
                let conn = self.next_conn;
                self.next_conn += 1;
                let welcome = ServerMsg::Welcome {
                    protocol: PROTOCOL_VERSION,
                    server: self.info.clone(),
                    me: user.clone(),
                    conn,
                };
                let snapshot = match self.store.snapshot() {
                    Ok(data) => ServerMsg::Snapshot { data },
                    Err(e) => {
                        tracing::error!("snapshot failed: {e}");
                        return; // dropping `reply` makes the socket close
                    }
                };
                let _ = out.try_send(Outbound::Text(json(&welcome)));
                let _ = out.try_send(Outbound::Text(json(&snapshot)));
                tracing::info!(conn, user = %user.username, client = %client, "connected");
                self.conns.insert(conn, Conn { user, client, session, view: None, focus: None, out });
                let _ = reply.send(conn);
                self.broadcast_presence();
            }
            Cmd::Disconnect { conn } => {
                if self.conns.remove(&conn).is_some() {
                    tracing::info!(conn, "disconnected");
                    self.broadcast_presence();
                }
            }
            Cmd::Client { conn, msg } => self.client(conn, msg),
            Cmd::Reply { conn, msg } => self.send_to(conn, &msg),
            Cmd::PasswordInfo { conn, target, reply } => {
                let actor = self.conns.get(&conn).and_then(|c| self.store.user(c.user.id).ok().flatten());
                let hash = self.store.password_hash(target).ok().flatten();
                let _ = reply.send(actor.map(|a| (a, hash)));
            }
            Cmd::SetPasswordHash { conn, req, target, hash } => {
                let Some(c) = self.conns.get(&conn) else { return };
                let (actor, keep) = (c.user.clone(), c.session.clone());
                let mut res = ResultMsg { req, ..Default::default() };
                if actor.id != target && actor.role != Role::Admin {
                    res.error = Some(ErrorBody::new("forbidden", "you can only change your own password"));
                } else if self.store.set_password_hash(target, &hash).is_err() {
                    res.error = Some(ErrorBody::new("internal", "internal error"));
                } else {
                    // end the target's other sessions (the caller's own stays)
                    let _ = self.store.delete_user_sessions(target, Some(&keep));
                    self.kick(target, Some(&keep));
                    res.ok = true;
                }
                self.send_to(conn, &ServerMsg::Result(res));
            }
        }
    }

    fn client(&mut self, conn: u64, msg: ClientMsg) {
        let Some(user) = self.conns.get(&conn).map(|c| c.user.clone()) else { return };
        match msg {
            ClientMsg::Mutate { req, ops } => {
                if ops.len() > MAX_OPS {
                    let mut r = ResultMsg { req, ..Default::default() };
                    r.error = Some(ErrorBody::new("bad_request", format!("at most {MAX_OPS} ops per mutate")));
                    self.send_to(conn, &ServerMsg::Result(r));
                    return;
                }
                let out = self.store.mutate(&user, &ops);
                self.finish(conn, req, out);
            }
            ClientMsg::Undo { req } => {
                let out = self.store.undo(&user);
                self.finish(conn, req, out);
            }
            ClientMsg::Redo { req } => {
                let out = self.store.redo(&user);
                self.finish(conn, req, out);
            }
            ClientMsg::Focus { view, entity, id, field } => {
                if let Some(c) = self.conns.get_mut(&conn) {
                    let view = view.filter(|v| v.len() <= 32);
                    let field = field.filter(|f| f.len() <= 64);
                    let focus = match (entity, id) {
                        (Some(entity), Some(id)) => Some(Focus { entity, id, field }),
                        _ => None,
                    };
                    if c.view == view && c.focus == focus {
                        return;
                    }
                    c.view = view;
                    c.focus = focus;
                }
                self.broadcast_presence();
            }
            ClientMsg::Resync => match self.store.snapshot() {
                Ok(data) => self.send_to(conn, &ServerMsg::Snapshot { data }),
                Err(e) => tracing::error!("snapshot failed: {e}"),
            },
            ClientMsg::Ping => self.send_to(conn, &ServerMsg::Pong),
            // handled by the socket task (needs hashing off this thread)
            ClientMsg::SetPassword { .. } => {}
        }
    }

    fn finish(&mut self, conn: u64, req: u64, mut out: Outcome) {
        out.result.req = req;
        self.send_to(conn, &ServerMsg::Result(out.result));
        if let Some(changes) = out.changes {
            // role / name changes take effect on live connections immediately
            for u in &changes.upserts.users {
                for c in self.conns.values_mut().filter(|c| c.user.id == u.id) {
                    c.user = u.clone();
                }
            }
            self.broadcast(&ServerMsg::Changes(changes));
        }
        for (uid, keep) in out.kick {
            self.kick(uid, keep.as_deref());
        }
    }

    /// Close every connection of a user (except one session).
    fn kick(&mut self, user_id: i64, keep: Option<&str>) {
        let gone: Vec<u64> = self
            .conns
            .iter()
            .filter(|(_, c)| c.user.id == user_id && keep != Some(c.session.as_str()))
            .map(|(id, _)| *id)
            .collect();
        if gone.is_empty() {
            return;
        }
        for id in gone {
            // dropping the sender ends the socket's writer task, which closes the socket
            self.conns.remove(&id);
        }
        self.broadcast_presence();
    }

    fn send_to(&mut self, conn: u64, msg: &ServerMsg) {
        let text = json(msg);
        let full = match self.conns.get(&conn) {
            Some(c) => c.out.try_send(Outbound::Text(text)).is_err(),
            None => false,
        };
        if full {
            self.conns.remove(&conn);
            self.broadcast_presence();
        }
    }

    fn broadcast(&mut self, msg: &ServerMsg) {
        let text = json(msg);
        let slow: Vec<u64> = self
            .conns
            .iter()
            .filter(|(_, c)| c.out.try_send(Outbound::Text(text.clone())).is_err())
            .map(|(id, _)| *id)
            .collect();
        for id in &slow {
            tracing::warn!(conn = id, "client too slow or gone; disconnecting");
            self.conns.remove(id);
        }
        if !slow.is_empty() {
            self.broadcast_presence();
        }
    }

    fn broadcast_presence(&mut self) {
        let peers: Vec<Peer> = self
            .conns
            .iter()
            .map(|(id, c)| Peer {
                conn: *id,
                user_id: c.user.id,
                client: c.client.clone(),
                view: c.view.clone(),
                focus: c.focus.clone(),
            })
            .collect();
        let text = json(&ServerMsg::Presence { peers });
        // presence is best-effort: a full queue just misses this update
        for c in self.conns.values() {
            let _ = c.out.try_send(Outbound::Text(text.clone()));
        }
    }
}
