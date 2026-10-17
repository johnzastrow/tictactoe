package core

// Client-side copy of the server's data, kept current by snapshot and
// changes messages, plus the indexes and computed metrics every view
// needs (protocol §8).

import (
	"encoding/json"
	"fmt"
	"sort"
	"strconv"
	"strings"
)

type Metrics struct {
	TasksTotal   int
	TasksDone    int
	TasksOpen    int
	TasksBlocked int
	TasksOverdue int
	EstTotal     float64
	ActualTotal  float64
	NextDue      *Date
}

func (m Metrics) PctDone() float64 {
	if m.TasksTotal == 0 {
		return 0
	}
	return float64(m.TasksDone) * 100 / float64(m.TasksTotal)
}

// RowKey identifies a row.
type RowKey struct {
	Entity Entity
	ID     int64
}

// Applied says what a message changed, so UIs can flash rows and toast.
type Applied struct {
	Snapshot bool
	By       *int64
	Upserted []RowKey
	Deleted  []RowKey
	Activity []Activity
	Presence bool
}

type Replica struct {
	Me            *User
	Conn          uint64
	Server        *ServerInfo
	Today         Date
	Users         map[int64]*User
	Programs      map[int64]*Program
	Templates     map[int64]*Template
	TemplateTasks map[int64]*TemplateTask
	Projects      map[int64]*Project
	Tasks         map[int64]*Task
	CustomFields  map[int64]*CustomField
	Activity      map[int64]*Activity
	Peers         []Peer
	// Version is bumped on every change; views cache derived data against it.
	Version uint64

	tasksByProject map[int64][]int64
	metrics        map[int64]Metrics
	loaded         bool
}

func NewReplica(today Date) *Replica {
	r := &Replica{Today: today}
	r.reset()
	return r
}

func (r *Replica) reset() {
	r.Users = map[int64]*User{}
	r.Programs = map[int64]*Program{}
	r.Templates = map[int64]*Template{}
	r.TemplateTasks = map[int64]*TemplateTask{}
	r.Projects = map[int64]*Project{}
	r.Tasks = map[int64]*Task{}
	r.CustomFields = map[int64]*CustomField{}
	r.Activity = map[int64]*Activity{}
	r.tasksByProject = map[int64][]int64{}
	r.metrics = map[int64]Metrics{}
}

func (r *Replica) IsLoaded() bool { return r.loaded }

func (r *Replica) MeID() int64 {
	if r.Me == nil {
		return 0
	}
	return r.Me.ID
}

func (r *Replica) CanEdit() bool { return r.Me != nil && r.Me.CanEdit() }
func (r *Replica) IsAdmin() bool { return r.Me != nil && r.Me.IsAdmin() }

func (r *Replica) SetToday(d Date) {
	if d != r.Today {
		r.Today = d
		r.reindex()
	}
}

// Apply any decoded server message.
func (r *Replica) Apply(msg any) Applied {
	var out Applied
	switch m := msg.(type) {
	case WelcomeMsg:
		me := m.Me
		r.Me = &me
		r.Conn = m.Conn
		srv := m.Server
		r.Server = &srv
	case SnapshotMsg:
		r.Load(m.Data)
		out.Snapshot = true
	case Changes:
		out = r.ApplyChanges(m)
	case PresenceMsg:
		r.Peers = m.Peers
		out.Presence = true
	default:
		return out
	}
	r.Version++
	return out
}

func (r *Replica) Load(t Tables) {
	r.reset()
	for i := range t.Users {
		r.Users[t.Users[i].ID] = &t.Users[i]
	}
	for i := range t.Programs {
		r.Programs[t.Programs[i].ID] = &t.Programs[i]
	}
	for i := range t.Templates {
		r.Templates[t.Templates[i].ID] = &t.Templates[i]
	}
	for i := range t.TemplateTasks {
		r.TemplateTasks[t.TemplateTasks[i].ID] = &t.TemplateTasks[i]
	}
	for i := range t.Projects {
		r.Projects[t.Projects[i].ID] = &t.Projects[i]
	}
	for i := range t.Tasks {
		r.Tasks[t.Tasks[i].ID] = &t.Tasks[i]
	}
	for i := range t.CustomFields {
		r.CustomFields[t.CustomFields[i].ID] = &t.CustomFields[i]
	}
	for i := range t.Activity {
		r.Activity[t.Activity[i].ID] = &t.Activity[i]
	}
	r.loaded = true
	r.refreshMe()
	r.reindex()
	r.Version++
}

