//! The frame around the views (ui-spec §1, §2, §6): login page, menu bar,
//! tabs, query bar with live validation and autocomplete, status line,
//! nano-style hint bar, toasts, and the global keyboard handler.

use leptos::prelude::*;
use leptos::task::spawn_local;
use pt_core::model::*;
use pt_core::query::{self, Query, Target};
use wasm_bindgen::JsCast;

use crate::app::{Act, App, ConnState, Ctx, Level, Modal, THEMES, ViewId, run, use_app};
use crate::cells::avatar;
use crate::{net, util};

// ---- login -------------------------------------------------------------------------

#[component]
pub fn Login() -> impl IntoView {
    let app = use_app();
    let user = RwSignal::new(util::load("pt.lastuser").unwrap_or_default());
    let pass = RwSignal::new(String::new());
    let err = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let server = RwSignal::new(String::new());
    spawn_local(async move {
        let req = util::window().fetch_with_str("/api/health");
        if let Ok(r) = wasm_bindgen_futures::JsFuture::from(req).await {
            if let Ok(r) = r.dyn_into::<web_sys::Response>() {
                if let Ok(p) = r.json() {
                    if let Ok(v) = wasm_bindgen_futures::JsFuture::from(p).await {
                        let get = |k: &str| {
                            js_sys::Reflect::get(&v, &k.into())
                                .ok()
                                .and_then(|x| x.as_string())
                                .unwrap_or_default()
                        };
                        let _ =
                            server.try_set(format!("{} server {}", get("impl"), get("version")));
                    }
                }
            }
        }
    });
    let submit = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        err.set(None);
        let (u, p) = (user.get_untracked(), pass.get_untracked());
        spawn_local(async move {
            match net::login(u.trim(), &p).await {
                Ok(me) => {
                    util::save("pt.lastuser", u.trim());
                    let _ = pass.try_set(String::new());
                    let _ = busy.try_set(false);
                    app.start_session(me);
                    util::focus_later("g-projects");
                }
                Err(e) => {
                    // the form may be gone if the user navigated meanwhile
                    let _ = err.try_set(Some(e));
                    let _ = busy.try_set(false);
                }
            }
        });
    };
    let first = user.get_untracked().is_empty();
    view! {
        <div class="login-wrap">
            <form class="login" on:submit=submit>
                <div class="brand">"pt"<span>" projects & tasks"</span></div>
                <label>"Username"
                    <input id="login-user" autocomplete="username" autofocus=first prop:value=move || user.get()
                        on:input=move |e| user.set(event_target_value(&e)) />
                </label>
                <label>"Password"
                    <input id="login-pass" type="password" autocomplete="current-password" autofocus=!first prop:value=move || pass.get()
                        on:input=move |e| pass.set(event_target_value(&e)) />
                </label>
                <Show when=move || err.get().is_some()>
                    <div class="lerr">"✗ "{move || err.get()}</div>
                </Show>
                <button class="btn primary" type="submit" disabled=move || busy.get()>
                    {move || if busy.get() { "Signing in…" } else { "Sign in" }}
                </button>
                <div class="lfoot">{move || server.get()}" · "{move || app.status.get().1}</div>
            </form>
        </div>
    }
}

// ---- menus ------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum MenuItem {
    Do(Act),
    Saved(usize),
    Sep,
}

pub const MENU_TITLES: [(&str, char); 5] = [
    ("File", 'f'),
    ("Edit", 'e'),
    ("View", 'v'),
    ("Data", 'd'),
    ("Help", 'h'),
];

pub fn menu_items(app: App, m: usize) -> Vec<MenuItem> {
    use MenuItem::*;
    match m {
        0 => vec![
            Do(Act::NewProject),
            Do(Act::NewTask),
            Sep,
            Do(Act::SetPassword),
            Do(Act::Logout),
        ],
        1 => vec![
            Do(Act::Undo),
            Do(Act::Redo),
            Sep,
            Do(Act::Copy),
            Do(Act::Paste),
            Do(Act::FillDown),
            Do(Act::ClearCell),
            Sep,
            Do(Act::Edit),
            Do(Act::EditRecord),
            Do(Act::New),
            Do(Act::Delete),
            Sep,
            Do(Act::MarkRow),
            Do(Act::MarkAll),
        ],
        2 => {
            let mut v: Vec<MenuItem> = ViewId::ALL
                .iter()
                .map(|v| Do(crate::app::Act::View(*v)))
                .collect();
            v.extend([
                Sep,
                Do(Act::NextPane),
                Do(Act::ToggleDetail),
                Do(Act::Columns),
                Sep,
            ]);
            v.extend((0..THEMES.len()).map(|i| Do(crate::app::Act::ThemeSet(i))));
            v.extend([Do(Act::Density), Sep, Do(Act::Palette)]);
            v
        }
        3 => {
            let mut v = vec![
                Do(Act::Filter),
                Do(Act::ClearFilter),
                Sep,
                Do(Act::SortCol),
                Do(Act::SortDialog),
                Do(Act::GroupCol),
                Do(Act::ClearGroup),
                Sep,
                Do(Act::SaveFilter),
            ];
            let n = app.saved.with_untracked(|s| s.len());
            v.extend((0..n).map(Saved));
            v
        }
        _ => vec![Do(Act::Help), Do(Act::Palette), Do(Act::About)],
    }
}

