//! The client-side value store: what a TUI or web client knows about the
//! server's controls. Feed it every [`ServerMsg`] with [`Values::handle`];
//! build outgoing messages with [`Values::set`] / [`Values::reset`] /
//! [`Values::toast`], which assign `req` ids and remember what each request
//! was for, so acks come back as meaningful [`Event`]s.
//!
//! The store never applies an edit optimistically: a value changes only on
//! `welcome`, `changed`, a successful `ack` of our own `set`, or the
//! `current` entry of a conflict. Until `welcome` arrives every key holds its
//! default at version 0.

use std::collections::BTreeMap;

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::date::{Date, relative_time};
use crate::keys::{self, KEYS, SetError};
use crate::protocol::{Ack, ClientMsg, Entry, Peer, ServerInfo, ServerMsg, You, clean_toast};

/// What an outstanding request was for.
#[derive(Clone, Debug, PartialEq)]
enum Pending {
    Set { key: String, value: Value },
    Reset { keys: Vec<String> },
    Toast,
}

/// The outcome of one server message, for the UI's status line, flashes and
/// toasts. The store has already been updated when this is returned.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// All values replaced (first connect or reconnect).
    Welcome,
    /// A key changed. `mine` is true for the echo of our own set/reset.
    Changed { key: String, by: String, conn: u64, mine: bool },
    /// Our `set` was stored at `version`.
    Saved { key: String, version: u64 },
    /// Our `set` lost a race; the store now holds the server's entry.
    Conflict { key: String, by: String },
    /// Our `reset` was applied (each key also arrives as `Changed`).
    ResetDone { keys: Vec<String> },
    /// Our `toast` was broadcast.
    ToastSent,
    /// A request was refused. `key` is set for a refused `set`.
    Rejected { key: Option<String>, error: String },
    /// The peer list changed; read it with [`Values::peers`].
    Presence,
    /// A broadcast toast (ours too: `mine`).
    Toast { text: String, by: String, conn: u64, mine: bool },
    Pong,
    /// The server could not read something we sent.
    ServerError { message: String },
    /// An ack for a request we don't know (e.g. sent before a reconnect).
    Ignored,
}

#[derive(Clone, Debug)]
pub struct Values {
    entries: BTreeMap<String, Entry>,
    me: Option<You>,
    server: Option<ServerInfo>,
    peers: Vec<Peer>,
    next_req: u64,
    pending: BTreeMap<u64, Pending>,
}

impl Default for Values {
    fn default() -> Self {
        Self::new()
    }
}

impl Values {
    /// Every key at its default, version 0, no peers.
    pub fn new() -> Values {
        let entries = keys::defaults()
            .into_iter()
            .map(|(k, value)| (k.to_string(), Entry { value, version: 0, by: String::new(), at: String::new() }))
            .collect();
        Values { entries, me: None, server: None, peers: Vec::new(), next_req: 1, pending: BTreeMap::new() }
    }

    // ---- incoming ----

    /// Apply a server message and say what happened.
    pub fn handle(&mut self, msg: ServerMsg) -> Event {
        match msg {
            ServerMsg::Welcome { server, you, values, .. } => {
                // keep defaults for any key an older server didn't send
                let mut fresh = Values::new().entries;
                for (k, e) in values {
                    if keys::is_key(&k) {
                        fresh.insert(k, e);
                    }
                }
                self.entries = fresh;
                self.server = Some(server);
                self.me = Some(you);
                self.pending.clear();
                Event::Welcome
            }
            ServerMsg::Changed { key, value, version, by, conn, at } => {
                let mine = self.my_conn() == Some(conn);
                self.apply(&key, Entry { value, version, by: by.clone(), at });
                Event::Changed { key, by, conn, mine }
            }
            ServerMsg::Ack(ack) => self.ack(ack),
            ServerMsg::Presence { peers } => {
                self.peers = peers;
                Event::Presence
            }
            ServerMsg::Toast { text, by, conn } => {
                let mine = self.my_conn() == Some(conn);
                Event::Toast { text, by, conn, mine }
            }
            ServerMsg::Pong => Event::Pong,
            ServerMsg::Error { message } => Event::ServerError { message },
        }
    }

    /// Parse and apply one text frame. Unreadable frames become `ServerError`.
    pub fn handle_text(&mut self, text: &str) -> Event {
        match serde_json::from_str::<ServerMsg>(text) {
            Ok(m) => self.handle(m),
            Err(e) => Event::ServerError { message: format!("unreadable server message: {e}") },
        }
    }

    /// Store an entry unless we already hold a newer version.
    fn apply(&mut self, key: &str, e: Entry) {
        if !keys::is_key(key) {
            return;
        }
        match self.entries.get_mut(key) {
            Some(cur) if cur.version > e.version => {}
            Some(cur) => *cur = e,
            None => {
                self.entries.insert(key.to_string(), e);
            }
        }
    }

