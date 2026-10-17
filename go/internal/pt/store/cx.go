package store

import (
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"sort"
	"strconv"
	"strings"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/auth"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/core"
)

type entry struct {
	action   core.Action
	entity   core.Entity
	id       int64
	field    string
	old, new any // JSON-able; nil = SQL NULL
	hasOld   bool
	hasNew   bool
	label    string
	undoable bool
}

// cx is one mutate / undo / redo inside a transaction.
type cx struct {
	c               *sql.Tx
	actor           *core.User
	now             string
	touched         map[core.RowKey]bool
	deleted         map[core.RowKey]bool
	entries         []entry
	conflicts       []core.Conflict
	created         map[string]int64
	updatedActivity []int64
	kick            []Kick
}

func newCx(tx *sql.Tx, actor *core.User, now string) *cx {
	return &cx{c: tx, actor: actor, now: now, touched: map[core.RowKey]bool{}, deleted: map[core.RowKey]bool{}, created: map[string]int64{}}
}

func key(e core.Entity, id int64) core.RowKey { return core.RowKey{Entity: e, ID: id} }

// wireNum renders a float like serde_json does ("78.0", not "78"), so
// conflict replies are byte-compatible with the Rust server.
func wireNum(v any) any {
	f, ok := v.(float64)
	if !ok {
		return v
	}
	s := strconv.FormatFloat(f, 'f', -1, 64)
	if !strings.ContainsAny(s, ".eE") {
		s += ".0"
	}
	return json.Number(s)
}

func (x *cx) checkEntityPerm(e core.Entity) error {
	if e != core.EntProject && e != core.EntTask && x.actor.Role != core.RoleAdmin {
		return forbidden("only admins can change %ss", e.Label())
	}
	return nil
}

func (x *cx) checkFieldPerm(m *core.FieldMeta) error {
	if m.Perm == core.PermAdmin && x.actor.Role != core.RoleAdmin {
		return forbidden("only admins can change %s", m.Label)
	}
	return nil
}

func (x *cx) exists(e core.Entity, id int64) (bool, error) { return rowExists(x.c, e, id) }

func (x *cx) apply(idx int, op core.Op) error {
	if err := x.checkEntityPerm(op.Entity); err != nil {
		return err
	}
	switch op.Kind {
	case "set":
		current, ok, err := x.readField(op.Entity, op.ID, op.Field)
		if err != nil {
			return err
		}
		if !ok {
			return notFound("that %s no longer exists", op.Entity.Label())
		}
		if !op.Force && !core.ValuesEqual(current, op.Base) {
			x.conflicts = append(x.conflicts, core.Conflict{Index: idx, Entity: op.Entity, ID: op.ID, Field: op.Field,
				Current: wireNum(current), By: lastSetter(x.c, op.Entity, op.ID, op.Field)})
			return nil
		}
		value, err := x.normalizeSet(op.Entity, op.ID, op.Field, op.Value)
		if err != nil {
			return err
		}
		if core.ValuesEqual(current, value) {
			return nil
		}
		if err := x.writeField(op.Entity, op.ID, op.Field, value); err != nil {
			return err
		}
		x.entries = append(x.entries, entry{action: core.ActionSet, entity: op.Entity, id: op.ID, field: op.Field,
			old: current, new: value, hasOld: true, hasNew: true,
			label: x.setLabel(op.Entity, op.ID, op.Field, current, value), undoable: true})
		return nil
	case "create":
		id, err := x.create(op.Entity, op.Values)
		if err != nil {
			return err
		}
		if op.Ref != nil {
			x.created[*op.Ref] = id
		}
		return nil
	case "delete":
		if op.Entity == core.EntUser {
			return forbidden("users can't be deleted; set active to no instead")
		}
		ok, err := x.exists(op.Entity, op.ID)
		if err != nil {
			return err
		}
		if !ok {
			return notFound("that %s no longer exists", op.Entity.Label())
		}
		label := fmt.Sprintf("deleted %s %s", op.Entity.Label(), x.describe(op.Entity, op.ID))
		bundle, err := x.deleteRow(op.Entity, op.ID, true)
		if err != nil {
			return err
		}
		x.entries = append(x.entries, entry{action: core.ActionDelete, entity: op.Entity, id: op.ID,
			old: bundle, hasOld: bundle != nil, label: label,
			undoable: op.Entity == core.EntProject || op.Entity == core.EntTask})
		return nil
	}
	return invalid("unknown op")
}

func (x *cx) loadCF(fid int64) (*core.CustomField, error) {
	cfs, err := loadCustomFields(x.c, byID(fid))
	if err != nil || len(cfs) == 0 {
		return nil, err
	}
	return &cfs[0], nil
}

// normalizeSet validates a value for set, including references and uniqueness.
func (x *cx) normalizeSet(e core.Entity, id int64, field string, value any) (any, error) {
	if fid, ok := core.CfID(field); ok {
		cf, err := x.loadCF(fid)
		if err != nil {
			return nil, err
		}
		if cf == nil {
			return nil, invalid("that custom field was deleted")
		}
		if !(cf.Entity == core.CfProject && e == core.EntProject || cf.Entity == core.CfTask && e == core.EntTask) {
			return nil, invalid("%s is not a %s field", cf.Name, e.Label())
		}
		v, err := core.NormalizeCF(cf.Kind, cf.Options, cf.Name, value)
		if err != nil {
			return nil, invalid("%s", err.Error())
		}
		return v, nil
	}
	m := core.Field(e, field)
	if m == nil {
		return nil, invalid("unknown field '%s'", field)
	}
	if err := x.checkFieldPerm(m); err != nil {
		return nil, err
	}
	if m.CreateOnly {
		return nil, invalid("%s can only be set when creating", m.Label)
	}
	v, err := core.Normalize(m, value)
	if err != nil {
		return nil, invalid("%s", err.Error())
	}
	if err := x.checkValue(e, id, m, v); err != nil {
		return nil, err
	}
	return v, nil
}