func (r *Replica) refreshMe() {
	if r.Me != nil {
		if u, ok := r.Users[r.Me.ID]; ok {
			c := *u
			r.Me = &c
		}
	}
}

func (r *Replica) ApplyChanges(c Changes) Applied {
	out := Applied{By: c.By}
	for i := range c.Upserts.Users {
		v := c.Upserts.Users[i]
		r.Users[v.ID] = &v
		out.Upserted = append(out.Upserted, RowKey{EntUser, v.ID})
	}
	for _, id := range c.Deletes.Users {
		delete(r.Users, id)
		out.Deleted = append(out.Deleted, RowKey{EntUser, id})
	}
	for i := range c.Upserts.Programs {
		v := c.Upserts.Programs[i]
		r.Programs[v.ID] = &v
		out.Upserted = append(out.Upserted, RowKey{EntProgram, v.ID})
	}
	for _, id := range c.Deletes.Programs {
		delete(r.Programs, id)
		out.Deleted = append(out.Deleted, RowKey{EntProgram, id})
	}
	for i := range c.Upserts.Templates {
		v := c.Upserts.Templates[i]
		r.Templates[v.ID] = &v
		out.Upserted = append(out.Upserted, RowKey{EntTemplate, v.ID})
	}
	for _, id := range c.Deletes.Templates {
		delete(r.Templates, id)
		out.Deleted = append(out.Deleted, RowKey{EntTemplate, id})
	}
	for i := range c.Upserts.TemplateTasks {
		v := c.Upserts.TemplateTasks[i]
		r.TemplateTasks[v.ID] = &v
		out.Upserted = append(out.Upserted, RowKey{EntTemplateTask, v.ID})
	}
	for _, id := range c.Deletes.TemplateTasks {
		delete(r.TemplateTasks, id)
		out.Deleted = append(out.Deleted, RowKey{EntTemplateTask, id})
	}
	for i := range c.Upserts.Projects {
		v := c.Upserts.Projects[i]
		r.Projects[v.ID] = &v
		out.Upserted = append(out.Upserted, RowKey{EntProject, v.ID})
	}
	for _, id := range c.Deletes.Projects {
		delete(r.Projects, id)
		out.Deleted = append(out.Deleted, RowKey{EntProject, id})
	}
	for i := range c.Upserts.Tasks {
		v := c.Upserts.Tasks[i]
		r.Tasks[v.ID] = &v
		out.Upserted = append(out.Upserted, RowKey{EntTask, v.ID})
	}
	for _, id := range c.Deletes.Tasks {
		delete(r.Tasks, id)
		out.Deleted = append(out.Deleted, RowKey{EntTask, id})
	}
	for i := range c.Upserts.CustomFields {
		v := c.Upserts.CustomFields[i]
		r.CustomFields[v.ID] = &v
		out.Upserted = append(out.Upserted, RowKey{EntCustomField, v.ID})
	}
	for _, id := range c.Deletes.CustomFields {
		delete(r.CustomFields, id)
		out.Deleted = append(out.Deleted, RowKey{EntCustomField, id})
	}
	// A deleted project takes its tasks with it.
	for _, pid := range c.Deletes.Projects {
		for id, t := range r.Tasks {
			if t.ProjectID == pid {
				delete(r.Tasks, id)
			}
		}
	}
	for _, list := range [][]Activity{c.Activity, c.Upserts.Activity} {
		for i := range list {
			a := list[i]
			r.Activity[a.ID] = &a
			out.Activity = append(out.Activity, a)
		}
	}
	if len(r.Activity) > 2000 {
		ids := sortedKeys(r.Activity)
		for _, id := range ids[:len(ids)-2000] {
			delete(r.Activity, id)
		}
	}
	r.refreshMe()
	r.reindex()
	r.Version++
	return out
}

