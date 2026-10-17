// Package store is the SQLite store: loading rows in wire format and
// applying mutations with per-field optimistic concurrency, activity
// logging and undo/redo (protocol §5). A port of the Rust store.rs; activity
// bundles use the same JSON so undo works on a database shared with it.
//
// Only the hub calls into this, from one goroutine. Every SQL identifier
// comes from static tables (Entity.Table, FieldMeta.Name); every value is
// a bound parameter.
package store

import (
	"database/sql"
	_ "embed"
	"encoding/json"
	"errors"
	"fmt"
	"net/url"
	"sort"
	"strconv"
	"time"

	_ "modernc.org/sqlite"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/auth"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/core"
)

//go:embed schema.sql
var Schema string

const activityInSnapshot = 500

type Store struct {
	db *sql.DB
}

// Outcome of a mutate / undo / redo.
type Outcome struct {
	Result  core.ResultMsg
	Changes *core.Changes
	// Kick: users whose sessions end (deactivated), with a session to keep.
	Kick []Kick
}

type Kick struct {
	UserID int64
	Keep   string
}

func errOutcome(e *core.ErrorBody) Outcome {
	return Outcome{Result: core.ResultMsg{OK: false, Error: e}}
}

// opErr carries a protocol error code.
type opErr struct {
	code, msg string
}

func (e *opErr) Error() string { return e.msg }

func invalid(f string, a ...any) error   { return &opErr{"invalid", fmt.Sprintf(f, a...)} }
func forbidden(f string, a ...any) error { return &opErr{"forbidden", fmt.Sprintf(f, a...)} }
func notFound(f string, a ...any) error  { return &opErr{"not_found", fmt.Sprintf(f, a...)} }

func errBody(err error) *core.ErrorBody {
	var oe *opErr
	if errors.As(err, &oe) {
		return core.NewError(oe.code, oe.msg)
	}
	Logf("store: %v", err)
	return core.NewError("internal", "internal error")
}

// Logf is replaced by the server to route internal errors to its logger.
var Logf = func(format string, args ...any) {}

func NowSecs() int64   { return time.Now().Unix() }
func NowTS() string    { return core.RFC3339(NowSecs()) }
func Today() core.Date { d, _, _, _ := core.CivilFromUnix(NowSecs()); return d }

// q is satisfied by *sql.DB and *sql.Tx.
type q interface {
	Exec(query string, args ...any) (sql.Result, error)
	Query(query string, args ...any) (*sql.Rows, error)
	QueryRow(query string, args ...any) *sql.Row
}

// ---- open ----

func Open(path string) (*Store, error) {
	dsn := "file:" + url.PathEscape(path) +
		"?_pragma=foreign_keys(1)&_pragma=journal_mode(WAL)&_pragma=busy_timeout(5000)&_pragma=synchronous(NORMAL)"
	return open(dsn)
}

func OpenMemory() (*Store, error) {
	return open("file::memory:?_pragma=foreign_keys(1)")
}

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
	if _, err := db.Exec("PRAGMA foreign_keys = ON"); err != nil {
		db.Close()
		return nil, err
	}
	if _, err := db.Exec(Schema); err != nil {
		db.Close()
		return nil, fmt.Errorf("applying schema: %w", err)
	}
	return &Store{db: db}, nil
}

func (s *Store) Close() error { return s.db.Close() }
func (s *Store) DB() *sql.DB  { return s.db }

func (s *Store) IsEmpty() (bool, error) {
	var n int
	err := s.db.QueryRow("SELECT COUNT(*) FROM users").Scan(&n)
	return n == 0, err
}

// ---- value conversion ----

// toSQL converts a normalised field value to a driver value.
func toSQL(v any) any {
	switch x := v.(type) {
	case nil, string, int64, float64:
		return x
	case bool:
		if x {
			return int64(1)
		}
		return int64(0)
	case int:
		return int64(x)
	}
	b, _ := json.Marshal(v)
	return string(b)
}

