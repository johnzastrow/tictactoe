-- pt schema, version 1.
--
-- Normative and shared: the Rust server embeds this file directly and the Go
-- server embeds a copy that a test keeps byte-identical. Every statement is
-- idempotent so the file can be applied on every start.
--
-- Conventions
--   * Timestamps: RFC 3339 UTC text, e.g. 2026-10-17T14:03:00Z.
--   * Dates: ISO 8601 text YYYY-MM-DD, or NULL.
--   * Hours: REAL, >= 0.
--   * AUTOINCREMENT on rows that undo can re-create, so a deleted id is never
--     reused by an unrelated row.
--   * Connection setup (not here): PRAGMA foreign_keys=ON, journal_mode=WAL,
--     busy_timeout=5000, synchronous=NORMAL.

CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
INSERT OR IGNORE INTO meta (key, value) VALUES ('schema_version', '1');

CREATE TABLE IF NOT EXISTS users (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    username      TEXT    NOT NULL UNIQUE COLLATE NOCASE,
    display_name  TEXT    NOT NULL,
    role          TEXT    NOT NULL CHECK (role IN ('admin', 'member', 'viewer')),
    -- PHC string: $argon2id$v=19$m=19456,t=2,p=1$<salt b64>$<hash b64>
    password_hash TEXT    NOT NULL,
    active        INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
    created_at    TEXT    NOT NULL
);