func (x *cx) clash(query string, args ...any) (bool, error) {
	var id int64
	err := x.c.QueryRow(query, args...).Scan(&id)
	if errors.Is(err, sql.ErrNoRows) {
		return false, nil
	}
	return err == nil, err
}

// checkValue: reference existence, uniqueness and self-protection rules.
func (x *cx) checkValue(e core.Entity, id int64, m *core.FieldMeta, v any) error {
	switch m.Kind.Tag {
	case core.KRef:
		if rid, ok := core.AsInt(v); ok && v != nil {
			ok, err := x.exists(m.Kind.Target, rid)
			if err != nil {
				return err
			}
			if !ok {
				return invalid("%s %d doesn't exist", m.Kind.Target.Label(), rid)
			}
			if m.Kind.Target == core.EntUser {
				var active int64
				if err := x.c.QueryRow("SELECT active FROM users WHERE id = ?1", rid).Scan(&active); err != nil {
					return err
				}
				if active == 0 {
					return invalid("that user is deactivated")
				}
			}
		}
	case core.KCode:
		s, _ := v.(string)
		c, err := x.clash("SELECT id FROM projects WHERE code = ?1 AND id <> ?2", s, id)
		if err != nil {
			return err
		}
		if c {
			return invalid("code %s is already used", s)
		}
	}
	if e == core.EntUser && id == x.actor.ID {
		if m.Name == "active" && v == false {
			return invalid("you can't deactivate yourself")
		}
		if m.Name == "role" && v != "admin" {
			return invalid("you can't remove your own admin role")
		}
	}
	if (e == core.EntProgram || e == core.EntTemplate) && m.Name == "name" {
		s, _ := v.(string)
		c, err := x.clash("SELECT id FROM "+e.Table()+" WHERE name = ?1 AND id <> ?2", s, id)
		if err != nil {
			return err
		}
		if c {
			return invalid("the name '%s' is already used", s)
		}
	}
	if e == core.EntCustomField && m.Name == "name" {
		var ent sql.NullString
		_ = x.c.QueryRow("SELECT entity FROM custom_fields WHERE id = ?1", id).Scan(&ent)
		var entArg any
		if ent.Valid {
			entArg = ent.String
		}
		c, err := x.clash("SELECT id FROM custom_fields WHERE name = ?1 AND entity = ?2 AND id <> ?3", v, entArg, id)
		if err != nil {
			return err
		}
		if c {
			return invalid("a field with that name already exists")
		}
	}
	return nil
}

func tagTable(e core.Entity) (sel, del, ins string, err error) {
	switch e {
	case core.EntProject:
		return "SELECT tag FROM project_tags WHERE project_id = ?1 ORDER BY tag",
			"DELETE FROM project_tags WHERE project_id = ?1",
			"INSERT OR IGNORE INTO project_tags (project_id, tag) VALUES (?1, ?2)", nil
	case core.EntTask:
		return "SELECT tag FROM task_tags WHERE task_id = ?1 ORDER BY tag",
			"DELETE FROM task_tags WHERE task_id = ?1",
			"INSERT OR IGNORE INTO task_tags (task_id, tag) VALUES (?1, ?2)", nil
	}
	return "", "", "", invalid("this entity has no tags")
}