func sortedKeys[V any](m map[int64]V) []int64 {
	ids := make([]int64, 0, len(m))
	for id := range m {
		ids = append(ids, id)
	}
	sort.Slice(ids, func(i, j int) bool { return ids[i] < ids[j] })
	return ids
}

// SortedIDs returns a map's ids in ascending order (Rust BTreeMap order).
func SortedIDs[V any](m map[int64]V) []int64 { return sortedKeys(m) }

func parseDatePtr(s *string) (Date, bool) {
	if s == nil {
		return 0, false
	}
	return ParseISO(*s)
}

func (r *Replica) reindex() {
	byProject := map[int64][]int64{}
	for _, id := range sortedKeys(r.Tasks) {
		t := r.Tasks[id]
		byProject[t.ProjectID] = append(byProject[t.ProjectID], id)
	}
	for _, ids := range byProject {
		sort.SliceStable(ids, func(i, j int) bool { return r.Tasks[ids[i]].Seq < r.Tasks[ids[j]].Seq })
	}
	metrics := map[int64]Metrics{}
	for _, p := range r.Projects {
		var m Metrics
		for _, id := range byProject[p.ID] {
			t := r.Tasks[id]
			m.TasksTotal++
			if t.Status == TaskDone {
				m.TasksDone++
			} else {
				m.TasksOpen++
				if d, ok := parseDatePtr(t.DueDate); ok {
					if d < r.Today {
						m.TasksOverdue++
					}
					if m.NextDue == nil || d < *m.NextDue {
						dd := d
						m.NextDue = &dd
					}
				}
			}
			if t.Status == TaskBlocked {
				m.TasksBlocked++
			}
			if t.EstHours != nil {
				m.EstTotal += *t.EstHours
			}
			m.ActualTotal += t.ActualHours
		}
		metrics[p.ID] = m
	}
	r.tasksByProject = byProject
	r.metrics = metrics
}

// ---- lookups ----

func (r *Replica) ProjectTasks(pid int64) []int64 { return r.tasksByProject[pid] }
func (r *Replica) Metrics(pid int64) Metrics      { return r.metrics[pid] }

func (r *Replica) UserName(id *int64) string {
	if id != nil {
		if u, ok := r.Users[*id]; ok {
			return u.DisplayName
		}
	}
	return ""
}

func (r *Replica) Username(id *int64) string {
	if id != nil {
		if u, ok := r.Users[*id]; ok {
			return u.Username
		}
	}
	return ""
}

func (r *Replica) ProgramName(id *int64) string {
	if id != nil {
		if p, ok := r.Programs[*id]; ok {
			return p.Name
		}
	}
	return ""
}

func (r *Replica) ProjectCode(id int64) string {
	if p, ok := r.Projects[id]; ok {
		return p.Code
	}
	return "?"
}

// TaskRef: "NWR-004 #5".
func (r *Replica) TaskRef(t *Task) string {
	return fmt.Sprintf("%s #%d", r.ProjectCode(t.ProjectID), t.Seq)
}

func (r *Replica) TaskStatusOf(id int64) (TaskStatus, bool) {
	if t, ok := r.Tasks[id]; ok {
		return t.Status, true
	}
	return "", false
}

// TaskBlocked: status blocked, or a dependency not done while work is under way.
func (r *Replica) TaskBlocked(t *Task) bool {
	if t.Status == TaskBlocked {
		return true
	}
	if t.Status == TaskInProgress || t.Status == TaskReview {
		for _, d := range t.Deps {
			if s, ok := r.TaskStatusOf(d); ok && s != TaskDone {
				return true
			}
		}
	}
	return false
}

func (r *Replica) TaskOverdue(t *Task) bool {
	d, ok := parseDatePtr(t.DueDate)
	return t.Status != TaskDone && ok && d < r.Today
}