func cfFromText(kind, s string) any {
	if kind == "number" {
		f, err := strconv.ParseFloat(s, 64)
		if err != nil {
			return nil
		}
		return f
	}
	return s
}

func cfToText(v any) (string, bool) {
	switch x := v.(type) {
	case nil:
		return "", false
	case string:
		return x, true
	}
	if n, ok := v.(int64); ok {
		return strconv.FormatInt(n, 10), true
	}
	if f, ok := core.AsFloat(v); ok {
		return strconv.FormatFloat(f, 'f', -1, 64), true
	}
	b, _ := json.Marshal(v)
	return string(b), true
}

// ---- loading ----

type filter struct {
	id      *int64
	project *int64 // tasks of a project / template tasks of a template
}

var all = filter{}

func byID(id int64) filter      { return filter{id: &id} }
func byProject(id int64) filter { return filter{project: &id} }
func nullable(p *int64) any {
	if p == nil {
		return nil
	}
	return *p
}

func loadUsers(c q, f filter) ([]core.User, error) {
	rows, err := c.Query("SELECT id, username, display_name, role, active FROM users WHERE (?1 IS NULL OR id = ?1) ORDER BY id", nullable(f.id))
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []core.User{}
	for rows.Next() {
		var u core.User
		var role string
		var active int64
		if err := rows.Scan(&u.ID, &u.Username, &u.DisplayName, &role, &active); err != nil {
			return nil, err
		}
		u.Role, _ = core.ParseRole(role)
		if u.Role == "" {
			u.Role = core.RoleViewer
		}
		u.Active = active != 0
		out = append(out, u)
	}
	return out, rows.Err()
}

func loadPrograms(c q, f filter) ([]core.Program, error) {
	rows, err := c.Query("SELECT id, name, client, prefix, color FROM programs WHERE (?1 IS NULL OR id = ?1) ORDER BY name", nullable(f.id))
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []core.Program{}
	for rows.Next() {
		var p core.Program
		if err := rows.Scan(&p.ID, &p.Name, &p.Client, &p.Prefix, &p.Color); err != nil {
			return nil, err
		}
		out = append(out, p)
	}
	return out, rows.Err()
}

func loadTemplates(c q, f filter) ([]core.Template, error) {
	rows, err := c.Query("SELECT id, name, description FROM templates WHERE (?1 IS NULL OR id = ?1) ORDER BY name", nullable(f.id))
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []core.Template{}
	for rows.Next() {
		var t core.Template
		if err := rows.Scan(&t.ID, &t.Name, &t.Description); err != nil {
			return nil, err
		}
		out = append(out, t)
	}
	return out, rows.Err()
}

func loadTemplateTasks(c q, f filter) ([]core.TemplateTask, error) {
	rows, err := c.Query(`SELECT id, template_id, seq, title, phase, est_hours, offset_days, duration_days FROM template_tasks
		WHERE (?1 IS NULL OR id = ?1) AND (?2 IS NULL OR template_id = ?2) ORDER BY template_id, seq, id`, nullable(f.id), nullable(f.project))
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []core.TemplateTask{}
	for rows.Next() {
		var t core.TemplateTask
		if err := rows.Scan(&t.ID, &t.TemplateID, &t.Seq, &t.Title, &t.Phase, &t.EstHours, &t.OffsetDays, &t.DurationDays); err != nil {
			return nil, err
		}
		out = append(out, t)
	}
	return out, rows.Err()
}

func loadCustomFields(c q, f filter) ([]core.CustomField, error) {
	rows, err := c.Query("SELECT id, entity, name, kind, options, seq FROM custom_fields WHERE (?1 IS NULL OR id = ?1) ORDER BY entity, seq, id", nullable(f.id))
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []core.CustomField{}
	for rows.Next() {
		var cf core.CustomField
		var ent, kind, opts string
		if err := rows.Scan(&cf.ID, &ent, &cf.Name, &kind, &opts, &cf.Seq); err != nil {
			return nil, err
		}
		cf.Entity, _ = core.ParseCfEntity(ent)
		cf.Kind, _ = core.ParseCfKind(kind)
		cf.Options = []string{}
		_ = json.Unmarshal([]byte(opts), &cf.Options)
		if cf.Options == nil {
			cf.Options = []string{}
		}
		out = append(out, cf)
	}
	return out, rows.Err()
}