pub fn apply_saved(app: App, i: usize) {
    let Some(f) = app.saved.with_untracked(|s| s.get(i).cloned()) else {
        return;
    };
    let view = ViewId::ALL
        .iter()
        .copied()
        .find(|v| v.key() == f.view)
        .unwrap_or(ViewId::Tasks);
    app.set_view(view);
    if let Some((q, _)) = app.q.of(view) {
        q.set(f.query.clone());
    }
    if let Some(g) = app.grids.of_view(view).first() {
        g.sort.set(
            f.sort
                .iter()
                .map(|(k, d)| pt_core::grid::SortSpec {
                    key: k.clone(),
                    desc: *d,
                })
                .collect(),
        );
        g.group.set(f.group.clone());
    }
    app.say(Level::Ok, format!("Applied “{}”", f.name));
}

fn run_item(app: App, it: MenuItem) {
    match it {
        MenuItem::Do(a) => run(app, a),
        MenuItem::Saved(i) => {
            app.menu.set(None);
            apply_saved(app, i);
        }
        MenuItem::Sep => {}
    }
}

#[component]
pub fn MenuBar() -> impl IntoView {
    let app = use_app();
    let conn_dot = move || match app.conn.get() {
        ConnState::Online => ("dot-on", "connected"),
        ConnState::Connecting => ("dot-wait", "connecting"),
        ConnState::Offline => ("dot-off", "offline — reconnecting"),
    };
    view! {
        <div class="menubar">
            <span class="logo">"pt"</span>
            {MENU_TITLES.iter().enumerate().map(|(i, (t, _))| {
                let t = *t;
                let first = t[..1].to_string();
                let rest = t[1..].to_string();
                view! {
                    <div class="menu" class:open=move || app.menu.get() == Some(i)
                        on:mousedown=move |e| { e.prevent_default(); if app.menu.get_untracked() == Some(i) { app.menu.set(None) } else { app.menu.set(Some(i)); app.menu_sel.set(0); } }
                        on:mouseenter=move |_| { if app.menu.get_untracked().is_some() { app.menu.set(Some(i)); app.menu_sel.set(0); } }>
                        <u>{first}</u>{rest}
                        <Show when=move || app.menu.get() == Some(i)>
                            <MenuDrop m=i />
                        </Show>
                    </div>
                }
            }).collect_view()}
            <span class="spacer"></span>
            <span class="who">
                {move || app.user.get().map(|u| view! { {avatar(u.id, &u.display_name)}" "{u.display_name.clone()}<span class="muted">" ("{u.role.label()}")"</span> })}
            </span>
            <span class="conn" title=move || conn_dot().1>
                <i class=move || format!("cdot {}", conn_dot().0)></i>
                {move || app.rep.with(|r| r.server.as_ref().map(|s| format!(" {} {}", s.implementation, s.version)).unwrap_or_default())}
            </span>
        </div>
    }
}

#[component]
fn MenuDrop(m: usize) -> impl IntoView {
    let app = use_app();
    let items = menu_items(app, m);
    view! {
        <div class="drop" on:mousedown=|e| e.stop_propagation()>
            {items.into_iter().enumerate().map(|(i, it)| match it {
                MenuItem::Sep => view! { <div class="sep"></div> }.into_any(),
                MenuItem::Do(a) => {
                    let checked = move || match a {
                        Act::ThemeSet(t) => app.theme.get() == t,
                        Act::View(v) => app.view.get() == v,
                        Act::ToggleDetail => app.detail.get(),
                        Act::Density => app.compact.get(),
                        _ => false,
                    };
                    view! {
                        <div class="item" class:sel=move || app.menu_sel.get() == i
                            on:mouseenter=move |_| app.menu_sel.set(i)
                            on:mousedown=move |e| { e.prevent_default(); run_item(app, it); }>
                            <span class="chk">{move || if checked() { "✓" } else { "" }}</span>
                            <span class="il">{a.label()}</span>
                            <span class="ik">{a.key()}</span>
                        </div>
                    }.into_any()
                }
                MenuItem::Saved(s) => {
                    let name = app.saved.with_untracked(|v| v.get(s).map(|f| format!("★ {}", f.name)).unwrap_or_default());
                    view! {
                        <div class="item" class:sel=move || app.menu_sel.get() == i on:mouseenter=move |_| app.menu_sel.set(i)
                            on:mousedown=move |e| { e.prevent_default(); run_item(app, it); }>
                            <span class="chk"></span><span class="il">{name}</span><span class="ik"></span>
                        </div>
                    }.into_any()
                }
            }).collect_view()}
        </div>
    }
}

