package core

// Row types exactly as they travel on the wire (protocol §3).

import (
	"encoding/json"
	"fmt"
	"strings"
	"unicode"
)

// enumDef describes a string enum: wire names and labels, in order.
type enumDef struct {
	names  []string
	labels []string
}

func (e enumDef) label(s string) string {
	for i, n := range e.names {
		if n == s {
			return e.labels[i]
		}
	}
	return s
}

func (e enumDef) index(s string) int {
	for i, n := range e.names {
		if n == s {
			return i
		}
	}
	return 0
}

// parse accepts the wire name or the label, case-insensitively.
func (e enumDef) parse(s string) (string, bool) {
	t := strings.TrimSpace(s)
	for i, n := range e.names {
		if strings.EqualFold(n, t) || strings.EqualFold(e.labels[i], t) {
			return n, true
		}
	}
	return "", false
}

func (e enumDef) unmarshal(kind string, b []byte) (string, error) {
	var s string
	if err := json.Unmarshal(b, &s); err != nil {
		return "", err
	}
	for _, n := range e.names {
		if n == s {
			return s, nil
		}
	}
	return "", fmt.Errorf("unknown %s %q", kind, s)
}

var (
	roleDef          = enumDef{[]string{"admin", "member", "viewer"}, []string{"Admin", "Member", "Viewer"}}
	projectStatusDef = enumDef{[]string{"planning", "active", "on_hold", "done", "cancelled"}, []string{"Planning", "Active", "On hold", "Done", "Cancelled"}}
	taskStatusDef    = enumDef{[]string{"todo", "in_progress", "blocked", "review", "done"}, []string{"To do", "In progress", "Blocked", "Review", "Done"}}
	priorityDef      = enumDef{[]string{"low", "medium", "high", "critical"}, []string{"Low", "Medium", "High", "Critical"}}
	healthDef        = enumDef{[]string{"green", "amber", "red"}, []string{"Green", "Amber", "Red"}}
	cfKindDef        = enumDef{[]string{"text", "number", "date", "enum"}, []string{"Text", "Number", "Date", "Enum"}}
	cfEntityDef      = enumDef{[]string{"project", "task"}, []string{"Project", "Task"}}
	actionDef        = enumDef{[]string{"set", "create", "delete"}, []string{"Set", "Create", "Delete"}}
	actStateDef      = enumDef{[]string{"done", "undone", "dead"}, []string{"Done", "Undone", "Final"}}
)

type Role string
type ProjectStatus string
type TaskStatus string
type Priority string
type Health string
type CfKind string
type CfEntity string
type Action string
type ActState string

const (
	RoleAdmin  Role = "admin"
	RoleMember Role = "member"
	RoleViewer Role = "viewer"

	ProjectPlanning  ProjectStatus = "planning"
	ProjectActive    ProjectStatus = "active"
	ProjectOnHold    ProjectStatus = "on_hold"
	ProjectDone      ProjectStatus = "done"
	ProjectCancelled ProjectStatus = "cancelled"

	TaskTodo       TaskStatus = "todo"
	TaskInProgress TaskStatus = "in_progress"
	TaskBlocked    TaskStatus = "blocked"
	TaskReview     TaskStatus = "review"
	TaskDone       TaskStatus = "done"

	PriorityLow      Priority = "low"
	PriorityMedium   Priority = "medium"
	PriorityHigh     Priority = "high"
	PriorityCritical Priority = "critical"

	HealthGreen Health = "green"
	HealthAmber Health = "amber"
	HealthRed   Health = "red"

	CfText   CfKind = "text"
	CfNumber CfKind = "number"
	CfDate   CfKind = "date"
	CfEnum   CfKind = "enum"

	CfProject CfEntity = "project"
	CfTask    CfEntity = "task"

	ActionSet    Action = "set"
	ActionCreate Action = "create"
	ActionDelete Action = "delete"

	StateDone   ActState = "done"
	StateUndone ActState = "undone"
	StateDead   ActState = "dead"
)

func castAll[T ~string](names []string) []T {
	out := make([]T, len(names))
	for i, n := range names {
		out[i] = T(n)
	}
	return out
}