func (r *Replica) ProjectOpen(p *Project) bool {
	return p.Status != ProjectDone && p.Status != ProjectCancelled
}

func (r *Replica) ProjectOverdue(p *Project) bool {
	d, ok := parseDatePtr(p.DueDate)
	return r.ProjectOpen(p) && ok && d < r.Today
}

func (r *Replica) BurnPct(p *Project) (float64, bool) {
	if p.BudgetHours == nil || *p.BudgetHours <= 0 {
		return 0, false
	}
	return r.Metrics(p.ID).ActualTotal * 100 / *p.BudgetHours, true
}

func (r *Replica) CustomFieldsFor(e CfEntity) []*CustomField {
	var v []*CustomField
	for _, c := range r.CustomFields {
		if c.Entity == e {
			v = append(v, c)
		}
	}
	sort.Slice(v, func(i, j int) bool {
		if v[i].Seq != v[j].Seq {
			return v[i].Seq < v[j].Seq
		}
		return v[i].ID < v[j].ID
	})
	return v
}

func (r *Replica) TemplateTasksOf(tid int64) []*TemplateTask {
	var v []*TemplateTask
	for _, t := range r.TemplateTasks {
		if t.TemplateID == tid {
			v = append(v, t)
		}
	}
	sort.Slice(v, func(i, j int) bool {
		if v[i].Seq != v[j].Seq {
			return v[i].Seq < v[j].Seq
		}
		return v[i].ID < v[j].ID
	})
	return v
}

// TemplateStats: (task count, estimated hours, span in days).
func (r *Replica) TemplateStats(tid int64) (int, float64, int64) {
	tt := r.TemplateTasksOf(tid)
	hours := 0.0
	var span int64
	for _, t := range tt {
		if t.EstHours != nil {
			hours += *t.EstHours
		}
		span = max(span, t.OffsetDays+t.DurationDays)
	}
	return len(tt), hours, span
}

// PhasesForProject: template phases in template order, then other phases in use.
func (r *Replica) PhasesForProject(pid int64) []string {
	out := []string{}
	if p, ok := r.Projects[pid]; ok && p.TemplateID != nil {
		for _, t := range r.TemplateTasksOf(*p.TemplateID) {
			if t.Phase != "" && !containsStr(out, t.Phase) {
				out = append(out, t.Phase)
			}
		}
	}
	for _, id := range r.ProjectTasks(pid) {
		ph := r.Tasks[id].Phase
		if ph != "" && !containsStr(out, ph) {
			out = append(out, ph)
		}
	}
	return out
}

// NextCode: next free project code for a program prefix, e.g. NWR-008.
func (r *Replica) NextCode(programID *int64) string {
	prefix := "PRJ"
	if programID != nil {
		if p, ok := r.Programs[*programID]; ok && p.Prefix != "" {
			prefix = p.Prefix
		}
	}
	maxN := 0
	for _, p := range r.Projects {
		if rest, ok := strings.CutPrefix(p.Code, prefix+"-"); ok {
			if n, err := strconv.ParseUint(rest, 10, 32); err == nil && int(n) > maxN {
				maxN = int(n)
			}
		}
	}
	return fmt.Sprintf("%s-%03d", prefix, maxN+1)
}

type TagCount struct {
	Tag   string
	Count int
}

// TagCounts: tags in use for an entity kind, most used first.
func (r *Replica) TagCounts(e Entity) []TagCount {
	m := map[string]int{}
	if e == EntProject {
		for _, p := range r.Projects {
			for _, t := range p.Tags {
				m[t]++
			}
		}
	} else {
		for _, t := range r.Tasks {
			for _, g := range t.Tags {
				m[g]++
			}
		}
	}
	out := make([]TagCount, 0, len(m))
	for k, v := range m {
		out = append(out, TagCount{k, v})
	}
	sort.Slice(out, func(i, j int) bool {
		if out[i].Count != out[j].Count {
			return out[i].Count > out[j].Count
		}
		return out[i].Tag < out[j].Tag
	})
	return out
}