fn menu_key(app: App, e: &web_sys::KeyboardEvent) -> bool {
    let Some(m) = app.menu.get_untracked() else {
        return false;
    };
    let items = menu_items(app, m);
    let n = items.len();
    let step = |dir: i64| {
        let mut s = app.menu_sel.get_untracked() as i64;
        for _ in 0..n {
            s = (s + dir).rem_euclid(n as i64);
            if items[s as usize] != MenuItem::Sep {
                break;
            }
        }
        app.menu_sel.set(s as usize);
    };
    match e.key().as_str() {
        "ArrowDown" => step(1),
        "ArrowUp" => step(-1),
        "ArrowLeft" => {
            app.menu
                .set(Some((m + MENU_TITLES.len() - 1) % MENU_TITLES.len()));
            app.menu_sel.set(0);
        }
        "ArrowRight" => {
            app.menu.set(Some((m + 1) % MENU_TITLES.len()));
            app.menu_sel.set(0);
        }
        "Enter" | " " => {
            if let Some(it) = items.get(app.menu_sel.get_untracked()) {
                run_item(app, *it);
            }
        }
        "Escape" | "F10" => app.menu.set(None),
        _ => {
            // first letter of an item jumps to it
            let k = e.key().to_lowercase();
            if k.chars().count() == 1 {
                if let Some(i) = items.iter().position(
                    |it| matches!(it, MenuItem::Do(a) if a.label().to_lowercase().starts_with(&k)),
                ) {
                    app.menu_sel.set(i);
                }
            }
        }
    }
    true
}

// ---- tabs ----------------------------------------------------------------------------------

#[component]
pub fn Tabs() -> impl IntoView {
    let app = use_app();
    view! {
        <div class="tabs">
            {ViewId::ALL.iter().map(|v| {
                let v = *v;
                let n = v.index() + 1;
                let badge = move || -> Option<String> {
                    app.rep.with(|r| match v {
                        ViewId::Tasks => Some(r.tasks.values().filter(|t| r.task_overdue(t)).count()).filter(|n| *n > 0).map(|n| format!("{n} late")),
                        ViewId::Users => Some(app.peers.with(|p| { let mut u: Vec<i64> = p.iter().map(|p| p.user_id).collect(); u.sort(); u.dedup(); u.len() })).map(|n| format!("{n} on")),
                        _ => None,
                    })
                };
                view! {
                    <div class="tab" class:active=move || app.view.get() == v on:click=move |_| app.set_view(v) title=format!("Alt+{n}")>
                        <span class="tn">{n}</span>" "{v.label()}
                        {move || badge().map(|b| view! { <span class="tb">{b}</span> })}
                    </div>
                }
            }).collect_view()}
        </div>
    }
}

// ---- query bar -------------------------------------------------------------------------------

#[derive(Clone, PartialEq)]
struct Suggestion {
    label: String,
    replace: String,
    detail: String,
}

/// The token around the caret: (start char index, text up to caret).
fn token_at(text: &str, caret: usize) -> (usize, String) {
    let chars: Vec<char> = text.chars().collect();
    let caret = caret.min(chars.len());
    let mut start = caret;
    while start > 0 && !chars[start - 1].is_whitespace() {
        start -= 1;
    }
    (start, chars[start..caret].iter().collect())
}