var (
	AllRoles           = castAll[Role](roleDef.names)
	AllProjectStatuses = castAll[ProjectStatus](projectStatusDef.names)
	AllTaskStatuses    = castAll[TaskStatus](taskStatusDef.names)
	AllPriorities      = castAll[Priority](priorityDef.names)
	AllHealth          = castAll[Health](healthDef.names)
	AllCfKinds         = castAll[CfKind](cfKindDef.names)
	AllCfEntities      = castAll[CfEntity](cfEntityDef.names)
)

func (v Role) Label() string          { return roleDef.label(string(v)) }
func (v ProjectStatus) Label() string { return projectStatusDef.label(string(v)) }
func (v TaskStatus) Label() string    { return taskStatusDef.label(string(v)) }
func (v Priority) Label() string      { return priorityDef.label(string(v)) }
func (v Health) Label() string        { return healthDef.label(string(v)) }
func (v CfKind) Label() string        { return cfKindDef.label(string(v)) }
func (v CfEntity) Label() string      { return cfEntityDef.label(string(v)) }
func (v Action) Label() string        { return actionDef.label(string(v)) }
func (v ActState) Label() string      { return actStateDef.label(string(v)) }

func (v Role) Index() int          { return roleDef.index(string(v)) }
func (v ProjectStatus) Index() int { return projectStatusDef.index(string(v)) }
func (v TaskStatus) Index() int    { return taskStatusDef.index(string(v)) }
func (v Priority) Index() int      { return priorityDef.index(string(v)) }
func (v Health) Index() int        { return healthDef.index(string(v)) }

func ParseRole(s string) (Role, bool) { n, ok := roleDef.parse(s); return Role(n), ok }
func ParseProjectStatus(s string) (ProjectStatus, bool) {
	n, ok := projectStatusDef.parse(s)
	return ProjectStatus(n), ok
}
func ParseTaskStatus(s string) (TaskStatus, bool) {
	n, ok := taskStatusDef.parse(s)
	return TaskStatus(n), ok
}
func ParsePriority(s string) (Priority, bool) { n, ok := priorityDef.parse(s); return Priority(n), ok }
func ParseHealth(s string) (Health, bool)     { n, ok := healthDef.parse(s); return Health(n), ok }
func ParseCfKind(s string) (CfKind, bool)     { n, ok := cfKindDef.parse(s); return CfKind(n), ok }
func ParseCfEntity(s string) (CfEntity, bool) { n, ok := cfEntityDef.parse(s); return CfEntity(n), ok }
func ParseAction(s string) (Action, bool)     { n, ok := actionDef.parse(s); return Action(n), ok }
func ParseActState(s string) (ActState, bool) { n, ok := actStateDef.parse(s); return ActState(n), ok }

func (v *Role) UnmarshalJSON(b []byte) error {
	s, err := roleDef.unmarshal("role", b)
	*v = Role(s)
	return err
}
func (v *ProjectStatus) UnmarshalJSON(b []byte) error {
	s, err := projectStatusDef.unmarshal("project status", b)
	*v = ProjectStatus(s)
	return err
}
func (v *TaskStatus) UnmarshalJSON(b []byte) error {
	s, err := taskStatusDef.unmarshal("task status", b)
	*v = TaskStatus(s)
	return err
}
func (v *Priority) UnmarshalJSON(b []byte) error {
	s, err := priorityDef.unmarshal("priority", b)
	*v = Priority(s)
	return err
}
func (v *Health) UnmarshalJSON(b []byte) error {
	s, err := healthDef.unmarshal("health", b)
	*v = Health(s)
	return err
}
func (v *CfKind) UnmarshalJSON(b []byte) error {
	s, err := cfKindDef.unmarshal("kind", b)
	*v = CfKind(s)
	return err
}
func (v *CfEntity) UnmarshalJSON(b []byte) error {
	s, err := cfEntityDef.unmarshal("entity", b)
	*v = CfEntity(s)
	return err
}
func (v *Action) UnmarshalJSON(b []byte) error {
	s, err := actionDef.unmarshal("action", b)
	*v = Action(s)
	return err
}
func (v *ActState) UnmarshalJSON(b []byte) error {
	s, err := actStateDef.unmarshal("state", b)
	*v = ActState(s)
	return err
}

type User struct {
	ID          int64  `json:"id"`
	Username    string `json:"username"`
	DisplayName string `json:"display_name"`
	Role        Role   `json:"role"`
	Active      bool   `json:"active"`
}