type side struct {
	tags map[int64][]string
	cf   map[int64]core.CfValues
	deps map[int64][]int64
}

func loadSide(c q, entity core.CfEntity, f filter) (*side, error) {
	s := &side{tags: map[int64][]string{}, cf: map[int64]core.CfValues{}, deps: map[int64][]int64{}}
	var tagSQL, cfSQL string
	args := []any{nullable(f.id)}
	if entity == core.CfProject {
		tagSQL = "SELECT project_id, tag FROM project_tags WHERE (?1 IS NULL OR project_id = ?1) ORDER BY tag"
		cfSQL = `SELECT v.entity_id, v.field_id, v.value, f.kind FROM custom_values v JOIN custom_fields f ON f.id = v.field_id
			WHERE f.entity = 'project' AND (?1 IS NULL OR v.entity_id = ?1)`
	} else {
		args = append(args, nullable(f.project))
		tagSQL = `SELECT t.task_id, t.tag FROM task_tags t JOIN tasks k ON k.id = t.task_id
			WHERE (?1 IS NULL OR t.task_id = ?1) AND (?2 IS NULL OR k.project_id = ?2) ORDER BY t.tag`
		cfSQL = `SELECT v.entity_id, v.field_id, v.value, f.kind FROM custom_values v JOIN custom_fields f ON f.id = v.field_id
			JOIN tasks k ON k.id = v.entity_id
			WHERE f.entity = 'task' AND (?1 IS NULL OR v.entity_id = ?1) AND (?2 IS NULL OR k.project_id = ?2)`
	}
	rows, err := c.Query(tagSQL, args...)
	if err != nil {
		return nil, err
	}
	for rows.Next() {
		var id int64
		var tag string
		if err := rows.Scan(&id, &tag); err != nil {
			rows.Close()
			return nil, err
		}
		s.tags[id] = append(s.tags[id], tag)
	}
	rows.Close()
	rows, err = c.Query(cfSQL, args...)
	if err != nil {
		return nil, err
	}
	for rows.Next() {
		var id, fid int64
		var val, kind string
		if err := rows.Scan(&id, &fid, &val, &kind); err != nil {
			rows.Close()
			return nil, err
		}
		if s.cf[id] == nil {
			s.cf[id] = core.CfValues{}
		}
		s.cf[id][strconv.FormatInt(fid, 10)] = cfFromText(kind, val)
	}
	rows.Close()
	if entity == core.CfTask {
		rows, err = c.Query(`SELECT d.task_id, d.depends_on FROM task_deps d JOIN tasks k ON k.id = d.task_id
			WHERE (?1 IS NULL OR d.task_id = ?1) AND (?2 IS NULL OR k.project_id = ?2) ORDER BY d.depends_on`, args...)
		if err != nil {
			return nil, err
		}
		for rows.Next() {
			var t, dep int64
			if err := rows.Scan(&t, &dep); err != nil {
				rows.Close()
				return nil, err
			}
			s.deps[t] = append(s.deps[t], dep)
		}
		rows.Close()
	}
	return s, nil
}

func orEmpty[T any](v []T) []T {
	if v == nil {
		return []T{}
	}
	return v
}

func cfOrEmpty(v core.CfValues) core.CfValues {
	if v == nil {
		return core.CfValues{}
	}
	return v
}