// rowJSON round-trips a row through JSON to read a field generically.
func rowJSON(v any) map[string]any {
	b, err := json.Marshal(v)
	if err != nil {
		return nil
	}
	var m map[string]any
	if json.Unmarshal(b, &m) != nil {
		return nil
	}
	return m
}

// FieldValue is the current value of a field as JSON: what an edit sends as base.
func (r *Replica) FieldValue(e Entity, id int64, field string) any {
	if cf, ok := strings.CutPrefix(field, "cf."); ok {
		var m CfValues
		switch e {
		case EntProject:
			if p, ok := r.Projects[id]; ok {
				m = p.CF
			}
		case EntTask:
			if t, ok := r.Tasks[id]; ok {
				m = t.CF
			}
		}
		if v, ok := m[cf]; ok {
			return v
		}
		return nil
	}
	var row any
	switch e {
	case EntUser:
		if v, ok := r.Users[id]; ok {
			row = v
		}
	case EntProgram:
		if v, ok := r.Programs[id]; ok {
			row = v
		}
	case EntTemplate:
		if v, ok := r.Templates[id]; ok {
			row = v
		}
	case EntTemplateTask:
		if v, ok := r.TemplateTasks[id]; ok {
			row = v
		}
	case EntProject:
		if v, ok := r.Projects[id]; ok {
			row = v
		}
	case EntTask:
		if v, ok := r.Tasks[id]; ok {
			row = v
		}
	case EntCustomField:
		if v, ok := r.CustomFields[id]; ok {
			row = v
		}
	}
	if row == nil {
		return nil
	}
	return rowJSON(row)[field]
}

func (r *Replica) Exists(e Entity, id int64) bool {
	switch e {
	case EntUser:
		_, ok := r.Users[id]
		return ok
	case EntProgram:
		_, ok := r.Programs[id]
		return ok
	case EntTemplate:
		_, ok := r.Templates[id]
		return ok
	case EntTemplateTask:
		_, ok := r.TemplateTasks[id]
		return ok
	case EntProject:
		_, ok := r.Projects[id]
		return ok
	case EntTask:
		_, ok := r.Tasks[id]
		return ok
	case EntCustomField:
		_, ok := r.CustomFields[id]
		return ok
	}
	return false
}

// RowLabel is a short label for toasts and the activity view.
func (r *Replica) RowLabel(e Entity, id int64) string {
	switch e {
	case EntProject:
		if p, ok := r.Projects[id]; ok {
			return p.Code
		}
		return fmt.Sprintf("project %d", id)
	case EntTask:
		if t, ok := r.Tasks[id]; ok {
			return r.TaskRef(t)
		}
		return fmt.Sprintf("task %d", id)
	case EntUser:
		if u, ok := r.Users[id]; ok {
			return u.Username
		}
		return fmt.Sprintf("user %d", id)
	case EntProgram:
		if p, ok := r.Programs[id]; ok {
			return p.Name
		}
		return fmt.Sprintf("program %d", id)
	case EntTemplate:
		if t, ok := r.Templates[id]; ok {
			return t.Name
		}
		return fmt.Sprintf("template %d", id)
	case EntTemplateTask:
		return fmt.Sprintf("template task %d", id)
	}
	if c, ok := r.CustomFields[id]; ok {
		return c.Name
	}
	return fmt.Sprintf("field %d", id)
}

// OtherPeers: connections that aren't ours.
func (r *Replica) OtherPeers() []Peer {
	var out []Peer
	for _, p := range r.Peers {
		if p.Conn != r.Conn {
			out = append(out, p)
		}
	}
	return out
}

// OnlineUsers: distinct online users (including me), by id.
func (r *Replica) OnlineUsers() []*User {
	seen := map[int64]bool{}
	var ids []int64
	for _, p := range r.Peers {
		if !seen[p.UserID] {
			seen[p.UserID] = true
			ids = append(ids, p.UserID)
		}
	}
	sort.Slice(ids, func(i, j int) bool { return ids[i] < ids[j] })
	var out []*User
	for _, id := range ids {
		if u, ok := r.Users[id]; ok {
			out = append(out, u)
		}
	}
	return out
}