func (u User) Initials() string {
	words := strings.Fields(u.DisplayName)
	if len(words) >= 2 {
		a := []rune(words[0])[0]
		b := []rune(words[len(words)-1])[0]
		return strings.ToUpper(string([]rune{a, b}))
	}
	r := []rune(u.Username)
	if len(r) > 2 {
		r = r[:2]
	}
	return strings.ToUpper(string(r))
}

func (u User) CanEdit() bool { return u.Role != RoleViewer }
func (u User) IsAdmin() bool { return u.Role == RoleAdmin }

type Program struct {
	ID     int64  `json:"id"`
	Name   string `json:"name"`
	Client string `json:"client"`
	Prefix string `json:"prefix"`
	Color  string `json:"color"`
}

type Template struct {
	ID          int64  `json:"id"`
	Name        string `json:"name"`
	Description string `json:"description"`
}

type TemplateTask struct {
	ID           int64    `json:"id"`
	TemplateID   int64    `json:"template_id"`
	Seq          int64    `json:"seq"`
	Title        string   `json:"title"`
	Phase        string   `json:"phase"`
	EstHours     *float64 `json:"est_hours"`
	OffsetDays   int64    `json:"offset_days"`
	DurationDays int64    `json:"duration_days"`
}

// CfValues maps custom-field id (as a string) to its value.
type CfValues map[string]any

type Project struct {
	ID          int64         `json:"id"`
	Code        string        `json:"code"`
	Name        string        `json:"name"`
	ProgramID   *int64        `json:"program_id"`
	TemplateID  *int64        `json:"template_id"`
	OwnerID     *int64        `json:"owner_id"`
	Status      ProjectStatus `json:"status"`
	Priority    Priority      `json:"priority"`
	Health      Health        `json:"health"`
	StartDate   *string       `json:"start_date"`
	DueDate     *string       `json:"due_date"`
	BudgetHours *float64      `json:"budget_hours"`
	Tags        []string      `json:"tags"`
	Notes       string        `json:"notes"`
	CF          CfValues      `json:"cf"`
	CreatedAt   string        `json:"created_at"`
	UpdatedAt   string        `json:"updated_at"`
	UpdatedBy   *int64        `json:"updated_by"`
}

type Task struct {
	ID          int64      `json:"id"`
	ProjectID   int64      `json:"project_id"`
	Seq         int64      `json:"seq"`
	Title       string     `json:"title"`
	Phase       string     `json:"phase"`
	Status      TaskStatus `json:"status"`
	Priority    Priority   `json:"priority"`
	AssigneeID  *int64     `json:"assignee_id"`
	StartDate   *string    `json:"start_date"`
	DueDate     *string    `json:"due_date"`
	EstHours    *float64   `json:"est_hours"`
	ActualHours float64    `json:"actual_hours"`
	Deps        []int64    `json:"deps"`
	Tags        []string   `json:"tags"`
	Notes       string     `json:"notes"`
	CF          CfValues   `json:"cf"`
	CreatedAt   string     `json:"created_at"`
	UpdatedAt   string     `json:"updated_at"`
	UpdatedBy   *int64     `json:"updated_by"`
}

// Rust's serde rejects null where it expects a list or map, so rows always
// serialise empty collections as [] / {}.

func (p Project) MarshalJSON() ([]byte, error) {
	type plain Project
	if p.Tags == nil {
		p.Tags = []string{}
	}
	if p.CF == nil {
		p.CF = CfValues{}
	}
	return json.Marshal(plain(p))
}

func (t Task) MarshalJSON() ([]byte, error) {
	type plain Task
	if t.Tags == nil {
		t.Tags = []string{}
	}
	if t.Deps == nil {
		t.Deps = []int64{}
	}
	if t.CF == nil {
		t.CF = CfValues{}
	}
	return json.Marshal(plain(t))
}

type CustomField struct {
	ID      int64    `json:"id"`
	Entity  CfEntity `json:"entity"`
	Name    string   `json:"name"`
	Kind    CfKind   `json:"kind"`
	Options []string `json:"options"`
	Seq     int64    `json:"seq"`
}

func (c CustomField) MarshalJSON() ([]byte, error) {
	type plain CustomField
	if c.Options == nil {
		c.Options = []string{}
	}
	return json.Marshal(plain(c))
}

// QueryName is the name used in the query language: lowercase, spaces → _.
func (c CustomField) QueryName() string {
	var b strings.Builder
	for _, r := range strings.ToLower(strings.TrimSpace(c.Name)) {
		if unicode.IsSpace(r) {
			b.WriteRune('_')
		} else {
			b.WriteRune(r)
		}
	}
	return b.String()
}