func loadProjects(c q, f filter) ([]core.Project, error) {
	sd, err := loadSide(c, core.CfProject, f)
	if err != nil {
		return nil, err
	}
	rows, err := c.Query(`SELECT id, code, name, program_id, template_id, owner_id, status, priority, health, start_date, due_date,
		budget_hours, notes, created_at, updated_at, updated_by FROM projects WHERE (?1 IS NULL OR id = ?1) ORDER BY code`, nullable(f.id))
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []core.Project{}
	for rows.Next() {
		var p core.Project
		var st, pr, h string
		if err := rows.Scan(&p.ID, &p.Code, &p.Name, &p.ProgramID, &p.TemplateID, &p.OwnerID, &st, &pr, &h,
			&p.StartDate, &p.DueDate, &p.BudgetHours, &p.Notes, &p.CreatedAt, &p.UpdatedAt, &p.UpdatedBy); err != nil {
			return nil, err
		}
		p.Status, p.Priority, p.Health = core.ProjectStatus(st), core.Priority(pr), core.Health(h)
		p.Tags = orEmpty(sd.tags[p.ID])
		p.CF = cfOrEmpty(sd.cf[p.ID])
		out = append(out, p)
	}
	return out, rows.Err()
}

func loadTasks(c q, f filter) ([]core.Task, error) {
	sd, err := loadSide(c, core.CfTask, f)
	if err != nil {
		return nil, err
	}
	rows, err := c.Query(`SELECT id, project_id, seq, title, phase, status, priority, assignee_id, start_date, due_date,
		est_hours, actual_hours, notes, created_at, updated_at, updated_by
		FROM tasks WHERE (?1 IS NULL OR id = ?1) AND (?2 IS NULL OR project_id = ?2) ORDER BY project_id, seq`, nullable(f.id), nullable(f.project))
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []core.Task{}
	for rows.Next() {
		var t core.Task
		var st, pr string
		if err := rows.Scan(&t.ID, &t.ProjectID, &t.Seq, &t.Title, &t.Phase, &st, &pr, &t.AssigneeID, &t.StartDate, &t.DueDate,
			&t.EstHours, &t.ActualHours, &t.Notes, &t.CreatedAt, &t.UpdatedAt, &t.UpdatedBy); err != nil {
			return nil, err
		}
		t.Status, t.Priority = core.TaskStatus(st), core.Priority(pr)
		t.Deps = orEmpty(sd.deps[t.ID])
		t.Tags = orEmpty(sd.tags[t.ID])
		t.CF = cfOrEmpty(sd.cf[t.ID])
		out = append(out, t)
	}
	return out, rows.Err()
}

const activityCols = "id, ts, user_id, grp, action, entity, entity_id, field, old, new, label, state"

func scanActivity(rows interface{ Scan(...any) error }) (core.Activity, error) {
	var a core.Activity
	var old, nw sql.NullString
	var action, state string
	if err := rows.Scan(&a.ID, &a.TS, &a.UserID, &a.Group, &action, &a.Entity, &a.EntityID, &a.Field, &old, &nw, &a.Label, &state); err != nil {
		return a, err
	}
	a.Action, a.State = core.Action(action), core.ActState(state)
	// bundles are internal: clients only need set values
	if action == "set" {
		a.Old = parseJSON(old)
		a.New = parseJSON(nw)
	}
	return a, nil
}

func parseJSON(s sql.NullString) any {
	if !s.Valid {
		return nil
	}
	var v any
	if json.Unmarshal([]byte(s.String), &v) != nil {
		return nil
	}
	return v
}

func loadActivityIDs(c q, ids []int64) ([]core.Activity, error) {
	out := []core.Activity{}
	for _, id := range ids {
		a, err := scanActivity(c.QueryRow("SELECT "+activityCols+" FROM activity WHERE id = ?1", id))
		if errors.Is(err, sql.ErrNoRows) {
			continue
		}
		if err != nil {
			return nil, err
		}
		out = append(out, a)
	}
	return out, nil
}

