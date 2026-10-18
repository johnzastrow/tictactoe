//! Wire messages (protocol §2–3) and the transport constants of §1.
//! Shared by the server and every Rust client; the JSON shapes are exact.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROTOCOL_VERSION: u32 = 1;
pub const DEFAULT_PORT: u16 = 8790;
pub const DEFAULT_BIND: &str = "127.0.0.1:8790";
pub const DEFAULT_SERVER: &str = "http://127.0.0.1:8790";
/// Largest accepted text frame.
pub const MAX_FRAME: usize = 256 * 1024;
/// Token bucket per connection: sustained rate and burst.
pub const RATE_PER_SEC: f64 = 20.0;
pub const RATE_BURST: f64 = 40.0;
pub const MAX_SOCKETS: usize = 64;
/// Minimum gap between two `toast`s from one connection.
pub const TOAST_INTERVAL_MS: u64 = 2000;
pub const MAX_TOAST: usize = 120;
pub const PING_INTERVAL_SECS: u64 = 30;
/// WebSocket close code for frame-size and rate-limit violations.
pub const CLOSE_POLICY: u16 = 1008;

/// `"impl"` / `"version"` of a server.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerInfo {
    #[serde(rename = "impl")]
    pub implementation: String,
    pub version: String,
}

/// `GET /api/health`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthInfo {
    pub ok: bool,
    #[serde(rename = "impl")]
    pub implementation: String,
    pub version: String,
    pub protocol: u32,
}

/// The current state of one key: value, version, who set it and when.
/// `by` is `""` for a default never changed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub value: Value,
    pub version: u64,
    #[serde(default)]
    pub by: String,
    #[serde(default)]
    pub at: String,
}

/// `welcome.you`: this connection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct You {
    pub conn: u64,
    pub name: String,
    pub color: String,
}

/// One connected client in `presence` (the receiver included).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Peer {
    pub conn: u64,
    pub name: String,
    pub color: String,
    pub client: String,
    /// Tab id from the last `focus`; `null` before the first.
    #[serde(default)]
    pub tab: Option<String>,
}

/// Reply to `set` / `reset` / `toast`, matched by `req`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Ack {
    pub req: u64,
    pub ok: bool,
    /// The key's new version after a successful `set`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub conflict: bool,
    /// The server's entry when `conflict` is true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<Entry>,
}

