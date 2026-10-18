// Package store is demo's SQLite store: one row per control key holding its
// current value (JSON), version, and who changed it when (schema.sql, a
// drift-checked copy of docs/demo/schema.sql).
//
// Only the hub writes, from one goroutine; `demo reset` may also open the
// file. Every SQL statement is a constant; every value is a bound parameter.
package store

import (
	"database/sql"
	_ "embed"
	"encoding/json"
	"fmt"
	"net/url"

	_ "modernc.org/sqlite"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/demo/core"
)

//go:embed schema.sql
var Schema string

const schemaVersion = "1"

type Store struct {
	db *sql.DB
}

// Open opens (creating if needed) a database file, applies the schema and
// inserts any missing key with its default.
func Open(path string) (*Store, error) {
	dsn := "file:" + url.PathEscape(path) +
		"?_pragma=journal_mode(WAL)&_pragma=busy_timeout(5000)&_pragma=synchronous(NORMAL)"
	return open(dsn)
}

// OpenMemory opens a private in-memory database (tests).
func OpenMemory() (*Store, error) { return open("file::memory:") }

func open(dsn string) (*Store, error) {
	db, err := sql.Open("sqlite", dsn)
	if err != nil {
		return nil, err
	}
	// One writer by design (the hub); one connection keeps pragmas and an
	// in-memory database alive.
	db.SetMaxOpenConns(1)
	db.SetMaxIdleConns(1)
	db.SetConnMaxLifetime(0)
	if _, err := db.Exec(Schema); err != nil {
		db.Close()
		return nil, fmt.Errorf("applying schema: %w", err)
	}
	s := &Store{db: db}
	if err := s.init(core.NowTS()); err != nil {
		db.Close()
		return nil, err
	}
	return s, nil
}

func (s *Store) Close() error { return s.db.Close() }

// init records the schema version and inserts every missing key at its
// default (version 1, by "").
func (s *Store) init(at string) error {
	tx, err := s.db.Begin()
	if err != nil {
		return err
	}
	defer tx.Rollback()
	if _, err := tx.Exec(`INSERT OR IGNORE INTO meta (key, value) VALUES ('schema_version', ?)`, schemaVersion); err != nil {
		return err
	}
	for _, k := range core.Keys {
		if _, err := tx.Exec(`INSERT OR IGNORE INTO control (key, value, version, updated_by, updated_at) VALUES (?, ?, 1, '', ?)`,
			k, string(core.Default(k)), at); err != nil {
			return err
		}
	}
	return tx.Commit()
}

// Get returns one key's entry. A stored value that is not valid JSON (a
// hand-edited file) reads as the default, keeping its version.
func (s *Store) Get(key string) (core.Entry, error) {
	var (
		e core.Entry
		v string
	)
	err := s.db.QueryRow(`SELECT value, version, updated_by, updated_at FROM control WHERE key = ?`, key).
		Scan(&v, &e.Version, &e.By, &e.At)
	if err != nil {
		return e, fmt.Errorf("reading %s: %w", key, err)
	}
	e.Value = sane(key, v)
	return e, nil
}

func sane(key, v string) json.RawMessage {
	if !json.Valid([]byte(v)) {
		return core.Default(key)
	}
	return json.RawMessage(v)
}

// All returns every catalogue key's entry (rows for unknown keys are skipped).
func (s *Store) All() (map[string]core.Entry, error) {
	rows, err := s.db.Query(`SELECT key, value, version, updated_by, updated_at FROM control`)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := make(map[string]core.Entry, len(core.Keys))
	for rows.Next() {
		var (
			k, v string
			e    core.Entry
		)
		if err := rows.Scan(&k, &v, &e.Version, &e.By, &e.At); err != nil {
			return nil, err
		}
		if core.IsKey(k) {
			e.Value = sane(k, v)
			out[k] = e
		}
	}
	if err := rows.Err(); err != nil {
		return nil, err
	}
	for _, k := range core.Keys {
		if _, ok := out[k]; !ok {
			return nil, fmt.Errorf("key %s missing from the database", k)
		}
	}
	return out, nil
}

// Put stores a key's new entry (value already normalised by the caller).
func (s *Store) Put(key string, e core.Entry) error {
	res, err := s.db.Exec(`UPDATE control SET value = ?, version = ?, updated_by = ?, updated_at = ? WHERE key = ?`,
		string(e.Value), e.Version, e.By, e.At, key)
	if err != nil {
		return err
	}
	if n, _ := res.RowsAffected(); n != 1 {
		return fmt.Errorf("no row for %s", key)
	}
	return nil
}

// Reset restores keys (every key when empty) to their defaults in one
// transaction, bumping each version, and returns the new entries in the
// order given. Keys must be catalogue keys.
func (s *Store) Reset(keys []string, by string, at string) ([]core.Entry, error) {
	if len(keys) == 0 {
		keys = core.Keys
	}
	tx, err := s.db.Begin()
	if err != nil {
		return nil, err
	}
	defer tx.Rollback()
	out := make([]core.Entry, 0, len(keys))
	for _, k := range keys {
		if !core.IsKey(k) {
			return nil, fmt.Errorf("unknown key %q", k)
		}
		var version int64
		if err := tx.QueryRow(`SELECT version FROM control WHERE key = ?`, k).Scan(&version); err != nil {
			return nil, fmt.Errorf("reading %s: %w", k, err)
		}
		e := core.Entry{Value: core.Default(k), Version: version + 1, By: by, At: at}
		if _, err := tx.Exec(`UPDATE control SET value = ?, version = ?, updated_by = ?, updated_at = ? WHERE key = ?`,
			string(e.Value), e.Version, e.By, e.At, k); err != nil {
			return nil, err
		}
		out = append(out, e)
	}
	return out, tx.Commit()
}