    fn ack(&mut self, ack: Ack) -> Event {
        let Some(p) = self.pending.remove(&ack.req) else { return Event::Ignored };
        match p {
            Pending::Set { key, value } => {
                if ack.ok {
                    let version = ack.version.unwrap_or(0);
                    let by = self.me.as_ref().map(|m| m.name.clone()).unwrap_or_default();
                    let at = self.entries.get(&key).map(|e| e.at.clone()).unwrap_or_default();
                    // `changed` follows with the authoritative `at`
                    self.apply(&key, Entry { value, version, by, at });
                    Event::Saved { key, version }
                } else if ack.conflict {
                    let by = ack.current.as_ref().map(|c| c.by.clone()).unwrap_or_default();
                    if let Some(cur) = ack.current {
                        self.apply(&key, cur);
                    }
                    Event::Conflict { key, by }
                } else {
                    Event::Rejected { key: Some(key), error: ack.error.unwrap_or_else(|| "refused".into()) }
                }
            }
            Pending::Reset { keys } if ack.ok => Event::ResetDone { keys },
            Pending::Toast if ack.ok => Event::ToastSent,
            _ => Event::Rejected { key: None, error: ack.error.unwrap_or_else(|| "refused".into()) },
        }
    }

    // ---- outgoing ----

    fn req(&mut self, p: Pending) -> u64 {
        let req = self.next_req;
        self.next_req += 1;
        self.pending.insert(req, p);
        req
    }

    /// A `set` for `key`, validated and normalised exactly as the server
    /// will (including the date cross-key rule against our current values),
    /// based on the version we hold. `Err` means: show the message, send nothing.
    pub fn set(&mut self, key: &str, value: Value) -> Result<ClientMsg, SetError> {
        self.build_set(key, value, false)
    }

    /// Like [`Values::set`] but with `force: true` (skips the version check).
    pub fn set_forced(&mut self, key: &str, value: Value) -> Result<ClientMsg, SetError> {
        self.build_set(key, value, true)
    }

    fn build_set(&mut self, key: &str, value: Value, force: bool) -> Result<ClientMsg, SetError> {
        let norm = keys::validate_with(key, &value, |k| self.value(k).cloned())?;
        let base = self.version(key);
        let req = self.req(Pending::Set { key: key.to_string(), value: norm.clone() });
        Ok(ClientMsg::Set { req, key: key.to_string(), base, value: norm, force })
    }

    /// A `reset` of the given keys (empty = all). Unknown keys are an error.
    pub fn reset(&mut self, which: &[&str]) -> Result<ClientMsg, SetError> {
        if which.iter().any(|k| !keys::is_key(k)) {
            return Err(SetError::UnknownKey);
        }
        let list: Vec<String> = which.iter().map(|k| k.to_string()).collect();
        let all = if list.is_empty() { KEYS.iter().map(|k| k.key.to_string()).collect() } else { list.clone() };
        let req = self.req(Pending::Reset { keys: all });
        Ok(ClientMsg::Reset { req, keys: list })
    }

    /// Reset every key of one tab (`^R`).
    pub fn reset_tab(&mut self, tab_id: &str) -> Result<ClientMsg, SetError> {
        let tab = crate::dataset::tab_by_id(tab_id).ok_or(SetError::UnknownKey)?;
        self.reset(tab.keys)
    }

    /// A `toast` broadcast, or `None` if the text is empty or over 120 chars.
    pub fn toast(&mut self, text: &str) -> Option<ClientMsg> {
        let text = clean_toast(text)?;
        let req = self.req(Pending::Toast);
        Some(ClientMsg::Toast { req, text })
    }

    /// `focus` for a tab id (no reply comes back).
    pub fn focus(tab_id: &str) -> ClientMsg {
        ClientMsg::Focus { tab: tab_id.to_string() }
    }

    /// Forget outstanding requests (call when the socket drops).
    pub fn disconnected(&mut self) {
        self.pending.clear();
        self.peers.clear();
    }

    /// Requests still waiting for an ack.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    // ---- reading ----

    pub fn entry(&self, key: &str) -> Option<&Entry> {
        self.entries.get(key)
    }

    pub fn value(&self, key: &str) -> Option<&Value> {
        self.entries.get(key).map(|e| &e.value)
    }

    /// 0 for unknown keys and before `welcome`.
    pub fn version(&self, key: &str) -> u64 {
        self.entries.get(key).map_or(0, |e| e.version)
    }

    /// A key's value deserialised into `T` (e.g. `Vec<GridRow>`, `CascadePick`,
    /// `Option<String>`, `f64`). `None` if unknown or the shape doesn't fit.
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.value(key).and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    /// String value, `""` if unset/null/not a string.
    pub fn str(&self, key: &str) -> &str {
        self.value(key).and_then(Value::as_str).unwrap_or("")
    }