func loadRows(c q, e core.Entity, f filter, t *core.Tables) error {
	switch e {
	case core.EntUser:
		v, err := loadUsers(c, f)
		t.Users = append(t.Users, v...)
		return err
	case core.EntProgram:
		v, err := loadPrograms(c, f)
		t.Programs = append(t.Programs, v...)
		return err
	case core.EntTemplate:
		v, err := loadTemplates(c, f)
		t.Templates = append(t.Templates, v...)
		return err
	case core.EntTemplateTask:
		v, err := loadTemplateTasks(c, f)
		t.TemplateTasks = append(t.TemplateTasks, v...)
		return err
	case core.EntProject:
		v, err := loadProjects(c, f)
		t.Projects = append(t.Projects, v...)
		return err
	case core.EntTask:
		v, err := loadTasks(c, f)
		t.Tasks = append(t.Tasks, v...)
		return err
	}
	v, err := loadCustomFields(c, f)
	t.CustomFields = append(t.CustomFields, v...)
	return err
}

func rowExists(c q, e core.Entity, id int64) (bool, error) {
	var one int
	err := c.QueryRow("SELECT 1 FROM "+e.Table()+" WHERE id = ?1", id).Scan(&one)
	if errors.Is(err, sql.ErrNoRows) {
		return false, nil
	}
	return err == nil, err
}

// ---- inserts (create, restore, seed) ----

func nz(id int64) any {
	if id > 0 {
		return id
	}
	return nil
}

func lastID(r sql.Result, err error) (int64, error) {
	if err != nil {
		return 0, err
	}
	return r.LastInsertId()
}

func InsertUser(c q, u *core.User, hash, now string) (int64, error) {
	active := int64(0)
	if u.Active {
		active = 1
	}
	return lastID(c.Exec("INSERT INTO users (id, username, display_name, role, password_hash, active, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
		nz(u.ID), u.Username, u.DisplayName, string(u.Role), hash, active, now))
}

func InsertProgram(c q, p *core.Program) (int64, error) {
	return lastID(c.Exec("INSERT INTO programs (id, name, client, prefix, color) VALUES (?1,?2,?3,?4,?5)", nz(p.ID), p.Name, p.Client, p.Prefix, p.Color))
}

func InsertTemplate(c q, t *core.Template) (int64, error) {
	return lastID(c.Exec("INSERT INTO templates (id, name, description) VALUES (?1,?2,?3)", nz(t.ID), t.Name, t.Description))
}

func InsertTemplateTask(c q, t *core.TemplateTask) (int64, error) {
	return lastID(c.Exec(`INSERT INTO template_tasks (id, template_id, seq, title, phase, est_hours, offset_days, duration_days)
		VALUES (?1,?2,?3,?4,?5,?6,?7,?8)`, nz(t.ID), t.TemplateID, t.Seq, t.Title, t.Phase, t.EstHours, t.OffsetDays, t.DurationDays))
}

func InsertCustomField(c q, f *core.CustomField) (int64, error) {
	opts, _ := json.Marshal(orEmpty(f.Options))
	return lastID(c.Exec("INSERT INTO custom_fields (id, entity, name, kind, options, seq) VALUES (?1,?2,?3,?4,?5,?6)",
		nz(f.ID), string(f.Entity), f.Name, string(f.Kind), string(opts), f.Seq))
}