fn suggestions(app: App, target: Target, token: &str) -> Vec<Suggestion> {
    let tok = token.trim_start_matches('-');
    let neg = if token.starts_with('-') { "-" } else { "" };
    let mut out: Vec<Suggestion> = Vec::new();
    app.rep.with_untracked(|r| {
        let (field, val) = if let Some(rest) = tok.strip_prefix('@') {
            ("@".to_string(), rest.to_string())
        } else if let Some(rest) = tok.strip_prefix('#') {
            ("#".to_string(), rest.to_string())
        } else if let Some(p) = tok.find([':', '>', '<', '=']) {
            (
                tok[..p].to_lowercase(),
                tok[p..]
                    .trim_start_matches([':', '>', '<', '='])
                    .to_string(),
            )
        } else {
            // field names
            let lower = tok.to_lowercase();
            let mut names: Vec<String> = query::field_names(target)
                .iter()
                .map(|s| s.to_string())
                .collect();
            if target != Target::Activity {
                let ent = if target == Target::Projects {
                    CfEntity::Project
                } else {
                    CfEntity::Task
                };
                names.extend(r.custom_fields_for(ent).iter().map(|c| c.query_name()));
            }
            for n in names
                .into_iter()
                .filter(|n| n.starts_with(&lower) && !lower.is_empty())
            {
                out.push(Suggestion {
                    label: format!("{n}:"),
                    replace: format!("{neg}{n}:"),
                    detail: "field".into(),
                });
            }
            if !lower.is_empty() {
                for (k, d) in [
                    ("@me", "assigned to me"),
                    ("is:overdue", "late"),
                    ("is:open", "not done"),
                ] {
                    if k.starts_with(&lower) {
                        out.push(Suggestion {
                            label: k.into(),
                            replace: format!("{neg}{k}"),
                            detail: d.into(),
                        });
                    }
                }
            }
            return;
        };
        // value part: keep everything up to the last comma
        let prefix_len = tok.len() - val.len();
        let head = &tok[..prefix_len];
        let (done, cur) = match val.rfind(',') {
            Some(p) => (&val[..=p], &val[p + 1..]),
            None => ("", val.as_str()),
        };
        let cur_l = cur.to_lowercase();
        let users = || -> Vec<(String, String)> {
            let mut v = vec![("me".to_string(), "me".to_string())];
            v.extend(
                r.users
                    .values()
                    .map(|u| (u.username.clone(), u.display_name.clone())),
            );
            v
        };
        let vals: Vec<(String, String)> = match field.as_str() {
            "@" | "owner" | "assignee" | "who" | "user" | "by" => users(),
            "#" | "tag" | "tags" => r
                .tag_counts(if target == Target::Projects {
                    Entity::Project
                } else {
                    Entity::Task
                })
                .into_iter()
                .map(|(t, n)| (t, format!("{n}×")))
                .collect(),
            "status" if target == Target::Projects => ProjectStatus::ALL
                .iter()
                .map(|s| (s.as_str().to_string(), s.label().to_string()))
                .collect(),
            "status" => TaskStatus::ALL
                .iter()
                .map(|s| (s.as_str().to_string(), s.label().to_string()))
                .collect(),
            "priority" => Priority::ALL
                .iter()
                .map(|s| (s.as_str().to_string(), s.label().to_string()))
                .collect(),
            "health" => Health::ALL
                .iter()
                .map(|s| (s.as_str().to_string(), s.label().to_string()))
                .collect(),
            "program" => r
                .programs
                .values()
                .map(|p| (p.prefix.to_lowercase(), p.name.clone()))
                .collect(),
            "project" => r
                .projects
                .values()
                .map(|p| (p.code.to_lowercase(), p.name.clone()))
                .collect(),
            "is" => query::is_values(target)
                .iter()
                .map(|s| (s.to_string(), String::new()))
                .collect(),
            "has" => query::has_values(target)
                .iter()
                .map(|s| (s.to_string(), String::new()))
                .collect(),
            "due" | "start" => [
                "today", "tomorrow", "eow", "eom", "+7d", "-7d", "+30d", "none",
            ]
            .iter()
            .map(|s| (s.to_string(), String::new()))
            .collect(),
            "action" => ["set", "create", "delete"]
                .iter()
                .map(|s| (s.to_string(), String::new()))
                .collect(),
            "entity" => [
                "project",
                "task",
                "user",
                "program",
                "template",
                "custom_field",
            ]
            .iter()
            .map(|s| (s.to_string(), String::new()))
            .collect(),
            name => {
                let ent = if target == Target::Projects {
                    CfEntity::Project
                } else {
                    CfEntity::Task
                };
                r.custom_fields_for(ent)
                    .into_iter()
                    .find(|c| c.query_name() == name && c.kind == CfKind::Enum)
                    .map(|c| {
                        c.options
                            .iter()
                            .map(|o| (o.to_lowercase(), String::new()))
                            .collect()
                    })
                    .unwrap_or_default()
            }
        };
        for (v, d) in vals {
            if v.to_lowercase().starts_with(&cur_l)
                || (!cur_l.is_empty() && d.to_lowercase().contains(&cur_l))
            {
                out.push(Suggestion {
                    label: v.clone(),
                    replace: format!("{neg}{head}{done}{v}"),
                    detail: d,
                });
            }
        }
    });
    out.truncate(10);
    out
}