func queryStrings(c *sql.Tx, query string, args ...any) ([]string, error) {
	rows, err := c.Query(query, args...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []string{}
	for rows.Next() {
		var s string
		if err := rows.Scan(&s); err != nil {
			return nil, err
		}
		out = append(out, s)
	}
	return out, rows.Err()
}

func queryInts(c *sql.Tx, query string, args ...any) ([]int64, error) {
	rows, err := c.Query(query, args...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []int64{}
	for rows.Next() {
		var n int64
		if err := rows.Scan(&n); err != nil {
			return nil, err
		}
		out = append(out, n)
	}
	return out, rows.Err()
}

// readField returns the current value; ok is false if the row is gone.
func (x *cx) readField(e core.Entity, id int64, field string) (any, bool, error) {
	ok, err := x.exists(e, id)
	if err != nil || !ok {
		return nil, false, err
	}
	if fid, isCF := core.CfID(field); isCF {
		var v, k string
		err := x.c.QueryRow(`SELECT v.value, f.kind FROM custom_values v JOIN custom_fields f ON f.id = v.field_id
			WHERE v.field_id = ?1 AND v.entity_id = ?2`, fid, id).Scan(&v, &k)
		if errors.Is(err, sql.ErrNoRows) {
			return nil, true, nil
		}
		if err != nil {
			return nil, false, err
		}
		return cfFromText(k, v), true, nil
	}
	m := core.Field(e, field)
	if m == nil {
		return nil, false, invalid("unknown field '%s'", field)
	}
	switch m.Kind.Tag {
	case core.KTags:
		sel, _, _, err := tagTable(e)
		if err != nil {
			return nil, false, err
		}
		tags, err := queryStrings(x.c, sel, id)
		return tags, err == nil, err
	case core.KDeps:
		deps, err := queryInts(x.c, "SELECT depends_on FROM task_deps WHERE task_id = ?1 ORDER BY depends_on", id)
		return deps, err == nil, err
	case core.KPassword:
		return nil, true, nil
	}
	var raw any
	if err := x.c.QueryRow("SELECT "+m.Name+" FROM "+e.Table()+" WHERE id = ?1", id).Scan(&raw); err != nil {
		return nil, false, err
	}
	switch v := raw.(type) {
	case nil:
		return nil, true, nil
	case int64:
		switch m.Kind.Tag {
		case core.KBool:
			return v != 0, true, nil
		case core.KHours:
			return float64(v), true, nil
		}
		return v, true, nil
	case float64:
		return v, true, nil
	case []byte:
		raw = string(v)
	}
	s, _ := raw.(string)
	if m.Kind.Tag == core.KOptions {
		var opts []any
		if json.Unmarshal([]byte(s), &opts) != nil || opts == nil {
			opts = []any{}
		}
		return opts, true, nil
	}
	return s, true, nil
}

func (x *cx) writeField(e core.Entity, id int64, field string, v any) error {
	if fid, ok := core.CfID(field); ok {
		var err error
		if text, ok := cfToText(v); ok {
			_, err = x.c.Exec("INSERT OR REPLACE INTO custom_values (field_id, entity_id, value) VALUES (?1, ?2, ?3)", fid, id, text)
		} else {
			_, err = x.c.Exec("DELETE FROM custom_values WHERE field_id = ?1 AND entity_id = ?2", fid, id)
		}
		if err != nil {
			return err
		}
	} else {
		m := core.Field(e, field)
		if m == nil {
			return invalid("unknown field '%s'", field)
		}
		switch m.Kind.Tag {
		case core.KTags:
			_, del, ins, err := tagTable(e)
			if err != nil {
				return err
			}
			if _, err := x.c.Exec(del, id); err != nil {
				return err
			}
			list, _ := core.AsList(v)
			for _, t := range list {
				if s, ok := t.(string); ok {
					if _, err := x.c.Exec(ins, id, s); err != nil {
						return err
					}
				}
			}
		case core.KDeps:
			if _, err := x.c.Exec("DELETE FROM task_deps WHERE task_id = ?1", id); err != nil {
				return err
			}
			list, _ := core.AsList(v)
			for _, d := range list {
				if n, ok := core.AsInt(d); ok {
					if _, err := x.c.Exec("INSERT INTO task_deps (task_id, depends_on) VALUES (?1, ?2)", id, n); err != nil {
						return err
					}
				}
			}
		case core.KOptions:
			list, _ := core.AsList(v)
			if list == nil {
				list = []any{}
			}
			b, _ := json.Marshal(list)
			if _, err := x.c.Exec("UPDATE custom_fields SET options = ?1 WHERE id = ?2", string(b), id); err != nil {
				return err
			}
		case core.KPassword:
			s, _ := v.(string)
			h, err := auth.HashPassword(s)
			if err != nil {
				return err
			}
			if _, err := x.c.Exec("UPDATE users SET password_hash = ?1 WHERE id = ?2", h, id); err != nil {
				return err
			}
		default:
			if _, err := x.c.Exec("UPDATE "+e.Table()+" SET "+m.Name+" = ?1 WHERE id = ?2", toSQL(v), id); err != nil {
				return err
			}
		}
		if e == core.EntUser && field == "active" && v == false {
			if _, err := x.c.Exec("DELETE FROM sessions WHERE user_id = ?1", id); err != nil {
				return err
			}
			x.kick = append(x.kick, Kick{UserID: id})
		}
	}
	if e == core.EntProject || e == core.EntTask {
		if _, err := x.c.Exec("UPDATE "+e.Table()+" SET updated_at = ?1, updated_by = ?2 WHERE id = ?3", x.now, x.actor.ID, id); err != nil {
			return err
		}
	}
	x.touched[key(e, id)] = true
	return nil
}

// revertSet: undo/redo of a set — apply `to` if the field still holds `expect`.
func (x *cx) revertSet(idx int, e core.Entity, id int64, field string, expect, to any) error {
	current, ok, err := x.readField(e, id, field)
	if err != nil {
		return err
	}
	if !ok {
		x.conflicts = append(x.conflicts, core.Conflict{Index: idx, Entity: e, ID: id, Field: field})
		return nil
	}
	if !core.ValuesEqual(current, expect) {
		x.conflicts = append(x.conflicts, core.Conflict{Index: idx, Entity: e, ID: id, Field: field, Current: wireNum(current),
			By: lastSetter(x.c, e, id, field)})
		return nil
	}
	// references may have disappeared since; check them again
	if m := core.Field(e, field); m != nil {
		if err := x.checkValue(e, id, m, to); err != nil {
			return err
		}
	}
	return x.writeField(e, id, field, to)
}

// defaultRow: defaults for create (protocol §4); provided values overlay these.
func defaultRow(e core.Entity, actor int64, now string) map[string]any {
	switch e {
	case core.EntProject:
		return map[string]any{"id": 0, "code": "", "name": "", "program_id": nil, "template_id": nil, "owner_id": nil,
			"status": "planning", "priority": "medium", "health": "green", "start_date": nil, "due_date": nil,
			"budget_hours": nil, "tags": []any{}, "notes": "", "cf": map[string]any{},
			"created_at": now, "updated_at": now, "updated_by": actor}
	case core.EntTask:
		return map[string]any{"id": 0, "project_id": 0, "seq": 0, "title": "", "phase": "", "status": "todo", "priority": "medium",
			"assignee_id": nil, "start_date": nil, "due_date": nil, "est_hours": nil, "actual_hours": 0.0,
			"deps": []any{}, "tags": []any{}, "notes": "", "cf": map[string]any{},
			"created_at": now, "updated_at": now, "updated_by": actor}
	case core.EntProgram:
		return map[string]any{"id": 0, "name": "", "client": "", "prefix": "", "color": "#888888"}
	case core.EntTemplate:
		return map[string]any{"id": 0, "name": "", "description": ""}
	case core.EntTemplateTask:
		return map[string]any{"id": 0, "template_id": 0, "seq": 0, "title": "", "phase": "", "est_hours": nil,
			"offset_days": 0, "duration_days": 1}
	case core.EntCustomField:
		return map[string]any{"id": 0, "entity": "task", "name": "", "kind": "text", "options": []any{}, "seq": 0}
	}
	return map[string]any{"id": 0, "username": "", "display_name": "", "role": "member", "active": true}
}

// remarshal converts a JSON-ish map into a typed row.
func remarshal(v any, out any) error {
	b, err := json.Marshal(v)
	if err != nil {
		return err
	}
	if err := json.Unmarshal(b, out); err != nil {
		return fmt.Errorf("row build: %w", err)
	}
	return nil
}

func (x *cx) nextInt(query string, args ...any) (int64, error) {
	var n int64
	err := x.c.QueryRow(query, args...).Scan(&n)
	return n, err
}

func (x *cx) create(e core.Entity, values map[string]any) (int64, error) {
	row := defaultRow(e, x.actor.ID, x.now)
	cf := map[string]any{}
	var password *string
	for k, v := range values {
		if fid, ok := core.CfID(k); ok {
			def, err := x.loadCF(fid)
			if err != nil {
				return 0, err
			}
			if def == nil {
				return 0, invalid("unknown custom field")
			}
			nv, err := core.NormalizeCF(def.Kind, def.Options, def.Name, v)
			if err != nil {
				return 0, invalid("%s", err.Error())
			}
			if nv != nil {
				cf[fmt.Sprint(fid)] = nv
			}
			continue
		}
		m := core.Field(e, k)
		if m == nil {
			return 0, invalid("unknown field '%s'", k)
		}
		if err := x.checkFieldPerm(m); err != nil {
			return 0, err
		}
		nv, err := core.Normalize(m, v)
		if err != nil {
			return 0, invalid("%s", err.Error())
		}
		if err := x.checkValue(e, 0, m, nv); err != nil {
			return 0, err
		}
		if m.Kind.Tag == core.KPassword {
			s, _ := nv.(string)
			password = &s
		} else {
			row[k] = nv
		}
	}
	for _, m := range core.Fields(e) {
		if !m.Required {
			continue
		}
		missing := false
		if m.Kind.Tag == core.KPassword {
			missing = password == nil
		} else {
			v := row[m.Name]
			missing = v == nil || v == ""
			if n, ok := core.AsInt(v); ok && n == 0 && (m.Kind.Tag == core.KRef) {
				missing = true
			}
		}
		if missing {
			return 0, invalid("%s is required", m.Label)
		}
	}
	row["cf"] = cf
	var id int64
	var err error
	switch e {
	case core.EntProject:
		var p core.Project
		if err := remarshal(row, &p); err != nil {
			return 0, err
		}
		if id, err = InsertProject(x.c, &p); err != nil {
			return 0, err
		}
		if p.TemplateID != nil {
			if err := x.instantiate(id, *p.TemplateID, &p); err != nil {
				return 0, err
			}
		}
	case core.EntTask:
		var t core.Task
		if err := remarshal(row, &t); err != nil {
			return 0, err
		}
		if t.Seq, err = x.nextInt("SELECT COALESCE(MAX(seq), 0) + 1 FROM tasks WHERE project_id = ?1", t.ProjectID); err != nil {
			return 0, err
		}
		if id, err = InsertTask(x.c, &t); err != nil {
			return 0, err
		}
		for _, d := range t.Deps {
			if _, err := x.c.Exec("INSERT INTO task_deps (task_id, depends_on) VALUES (?1, ?2)", id, d); err != nil {
				return 0, err
			}
		}
	case core.EntProgram:
		var p core.Program
		if err := remarshal(row, &p); err != nil {
			return 0, err
		}
		id, err = InsertProgram(x.c, &p)
	case core.EntTemplate:
		var t core.Template
		if err := remarshal(row, &t); err != nil {
			return 0, err
		}
		id, err = InsertTemplate(x.c, &t)
	case core.EntTemplateTask:
		var t core.TemplateTask
		if err := remarshal(row, &t); err != nil {
			return 0, err
		}
		if _, has := values["seq"]; !has {
			if t.Seq, err = x.nextInt("SELECT COALESCE(MAX(seq), 0) + 1 FROM template_tasks WHERE template_id = ?1", t.TemplateID); err != nil {
				return 0, err
			}
		}
		id, err = InsertTemplateTask(x.c, &t)
	case core.EntCustomField:
		var f core.CustomField
		if err := remarshal(row, &f); err != nil {
			return 0, err
		}
		c, err := x.clash("SELECT id FROM custom_fields WHERE name = ?1 AND entity = ?2", f.Name, string(f.Entity))
		if err != nil {
			return 0, err
		}
		if c {
			return 0, invalid("a field with that name already exists")
		}
		if _, has := values["seq"]; !has {
			if f.Seq, err = x.nextInt("SELECT COALESCE(MAX(seq), 0) + 1 FROM custom_fields"); err != nil {
				return 0, err
			}
		}
		id, err = InsertCustomField(x.c, &f)
	case core.EntUser:
		var u core.User
		if err := remarshal(row, &u); err != nil {
			return 0, err
		}
		taken, err := x.clash("SELECT id FROM users WHERE username = ?1 COLLATE NOCASE", u.Username)
		if err != nil {
			return 0, err
		}
		if taken {
			return 0, invalid("username %s is taken", u.Username)
		}
		h, err := auth.HashPassword(*password)
		if err != nil {
			return 0, err
		}
		id, err = InsertUser(x.c, &u, h, x.now)
	}
	if err != nil {
		return 0, err
	}
	x.touched[key(e, id)] = true
	if e == core.EntProject {
		ids, err := queryInts(x.c, "SELECT id FROM tasks WHERE project_id = ?1", id)
		if err != nil {
			return 0, err
		}
		for _, tid := range ids {
			x.touched[key(core.EntTask, tid)] = true
		}
	}
	bundle, err := x.bundle(e, id)
	if err != nil {
		return 0, err
	}
	x.entries = append(x.entries, entry{action: core.ActionCreate, entity: e, id: id, new: bundle, hasNew: bundle != nil,
		label: fmt.Sprintf("created %s %s", e.Label(), x.describe(e, id)), undoable: e != core.EntUser})
	return id, nil
}

// instantiate: protocol §7 — tasks from the template, dates from the project start.
func (x *cx) instantiate(pid, tid int64, p *core.Project) error {
	tts, err := loadTemplateTasks(x.c, byProject(tid))
	if err != nil {
		return err
	}
	start := Today()
	if p.StartDate != nil {
		if d, ok := core.ParseISO(*p.StartDate); ok {
			start = d
		}
	}
	type phaseLast struct {
		phase string
		id    int64
	}
	var last []phaseLast
	for _, tt := range tts {
		s := start.AddDays(int(tt.OffsetDays))
		t := core.Task{ProjectID: pid, Seq: tt.Seq, Title: tt.Title, Phase: tt.Phase, Status: core.TaskTodo,
			Priority: core.PriorityMedium, AssigneeID: p.OwnerID, StartDate: core.Ptr(s.ISO()),
			DueDate: core.Ptr(s.AddDays(int(max(tt.DurationDays, 1)) - 1).ISO()), EstHours: tt.EstHours,
			CreatedAt: x.now, UpdatedAt: x.now, UpdatedBy: &x.actor.ID}
		id, err := InsertTask(x.c, &t)
		if err != nil {
			return err
		}
		found := false
		for i := range last {
			if last[i].phase == tt.Phase {
				last[i].id = id
				found = true
			}
		}
		if !found {
			if len(last) > 0 {
				if err := InsertDep(x.c, id, last[len(last)-1].id); err != nil {
					return err
				}
			}
			last = append(last, phaseLast{tt.Phase, id})
		}
	}
	return nil
}

// bundle: full row (plus dependent rows) as stored in activity for undo/redo.
func (x *cx) bundle(e core.Entity, id int64) (any, error) {
	var t core.Tables
	if err := loadRows(x.c, e, byID(id), &t); err != nil {
		return nil, err
	}
	var row any
	switch {
	case len(t.Users) > 0:
		row = t.Users[0]
	case len(t.Programs) > 0:
		row = t.Programs[0]
	case len(t.Templates) > 0:
		row = t.Templates[0]
	case len(t.TemplateTasks) > 0:
		row = t.TemplateTasks[0]
	case len(t.Projects) > 0:
		row = t.Projects[0]
	case len(t.Tasks) > 0:
		row = t.Tasks[0]
	case len(t.CustomFields) > 0:
		row = t.CustomFields[0]
	default:
		return nil, nil
	}
	b := map[string]any{"row": row}
	if e == core.EntProject {
		tasks, err := loadTasks(x.c, byProject(id))
		if err != nil {
			return nil, err
		}
		b["tasks"] = tasks
	}
	if e == core.EntTask {
		deps, err := queryInts(x.c, "SELECT task_id FROM task_deps WHERE depends_on = ?1 ORDER BY task_id", id)
		if err != nil {
			return nil, err
		}
		b["dependents"] = deps
	}
	// round-trip so the value is plain JSON data (as when read back from the DB)
	raw, err := json.Marshal(b)
	if err != nil {
		return nil, err
	}
	var out any
	return out, json.Unmarshal(raw, &out)
}

// deleteRow deletes a row and its dependents; returns its bundle when asked.
func (x *cx) deleteRow(e core.Entity, id int64, wantBundle bool) (any, error) {
	ok, err := x.exists(e, id)
	if err != nil || !ok {
		return nil, err
	}
	var bundle any
	if wantBundle {
		if bundle, err = x.bundle(e, id); err != nil {
			return nil, err
		}
	}
	switch e {
	case core.EntProject:
		tids, err := queryInts(x.c, "SELECT id FROM tasks WHERE project_id = ?1", id)
		if err != nil {
			return nil, err
		}
		for _, tid := range tids {
			if _, err := x.c.Exec("DELETE FROM custom_values WHERE entity_id = ?1 AND field_id IN (SELECT id FROM custom_fields WHERE entity = 'task')", tid); err != nil {
				return nil, err
			}
			x.deleted[key(core.EntTask, tid)] = true
		}
		if _, err := x.c.Exec("DELETE FROM custom_values WHERE entity_id = ?1 AND field_id IN (SELECT id FROM custom_fields WHERE entity = 'project')", id); err != nil {
			return nil, err
		}
	case core.EntTask:
		deps, err := queryInts(x.c, "SELECT task_id FROM task_deps WHERE depends_on = ?1", id)
		if err != nil {
			return nil, err
		}
		for _, d := range deps {
			x.touched[key(core.EntTask, d)] = true
		}
		if _, err := x.c.Exec("DELETE FROM custom_values WHERE entity_id = ?1 AND field_id IN (SELECT id FROM custom_fields WHERE entity = 'task')", id); err != nil {
			return nil, err
		}
	case core.EntProgram:
		ps, err := queryInts(x.c, "SELECT id FROM projects WHERE program_id = ?1", id)
		if err != nil {
			return nil, err
		}
		for _, p := range ps {
			x.touched[key(core.EntProject, p)] = true
		}
	case core.EntTemplate:
		ps, err := queryInts(x.c, "SELECT id FROM projects WHERE template_id = ?1", id)
		if err != nil {
			return nil, err
		}
		for _, p := range ps {
			x.touched[key(core.EntProject, p)] = true
		}
		ts, err := queryInts(x.c, "SELECT id FROM template_tasks WHERE template_id = ?1", id)
		if err != nil {
			return nil, err
		}
		for _, t := range ts {
			x.deleted[key(core.EntTemplateTask, t)] = true
		}
	case core.EntCustomField:
		var ent string
		if err := x.c.QueryRow("SELECT entity FROM custom_fields WHERE id = ?1", id).Scan(&ent); err != nil {
			return nil, err
		}
		target := core.EntTask
		if ent == "project" {
			target = core.EntProject
		}
		rows, err := queryInts(x.c, "SELECT entity_id FROM custom_values WHERE field_id = ?1", id)
		if err != nil {
			return nil, err
		}
		for _, r := range rows {
			x.touched[key(target, r)] = true
		}
	}
	if _, err := x.c.Exec("DELETE FROM "+e.Table()+" WHERE id = ?1", id); err != nil {
		return nil, err
	}
	x.deleted[key(e, id)] = true
	delete(x.touched, key(e, id))
	return bundle, nil
}

func (x *cx) markRestored(e core.Entity, id int64) {
	x.touched[key(e, id)] = true
	delete(x.deleted, key(e, id))
}

// restore re-inserts a bundle with its original ids (undo of delete, redo of create).
func (x *cx) restore(e core.Entity, bundle any) error {
	bm, _ := bundle.(map[string]any)
	row := bm["row"]
	bad := func(err error) error { return fmt.Errorf("bad bundle: %w", err) }
	switch e {
	case core.EntProject:
		var p core.Project
		if err := remarshal(row, &p); err != nil {
			return bad(err)
		}
		if ok, err := x.exists(e, p.ID); err != nil || ok {
			return err
		}
		c, err := x.clash("SELECT id FROM projects WHERE code = ?1", p.Code)
		if err != nil {
			return err
		}
		if c {
			return invalid("code %s has been reused; can't restore", p.Code)
		}
		if _, err := InsertProject(x.c, &p); err != nil {
			return err
		}
		var tasks []core.Task
		if bm["tasks"] != nil {
			if err := remarshal(bm["tasks"], &tasks); err != nil {
				return bad(err)
			}
		}
		for i := range tasks {
			if _, err := InsertTask(x.c, &tasks[i]); err != nil {
				return err
			}
			x.markRestored(core.EntTask, tasks[i].ID)
		}
		for _, t := range tasks {
			for _, d := range t.Deps {
				if err := InsertDep(x.c, t.ID, d); err != nil {
					return err
				}
			}
		}
		x.markRestored(e, p.ID)
	case core.EntTask:
		var t core.Task
		if err := remarshal(row, &t); err != nil {
			return bad(err)
		}
		if ok, err := x.exists(e, t.ID); err != nil || ok {
			return err
		}
		if ok, err := x.exists(core.EntProject, t.ProjectID); err != nil {
			return err
		} else if !ok {
			return notFound("its project was deleted; can't restore the task")
		}
		taken, err := x.clash("SELECT id FROM tasks WHERE project_id = ?1 AND seq = ?2", t.ProjectID, t.Seq)
		if err != nil {
			return err
		}
		if taken {
			if t.Seq, err = x.nextInt("SELECT COALESCE(MAX(seq), 0) + 1 FROM tasks WHERE project_id = ?1", t.ProjectID); err != nil {
				return err
			}
		}
		if _, err := InsertTask(x.c, &t); err != nil {
			return err
		}
		for _, d := range t.Deps {
			if err := InsertDep(x.c, t.ID, d); err != nil {
				return err
			}
		}
		var dependents []int64
		if bm["dependents"] != nil {
			_ = remarshal(bm["dependents"], &dependents)
		}
		for _, dep := range dependents {
			if err := InsertDep(x.c, dep, t.ID); err != nil {
				return err
			}
			if ok, _ := x.exists(core.EntTask, dep); ok {
				x.touched[key(core.EntTask, dep)] = true
			}
		}
		x.markRestored(e, t.ID)
	case core.EntProgram:
		var p core.Program
		if err := remarshal(row, &p); err != nil {
			return bad(err)
		}
		if ok, err := x.exists(e, p.ID); err != nil {
			return err
		} else if !ok {
			if _, err := InsertProgram(x.c, &p); err != nil {
				return err
			}
		}
		x.markRestored(e, p.ID)
	case core.EntTemplate:
		var t core.Template
		if err := remarshal(row, &t); err != nil {
			return bad(err)
		}
		if ok, err := x.exists(e, t.ID); err != nil {
			return err
		} else if !ok {
			if _, err := InsertTemplate(x.c, &t); err != nil {
				return err
			}
		}
		x.markRestored(e, t.ID)
	case core.EntTemplateTask:
		var t core.TemplateTask
		if err := remarshal(row, &t); err != nil {
			return bad(err)
		}
		if ok, err := x.exists(core.EntTemplate, t.TemplateID); err != nil {
			return err
		} else if !ok {
			return notFound("its template was deleted")
		}
		if ok, err := x.exists(e, t.ID); err != nil {
			return err
		} else if !ok {
			if _, err := InsertTemplateTask(x.c, &t); err != nil {
				return err
			}
		}
		x.markRestored(e, t.ID)
	case core.EntCustomField:
		var f core.CustomField
		if err := remarshal(row, &f); err != nil {
			return bad(err)
		}
		if ok, err := x.exists(e, f.ID); err != nil {
			return err
		} else if !ok {
			if _, err := InsertCustomField(x.c, &f); err != nil {
				return err
			}
		}
		x.markRestored(e, f.ID)
	case core.EntUser:
		return invalid("user changes can't be undone")
	}
	return nil
}

func sortedKeys(m map[core.RowKey]bool) []core.RowKey {
	order := map[core.Entity]int{}
	for i, e := range core.AllEntities {
		order[e] = i
	}
	out := make([]core.RowKey, 0, len(m))
	for k := range m {
		out = append(out, k)
	}
	sort.Slice(out, func(i, j int) bool {
		if out[i].Entity != out[j].Entity {
			return order[out[i].Entity] < order[out[j].Entity]
		}
		return out[i].ID < out[j].ID
	})
	return out
}

// validate: cross-field rules over every touched row (protocol §5 rule 4).
func (x *cx) validate() error {
	for _, k := range sortedKeys(x.touched) {
		if k.Entity != core.EntProject && k.Entity != core.EntTask {
			continue
		}
		ok, err := x.exists(k.Entity, k.ID)
		if err != nil {
			return err
		}
		if !ok {
			continue
		}
		var s, d sql.NullString
		if err := x.c.QueryRow("SELECT start_date, due_date FROM "+k.Entity.Table()+" WHERE id = ?1", k.ID).Scan(&s, &d); err != nil {
			return err
		}
		if s.Valid && d.Valid && d.String < s.String {
			return invalid("%s: due date %s is before start date %s", x.describe(k.Entity, k.ID), d.String, s.String)
		}
		if k.Entity == core.EntTask {
			if err := x.checkDeps(k.ID); err != nil {
				return err
			}
		}
	}
	return nil
}

func (x *cx) checkDeps(id int64) error {
	var pid int64
	if err := x.c.QueryRow("SELECT project_id FROM tasks WHERE id = ?1", id).Scan(&pid); err != nil {
		return err
	}
	foreign, err := x.clash(`SELECT d.depends_on FROM task_deps d JOIN tasks b ON b.id = d.depends_on
		WHERE d.task_id = ?1 AND b.project_id <> ?2`, id, pid)
	if err != nil {
		return err
	}
	if foreign {
		return invalid("a task can only depend on tasks in the same project")
	}
	rows, err := x.c.Query(`SELECT d.task_id, d.depends_on FROM task_deps d JOIN tasks a ON a.id = d.task_id WHERE a.project_id = ?1`, pid)
	if err != nil {
		return err
	}
	adj := map[int64][]int64{}
	for rows.Next() {
		var a, b int64
		if err := rows.Scan(&a, &b); err != nil {
			rows.Close()
			return err
		}
		adj[a] = append(adj[a], b)
	}
	rows.Close()
	// DFS from id looking for a path back to id
	stack := append([]int64(nil), adj[id]...)
	seen := map[int64]bool{}
	for len(stack) > 0 {
		n := stack[len(stack)-1]
		stack = stack[:len(stack)-1]
		if n == id {
			return invalid("%s would depend on itself (dependency loop)", x.describe(core.EntTask, id))
		}
		if !seen[n] {
			seen[n] = true
			stack = append(stack, adj[n]...)
		}
	}
	return nil
}

func jsonText(v any, has bool) any {
	if !has {
		return nil
	}
	b, err := json.Marshal(v)
	if err != nil {
		return nil
	}
	return string(b)
}

// finish writes activity and collects changed rows. record is false for undo/redo.
func (x *cx) finish(record bool) (int64, *core.Changes, []Kick, error) {
	var group int64
	if record {
		var err error
		if group, err = x.nextInt("SELECT COALESCE(MAX(grp), 0) + 1 FROM activity"); err != nil {
			return 0, nil, nil, err
		}
		// a new change makes this user's undone groups unreachable for redo
		dead, err := queryInts(x.c, "SELECT id FROM activity WHERE user_id = ?1 AND state = 'undone'", x.actor.ID)
		if err != nil {
			return 0, nil, nil, err
		}
		if _, err := x.c.Exec("UPDATE activity SET state = 'dead' WHERE user_id = ?1 AND state = 'undone'", x.actor.ID); err != nil {
			return 0, nil, nil, err
		}
		x.updatedActivity = append(x.updatedActivity, dead...)
		state := "done"
		for _, en := range x.entries {
			if !en.undoable {
				state = "dead"
			}
		}
		for _, en := range x.entries {
			r, err := x.c.Exec(`INSERT INTO activity (ts, user_id, grp, action, entity, entity_id, field, old, new, label, state)
				VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)`,
				x.now, x.actor.ID, group, string(en.action), string(en.entity), en.id, en.field,
				jsonText(en.old, en.hasOld), jsonText(en.new, en.hasNew), en.label, state)
			if err != nil {
				return 0, nil, nil, err
			}
			id, _ := r.LastInsertId()
			x.updatedActivity = append(x.updatedActivity, id)
		}
	}
	var ch core.Changes
	for _, k := range sortedKeys(x.touched) {
		if !x.deleted[k] {
			if err := loadRows(x.c, k.Entity, byID(k.ID), &ch.Upserts); err != nil {
				return 0, nil, nil, err
			}
		}
	}
	for _, k := range sortedKeys(x.deleted) {
		d := ch.Deletes.Get(k.Entity)
		*d = append(*d, k.ID)
	}
	sort.Slice(x.updatedActivity, func(i, j int) bool { return x.updatedActivity[i] < x.updatedActivity[j] })
	var ids []int64
	for i, id := range x.updatedActivity {
		if i == 0 || id != x.updatedActivity[i-1] {
			ids = append(ids, id)
		}
	}
	act, err := loadActivityIDs(x.c, ids)
	if err != nil {
		return 0, nil, nil, err
	}
	ch.Activity = act
	by := x.actor.ID
	ch.By = &by
	if group > 0 {
		g := group
		ch.Group = &g
	}
	kick := x.kick
	x.kick = nil
	return group, &ch, kick, nil
}

// ---- labels ----

func (x *cx) str(query string, id int64) (string, bool) {
	var s sql.NullString
	if err := x.c.QueryRow(query, id).Scan(&s); err != nil || !s.Valid {
		return "", false
	}
	return s.String, true
}

func (x *cx) describe(e core.Entity, id int64) string {
	var qy string
	switch e {
	case core.EntProject:
		qy = "SELECT code || ' ''' || name || '''' FROM projects WHERE id = ?1"
	case core.EntTask:
		qy = "SELECT p.code || ' #' || t.seq || ' ''' || t.title || '''' FROM tasks t JOIN projects p ON p.id = t.project_id WHERE t.id = ?1"
	case core.EntUser:
		qy = "SELECT username FROM users WHERE id = ?1"
	case core.EntProgram:
		qy = "SELECT name FROM programs WHERE id = ?1"
	case core.EntTemplate:
		qy = "SELECT name FROM templates WHERE id = ?1"
	case core.EntTemplateTask:
		qy = "SELECT tt.title || ' (' || t.name || ')' FROM template_tasks tt JOIN templates t ON t.id = tt.template_id WHERE tt.id = ?1"
	default:
		qy = "SELECT name FROM custom_fields WHERE id = ?1"
	}
	if s, ok := x.str(qy, id); ok {
		return s
	}
	return fmt.Sprintf("#%d", id)
}

func (x *cx) shortRef(e core.Entity, id int64) string {
	var qy string
	switch e {
	case core.EntProject:
		qy = "SELECT code FROM projects WHERE id = ?1"
	case core.EntTask:
		qy = "SELECT p.code || ' #' || t.seq FROM tasks t JOIN projects p ON p.id = t.project_id WHERE t.id = ?1"
	default:
		return x.describe(e, id)
	}
	if s, ok := x.str(qy, id); ok {
		return s
	}
	return fmt.Sprintf("#%d", id)
}

func (x *cx) fmtValue(e core.Entity, field string, v any) string {
	name := func(qy string, id int64) string {
		if s, ok := x.str(qy, id); ok {
			return s
		}
		return fmt.Sprint(id)
	}
	var s string
	switch val := v.(type) {
	case nil:
		s = "∅"
	case bool:
		s = "no"
		if val {
			s = "yes"
		}
	case string:
		s = val
		switch {
		case e == core.EntProject && field == "status":
			if p, ok := core.ParseProjectStatus(val); ok {
				s = p.Label()
			}
		case e == core.EntTask && field == "status":
			if p, ok := core.ParseTaskStatus(val); ok {
				s = p.Label()
			}
		case field == "priority":
			if p, ok := core.ParsePriority(val); ok {
				s = p.Label()
			}
		case field == "health":
			if p, ok := core.ParseHealth(val); ok {
				s = p.Label()
			}
		}
	default:
		if list, ok := core.AsList(v); ok {
			parts := make([]string, 0, len(list))
			for _, it := range list {
				if field == "deps" {
					if id, ok := core.AsInt(it); ok {
						parts = append(parts, name("SELECT '#' || seq FROM tasks WHERE id = ?1", id))
					}
				} else if str, ok := it.(string); ok {
					parts = append(parts, str)
				} else {
					parts = append(parts, "?")
				}
			}
			s = strings.Join(parts, " ")
		} else if f, ok := core.AsFloat(v); ok {
			_, isInt := v.(int64)
			id := int64(f)
			switch {
			case isInt && (field == "owner_id" || field == "assignee_id"):
				s = name("SELECT username FROM users WHERE id = ?1", id)
			case isInt && field == "program_id":
				s = name("SELECT name FROM programs WHERE id = ?1", id)
			default:
				s = core.FmtHours(f)
			}
		} else {
			s = "…"
		}
	}
	s = strings.ReplaceAll(s, "\n", " ")
	if r := []rune(s); len(r) > 40 {
		s = string(r[:39]) + "…"
	}
	return s
}

func (x *cx) setLabel(e core.Entity, id int64, field string, old, nw any) string {
	fname := field
	if fid, ok := core.CfID(field); ok {
		if s, ok := x.str("SELECT name FROM custom_fields WHERE id = ?1", fid); ok {
			fname = s
		}
	} else if m := core.Field(e, field); m != nil {
		fname = strings.ToLower(m.Label)
	}
	if field == "password" {
		return x.shortRef(e, id) + " password changed"
	}
	return fmt.Sprintf("%s %s: %s → %s", x.shortRef(e, id), fname, x.fmtValue(e, field, old), x.fmtValue(e, field, nw))
}