func writeCF(c q, entityID int64, cf core.CfValues) error {
	keys := make([]string, 0, len(cf))
	for k := range cf {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	for _, k := range keys {
		fid, err := strconv.ParseInt(k, 10, 64)
		if err != nil {
			continue
		}
		if text, ok := cfToText(cf[k]); ok {
			// skip values for fields that no longer exist
			if _, err := c.Exec(`INSERT OR REPLACE INTO custom_values (field_id, entity_id, value)
				SELECT ?1, ?2, ?3 WHERE EXISTS (SELECT 1 FROM custom_fields WHERE id = ?1)`, fid, entityID, text); err != nil {
				return err
			}
		}
	}
	return nil
}

func InsertProject(c q, p *core.Project) (int64, error) {
	id, err := lastID(c.Exec(`INSERT INTO projects (id, code, name, program_id, template_id, owner_id, status, priority, health,
		start_date, due_date, budget_hours, notes, created_at, updated_at, updated_by)
		VALUES (?1,?2,?3,
		(SELECT id FROM programs WHERE id = ?4), (SELECT id FROM templates WHERE id = ?5),
		(SELECT id FROM users WHERE id = ?6), ?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)`,
		nz(p.ID), p.Code, p.Name, p.ProgramID, p.TemplateID, p.OwnerID, string(p.Status), string(p.Priority), string(p.Health),
		p.StartDate, p.DueDate, p.BudgetHours, p.Notes, p.CreatedAt, p.UpdatedAt, p.UpdatedBy))
	if err != nil {
		return 0, err
	}
	for _, t := range p.Tags {
		if _, err := c.Exec("INSERT OR IGNORE INTO project_tags (project_id, tag) VALUES (?1, ?2)", id, t); err != nil {
			return 0, err
		}
	}
	return id, writeCF(c, id, p.CF)
}

// InsertTask inserts a task without its deps (callers add edges once all tasks exist).
func InsertTask(c q, t *core.Task) (int64, error) {
	id, err := lastID(c.Exec(`INSERT INTO tasks (id, project_id, seq, title, phase, status, priority, assignee_id, start_date, due_date,
		est_hours, actual_hours, notes, created_at, updated_at, updated_by)
		VALUES (?1,?2,?3,?4,?5,?6,?7,(SELECT id FROM users WHERE id = ?8),?9,?10,?11,?12,?13,?14,?15,?16)`,
		nz(t.ID), t.ProjectID, t.Seq, t.Title, t.Phase, string(t.Status), string(t.Priority), t.AssigneeID, t.StartDate, t.DueDate,
		t.EstHours, t.ActualHours, t.Notes, t.CreatedAt, t.UpdatedAt, t.UpdatedBy))
	if err != nil {
		return 0, err
	}
	for _, tag := range t.Tags {
		if _, err := c.Exec("INSERT OR IGNORE INTO task_tags (task_id, tag) VALUES (?1, ?2)", id, tag); err != nil {
			return 0, err
		}
	}
	return id, writeCF(c, id, t.CF)
}

// InsertDep adds an edge if both tasks still exist and share a project.
func InsertDep(c q, task, dep int64) error {
	_, err := c.Exec(`INSERT OR IGNORE INTO task_deps (task_id, depends_on)
		SELECT a.id, b.id FROM tasks a JOIN tasks b ON a.project_id = b.project_id
		WHERE a.id = ?1 AND b.id = ?2 AND a.id <> b.id`, task, dep)
	return err
}

// ---- reads ----

func (s *Store) Snapshot() (core.Tables, error) {
	var t core.Tables
	for _, e := range core.AllEntities {
		if err := loadRows(s.db, e, all, &t); err != nil {
			return t, err
		}
	}
	rows, err := s.db.Query("SELECT "+activityCols+" FROM (SELECT * FROM activity ORDER BY id DESC LIMIT ?1) ORDER BY id", activityInSnapshot)
	if err != nil {
		return t, err
	}
	defer rows.Close()
	for rows.Next() {
		a, err := scanActivity(rows)
		if err != nil {
			return t, err
		}
		t.Activity = append(t.Activity, a)
	}
	return t, rows.Err()
}

// ---- auth ----

// UserForLogin returns the user and password hash, or nil.
func (s *Store) UserForLogin(username string) (*core.User, string, error) {
	var id int64
	var hash string
	err := s.db.QueryRow("SELECT id, password_hash FROM users WHERE username = ?1 COLLATE NOCASE", username).Scan(&id, &hash)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, "", nil
	}
	if err != nil {
		return nil, "", err
	}
	u, err := s.User(id)
	return u, hash, err
}

func (s *Store) User(id int64) (*core.User, error) {
	us, err := loadUsers(s.db, byID(id))
	if err != nil || len(us) == 0 {
		return nil, err
	}
	return &us[0], nil
}