    /// Optional string (combo.owner, date.*): `None` for null.
    pub fn opt_str(&self, key: &str) -> Option<&str> {
        self.value(key).and_then(Value::as_str)
    }

    pub fn f64(&self, key: &str) -> f64 {
        self.value(key).and_then(Value::as_f64).unwrap_or(0.0)
    }

    pub fn i64(&self, key: &str) -> i64 {
        self.value(key).and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64))).unwrap_or(0)
    }

    pub fn bool(&self, key: &str) -> bool {
        self.value(key).and_then(Value::as_bool).unwrap_or(false)
    }

    /// A date key (`date.start`, `date.due`) as a [`Date`]; `None` for null.
    pub fn date(&self, key: &str) -> Option<Date> {
        self.opt_str(key).and_then(Date::parse_iso)
    }

    /// A list of strings (`tags.tags`, `choice.channels`).
    pub fn strings(&self, key: &str) -> Vec<String> {
        self.get(key).unwrap_or_default()
    }

    /// The value as compact JSON, for the Value panel: `"Ada Lovelace"`.
    pub fn value_text(&self, key: &str) -> String {
        self.value(key).map(|v| serde_json::to_string(v).unwrap_or_default()).unwrap_or_default()
    }

    /// The Value panel's second line: `v3 · by bob · 2m ago`
    /// (`v1 · default` for a value nobody changed).
    pub fn describe(&self, key: &str, now_secs: i64) -> String {
        let Some(e) = self.entry(key) else { return String::new() };
        if e.by.is_empty() {
            return format!("v{} · default", e.version);
        }
        let when = if e.at.is_empty() { String::new() } else { format!(" · {}", relative_time(&e.at, now_secs)) };
        format!("v{} · by {}{}", e.version, e.by, when)
    }

    // ---- connection and peers ----

    pub fn me(&self) -> Option<&You> {
        self.me.as_ref()
    }

    pub fn my_conn(&self) -> Option<u64> {
        self.me.as_ref().map(|m| m.conn)
    }

    pub fn server(&self) -> Option<&ServerInfo> {
        self.server.as_ref()
    }

    /// Everyone connected, us included, in `conn` order as the server sent it.
    pub fn peers(&self) -> &[Peer] {
        &self.peers
    }

    /// Everyone but us.
    pub fn others(&self) -> impl Iterator<Item = &Peer> {
        let me = self.my_conn();
        self.peers.iter().filter(move |p| Some(p.conn) != me)
    }

    /// Other peers whose last focus is `tab_id` (for the sidebar dots).
    pub fn others_on(&self, tab_id: &str) -> Vec<&Peer> {
        self.others().filter(|p| p.tab.as_deref() == Some(tab_id)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::GridRow;
    use serde_json::json;

    fn welcome(conn: u64) -> ServerMsg {
        let values = keys::defaults()
            .into_iter()
            .map(|(k, v)| (k.to_string(), Entry { value: v, version: 1, by: String::new(), at: "2026-10-18T09:00:00Z".into() }))
            .collect();
        ServerMsg::Welcome {
            protocol: 1,
            server: ServerInfo { implementation: "rust".into(), version: "0.5.0".into() },
            you: You { conn, name: "alice".into(), color: crate::dataset::peer_color(conn).into() },
            values,
        }
    }

    #[test]
    fn defaults_before_welcome() {
        let v = Values::new();
        assert_eq!(v.str("text.name"), "Ada Lovelace");
        assert_eq!(v.version("text.name"), 0);
        assert_eq!(v.opt_str("combo.reviewer"), None);
        assert_eq!(v.date("date.start").unwrap().iso(), "2026-11-02");
        assert_eq!(v.f64("number.hours"), 12.5);
        assert_eq!(v.i64("number.qty"), 3);
        assert!(v.bool("choice.notify"));
        assert_eq!(v.strings("tags.tags"), vec!["frontend", "q4"]);
        assert_eq!(v.get::<Vec<GridRow>>("grid.rows").unwrap().len(), 6);
        assert_eq!(v.value_text("text.code"), "\"ENG-042\"");
    }

    #[test]
    fn set_ack_changed_flow() {
        let mut v = Values::new();
        assert_eq!(v.handle(welcome(3)), Event::Welcome);
        assert_eq!(v.version("select.status"), 1);
        let msg = v.set("select.status", json!("done")).unwrap();
        let ClientMsg::Set { req, base, .. } = msg else { panic!() };
        assert_eq!(base, 1);
        assert_eq!(v.pending_count(), 1);
        let ev = v.handle(ServerMsg::Ack(Ack { req, ok: true, version: Some(2), ..Default::default() }));
        assert_eq!(ev, Event::Saved { key: "select.status".into(), version: 2 });
        assert_eq!(v.str("select.status"), "done");
        assert_eq!(v.version("select.status"), 2);
        let ev = v.handle(ServerMsg::Changed {
            key: "select.status".into(),
            value: json!("done"),
            version: 2,
            by: "alice".into(),
            conn: 3,
            at: "2026-10-18T09:05:00Z".into(),
        });
        assert_eq!(ev, Event::Changed { key: "select.status".into(), by: "alice".into(), conn: 3, mine: true });
        assert_eq!(v.entry("select.status").unwrap().at, "2026-10-18T09:05:00Z");
        assert_eq!(v.describe("select.status", 0).split(" · ").take(2).collect::<Vec<_>>(), ["v2", "by alice"]);
        // a stale changed is ignored
        v.handle(ServerMsg::Changed {
            key: "select.status".into(),
            value: json!("todo"),
            version: 1,
            by: "bob".into(),
            conn: 4,
            at: String::new(),
        });
        assert_eq!(v.str("select.status"), "done");
        assert_eq!(v.describe("text.name", 0), "v1 · default");
    }

    #[test]
    fn normalises_and_rejects_locally() {
        let mut v = Values::new();
        v.handle(welcome(1));
        let ClientMsg::Set { value, .. } = v.set("text.code", json!("eng-007")).unwrap() else { panic!() };
        assert_eq!(value, json!("ENG-007"));
        assert!(matches!(v.set("text.code", json!("nope")), Err(SetError::Invalid(_))));
        assert_eq!(v.set("nope", json!(1)), Err(SetError::UnknownKey));
        // cross-key rule against our values: due 2026-11-20
        assert!(v.set("date.start", json!("2026-12-01")).is_err());
        assert!(v.set("date.start", json!("2026-11-20")).is_ok());
    }

    #[test]
    fn conflict_and_rejection() {
        let mut v = Values::new();
        v.handle(welcome(1));
        let ClientMsg::Set { req, .. } = v.set("text.name", json!("Me")).unwrap() else { panic!() };
        let cur = Entry { value: json!("Bob's"), version: 5, by: "bob".into(), at: "2026-10-18T09:00:00Z".into() };
        let ev = v.handle(ServerMsg::Ack(Ack::conflict(req, cur)));
        assert_eq!(ev, Event::Conflict { key: "text.name".into(), by: "bob".into() });
        assert_eq!(v.str("text.name"), "Bob's");
        assert_eq!(v.version("text.name"), 5);
        let ClientMsg::Set { req, .. } = v.set("text.name", json!("Me")).unwrap() else { panic!() };
        let ev = v.handle(ServerMsg::Ack(Ack::error(req, "nope")));
        assert_eq!(ev, Event::Rejected { key: Some("text.name".into()), error: "nope".into() });
        assert_eq!(v.handle(ServerMsg::Ack(Ack::ok(999))), Event::Ignored);
    }

    #[test]
    fn reset_toast_presence() {
        let mut v = Values::new();
        v.handle(welcome(2));
        let ClientMsg::Reset { req, keys } = v.reset_tab("date").unwrap() else { panic!() };
        assert_eq!(keys, vec!["date.start", "date.due"]);
        assert_eq!(v.handle(ServerMsg::Ack(Ack::ok(req))), Event::ResetDone { keys: keys.clone() });
        let ClientMsg::Reset { keys, .. } = v.reset(&[]).unwrap() else { panic!() };
        assert!(keys.is_empty());
        assert!(v.reset(&["bogus"]).is_err());
        assert!(v.toast("   ").is_none());
        let ClientMsg::Toast { req, text } = v.toast(" hi ").unwrap() else { panic!() };
        assert_eq!(text, "hi");
        assert_eq!(v.handle(ServerMsg::Ack(Ack::ok(req))), Event::ToastSent);
        let peer = |conn: u64, tab: Option<&str>| Peer {
            conn,
            name: format!("p{conn}"),
            color: crate::dataset::peer_color(conn).into(),
            client: "conform".into(),
            tab: tab.map(String::from),
        };
        v.handle(ServerMsg::Presence { peers: vec![peer(1, Some("date")), peer(2, Some("date")), peer(3, None)] });
        assert_eq!(v.peers().len(), 3);
        assert_eq!(v.others().count(), 2);
        assert_eq!(v.others_on("date").len(), 1);
        let ev = v.handle_text(r#"{"type":"toast","text":"yo","by":"p1","conn":1}"#);
        assert_eq!(ev, Event::Toast { text: "yo".into(), by: "p1".into(), conn: 1, mine: false });
        assert!(matches!(v.handle_text("{"), Event::ServerError { .. }));
        v.disconnected();
        assert!(v.peers().is_empty());
    }
}
