// Package seed fills an empty database with the demo dataset — an exact
// port of the Rust seed.rs: the same SplitMix64 sequence and the same order
// of draws, so both implementations produce identical data (except
// password hashes and timestamps). The RNG is not security-relevant.
package seed

import (
	"errors"
	"fmt"
	"math"
	"sort"

	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/auth"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/core"
	"git.wharf.fluidgrid.org/jcz/tictactoe/go/internal/pt/store"
)

type rng uint64

func (r *rng) next() uint64 {
	*r += 0x9E3779B97F4A7C15
	z := uint64(*r)
	z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9
	z = (z ^ (z >> 27)) * 0x94D049BB133111EB
	return z ^ (z >> 31)
}

func (r *rng) below(n uint64) uint64 { return r.next() % max(n, 1) }
func (r *rng) rangeI(lo, hi int64) int64 {
	return lo + int64(r.below(uint64(hi-lo+1)))
}
func (r *rng) f() float64            { return float64(r.next()>>11) / float64(uint64(1)<<53) }
func (r *rng) chance(p float64) bool { return r.f() < p }
func pick[T any](r *rng, xs []T) T   { return xs[r.below(uint64(len(xs)))] }

type userDef struct {
	name, display string
	role          core.Role
}

var users = []userDef{
	{"admin", "Ada Admin", core.RoleAdmin},
	{"alice", "Alice Nguyen", core.RoleMember},
	{"bob", "Bob Stone", core.RoleMember},
	{"carol", "Carol Diaz", core.RoleMember},
	{"dave", "Dave Okafor", core.RoleMember},
	{"erin", "Erin Walsh", core.RoleMember},
	{"frank", "Frank Moreau", core.RoleMember},
	{"grace", "Grace Kim", core.RoleMember},
	{"heidi", "Heidi Larsen", core.RoleMember},
	{"victor", "Victor Viewer", core.RoleViewer},
}

type programDef struct {
	name, client, prefix, color string
	projects                    []string
}

var programs = []programDef{
	{"Northwind Retail", "Northwind Traders", "NWR", "#4f9dde", []string{
		"POS refresh", "Loyalty app v2", "Store Wi-Fi upgrade", "Inventory RFID pilot", "Online returns portal",
		"Holiday campaign site", "Supplier EDI onboarding"}},
	{"Contoso Health", "Contoso Medical Group", "CTH", "#e05d6f", []string{
		"Patient portal redesign", "EHR data migration", "Telehealth rollout", "HIPAA audit prep", "Lab results API",
		"Clinic scheduling"}},
	{"Fabrikam Energy", "Fabrikam Power & Light", "FAB", "#f2a541", []string{
		"Smart meter analytics", "Outage map", "Substation GIS update", "Field crew mobile app", "Rate case data pack",
		"Solar interconnect queue"}},
	{"Municipal GIS", "City of Riverton", "GIS", "#3fb68b", []string{
		"Parcel fabric migration", "Street centerline QA", "Storm drain inventory", "Zoning web map",
		"911 address points", "LiDAR canopy study", "Snow route optimizer"}},
	{"Internal IT", "Internal", "ITX", "#9b7ede", []string{
		"Laptop refresh", "SSO rollout", "Backup modernization", "Ticketing migration", "Network segmentation",
		"Endpoint EDR"}},
	{"Research & Data", "University Partners", "RND", "#2bb5c9", []string{
		"Survey data warehouse", "Grant reporting dashboard", "Research data catalog", "ML forecasting pilot",
		"Open data portal", "Sensor network ingest"}},
	{"Facilities", "Internal", "FAC", "#b08968", []string{
		"HVAC controls upgrade", "Office move: floor 3", "EV chargers install", "Badge access upgrade",
		"Space utilization study"}},
	{"Marketing", "Internal", "MKT", "#e47fc4", []string{
		"Website relaunch", "Annual conference", "Brand refresh", "Customer webinar series", "Partner summit",
		"Case study program", "Q4 product launch"}},
}