func (c CustomField) Key() string { return fmt.Sprintf("cf.%d", c.ID) }

type Activity struct {
	ID       int64    `json:"id"`
	TS       string   `json:"ts"`
	UserID   *int64   `json:"user_id"`
	Group    int64    `json:"group"`
	Action   Action   `json:"action"`
	Entity   string   `json:"entity"`
	EntityID int64    `json:"entity_id"`
	Field    string   `json:"field"`
	Old      any      `json:"old"`
	New      any      `json:"new"`
	Label    string   `json:"label"`
	State    ActState `json:"state"`
}

// Entity is an entity name used on the wire.
type Entity string

const (
	EntUser         Entity = "user"
	EntProgram      Entity = "program"
	EntTemplate     Entity = "template"
	EntTemplateTask Entity = "template_task"
	EntProject      Entity = "project"
	EntTask         Entity = "task"
	EntCustomField  Entity = "custom_field"
)

var AllEntities = []Entity{EntUser, EntProgram, EntTemplate, EntTemplateTask, EntProject, EntTask, EntCustomField}

func ParseEntity(s string) (Entity, bool) {
	for _, e := range AllEntities {
		if string(e) == s {
			return e, true
		}
	}
	return "", false
}

func (e *Entity) UnmarshalJSON(b []byte) error {
	var s string
	if err := json.Unmarshal(b, &s); err != nil {
		return err
	}
	v, ok := ParseEntity(s)
	if !ok {
		return fmt.Errorf("unknown entity %s", s)
	}
	*e = v
	return nil
}

// Plural is the key in snapshot / changes objects; also the table name.
func (e Entity) Plural() string {
	switch e {
	case EntUser:
		return "users"
	case EntProgram:
		return "programs"
	case EntTemplate:
		return "templates"
	case EntTemplateTask:
		return "template_tasks"
	case EntProject:
		return "projects"
	case EntTask:
		return "tasks"
	}
	return "custom_fields"
}

func (e Entity) Table() string { return e.Plural() }

func (e Entity) Label() string {
	switch e {
	case EntTemplateTask:
		return "template task"
	case EntCustomField:
		return "custom field"
	}
	return string(e)
}

// Tables is what a snapshot carries; also the shape of changes.upserts.
type Tables struct {
	Users         []User         `json:"users,omitempty"`
	Programs      []Program      `json:"programs,omitempty"`
	Templates     []Template     `json:"templates,omitempty"`
	TemplateTasks []TemplateTask `json:"template_tasks,omitempty"`
	Projects      []Project      `json:"projects,omitempty"`
	Tasks         []Task         `json:"tasks,omitempty"`
	CustomFields  []CustomField  `json:"custom_fields,omitempty"`
	Activity      []Activity     `json:"activity,omitempty"`
}

func (t *Tables) IsEmpty() bool {
	return len(t.Users)+len(t.Programs)+len(t.Templates)+len(t.TemplateTasks)+len(t.Projects)+
		len(t.Tasks)+len(t.CustomFields)+len(t.Activity) == 0
}

// Deletes holds ids per entity in changes.deletes.
type Deletes struct {
	Users         []int64 `json:"users,omitempty"`
	Programs      []int64 `json:"programs,omitempty"`
	Templates     []int64 `json:"templates,omitempty"`
	TemplateTasks []int64 `json:"template_tasks,omitempty"`
	Projects      []int64 `json:"projects,omitempty"`
	Tasks         []int64 `json:"tasks,omitempty"`
	CustomFields  []int64 `json:"custom_fields,omitempty"`
}

func (d *Deletes) Get(e Entity) *[]int64 {
	switch e {
	case EntUser:
		return &d.Users
	case EntProgram:
		return &d.Programs
	case EntTemplate:
		return &d.Templates
	case EntTemplateTask:
		return &d.TemplateTasks
	case EntProject:
		return &d.Projects
	case EntTask:
		return &d.Tasks
	}
	return &d.CustomFields
}

func (d *Deletes) IsEmpty() bool {
	for _, e := range AllEntities {
		if len(*d.Get(e)) > 0 {
			return false
		}
	}
	return true
}

// Ptr helpers for optional fields.
func Ptr[T any](v T) *T { return &v }