func (s *Store) UserByName(username string) (*core.User, error) {
	u, _, err := s.UserForLogin(username)
	return u, err
}

func (s *Store) PasswordHash(id int64) (string, bool, error) {
	var h string
	err := s.db.QueryRow("SELECT password_hash FROM users WHERE id = ?1", id).Scan(&h)
	if errors.Is(err, sql.ErrNoRows) {
		return "", false, nil
	}
	return h, err == nil, err
}

func (s *Store) SetPasswordHash(id int64, hash string) error {
	_, err := s.db.Exec("UPDATE users SET password_hash = ?1 WHERE id = ?2", hash, id)
	return err
}

func (s *Store) ListUsers() ([]core.User, error) { return loadUsers(s.db, all) }

func (s *Store) AddUser(username, display string, role core.Role, hash string) (int64, error) {
	u := core.User{Username: username, DisplayName: display, Role: role, Active: true}
	return InsertUser(s.db, &u, hash, NowTS())
}

func (s *Store) CreateSession(tokenHash string, userID int64, client string) error {
	now := NowSecs()
	if _, err := s.db.Exec("INSERT INTO sessions (token_hash, user_id, created_at, expires_at, client) VALUES (?1,?2,?3,?4,?5)",
		tokenHash, userID, core.RFC3339(now), core.RFC3339(now+auth.SessionTTLSecs), client); err != nil {
		return err
	}
	// opportunistic cleanup
	_, err := s.db.Exec("DELETE FROM sessions WHERE expires_at < ?1", core.RFC3339(now))
	return err
}

// SessionUser returns the active user owning an unexpired session, or nil.
func (s *Store) SessionUser(tokenHash string) (*core.User, error) {
	var id int64
	err := s.db.QueryRow(`SELECT s.user_id FROM sessions s JOIN users u ON u.id = s.user_id
		WHERE s.token_hash = ?1 AND s.expires_at > ?2 AND u.active = 1`, tokenHash, NowTS()).Scan(&id)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	return s.User(id)
}

func (s *Store) DeleteSession(tokenHash string) error {
	_, err := s.db.Exec("DELETE FROM sessions WHERE token_hash = ?1", tokenHash)
	return err
}

func (s *Store) DeleteUserSessions(userID int64, keep string) error {
	var k any
	if keep != "" {
		k = keep
	}
	_, err := s.db.Exec("DELETE FROM sessions WHERE user_id = ?1 AND (?2 IS NULL OR token_hash <> ?2)", userID, k)
	return err
}

func lastSetter(c q, e core.Entity, id int64, field string) *int64 {
	var uid sql.NullInt64
	err := c.QueryRow(`SELECT user_id FROM activity WHERE entity = ?1 AND entity_id = ?2 AND field = ?3 AND action = 'set'
		AND state <> 'undone' ORDER BY id DESC LIMIT 1`, string(e), id, field).Scan(&uid)
	if err != nil || !uid.Valid {
		return nil
	}
	return &uid.Int64
}

// ---- writes ----

func (s *Store) Mutate(actor *core.User, ops []core.Op) Outcome {
	if actor.Role == core.RoleViewer {
		return errOutcome(core.NewError("forbidden", "viewers cannot make changes"))
	}
	tx, err := s.db.Begin()
	if err != nil {
		return errOutcome(errBody(err))
	}
	defer tx.Rollback()
	cx := newCx(tx, actor, NowTS())
	for i, op := range ops {
		if err := cx.apply(i, op); err != nil {
			return errOutcome(errBody(err).At(i))
		}
	}
	if err := cx.validate(); err != nil {
		return errOutcome(errBody(err))
	}
	res := core.ResultMsg{OK: true, Conflicts: cx.conflicts, Created: cx.created}
	if len(res.Created) == 0 {
		res.Created = nil
	}
	if len(cx.entries) == 0 {
		return Outcome{Result: res}
	}
	group, changes, kick, err := cx.finish(true)
	if err != nil {
		return errOutcome(errBody(err))
	}
	if err := tx.Commit(); err != nil {
		return errOutcome(errBody(err))
	}
	res.Group = &group
	return Outcome{Result: res, Changes: changes, Kick: kick}
}