-- Bearer/cookie sessions. Only the SHA-256 of the token is stored.
CREATE TABLE IF NOT EXISTS sessions (
    token_hash TEXT    PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at TEXT    NOT NULL,
    expires_at TEXT    NOT NULL,
    client     TEXT    NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS sessions_user ON sessions (user_id);

-- A program groups projects for one client or business line.
CREATE TABLE IF NOT EXISTS programs (
    id     INTEGER PRIMARY KEY AUTOINCREMENT,
    name   TEXT NOT NULL UNIQUE,
    client TEXT NOT NULL DEFAULT '',
    prefix TEXT NOT NULL DEFAULT '',          -- project code prefix, e.g. NWR
    color  TEXT NOT NULL DEFAULT '#888888'    -- #rrggbb
);

CREATE TABLE IF NOT EXISTS templates (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS template_tasks (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    template_id   INTEGER NOT NULL REFERENCES templates (id) ON DELETE CASCADE,
    seq           INTEGER NOT NULL,
    title         TEXT    NOT NULL,
    phase         TEXT    NOT NULL DEFAULT '',
    est_hours     REAL,
    offset_days   INTEGER NOT NULL DEFAULT 0,  -- start, relative to project start
    duration_days INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX IF NOT EXISTS template_tasks_template ON template_tasks (template_id);

CREATE TABLE IF NOT EXISTS projects (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    code         TEXT    NOT NULL UNIQUE,
    name         TEXT    NOT NULL,
    program_id   INTEGER REFERENCES programs (id) ON DELETE SET NULL,
    template_id  INTEGER REFERENCES templates (id) ON DELETE SET NULL,
    owner_id     INTEGER REFERENCES users (id) ON DELETE SET NULL,
    status       TEXT    NOT NULL DEFAULT 'planning'
                 CHECK (status IN ('planning', 'active', 'on_hold', 'done', 'cancelled')),
    priority     TEXT    NOT NULL DEFAULT 'medium'
                 CHECK (priority IN ('low', 'medium', 'high', 'critical')),
    health       TEXT    NOT NULL DEFAULT 'green'
                 CHECK (health IN ('green', 'amber', 'red')),
    start_date   TEXT,
    due_date     TEXT,
    budget_hours REAL,
    notes        TEXT    NOT NULL DEFAULT '',
    created_at   TEXT    NOT NULL,
    updated_at   TEXT    NOT NULL,
    updated_by   INTEGER
);

CREATE TABLE IF NOT EXISTS tasks (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id   INTEGER NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    seq          INTEGER NOT NULL,             -- display number within the project
    title        TEXT    NOT NULL,
    phase        TEXT    NOT NULL DEFAULT '',
    status       TEXT    NOT NULL DEFAULT 'todo'
                 CHECK (status IN ('todo', 'in_progress', 'blocked', 'review', 'done')),
    priority     TEXT    NOT NULL DEFAULT 'medium'
                 CHECK (priority IN ('low', 'medium', 'high', 'critical')),
    assignee_id  INTEGER REFERENCES users (id) ON DELETE SET NULL,
    start_date   TEXT,
    due_date     TEXT,
    est_hours    REAL,
    actual_hours REAL    NOT NULL DEFAULT 0,
    notes        TEXT    NOT NULL DEFAULT '',
    created_at   TEXT    NOT NULL,
    updated_at   TEXT    NOT NULL,
    updated_by   INTEGER
);
CREATE INDEX IF NOT EXISTS tasks_project ON tasks (project_id);

-- "task_id is blocked by depends_on". Both tasks are in the same project.
CREATE TABLE IF NOT EXISTS task_deps (
    task_id    INTEGER NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
    depends_on INTEGER NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
    PRIMARY KEY (task_id, depends_on),
    CHECK (task_id <> depends_on)
);
CREATE INDEX IF NOT EXISTS task_deps_rev ON task_deps (depends_on);

CREATE TABLE IF NOT EXISTS project_tags (
    project_id INTEGER NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    tag        TEXT    NOT NULL,
    PRIMARY KEY (project_id, tag)
);

CREATE TABLE IF NOT EXISTS task_tags (
    task_id INTEGER NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
    tag     TEXT    NOT NULL,
    PRIMARY KEY (task_id, tag)
);

CREATE TABLE IF NOT EXISTS custom_fields (
    id      INTEGER PRIMARY KEY AUTOINCREMENT,
    entity  TEXT    NOT NULL CHECK (entity IN ('project', 'task')),
    name    TEXT    NOT NULL,
    kind    TEXT    NOT NULL CHECK (kind IN ('text', 'number', 'date', 'enum')),
    options TEXT    NOT NULL DEFAULT '[]',     -- JSON array of strings (enum only)
    seq     INTEGER NOT NULL DEFAULT 0,
    UNIQUE (entity, name)
);

-- Polymorphic (project or task, per custom_fields.entity), so entity_id has no
-- foreign key; the server deletes values together with their row.
-- value: text/enum/date as-is; number as its decimal text.
CREATE TABLE IF NOT EXISTS custom_values (
    field_id  INTEGER NOT NULL REFERENCES custom_fields (id) ON DELETE CASCADE,
    entity_id INTEGER NOT NULL,
    value     TEXT    NOT NULL,
    PRIMARY KEY (field_id, entity_id)
);

-- Change log: feeds the activity view and undo/redo.
-- One user action (one `mutate`) = one grp; it may touch many rows.
CREATE TABLE IF NOT EXISTS activity (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    ts        TEXT    NOT NULL,
    user_id   INTEGER,
    grp       INTEGER NOT NULL,
    action    TEXT    NOT NULL CHECK (action IN ('set', 'create', 'delete')),
    entity    TEXT    NOT NULL,
    entity_id INTEGER NOT NULL,
    field     TEXT    NOT NULL DEFAULT '',     -- set only
    old       TEXT,                            -- JSON; delete: full row bundle
    new       TEXT,                            -- JSON; create: full row bundle
    label     TEXT    NOT NULL DEFAULT '',     -- human-readable summary
    state     TEXT    NOT NULL DEFAULT 'done' CHECK (state IN ('done', 'undone', 'dead'))
);
CREATE INDEX IF NOT EXISTS activity_grp ON activity (grp);
CREATE INDEX IF NOT EXISTS activity_user ON activity (user_id, grp);