#[component]
pub fn QueryBar() -> impl IntoView {
    let app = use_app();
    let caret = RwSignal::new(0usize);
    let open = RwSignal::new(false);
    let sel = RwSignal::new(0usize);
    let input = NodeRef::<leptos::html::Input>::new();
    let current = move || app.q.of(app.view.get());
    let text = move || current().map(|(q, _)| q.get()).unwrap_or_default();
    let compiled = Memo::new(move |_| {
        let Some((q, t)) = current() else {
            return (vec![], 0usize, 0usize);
        };
        let s = q.get();
        app.rep.with(|r| {
            let c = Query::compile(&s, t, r);
            let errs: Vec<(usize, usize, String)> = c
                .errors
                .iter()
                .map(|e| (e.start, e.end, e.message.clone()))
                .collect();
            let (shown, total) = match app.view.get() {
                ViewId::Projects => (app.grids.projects.ids.with(|i| i.len()), r.projects.len()),
                ViewId::Tasks => (app.grids.tasks.ids.with(|i| i.len()), r.tasks.len()),
                ViewId::Activity => (app.grids.activity.ids.with(|i| i.len()), r.activity.len()),
                ViewId::Board => (
                    r.tasks.values().filter(|x| c.matches_task(x, r)).count(),
                    r.tasks.len(),
                ),
                _ => (
                    r.projects
                        .values()
                        .filter(|p| c.matches_project(p, r))
                        .count(),
                    r.projects.len(),
                ),
            };
            (errs, shown, total)
        })
    });
    let sugg = Memo::new(move |_| {
        if !open.get() {
            return vec![];
        }
        let Some((_, t)) = current() else {
            return vec![];
        };
        let (_, tok) = token_at(&text(), caret.get());
        if tok.is_empty() {
            return vec![];
        }
        suggestions(app, t, &tok)
    });
    let update_caret = move || {
        if let Some(i) = input.get_untracked() {
            caret.set(i.selection_start().ok().flatten().unwrap_or(0) as usize);
        }
    };
    let accept = move |i: usize| {
        let Some(s) = sugg.with_untracked(|s| s.get(i).cloned()) else {
            return;
        };
        let Some((q, _)) = current() else { return };
        let t = q.get_untracked();
        let (start, _) = token_at(&t, caret.get_untracked());
        let chars: Vec<char> = t.chars().collect();
        let c = caret.get_untracked().min(chars.len());
        let before: String = chars[..start].iter().collect();
        let after: String = chars[c..].iter().collect();
        let add_space = !s.replace.ends_with(':') && !after.starts_with(' ');
        let new = format!(
            "{before}{}{}{after}",
            s.replace,
            if add_space { " " } else { "" }
        );
        let pos = before.chars().count() + s.replace.chars().count() + usize::from(add_space);
        q.set(new);
        caret.set(pos);
        sel.set(0);
        request_animation_frame(move || {
            if let Some(i) = input.get_untracked() {
                let _ = i.set_selection_range(pos as u32, pos as u32);
            }
        });
    };
    let on_key = move |e: web_sys::KeyboardEvent| {
        let n = sugg.with_untracked(|s| s.len());
        let k = e.key();
        match k.as_str() {
            "ArrowDown" if n > 0 => sel.update(|s| *s = (*s + 1) % n),
            "ArrowUp" if n > 0 => sel.update(|s| *s = (*s + n - 1) % n),
            "Tab" | "Enter" if n > 0 && open.get_untracked() => accept(sel.get_untracked()),
            "Enter" | "ArrowDown" => {
                open.set(false);
                focus_main(app);
            }
            "Escape" => {
                if open.get_untracked() && n > 0 {
                    open.set(false);
                } else {
                    focus_main(app);
                }
            }
            _ => return,
        }
        e.prevent_default();
        e.stop_propagation();
    };
    let backdrop = move || {
        let t = text();
        let (errs, _, _) = compiled.get();
        let chars: Vec<char> = t.chars().collect();
        let mut out = Vec::new();
        let mut i = 0;
        let mut sorted = errs.clone();
        sorted.sort_by_key(|e| e.0);
        for (s, e, m) in sorted {
            if s > i {
                out.push(view! { <span>{chars[i..s.min(chars.len())].iter().collect::<String>()}</span> }.into_any());
            }
            out.push(view! { <span class="qerr" title=m>{chars[s.min(chars.len())..e.min(chars.len())].iter().collect::<String>()}</span> }.into_any());
            i = e;
        }
        if i < chars.len() {
            out.push(view! { <span>{chars[i..].iter().collect::<String>()}</span> }.into_any());
        }
        out.collect_view()
    };
    let indicators = move || {
        let v = app.view.get();
        let g = app.grids.of_view(v).first().copied()?;
        let sort = g.sort.get();
        let cols = g.all_cols.get();
        let name = |k: &str| {
            cols.iter()
                .find(|c| c.key == k)
                .map_or(k.to_string(), |c| c.title.clone())
        };
        let s = sort
            .iter()
            .map(|s| format!("{} {}", name(&s.key), if s.desc { "▼" } else { "▲" }))
            .collect::<Vec<_>>()
            .join(", ");
        let grp = g.group.get().map(|k| name(&k));
        Some(view! {
            {(!s.is_empty()).then(|| view! { <span class="qind" title="sort (^O)">"⇅ "{s}</span> })}
            {grp.map(|gname| view! { <span class="qind" title="group (^R)">"⊞ "{gname}</span> })}
        })
    };
    view! {
        <div class="querybar" class:hidden=move || current().is_none()>
            <span class="qlabel">"Filter ▸"</span>
            <div class="qwrap">
                <div class="qback" aria-hidden="true">{backdrop}</div>
                <input id="query" class="qin" node_ref=input spellcheck="false" autocomplete="off"
                    placeholder=move || match app.view.get() {
                        ViewId::Projects => "e.g.  status:active @me health:red,amber due<+30d #pilot",
                        ViewId::Activity => "e.g.  @bob action:delete",
                        ViewId::Timeline => "e.g.  program:gis is:open",
                        _ => "e.g.  is:open @me due<eow priority:high,critical \"load test\"",
                    }
                    prop:value=text
                    on:input=move |e| { if let Some((q, _)) = current() { q.set(event_target_value(&e)); } update_caret(); open.set(true); sel.set(0); }
                    on:keyup=move |e: web_sys::KeyboardEvent| { if !matches!(e.key().as_str(), "ArrowUp" | "ArrowDown" | "Enter" | "Tab" | "Escape") { update_caret(); } }
                    on:click=move |_| update_caret()
                    on:focus=move |_| { app.ctx.set(Ctx::Query); open.set(true); }
                    on:blur=move |_| { let _ = open.try_set(false); }
                    on:keydown=on_key />
                <Show when=move || !sugg.with(|s| s.is_empty())>
                    <div class="choices qsugg" on:mousedown=|e| e.prevent_default()>
                        {move || sugg.get().into_iter().enumerate().map(|(i, s)| view! {
                            <div class="choice" class:sel=move || sel.get() == i on:mousedown=move |_| accept(i)>
                                <span class="lbl">{s.label}</span><span class="det">{s.detail}</span>
                            </div>
                        }).collect_view()}
                    </div>
                </Show>
            </div>
            <span class="qmsg">{move || compiled.with(|(e, _, _)| e.first().map(|e| format!("⚠ {}", e.2)).unwrap_or_default())}</span>
            <Show when=move || !text().is_empty()>
                <button class="btn sm ghost" title="Clear filter" on:click=move |_| run(app, Act::ClearFilter)>"✕"</button>
            </Show>
            <span class="qcount">{move || { let (_, s, t) = compiled.get(); format!("[ {s} / {t} ]") }}</span>
            {indicators}
        </div>
    }
}