impl Ack {
    pub fn ok(req: u64) -> Ack {
        Ack { req, ok: true, ..Default::default() }
    }
    pub fn error(req: u64, msg: impl Into<String>) -> Ack {
        Ack { req, error: Some(msg.into()), ..Default::default() }
    }
    pub fn conflict(req: u64, current: Entry) -> Ack {
        Ack { req, conflict: true, current: Some(current), ..Default::default() }
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    Welcome {
        protocol: u32,
        server: ServerInfo,
        you: You,
        /// Every key in protocol §5.
        values: BTreeMap<String, Entry>,
    },
    Changed {
        key: String,
        value: Value,
        version: u64,
        by: String,
        conn: u64,
        at: String,
    },
    Ack(Ack),
    Presence {
        peers: Vec<Peer>,
    },
    Toast {
        text: String,
        by: String,
        conn: u64,
    },
    Pong,
    /// Malformed JSON or unknown type; the socket stays open.
    Error {
        message: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    Set {
        req: u64,
        key: String,
        #[serde(default)]
        base: u64,
        value: Value,
        #[serde(default, skip_serializing_if = "is_false")]
        force: bool,
    },
    /// `keys` empty (or omitted) = every key.
    Reset {
        req: u64,
        #[serde(default)]
        keys: Vec<String>,
    },
    Focus {
        tab: String,
    },
    Toast {
        req: u64,
        text: String,
    },
    Ping,
}

/// `client` query parameter: `[a-z0-9-]{1,24}`.
pub fn valid_client_id(s: &str) -> bool {
    (1..=24).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// A display name after trimming spaces: `^[A-Za-z][A-Za-z0-9 _.-]{0,23}$`.
/// Returns the trimmed name, or `None` if it doesn't qualify.
pub fn clean_name(s: &str) -> Option<String> {
    let t = s.trim_matches(' ');
    let b = t.as_bytes();
    let ok = (1..=24).contains(&b.len())
        && b[0].is_ascii_alphabetic()
        && b[1..].iter().all(|&c| c.is_ascii_alphanumeric() || matches!(c, b' ' | b'_' | b'.' | b'-'));
    ok.then(|| t.to_string())
}

/// Toast text after trimming, if it is 1–120 chars without control characters.
pub fn clean_toast(s: &str) -> Option<String> {
    let t = s.trim();
    let ok = (1..=MAX_TOAST).contains(&t.chars().count()) && !crate::keys::has_control(t, false);
    ok.then(|| t.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn roundtrip_server(j: Value) {
        let m: ServerMsg = serde_json::from_value(j.clone()).unwrap_or_else(|e| panic!("{j}: {e}"));
        assert_eq!(serde_json::to_value(&m).unwrap(), j);
    }

    #[test]
    fn server_shapes() {
        roundtrip_server(json!({"type":"welcome","protocol":1,"server":{"impl":"go","version":"0.5.0"},
            "you":{"conn":7,"name":"alice","color":"#e06c75"},
            "values":{"text.name":{"value":"Ada Lovelace","version":1,"by":"","at":"2026-10-18T09:00:00Z"}}}));
        roundtrip_server(json!({"type":"changed","key":"select.status","value":"done","version":4,"by":"bob","conn":8,"at":"2026-10-18T09:03:11Z"}));
        roundtrip_server(json!({"type":"ack","req":12,"ok":true,"version":4}));
        roundtrip_server(json!({"type":"ack","req":13,"ok":false,"error":"due must be on or after start"}));
        roundtrip_server(json!({"type":"ack","req":14,"ok":false,"conflict":true,
            "current":{"value":"review","version":5,"by":"bob","at":"2026-10-18T09:03:11Z"}}));
        roundtrip_server(json!({"type":"presence","peers":[{"conn":7,"name":"alice","color":"#e06c75","client":"tui-rust","tab":"date"},
            {"conn":8,"name":"bob","color":"#61afef","client":"web-go","tab":null}]}));
        roundtrip_server(json!({"type":"toast","text":"Lunch is here","by":"bob","conn":8}));
        roundtrip_server(json!({"type":"pong"}));
        roundtrip_server(json!({"type":"error","message":"bad message"}));
        // a null tab from another server reads as None
        let p: Peer = serde_json::from_value(json!({"conn":1,"name":"a","color":"#000","client":"x","tab":null})).unwrap();
        assert_eq!(p.tab, None);
    }

    #[test]
    fn client_shapes() {
        let m: ClientMsg = serde_json::from_str(r#"{"type":"set","req":12,"key":"select.status","base":3,"value":"done"}"#).unwrap();
        assert_eq!(m, ClientMsg::Set { req: 12, key: "select.status".into(), base: 3, value: json!("done"), force: false });
        assert_eq!(
            serde_json::to_value(&m).unwrap(),
            json!({"type":"set","req":12,"key":"select.status","base":3,"value":"done"})
        );
        let m: ClientMsg = serde_json::from_str(r#"{"type":"set","req":1,"key":"k","base":3,"value":null,"force":true}"#).unwrap();
        assert!(matches!(m, ClientMsg::Set { force: true, value: Value::Null, .. }));
        let m: ClientMsg = serde_json::from_str(r#"{"type":"reset","req":15}"#).unwrap();
        assert_eq!(m, ClientMsg::Reset { req: 15, keys: vec![] });
        let m: ClientMsg = serde_json::from_str(r#"{"type":"focus","tab":"date"}"#).unwrap();
        assert_eq!(m, ClientMsg::Focus { tab: "date".into() });
        assert_eq!(serde_json::to_string(&ClientMsg::Ping).unwrap(), r#"{"type":"ping"}"#);
        assert!(serde_json::from_str::<ClientMsg>(r#"{"type":"explode"}"#).is_err());
        assert!(serde_json::from_str::<ClientMsg>(r#"{"type":"set","req":1}"#).is_err());
    }

    #[test]
    fn names_and_clients() {
        assert_eq!(clean_name("  alice "), Some("alice".into()));
        assert_eq!(clean_name("Mary-Jo O.K_2"), Some("Mary-Jo O.K_2".into()));
        assert_eq!(clean_name("a".repeat(24).as_str()), Some("a".repeat(24)));
        for bad in ["", "   ", "1abc", "_x", "a".repeat(25).as_str(), "al!ce", "zoë", "a\tb"] {
            assert_eq!(clean_name(bad), None, "{bad:?}");
        }
        assert!(valid_client_id("tui-rust"));
        assert!(valid_client_id("conform"));
        assert!(!valid_client_id(""));
        assert!(!valid_client_id("TUI"));
        assert!(!valid_client_id(&"a".repeat(25)));
        assert_eq!(clean_toast("  hi "), Some("hi".into()));
        assert_eq!(clean_toast("   "), None);
        assert_eq!(clean_toast(&"x".repeat(121)), None);
        assert_eq!(clean_toast("hi\u{1b}[2J"), None);
    }
}