type tt struct {
	title, phase   string
	est            float64
	offset, length int64
}

type templateDef struct {
	name, desc string
	tasks      []tt
}

var templates = []templateDef{
	{"Standard Project", "General-purpose lifecycle: initiate, plan, execute, close.", []tt{
		{"Kickoff meeting", "Initiate", 4, 0, 1},
		{"Stakeholder map", "Initiate", 6, 1, 3},
		{"Project charter", "Initiate", 8, 2, 4},
		{"Requirements workshop", "Plan", 12, 6, 5},
		{"Schedule & budget", "Plan", 10, 8, 4},
		{"Risk register", "Plan", 6, 10, 3},
		{"Deliver the work", "Execute", 80, 14, 30},
		{"Status reporting", "Execute", 16, 14, 40},
		{"Quality review", "Execute", 16, 40, 7},
		{"Handover", "Close", 8, 48, 4},
		{"Lessons learned", "Close", 4, 52, 2},
		{"Closeout report", "Close", 6, 53, 3},
	}},
	{"Software Release", "Discovery to hypercare for an app or service release.", []tt{
		{"Backlog grooming", "Discover", 8, 0, 3},
		{"UX wireframes", "Design", 20, 3, 7},
		{"API design", "Design", 16, 4, 6},
		{"Sprint 1", "Build", 60, 10, 14},
		{"Sprint 2", "Build", 60, 24, 14},
		{"Sprint 3", "Build", 60, 38, 14},
		{"Integration tests", "Test", 24, 50, 7},
		{"Load test", "Test", 16, 54, 4},
		{"Security review", "Test", 12, 54, 5},
		{"User acceptance testing", "Test", 20, 58, 6},
		{"Release notes", "Release", 4, 63, 2},
		{"Go-live", "Release", 8, 65, 1},
		{"Hypercare", "Release", 16, 66, 10},
	}},
	{"Data Migration", "Move data between systems with reconciliation.", []tt{
		{"Source inventory", "Assess", 10, 0, 5},
		{"Data profiling", "Assess", 16, 4, 6},
		{"Field mapping", "Map", 24, 10, 8},
		{"Transform scripts", "Map", 40, 16, 12},
		{"Trial load", "Migrate", 16, 28, 4},
		{"Reconcile counts", "Validate", 12, 32, 4},
		{"Cutover plan", "Migrate", 8, 34, 3},
		{"Final load", "Migrate", 12, 40, 2},
		{"Business sign-off", "Validate", 4, 42, 3},
	}},
	{"GIS Mapping Project", "Collect, process and publish spatial data.", []tt{
		{"Define area of interest", "Scope", 4, 0, 2},
		{"Source imagery", "Collect", 12, 2, 6},
		{"Field survey", "Collect", 40, 6, 15},
		{"Digitize features", "Process", 60, 14, 20},
		{"Topology QA", "Process", 16, 32, 5},
		{"Attribute QA", "Process", 12, 34, 5},
		{"Symbology & labels", "Publish", 8, 38, 4},
		{"Publish web map", "Publish", 8, 42, 2},
		{"Metadata", "Publish", 6, 42, 3},
		{"User training", "Publish", 6, 45, 2},
	}},
	{"Event", "Plan, promote and run an event.", []tt{
		{"Venue booking", "Plan", 6, 0, 5},
		{"Event budget", "Plan", 6, 2, 4},
		{"Speaker outreach", "Plan", 16, 5, 15},
		{"Agenda", "Plan", 8, 15, 6},
		{"Registration site", "Promote", 12, 12, 6},
		{"Email campaign", "Promote", 10, 20, 15},
		{"Social posts", "Promote", 8, 22, 20},
		{"AV check", "Run", 4, 40, 1},
		{"Run of show", "Run", 6, 38, 3},
		{"Event day", "Run", 24, 42, 1},
		{"Attendee survey", "Wrap", 4, 43, 5},
		{"Wrap-up report", "Wrap", 6, 46, 4},
	}},
}