/// Move focus back to the main pane of the view.
pub fn focus_main(app: App) {
    let v = app.view.get_untracked();
    match v {
        ViewId::Board => util::focus_id("board"),
        ViewId::Timeline => util::focus_id("timeline"),
        ViewId::Dashboard => util::focus_id("dashboard"),
        _ => {
            let g = app.current_grid();
            if let Some(g) = g {
                util::focus_id(g.key.dom_id());
            }
        }
    }
}

// ---- status line, hint bar, toasts ---------------------------------------------------------

#[component]
pub fn StatusLine() -> impl IntoView {
    let app = use_app();
    let online = move || {
        let ids: Vec<i64> = app.peers.with(|p| {
            let mut v: Vec<i64> = p.iter().map(|p| p.user_id).collect();
            v.sort();
            v.dedup();
            v
        });
        app.rep.with(|r| {
            ids.iter()
                .filter_map(|id| r.users.get(id).cloned())
                .collect::<Vec<_>>()
        })
    };
    view! {
        <div class="statusline">
            <span class=move || format!("smsg {}", app.status.get().0.class())>
                {move || { let (l, t) = app.status.get(); format!("{} {t}", l.icon()) }}
            </span>
            <span class="spacer"></span>
            {move || {
                let n = app.marks.with(|m| m.values().filter(|v| matches!(v, crate::app::Mark::Conflict(_))).count());
                (n > 0).then(|| view! { <span class="sconf">"⚠ "{n}" conflict"{if n == 1 { "" } else { "s" }}</span> })
            }}
            <span class="avs">
                {move || online().into_iter().map(|u| {
                    let view_of = app.peers.with(|p| p.iter().filter(|p| p.user_id == u.id).filter_map(|p| p.view.clone()).collect::<Vec<_>>().join(", "));
                    view! { <span title=format!("{} — {}", u.display_name, view_of)>{avatar(u.id, &u.display_name)}</span> }
                }).collect_view()}
            </span>
            <span class="muted">{move || format!("online: {}", online().len())}</span>
            <span class="pend" class:busy=move || { app.pending.get() > 0 }>{move || format!("⟳ {} pending", app.pending.get())}</span>
        </div>
    }
}

