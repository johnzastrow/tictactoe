-- demo SQLite schema (normative; both servers embed this file verbatim).
-- One row per control key from protocol.md §5. Values are JSON text.
-- The server inserts any missing key with its default at startup, and
-- `demo reset` rewrites every row to its default.

PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
-- meta rows: ('schema_version', '1')

CREATE TABLE IF NOT EXISTS control (
  key        TEXT PRIMARY KEY,
  value      TEXT NOT NULL,              -- JSON
  version    INTEGER NOT NULL DEFAULT 1,
  updated_by TEXT NOT NULL DEFAULT '',   -- display name, '' for a default
  updated_at TEXT NOT NULL               -- UTC, 'YYYY-MM-DDTHH:MM:SSZ'
);