var adHoc = []string{
	"Vendor follow-up", "Fix review comments", "Update documentation", "Demo to sponsor", "Data cleanup",
	"Performance tuning", "Accessibility pass", "Budget re-forecast", "Procure licenses", "Training session",
	"Bug triage", "Steering committee prep", "Risk mitigation plan", "Test environment setup", "Change request",
	"Security exception review", "Update project plan", "Customer interview", "Draft FAQ", "Escalation call",
}

var tags = []string{
	"q4", "compliance", "customer-facing", "pilot", "infra", "mobile", "data", "gis", "security", "high-visibility",
	"vendor", "quick-win",
}

func contains(s, sub string) bool { return len(sub) <= len(s) && (indexOf(s, sub) >= 0) }

func indexOf(s, sub string) int {
	for i := 0; i+len(sub) <= len(s); i++ {
		if s[i:i+len(sub)] == sub {
			return i
		}
	}
	return -1
}

func lower(s string) string {
	b := []byte(s)
	for i, c := range b {
		if c >= 'A' && c <= 'Z' {
			b[i] = c + 32
		}
	}
	return string(b)
}

func templateFor(name, program string) int {
	n := lower(name)
	has := func(xs ...string) bool {
		for _, x := range xs {
			if contains(n, x) {
				return true
			}
		}
		return false
	}
	switch {
	case program == "Municipal GIS" || has("gis", "map", "lidar"):
		return 3
	case has("migration", "warehouse", "ingest", "catalog"):
		return 2
	case has("conference", "summit", "webinar", "launch"):
		return 4
	case has("app", "portal", "api", "site", "sso", "dashboard", "relaunch"):
		return 1
	}
	return 0
}

type Report struct {
	Users, Projects, Tasks int
}

type planItem struct {
	title, phase   string
	est            float64
	offset, length int64
}

type taskRef struct {
	ref string
	id  int64
}