fn hints(app: App) -> Vec<(&'static str, &'static str, Option<Act>)> {
    let v = app.view.get();
    let ctx = app.ctx.get();
    if app.menu.get().is_some() {
        return vec![
            ("←→", "Menus", None),
            ("↑↓", "Items", None),
            ("Enter", "Run", None),
            ("Esc", "Close", Some(Act::Close)),
        ];
    }
    if app.modal.get().is_some() {
        return vec![
            ("Tab", "Next field", None),
            ("^S", "Save", None),
            ("Enter", "OK", None),
            ("Esc", "Cancel", Some(Act::Close)),
            ("F1", "Help", Some(Act::Help)),
        ];
    }
    match ctx {
        Ctx::Edit => vec![
            ("Enter", "Save ↓", None),
            ("Tab", "Save →", None),
            ("Esc", "Cancel", None),
            ("↑↓", "Choose / ±", None),
            ("PgUp/Dn", "Month", None),
            ("Bksp", "Remove chip", None),
        ],
        Ctx::Query => vec![
            ("Tab", "Complete", None),
            ("Enter", "To grid", None),
            ("Esc", "Back", None),
            ("field:", "e.g. status:", None),
            ("@me", "Mine", None),
            ("#tag", "Tag", None),
            ("-term", "Exclude", None),
        ],
        _ => match v {
            ViewId::Board => vec![
                ("^K", "Commands", Some(Act::Palette)),
                ("^F", "Filter", Some(Act::Filter)),
                ("Shift+←→", "Move card", Some(Act::MoveRight)),
                ("Enter", "Open", Some(Act::EditRecord)),
                ("Ins", "New", Some(Act::New)),
                ("^R", "Swimlanes", Some(Act::Lanes)),
                ("^Z", "Undo", Some(Act::Undo)),
                ("F1", "Help", Some(Act::Help)),
                ("F10", "Menu", Some(Act::Menu)),
            ],
            ViewId::Timeline => vec![
                ("^K", "Commands", Some(Act::Palette)),
                ("→/Enter", "Expand", None),
                ("Shift+←→", "Move bar", Some(Act::MoveRight)),
                ("[ ]", "Due −/+", Some(Act::Extend)),
                ("+ −", "Zoom", Some(Act::ZoomIn)),
                ("t", "Today", Some(Act::Today)),
                ("^E", "Record", Some(Act::EditRecord)),
                ("^Z", "Undo", Some(Act::Undo)),
                ("F1", "Help", Some(Act::Help)),
            ],
            ViewId::Dashboard => vec![
                ("Tab/←→", "Tiles", None),
                ("Enter", "Drill down", None),
                ("Alt+1-9", "Views", None),
                ("^K", "Commands", Some(Act::Palette)),
                ("F1", "Help", Some(Act::Help)),
                ("F10", "Menu", Some(Act::Menu)),
            ],
            _ => vec![
                ("^K", "Commands", Some(Act::Palette)),
                ("^F", "Filter", Some(Act::Filter)),
                ("F2", "Edit", Some(Act::Edit)),
                ("Ins", "New", Some(Act::New)),
                ("F8", "Delete", Some(Act::Delete)),
                ("^E", "Record", Some(Act::EditRecord)),
                ("^Z", "Undo", Some(Act::Undo)),
                ("F1", "Help", Some(Act::Help)),
                ("F10", "Menu", Some(Act::Menu)),
            ],
        },
    }
}

fn hints2(app: App) -> Vec<(&'static str, &'static str, Option<Act>)> {
    let v = app.view.get();
    if app.ctx.get() != Ctx::Grid || app.modal.get().is_some() || app.menu.get().is_some() {
        return vec![];
    }
    if matches!(v, ViewId::Board | ViewId::Timeline | ViewId::Dashboard) {
        return vec![];
    }
    vec![
        ("Space", "Mark", Some(Act::MarkRow)),
        ("^C", "Copy", Some(Act::Copy)),
        ("^V", "Paste", Some(Act::Paste)),
        ("^D", "Fill down", Some(Act::FillDown)),
        ("^O", "Sort", Some(Act::SortCol)),
        ("^R", "Group", Some(Act::GroupCol)),
        ("F4", "Columns", Some(Act::Columns)),
        ("F6", "Pane", Some(Act::NextPane)),
        ("^B", "Detail", Some(Act::ToggleDetail)),
        ("Alt+1-9", "Views", None),
    ]
}

#[component]
pub fn HintBar() -> impl IntoView {
    let app = use_app();
    let render = move |hs: Vec<(&'static str, &'static str, Option<Act>)>| {
        hs.into_iter()
            .map(|(k, l, a)| {
                view! {
                    <span class="hint" class:click=a.is_some() on:mousedown=move |e| { if let Some(a) = a { e.prevent_default(); run(app, a); } }>
                        <kbd>{k}</kbd>" "{l}
                    </span>
                }
            })
            .collect_view()
    };
    view! {
        <div class="hintbar">
            <div class="hrow">{move || render(hints(app))}</div>
            <div class="hrow">{move || render(hints2(app))}</div>
        </div>
    }
}

