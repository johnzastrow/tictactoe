//! SQLite store: one row per control key (docs/demo/schema.sql). Values are
//! JSON text. Only the hub writes, from one thread, so there is exactly one
//! writer. Every value is a bound parameter.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use demo_core::Entry;
use demo_core::date::rfc3339;
use demo_core::keys::{self, KEYS};
use rusqlite::{Connection, params};
use serde_json::Value;

/// A copy of docs/demo/schema.sql, embedded verbatim (a test checks for drift).
pub const SCHEMA: &str = include_str!("schema.sql");
pub const SCHEMA_VERSION: &str = "1";

pub struct Store {
    conn: Connection,
}

/// Now, UTC, to the second: `2026-10-18T09:03:11Z`.
pub fn now_utc() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    rfc3339(secs)
}

/// The default entry for a key at `version`, as written by a reset.
pub fn default_entry(key: &str, version: u64, by: &str, at: &str) -> Option<Entry> {
    keys::default_value(key).map(|value| Entry { value, version, by: by.to_string(), at: at.to_string() })
}

impl Store {
    pub fn open(path: &Path) -> anyhow::Result<Store> {
        Self::init(Connection::open(path)?)
    }

    pub fn open_memory() -> anyhow::Result<Store> {
        Self::init(Connection::open_in_memory()?)
    }

    /// Apply the schema, then insert any missing key with its default.
    fn init(conn: Connection) -> anyhow::Result<Store> {
        conn.execute_batch("PRAGMA busy_timeout = 5000; PRAGMA synchronous = NORMAL;")?;
        conn.execute_batch(SCHEMA)?;
        conn.execute("INSERT OR IGNORE INTO meta (key, value) VALUES ('schema_version', ?1)", [SCHEMA_VERSION])?;
        let at = now_utc();
        {
            let mut st = conn.prepare(
                "INSERT OR IGNORE INTO control (key, value, version, updated_by, updated_at) VALUES (?1, ?2, 1, '', ?3)",
            )?;
            for (key, value) in keys::defaults() {
                st.execute(params![key, value.to_string(), at])?;
            }
        }
        Ok(Store { conn })
    }

    /// Every known key's entry. A row whose JSON is unreadable or no longer
    /// valid (edited by hand, older build) is rewritten to its default with
    /// a bumped version, so clients never see a value the core rejects.
    pub fn load(&mut self) -> anyhow::Result<BTreeMap<String, Entry>> {
        let rows: Vec<(String, String, i64, String, String)> = {
            let mut st = self.conn.prepare("SELECT key, value, version, updated_by, updated_at FROM control")?;
            st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?
                .collect::<rusqlite::Result<_>>()?
        };
        let mut out = BTreeMap::new();
        let mut repairs = Vec::new();
        for (key, text, version, by, at) in rows {
            if !keys::is_key(&key) {
                continue; // a key from another build: leave the row alone
            }
            let version = version.max(0) as u64;
            let valid = serde_json::from_str::<Value>(&text).ok().and_then(|v| keys::validate(&key, &v).ok());
            match valid {
                Some(value) => {
                    out.insert(key, Entry { value, version, by, at });
                }
                None => {
                    tracing::warn!(key = %key, "stored value is invalid; restoring the default");
                    if let Some(e) = default_entry(&key, version + 1, "", &now_utc()) {
                        repairs.push((key.clone(), e.clone()));
                        out.insert(key, e);
                    }
                }
            }
        }
        if !repairs.is_empty() {
            self.put_many(&repairs)?;
        }
        Ok(out)
    }

    /// Write one key's entry.
    pub fn put(&mut self, key: &str, e: &Entry) -> anyhow::Result<()> {
        self.put_many(&[(key.to_string(), e.clone())])
    }

