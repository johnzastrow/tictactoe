//! Wire messages (protocol §5–6). Shared by the server and every Rust client.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::model::{Activity, Deletes, Entity, Tables, User};

pub const PROTOCOL_VERSION: u32 = 1;
pub const DEFAULT_PORT: u16 = 8788;
pub const MAX_FRAME: usize = 512 * 1024;
pub const MAX_OPS: usize = 2000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    Set {
        entity: Entity,
        id: i64,
        field: String,
        #[serde(default)]
        base: Value,
        #[serde(default)]
        value: Value,
        #[serde(default, skip_serializing_if = "is_false")]
        force: bool,
    },
    Create {
        entity: Entity,
        #[serde(rename = "ref", default, skip_serializing_if = "Option::is_none")]
        reference: Option<String>,
        #[serde(default)]
        values: Map<String, Value>,
    },
    Delete {
        entity: Entity,
        id: i64,
    },
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl Op {
    pub fn set(entity: Entity, id: i64, field: &str, base: Value, value: Value) -> Op {
        Op::Set {
            entity,
            id,
            field: field.to_string(),
            base,
            value,
            force: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    Mutate {
        req: u64,
        ops: Vec<Op>,
    },
    Undo {
        req: u64,
    },
    Redo {
        req: u64,
    },
    Focus {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        view: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        entity: Option<Entity>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        field: Option<String>,
    },
    SetPassword {
        req: u64,
        user_id: i64,
        password: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        old_password: Option<String>,
    },
    Resync,
    Ping,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ServerInfo {
    #[serde(rename = "impl")]
    pub implementation: String,
    pub version: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Conflict {
    pub index: usize,
    pub entity: Entity,
    pub id: i64,
    pub field: String,
    pub current: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
}

impl ErrorBody {
    pub fn new(code: &str, message: impl Into<String>) -> ErrorBody {
        ErrorBody {
            code: code.into(),
            message: message.into(),
            index: None,
        }
    }
    pub fn at(mut self, index: usize) -> ErrorBody {
        self.index = Some(index);
        self
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ResultMsg {
    pub req: u64,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<i64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub created: BTreeMap<String, i64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicts: Vec<Conflict>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorBody>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Focus {
    pub entity: Entity,
    pub id: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Peer {
    pub conn: u64,
    pub user_id: i64,
    pub client: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<Focus>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Changes {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<i64>,
    #[serde(default)]
    pub upserts: Tables,
    #[serde(default)]
    pub deletes: Deletes,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub activity: Vec<Activity>,
}

// Messages are short-lived; boxing the large variants buys nothing.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    Welcome {
        protocol: u32,
        server: ServerInfo,
        me: User,
        conn: u64,
    },
    Snapshot {
        data: Tables,
    },
    Changes(Changes),
    Result(ResultMsg),
    Presence {
        peers: Vec<Peer>,
    },
    Error {
        code: String,
        message: String,
    },
    Pong,
}

/// HTTP bodies.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub client: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoginResponse {
    pub token: String,
    pub user: User,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HealthInfo {
    pub ok: bool,
    #[serde(rename = "impl")]
    pub implementation: String,
    pub version: String,
    pub protocol: u32,
}

/// Valid `client` ids: `[a-z0-9-]{1,24}`.
pub fn valid_client_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 24 && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn op_wire_format() {
        let m: ClientMsg = serde_json::from_value(json!({
            "type":"mutate","req":7,"ops":[
              {"op":"set","entity":"task","id":301,"field":"status","base":"todo","value":"done"},
              {"op":"create","entity":"task","ref":"n1","values":{"project_id":12,"title":"Retro"}},
              {"op":"delete","entity":"task","id":299}
            ]})).unwrap();
        let ClientMsg::Mutate { req, ops } = &m else { panic!() };
        assert_eq!(*req, 7);
        assert_eq!(ops.len(), 3);
        assert!(matches!(&ops[1], Op::Create { reference: Some(r), .. } if r == "n1"));
        let back = serde_json::to_value(&m).unwrap();
        assert_eq!(back["ops"][0]["field"], "status");
        assert!(back["ops"][0].get("force").is_none());
    }

    #[test]
    fn server_msgs() {
        let r = ServerMsg::Result(ResultMsg { req: 3, ok: true, ..Default::default() });
        assert_eq!(serde_json::to_string(&r).unwrap(), r#"{"type":"result","req":3,"ok":true}"#);
        let p: ServerMsg = serde_json::from_str(r#"{"type":"pong"}"#).unwrap();
        assert_eq!(p, ServerMsg::Pong);
        let w = serde_json::to_value(ServerMsg::Welcome {
            protocol: 1,
            server: ServerInfo { implementation: "rust".into(), version: "x".into() },
            me: User { id: 1, username: "a".into(), display_name: "A".into(), role: crate::model::Role::Admin, active: true },
            conn: 2,
        }).unwrap();
        assert_eq!(w["server"]["impl"], "rust");
        assert_eq!(w["me"]["role"], "admin");
    }
}