func (s *Store) Undo(actor *core.User) Outcome { return s.undoRedo(actor, true) }
func (s *Store) Redo(actor *core.User) Outcome { return s.undoRedo(actor, false) }

type entryRow struct {
	id                    int64
	action, entity, field string
	entityID              int64
	old, new              sql.NullString
}

func (s *Store) undoRedo(actor *core.User, undo bool) Outcome {
	if actor.Role == core.RoleViewer {
		return errOutcome(core.NewError("forbidden", "viewers cannot make changes"))
	}
	tx, err := s.db.Begin()
	if err != nil {
		return errOutcome(errBody(err))
	}
	defer tx.Rollback()
	want := "undone"
	if undo {
		want = "done"
	}
	var grp sql.NullInt64
	_ = tx.QueryRow("SELECT MAX(grp) FROM activity WHERE user_id = ?1 AND state = ?2", actor.ID, want).Scan(&grp)
	if !grp.Valid {
		if undo {
			return errOutcome(core.NewError("nothing_to_undo", "nothing to undo"))
		}
		return errOutcome(core.NewError("nothing_to_redo", "nothing to redo"))
	}
	order := "ASC"
	if undo {
		order = "DESC"
	}
	rows, err := tx.Query("SELECT id, action, entity, entity_id, field, old, new FROM activity WHERE grp = ?1 ORDER BY id "+order, grp.Int64)
	if err != nil {
		return errOutcome(errBody(err))
	}
	var entries []entryRow
	for rows.Next() {
		var e entryRow
		if err := rows.Scan(&e.id, &e.action, &e.entity, &e.entityID, &e.field, &e.old, &e.new); err != nil {
			rows.Close()
			return errOutcome(errBody(err))
		}
		entries = append(entries, e)
	}
	rows.Close()
	cx := newCx(tx, actor, NowTS())
	for idx, en := range entries {
		e, ok := core.ParseEntity(en.entity)
		if !ok {
			continue
		}
		old, nw := parseJSON(en.old), parseJSON(en.new)
		var err error
		switch {
		case en.action == "set" && undo:
			err = cx.revertSet(idx, e, en.entityID, en.field, nw, old)
		case en.action == "set":
			err = cx.revertSet(idx, e, en.entityID, en.field, old, nw)
		case en.action == "create" && undo, en.action == "delete" && !undo:
			_, err = cx.deleteRow(e, en.entityID, false)
		case en.action == "create":
			err = cx.restore(e, nw)
		case en.action == "delete":
			err = cx.restore(e, old)
		}
		if err != nil {
			return errOutcome(errBody(err))
		}
	}
	if len(cx.conflicts) > 0 {
		return Outcome{Result: core.ResultMsg{OK: false, Conflicts: cx.conflicts,
			Error: core.NewError("conflict", "someone else changed this since; nothing was changed")}}
	}
	if err := cx.validate(); err != nil {
		return errOutcome(errBody(err))
	}
	newState := "done"
	if undo {
		newState = "undone"
	}
	if _, err := tx.Exec("UPDATE activity SET state = ?1 WHERE grp = ?2", newState, grp.Int64); err != nil {
		return errOutcome(errBody(err))
	}
	for _, en := range entries {
		cx.updatedActivity = append(cx.updatedActivity, en.id)
	}
	_, changes, kick, err := cx.finish(false)
	if err != nil {
		return errOutcome(errBody(err))
	}
	if err := tx.Commit(); err != nil {
		return errOutcome(errBody(err))
	}
	g := grp.Int64
	changes.Group = &g
	return Outcome{Result: core.ResultMsg{OK: true, Group: &g}, Changes: changes, Kick: kick}
}