    /// Write several entries in one transaction (all or nothing).
    pub fn put_many(&mut self, entries: &[(String, Entry)]) -> anyhow::Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut st = tx.prepare_cached(
                "INSERT INTO control (key, value, version, updated_by, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value, version = excluded.version,
                   updated_by = excluded.updated_by, updated_at = excluded.updated_at",
            )?;
            for (key, e) in entries {
                st.execute(params![key, e.value.to_string(), e.version as i64, e.by, e.at])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// `demo reset`: every key back to its default, versions bumped, `by` empty.
    /// Returns the number of keys reset.
    pub fn reset_all(&mut self) -> anyhow::Result<usize> {
        let current = self.load()?;
        let at = now_utc();
        let entries: Vec<(String, Entry)> = KEYS
            .iter()
            .filter_map(|k| {
                let version = current.get(k.key).map_or(0, |e| e.version) + 1;
                default_entry(k.key, version, "", &at).map(|e| (k.key.to_string(), e))
            })
            .collect();
        self.put_many(&entries)?;
        Ok(entries.len())
    }

    pub fn schema_version(&self) -> anyhow::Result<String> {
        Ok(self.conn.query_row("SELECT value FROM meta WHERE key = 'schema_version'", [], |r| r.get(0))?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn schema_matches_docs() {
        let docs = include_str!("../../../../docs/demo/schema.sql");
        assert_eq!(SCHEMA, docs, "crates/demo-server/src/schema.sql drifted from docs/demo/schema.sql: copy it again");
    }

    #[test]
    fn fresh_store_has_every_default() {
        let mut s = Store::open_memory().unwrap();
        assert_eq!(s.schema_version().unwrap(), "1");
        let v = s.load().unwrap();
        assert_eq!(v.len(), KEYS.len());
        for (key, def) in keys::defaults() {
            let e = &v[key];
            assert_eq!(e.value, def);
            assert_eq!((e.version, e.by.as_str()), (1, ""));
            assert_eq!(e.at.len(), 20);
        }
    }

    #[test]
    fn put_load_reset() {
        let mut s = Store::open_memory().unwrap();
        let e = Entry { value: json!("done"), version: 2, by: "bob".into(), at: "2026-10-18T09:00:00Z".into() };
        s.put("select.status", &e).unwrap();
        assert_eq!(s.load().unwrap()["select.status"], e);
        assert_eq!(s.reset_all().unwrap(), KEYS.len());
        let v = s.load().unwrap();
        assert_eq!(v["select.status"].value, json!("in_progress"));
        assert_eq!(v["select.status"].version, 3);
        assert_eq!(v["text.name"].version, 2);
        assert_eq!(v["select.status"].by, "");
    }

    #[test]
    fn invalid_rows_are_repaired() {
        let mut s = Store::open_memory().unwrap();
        s.conn.execute("UPDATE control SET value = 'not json', version = 4 WHERE key = 'text.code'", []).unwrap();
        s.conn.execute("UPDATE control SET value = '\"lowercase\"' WHERE key = 'select.status'", []).unwrap();
        s.conn.execute("INSERT INTO control (key, value, updated_at) VALUES ('old.key', '1', '')", []).unwrap();
        let v = s.load().unwrap();
        assert_eq!(v["text.code"].value, json!("ENG-042"));
        assert_eq!(v["text.code"].version, 5);
        assert_eq!(v["select.status"].value, json!("in_progress"));
        assert!(!v.contains_key("old.key"));
        // the repair was persisted
        assert_eq!(s.load().unwrap()["text.code"].version, 5);
    }

    #[test]
    fn reopen_keeps_values() {
        let dir = std::env::temp_dir().join(format!("demo-store-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&dir);
        {
            let mut s = Store::open(&dir).unwrap();
            let e = Entry { value: json!(77), version: 9, by: "x".into(), at: "2026-10-18T09:00:00Z".into() };
            s.put("number.percent", &e).unwrap();
        }
        let mut s = Store::open(&dir).unwrap();
        assert_eq!(s.load().unwrap()["number.percent"].version, 9);
        drop(s);
        for ext in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{ext}", dir.display()));
        }
    }
}