#[component]
pub fn Toasts() -> impl IntoView {
    let app = use_app();
    view! {
        <div class="toasts">
            <For each=move || app.toasts.get() key=|t| t.id children=move |t| {
                let cls = format!("toast {}", t.level.class());
                let id = t.id;
                view! {
                    <div class=cls on:click=move |_| app.toasts.update(|v| v.retain(|x| x.id != id))>
                        <b>{t.level.icon()}</b>" "{t.text.clone()}
                    </div>
                }
            } />
        </div>
    }
}

// ---- global keys ------------------------------------------------------------------------------

/// Window-level keydown: app-wide shortcuts (ui-spec §2, Web column).
pub fn global_key(app: App, e: web_sys::KeyboardEvent) {
    if app.user.get_untracked().is_none() {
        return;
    }
    if menu_key(app, &e) {
        e.prevent_default();
        return;
    }
    let k = e.key();
    let code = e.code();
    let ctrl = util::ctrl(&e);
    let alt = e.alt_key();
    let shift = e.shift_key();
    let typing = util::typing_in_field();
    let modal = app.modal.get_untracked().is_some();
    let act: Option<Act> = match (k.as_str(), ctrl, alt) {
        ("F1", _, _) => Some(Act::Help),
        ("g", true, false) | ("G", true, false) => Some(Act::Help),
        ("F10", false, false) if shift => context_menu(app),
        ("ContextMenu", _, _) => context_menu(app),
        ("F10", false, false) => Some(Act::Menu),
        ("k", true, false) | ("K", true, false) | ("p", true, false) | ("P", true, false) => {
            Some(Act::Palette)
        }
        ("Escape", _, _) => {
            if modal {
                Some(Act::Close)
            } else if typing {
                focus_main(app);
                None
            } else {
                None
            }
        }
        _ if modal => None,
        ("f", true, false) | ("F", true, false) | ("F3", _, _) => Some(Act::Filter),
        ("F6", _, _) => Some(Act::NextPane),
        ("ArrowRight", false, true) => Some(Act::NextView),
        ("ArrowLeft", false, true) => Some(Act::PrevView),
        ("z", true, false) | ("Z", true, false) if !typing => {
            Some(if shift { Act::Redo } else { Act::Undo })
        }
        ("y", true, false) | ("Y", true, false) if !typing => Some(Act::Redo),
        ("b", true, false) | ("B", true, false) => Some(Act::ToggleDetail),
        ("t", true, false) | ("T", true, false) => Some(Act::Theme),
        ("Insert", _, _) if !typing => Some(Act::New),
        ("e", true, false) | ("E", true, false) if !typing => Some(Act::EditRecord),
        ("F8", _, _) if !typing => Some(Act::Delete),
        ("Delete", true, _) if !typing => Some(Act::Delete),
        ("F4", _, _) if !typing => Some(Act::Columns),
        ("o", true, false) | ("O", true, false) if !typing => Some(Act::SortCol),
        ("r", true, false) | ("R", true, false) if !typing => {
            Some(if app.view.get_untracked() == ViewId::Board {
                Act::Lanes
            } else {
                Act::GroupCol
            })
        }
        ("F2", _, _) if !typing => Some(Act::Edit),
        _ if alt && !ctrl => {
            // Alt+digit = view, Alt+letter = menu (use `code`: Option on macOS types symbols)
            if let Some(d) = code
                .strip_prefix("Digit")
                .and_then(|d| d.parse::<usize>().ok())
            {
                ViewId::ALL.get(d.wrapping_sub(1)).map(|v| Act::View(*v))
            } else if code == "KeyN" {
                Some(Act::New)
            } else if let Some(i) = MENU_TITLES
                .iter()
                .position(|(_, c)| code == format!("Key{}", c.to_ascii_uppercase()))
            {
                app.menu.set(Some(i));
                app.menu_sel.set(0);
                e.prevent_default();
                None
            } else {
                None
            }
        }
        _ => None,
    };
    if let Some(a) = act {
        e.prevent_default();
        run(app, a);
    }
}

fn context_menu(app: App) -> Option<Act> {
    let g = app.current_grid()?;
    let el = util::document()
        .query_selector(&format!("#{} .cell.cur", g.key.dom_id()))
        .ok()
        .flatten()?;
    let (x, y, _, h) = util::rect_of(&el);
    app.modal.set(Some(Modal::Context {
        x,
        y: y + h,
        grid: g.key,
    }));
    None
}
