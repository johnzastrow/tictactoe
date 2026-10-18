//! The hub: one thread that owns the store, the current value of every key
//! and every connection's outbound queue. Commands arrive in one mailbox and
//! run in order, so there are no locks and no write races. Broadcasts are
//! serialised once and shared.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use demo_core::dataset::{peer_color, tab_by_id};
use demo_core::keys::{self, KEYS};
use demo_core::protocol::{Ack, ClientMsg, PROTOCOL_VERSION, Peer, ServerInfo, ServerMsg, TOAST_INTERVAL_MS, You, clean_toast};
use demo_core::Entry;
use tokio::sync::{mpsc, oneshot};

use crate::store::{Store, default_entry, now_utc};

/// Per-connection outbound queue depth. A client that falls this far behind
/// is disconnected (it reconnects and gets a fresh `welcome`).
pub const OUTBOUND_DEPTH: usize = 512;

#[derive(Debug)]
pub enum Outbound {
    Text(Arc<str>),
}

pub enum Cmd {
    Connect {
        name: String,
        client: String,
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
    /// Send one message to one connection, in mailbox order.
    Reply {
        conn: u64,
        msg: ServerMsg,
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

    /// Register a connection; its `welcome` is already queued on `out` when
    /// this returns. `None` if the hub is gone.
    pub async fn connect(&self, name: String, client: String, out: mpsc::Sender<Outbound>) -> Option<u64> {
        let (reply, rx) = oneshot::channel();
        self.send(Cmd::Connect { name, client, out, reply });
        rx.await.ok()
    }
}

struct Conn {
    name: String,
    client: String,
    tab: Option<String>,
    last_toast: Option<Instant>,
    out: mpsc::Sender<Outbound>,
}

struct Hub {
    store: Store,
    info: ServerInfo,
    values: BTreeMap<String, Entry>,
    conns: BTreeMap<u64, Conn>,
    next_conn: u64,
}

/// Load every value and start the hub thread.
pub fn spawn(mut store: Store, info: ServerInfo) -> anyhow::Result<HubHandle> {
    let values = store.load()?;
    let (tx, mut rx) = mpsc::unbounded_channel::<Cmd>();
    thread::Builder::new().name("demo-hub".into()).spawn(move || {
        let mut hub = Hub { store, info, values, conns: BTreeMap::new(), next_conn: 1 };
        while let Some(cmd) = rx.blocking_recv() {
            hub.handle(cmd);
        }
    })?;
    Ok(HubHandle { tx })
}

fn json(msg: &ServerMsg) -> Arc<str> {
    // Serialising our own types cannot fail.
    Arc::from(serde_json::to_string(msg).unwrap_or_default())
}

impl Hub {
    fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Connect { name, client, out, reply } => {
                let conn = self.next_conn;
                self.next_conn += 1;
                let welcome = ServerMsg::Welcome {
                    protocol: PROTOCOL_VERSION,
                    server: self.info.clone(),
                    you: You { conn, name: name.clone(), color: peer_color(conn).into() },
                    values: self.values.clone(),
                };
                let _ = out.try_send(Outbound::Text(json(&welcome)));
                tracing::info!(conn, name = %name, client = %client, "connected");
                self.conns.insert(conn, Conn { name, client, tab: None, last_toast: None, out });
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
        }
    }

    fn client(&mut self, conn: u64, msg: ClientMsg) {
        let Some(name) = self.conns.get(&conn).map(|c| c.name.clone()) else { return };
        match msg {
            ClientMsg::Set { req, key, base, value, force } => {
                let (mut ack, changed) = self.set(conn, &name, &key, base, value, force);
                ack.req = req;
                self.send_to(conn, &ServerMsg::Ack(ack));
                if let Some(changed) = changed {
                    self.broadcast(&changed);
                }
            }
            ClientMsg::Reset { req, keys: list } => self.reset(conn, &name, req, list),
            ClientMsg::Focus { tab } => {
                // unknown tab ids are ignored (focus has no reply)
                if tab_by_id(&tab).is_none() {
                    return;
                }
                if let Some(c) = self.conns.get_mut(&conn) {
                    c.tab = Some(tab);
                }
                self.broadcast_presence();
            }
            ClientMsg::Toast { req, text } => {
                let Some(text) = clean_toast(&text) else {
                    return self.send_to(conn, &ServerMsg::Ack(Ack::error(req, "toast must be 1–120 characters")));
                };
                let now = Instant::now();
                let too_soon = self
                    .conns
                    .get(&conn)
                    .and_then(|c| c.last_toast)
                    .is_some_and(|t| now.duration_since(t) < Duration::from_millis(TOAST_INTERVAL_MS));
                if too_soon {
                    return self.send_to(conn, &ServerMsg::Ack(Ack::error(req, "one toast every 2 seconds, please")));
                }
                if let Some(c) = self.conns.get_mut(&conn) {
                    c.last_toast = Some(now);
                }
                self.broadcast(&ServerMsg::Toast { text, by: name, conn });
                self.send_to(conn, &ServerMsg::Ack(Ack::ok(req)));
            }
            ClientMsg::Ping => self.send_to(conn, &ServerMsg::Pong),
        }
    }

    /// `set` (protocol §3): unknown key, then version check, then validation
    /// and the cross-key rule, then store. Returns the ack and, on success,
    /// the `changed` to broadcast after it.
    fn set(&mut self, conn: u64, name: &str, key: &str, base: u64, value: serde_json::Value, force: bool) -> (Ack, Option<ServerMsg>) {
        let req_ack = |a: Ack| (a, None);
        let Some(cur) = self.values.get(key).cloned() else {
            return req_ack(Ack::error(0, "unknown key"));
        };
        if !force && base != cur.version {
            return req_ack(Ack::conflict(0, cur));
        }
        let norm = match keys::validate_with(key, &value, |k| self.values.get(k).map(|e| e.value.clone())) {
            Ok(v) => v,
            Err(e) => return req_ack(Ack::error(0, e.to_string())),
        };
        let entry = Entry { value: norm, version: cur.version + 1, by: name.to_string(), at: now_utc() };
        if let Err(e) = self.store.put(key, &entry) {
            tracing::error!(key, "store write failed: {e}");
            return req_ack(Ack::error(0, "internal error"));
        }
        let changed = ServerMsg::Changed {
            key: key.to_string(),
            value: entry.value.clone(),
            version: entry.version,
            by: entry.by.clone(),
            conn,
            at: entry.at.clone(),
        };
        let version = entry.version;
        self.values.insert(key.to_string(), entry);
        (Ack { ok: true, version: Some(version), ..Default::default() }, Some(changed))
    }

    /// `reset`: every listed key (all when empty) back to its default with a
    /// bumped version, in one transaction; then one `changed` per key.
    fn reset(&mut self, conn: u64, name: &str, req: u64, list: Vec<String>) {
        if let Some(bad) = list.iter().find(|k| !keys::is_key(k)) {
            let msg = format!("unknown key: {}", bad.chars().take(64).collect::<String>());
            return self.send_to(conn, &ServerMsg::Ack(Ack::error(req, msg)));
        }
        let wanted: BTreeSet<&str> = list.iter().map(String::as_str).collect();
        let at = now_utc();
        // catalogue order, so every server broadcasts in the same order
        let entries: Vec<(String, Entry)> = KEYS
            .iter()
            .filter(|k| wanted.is_empty() || wanted.contains(k.key))
            .filter_map(|k| {
                let version = self.values.get(k.key).map_or(0, |e| e.version) + 1;
                default_entry(k.key, version, name, &at).map(|e| (k.key.to_string(), e))
            })
            .collect();
        if let Err(e) = self.store.put_many(&entries) {
            tracing::error!("store write failed: {e}");
            return self.send_to(conn, &ServerMsg::Ack(Ack::error(req, "internal error")));
        }
        self.send_to(conn, &ServerMsg::Ack(Ack::ok(req)));
        for (key, e) in entries {
            let changed =
                ServerMsg::Changed { key: key.clone(), value: e.value.clone(), version: e.version, by: e.by.clone(), conn, at: e.at.clone() };
            self.values.insert(key, e);
            self.broadcast(&changed);
        }
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
            .map(|(id, c)| Peer { conn: *id, name: c.name.clone(), color: peer_color(*id).into(), client: c.client.clone(), tab: c.tab.clone() })
            .collect();
        let text = json(&ServerMsg::Presence { peers });
        // presence is best-effort: a full queue just misses this update
        for c in self.conns.values() {
            let _ = c.out.try_send(Outbound::Text(text.clone()));
        }
    }
}