// Seed fills an empty database; password is set for every seeded user.
func Seed(s *store.Store, password string) (Report, error) {
	empty, err := s.IsEmpty()
	if err != nil {
		return Report{}, err
	}
	if !empty {
		return Report{}, errors.New("the database already has data (use --force to wipe it first)")
	}
	today := store.Today()
	now := store.NowTS()
	r := rng(42)
	c, err := s.DB().Begin()
	if err != nil {
		return Report{}, err
	}
	defer c.Rollback()

	var userIDs []int64
	for _, u := range users {
		hash, err := auth.HashPassword(password)
		if err != nil {
			return Report{}, err
		}
		id, err := store.InsertUser(c, &core.User{Username: u.name, DisplayName: u.display, Role: u.role, Active: true}, hash, now)
		if err != nil {
			return Report{}, err
		}
		userIDs = append(userIDs, id)
	}
	members := userIDs[1:9]

	cf := func(e core.CfEntity, name string, kind core.CfKind, opts []string, seq int64) (int64, error) {
		if opts == nil {
			opts = []string{}
		}
		return store.InsertCustomField(c, &core.CustomField{Entity: e, Name: name, Kind: kind, Options: opts, Seq: seq})
	}
	cfContract, err := cf(core.CfProject, "Contract #", core.CfText, nil, 1)
	if err != nil {
		return Report{}, err
	}
	cfRisk, _ := cf(core.CfProject, "Risk score", core.CfNumber, nil, 2)
	cfRegion, _ := cf(core.CfProject, "Region", core.CfEnum, []string{"North", "South", "East", "West"}, 3)
	cfBillable, _ := cf(core.CfTask, "Billable", core.CfEnum, []string{"Yes", "No"}, 4)
	cfTicket, err := cf(core.CfTask, "Ticket", core.CfText, nil, 5)
	if err != nil {
		return Report{}, err
	}

	type tpl struct {
		id    int64
		tasks []core.TemplateTask
	}
	var tpls []tpl
	for _, td := range templates {
		tid, err := store.InsertTemplate(c, &core.Template{Name: td.name, Description: td.desc})
		if err != nil {
			return Report{}, err
		}
		var tts []core.TemplateTask
		for i, x := range td.tasks {
			t := core.TemplateTask{TemplateID: tid, Seq: int64(i + 1), Title: x.title, Phase: x.phase, EstHours: core.Ptr(x.est),
				OffsetDays: x.offset, DurationDays: x.length}
			if t.ID, err = store.InsertTemplateTask(c, &t); err != nil {
				return Report{}, err
			}
			tts = append(tts, t)
		}
		tpls = append(tpls, tpl{tid, tts})
	}

	nProjects, nTasks := 0, 0
	var refs []taskRef
	nowSecs := store.NowSecs()
	for pi, pd := range programs {
		programID, err := store.InsertProgram(c, &core.Program{Name: pd.name, Client: pd.client, Prefix: pd.prefix, Color: pd.color})
		if err != nil {
			return Report{}, err
		}
		for k, name := range pd.projects {
			tp := tpls[templateFor(name, pd.name)]
			var span int64 = 30
			if len(tp.tasks) > 0 {
				span = 0
				for _, t := range tp.tasks {
					span = max(span, t.OffsetDays+t.DurationDays)
				}
			}
			// where in its life the project is: <0 not started, >1 finished
			phaseRoll := r.f()
			var progress float64
			switch {
			case phaseRoll < 0.12:
				progress = -r.f() * 0.5
			case phaseRoll < 0.25:
				progress = 1 + r.f()*0.6
			default:
				progress = r.f() * 0.95
			}
			start := today.AddDays(-int(math.Round(progress * float64(span))))
			stretch := 1 + r.f()*0.25
			due := start.AddDays(int(float64(span) * stretch))
			var status core.ProjectStatus
			switch {
			case progress < 0:
				status = core.ProjectPlanning
			case progress > 1:
				if r.chance(0.15) {
					status = core.ProjectCancelled
				} else {
					status = core.ProjectDone
				}
			case r.chance(0.08):
				status = core.ProjectOnHold
			default:
				status = core.ProjectActive
			}
			owner := pick(&r, members)
			estTotal := 0.0
			for _, t := range tp.tasks {
				estTotal += *t.EstHours
			}
			ptags := []string{}
			for n := r.rangeI(0, 3); n > 0; n-- {
				t := pick(&r, tags)
				dup := false
				for _, x := range ptags {
					if x == t {
						dup = true
					}
				}
				if !dup {
					ptags = append(ptags, t)
				}
			}
			sort.Strings(ptags)
			cfv := core.CfValues{}
			cfv[fmt.Sprint(cfContract)] = fmt.Sprintf("PO-%d", 4000+r.below(6000))
			cfv[fmt.Sprint(cfRisk)] = r.rangeI(1, 10)
			cfv[fmt.Sprint(cfRegion)] = pick(&r, []string{"North", "South", "East", "West"})
			priority := pick(&r, []core.Priority{core.PriorityLow, core.PriorityMedium, core.PriorityMedium, core.PriorityHigh, core.PriorityCritical})
			code := fmt.Sprintf("%s-%03d", pd.prefix, k+1)
			budget := math.Round(estTotal*(1+r.f()*0.35)/5) * 5
			notes := ""
			if r.chance(0.4) {
				notes = fmt.Sprintf("Sponsor: %s.\nKey risk: %s.", pd.client,
					pick(&r, []string{"vendor delays", "scope creep", "data quality", "staff availability", "budget approval"}))
			}
			p := core.Project{Code: code, Name: name, ProgramID: &programID, TemplateID: &tp.id, OwnerID: &owner,
				Status: status, Priority: priority, Health: core.HealthGreen, StartDate: core.Ptr(start.ISO()),
				DueDate: core.Ptr(due.ISO()), BudgetHours: &budget, Tags: ptags, Notes: notes, CF: cfv,
				CreatedAt: core.RFC3339(nowSecs - 86400*int64(60+pi)), UpdatedAt: now, UpdatedBy: &owner}
			pid, err := store.InsertProject(c, &p)
			if err != nil {
				return Report{}, err
			}
			nProjects++

			type created struct {
				id    int64
				phase string
			}
			var tasks []created
			type phaseLast struct {
				phase string
				id    int64
			}
			var last []phaseLast
			var seq int64
			totalActual := 0.0
			totalOverdue := 0
			extra := r.rangeI(0, 14)
			var plan []planItem
			for _, t := range tp.tasks {
				plan = append(plan, planItem{t.Title, t.Phase, *t.EstHours, t.OffsetDays, t.DurationDays})
			}
			for i := int64(0); i < extra; i++ {
				t := pick(&r, tp.tasks)
				title := pick(&r, adHoc)
				if title == "Change request" {
					title = fmt.Sprintf("Change request #%d", r.rangeI(1, 40))
				}
				est := []float64{2, 4, 6, 8, 12, 16}[r.below(6)]
				off := t.OffsetDays + r.rangeI(0, 5)
				dur := r.rangeI(1, 6)
				plan = append(plan, planItem{title, t.Phase, est, off, dur})
			}
			sort.SliceStable(plan, func(a, b int) bool { return plan[a].offset < plan[b].offset })
			for _, it := range plan {
				seq++
				s := start.AddDays(int(it.offset))
				d := s.AddDays(int(max(it.length, 1)) - 1)
				finished := status == core.ProjectDone
				var st core.TaskStatus
				switch {
				case finished || (d < today && r.chance(0.85)):
					st = core.TaskDone
				case s <= today:
					st = pick(&r, []core.TaskStatus{core.TaskInProgress, core.TaskInProgress, core.TaskReview, core.TaskBlocked, core.TaskTodo})
				default:
					st = core.TaskTodo
				}
				var actual float64
				switch st {
				case core.TaskDone:
					actual = math.Round(it.est*(0.6+r.f()*0.9)*4) / 4
				case core.TaskTodo:
					actual = 0
				default:
					actual = math.Round(it.est*r.f()*0.9*4) / 4
				}
				totalActual += actual
				if st != core.TaskDone && d < today {
					totalOverdue++
				}
				tcf := core.CfValues{}
				if r.chance(0.7) {
					v := "No"
					if r.chance(0.75) {
						v = "Yes"
					}
					tcf[fmt.Sprint(cfBillable)] = v
				}
				if r.chance(0.2) {
					tcf[fmt.Sprint(cfTicket)] = fmt.Sprintf("%s-%d", pd.prefix, r.rangeI(100, 999))
				}
				tprio := pick(&r, []core.Priority{core.PriorityLow, core.PriorityMedium, core.PriorityMedium, core.PriorityMedium, core.PriorityHigh, core.PriorityCritical})
				var assignee *int64
				switch {
				case r.chance(0.08):
				case r.chance(0.35):
					assignee = core.Ptr(owner)
				default:
					assignee = core.Ptr(pick(&r, members))
				}
				var est *float64
				if !r.chance(0.05) {
					est = core.Ptr(it.est)
				}
				ttags := []string{}
				if r.chance(0.2) {
					ttags = []string{pick(&r, tags)}
				}
				tnotes := ""
				if r.chance(0.15) {
					tnotes = "Waiting on confirmation from the client."
				}
				t := core.Task{ProjectID: pid, Seq: seq, Title: it.title, Phase: it.phase, Status: st, Priority: tprio,
					AssigneeID: assignee, StartDate: core.Ptr(s.ISO()), DueDate: core.Ptr(d.ISO()), EstHours: est,
					ActualHours: actual, Tags: ttags, Notes: tnotes, CF: tcf, CreatedAt: p.CreatedAt, UpdatedAt: now, UpdatedBy: &owner}
				id, err := store.InsertTask(c, &t)
				if err != nil {
					return Report{}, err
				}
				nTasks++
				found := false
				for i := range last {
					if last[i].phase == it.phase {
						last[i].id = id
						found = true
					}
				}
				if !found {
					if len(last) > 0 {
						if err := store.InsertDep(c, id, last[len(last)-1].id); err != nil {
							return Report{}, err
						}
					}
					last = append(last, phaseLast{it.phase, id})
				}
				tasks = append(tasks, created{id, it.phase})
				refs = append(refs, taskRef{fmt.Sprintf("%s #%d", code, seq), id})
			}
			// a few extra deps inside the project
			for n := r.rangeI(0, 3); n > 0; n-- {
				a := r.below(uint64(len(tasks)))
				b := r.below(uint64(len(tasks)))
				if a > b {
					if err := store.InsertDep(c, tasks[a].id, tasks[b].id); err != nil {
						return Report{}, err
					}
				}
			}
			// health from the numbers, with some noise
			burn := totalActual / budget
			health := core.HealthGreen
			switch {
			case totalOverdue > 3 || burn > 1.05:
				health = core.HealthRed
			case totalOverdue > 0 || burn > 0.9 || r.chance(0.1):
				health = core.HealthAmber
			}
			if _, err := c.Exec("UPDATE projects SET health = ?1 WHERE id = ?2", string(health), pid); err != nil {
				return Report{}, err
			}
		}
	}

	// 30 days of synthetic history (not undoable)
	states := []string{"To do → In progress", "In progress → Review", "Review → Done", "In progress → Blocked", "Blocked → In progress"}
	var grp int64
	base := nowSecs - 30*86400
	for day := int64(0); day < 30; day++ {
		weekday := core.Date((base + day*86400) / 86400).Weekday()
		var n int64
		if weekday >= 5 {
			n = r.rangeI(0, 4)
		} else {
			n = r.rangeI(8, 30)
		}
		ts := make([]int64, 0, n)
		for i := int64(0); i < n; i++ {
			ts = append(ts, base+day*86400+8*3600+r.rangeI(0, 10*3600))
		}
		sort.Slice(ts, func(a, b int) bool { return ts[a] < ts[b] })
		for _, t := range ts {
			grp++
			ref := refs[r.below(uint64(len(refs)))]
			who := pick(&r, members)
			var field, label string
			if r.chance(0.6) {
				field, label = "status", fmt.Sprintf("%s status: %s", ref.ref, pick(&r, states))
			} else {
				a := r.rangeI(0, 20)
				b := r.rangeI(20, 40)
				field, label = "actual_hours", fmt.Sprintf("%s actual h: %d → %d", ref.ref, a, b)
			}
			if _, err := c.Exec(`INSERT INTO activity (ts, user_id, grp, action, entity, entity_id, field, old, new, label, state)
				VALUES (?1, ?2, ?3, 'set', 'task', ?4, ?5, NULL, NULL, ?6, 'dead')`, core.RFC3339(t), who, grp, ref.id, field, label); err != nil {
				return Report{}, err
			}
		}
	}
	if err := c.Commit(); err != nil {
		return Report{}, err
	}
	return Report{Users: len(users), Projects: nProjects, Tasks: nTasks}, nil
}

// Wipe deletes all data (for `pt seed --force`).
func Wipe(s *store.Store) error {
	_, err := s.DB().Exec(`PRAGMA foreign_keys = OFF;
		DELETE FROM activity; DELETE FROM custom_values; DELETE FROM custom_fields; DELETE FROM task_tags;
		DELETE FROM project_tags; DELETE FROM task_deps; DELETE FROM tasks; DELETE FROM projects;
		DELETE FROM template_tasks; DELETE FROM templates; DELETE FROM programs; DELETE FROM sessions;
		DELETE FROM users; DELETE FROM sqlite_sequence;
		PRAGMA foreign_keys = ON;`)
	return err
}
