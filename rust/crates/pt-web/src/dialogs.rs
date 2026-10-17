//! Dialogs and popups (ui-spec §5): command palette, help, about, confirm,
//! columns, sort, password, save filter, context menu, the cascading New
//! project / New task dialogs, the record form and the generic create form.

use leptos::prelude::*;
use pt_core::fields::{self, Kind, values_equal};
use pt_core::fuzzy;
use pt_core::grid::{self, Choice, SortSpec};
use pt_core::model::*;
use pt_core::protocol::{ClientMsg, Op};
use serde_json::{Map, Value};
use wasm_bindgen::JsCast;

use crate::app::{
    Act, App, ConfirmAct, GridKey, Level, Modal, SavedFilter, THEMES, ViewId, field_label, run,
    use_app,
};
use crate::forms::{Ctl, Field, ctl_for, value_text};
use crate::gridops;
use crate::widgets::highlight;
use crate::{net, shell, util};

#[component]
pub fn Modals() -> impl IntoView {
    let app = use_app();
    // return focus to the main pane when a dialog closes
    Effect::new(move |prev: Option<bool>| {
        let open = app.modal.with(|m| m.is_some());
        if prev == Some(true) && !open {
            request_animation_frame(move || {
                // another dialog may have opened meanwhile; leave its focus alone
                if app.modal.get_untracked().is_none() {
                    shell::focus_main(app);
                }
            });
        }
        if open {
            app.ctx.set(crate::app::Ctx::Dialog);
        } else if prev == Some(true) {
            app.ctx.set(crate::app::Ctx::Grid);
        }
        open
    });
    move || {
        let m = app.modal.get()?;
        Some(match m {
            Modal::Palette => view! { <Palette /> }.into_any(),
            Modal::Help => view! { <Help /> }.into_any(),
            Modal::About => view! { <About /> }.into_any(),
            Modal::Confirm { title, body, act } => {
                view! { <Confirm title=title body=body act=act /> }.into_any()
            }
            Modal::Columns(k) => view! { <Columns k=k /> }.into_any(),
            Modal::Sort(k) => view! { <SortDialog k=k /> }.into_any(),
            Modal::SetPassword { user } => view! { <SetPassword user=user /> }.into_any(),
            Modal::SaveFilter => view! { <SaveFilterDlg /> }.into_any(),
            Modal::Context { x, y, grid } => view! { <ContextMenu x=x y=y k=grid /> }.into_any(),
            Modal::NewProject { program } => view! { <NewProject program=program /> }.into_any(),
            Modal::NewTask { project, status } => {
                view! { <NewTask project=project status=status /> }.into_any()
            }
            Modal::Record { entity, id } => view! { <RecordForm entity=entity id=id /> }.into_any(),
            Modal::Create { entity, preset } => {
                view! { <CreateForm entity=entity preset=preset /> }.into_any()
            }
        })
    }
}

fn close(app: App) {
    app.modal.set(None);
}

/// Focus the first field of the dialog once rendered.
fn autofocus(sel: &'static str) {
    request_animation_frame(move || {
        if let Ok(Some(el)) = util::document().query_selector(sel) {
            if let Ok(h) = el.dyn_into::<web_sys::HtmlElement>() {
                let _ = h.focus();
            }
        }
    });
}

/// Keep Tab inside the dialog.
fn trap_tab(e: &web_sys::KeyboardEvent) {
    if e.key() != "Tab" || e.default_prevented() {
        return;
    }
    let Ok(Some(dlg)) = util::document().query_selector(".dialog") else {
        return;
    };
    let Ok(list) = dlg.query_selector_all("input, textarea, select, button, [tabindex='0']") else {
        return;
    };
    let els: Vec<web_sys::HtmlElement> = (0..list.length())
        .filter_map(|i| list.item(i))
        .filter_map(|n| n.dyn_into().ok())
        .collect();
    let (Some(first), Some(last)) = (els.first(), els.last()) else {
        return;
    };
    let active = util::active_element();
    let at = |el: &web_sys::HtmlElement| active.as_ref().is_some_and(|a| a.is_same_node(Some(el)));
    if e.shift_key() && at(first) {
        e.prevent_default();
        let _ = last.focus();
    } else if !e.shift_key() && at(last) {
        e.prevent_default();
        let _ = first.focus();
    }
}

#[component]
fn Dialog(
    title: String,
    #[prop(optional)] class: &'static str,
    #[prop(optional)] on_save: Option<Callback<bool>>,
    children: Children,
) -> impl IntoView {
    let app = use_app();
    let on_key = move |e: web_sys::KeyboardEvent| {
        if util::ctrl(&e) && (e.key() == "s" || e.key() == "S") {
            e.prevent_default();
            e.stop_propagation();
            if let Some(cb) = on_save {
                cb.run(e.shift_key());
            }
            return;
        }
        if e.key() == "Escape" {
            e.prevent_default();
            e.stop_propagation();
            close(app);
            return;
        }
        trap_tab(&e);
    };
    let label = title.clone();
    view! {
        <div class="modal-back" on:mousedown=move |e| {
            let is_back = e.target().zip(e.current_target()).is_some_and(|(t, c)| t == c);
            if is_back { close(app); }
        }>
            <div class=format!("dialog {class}") role="dialog" aria-label=label on:keydown=on_key>
                <div class="dlg-h"><span>{title}</span><button class="btn ghost sm" title="Close (Esc)" on:click=move |_| close(app)>"✕"</button></div>
                {children()}
            </div>
        </div>
    }
}

// ---- command palette -------------------------------------------------------------------

#[derive(Clone, PartialEq)]
enum PalItem {
    Do(Act),
    Saved(usize),
    Goto(Entity, i64),
}

#[derive(Clone, PartialEq)]
struct Pal {
    label: String,
    key: String,
    detail: String,
    item: PalItem,
}

const PAL_ACTS: &[Act] = &[
    Act::NewProject,
    Act::NewTask,
    Act::New,
    Act::EditRecord,
    Act::Edit,
    Act::Delete,
    Act::Undo,
    Act::Redo,
    Act::Copy,
    Act::Paste,
    Act::FillDown,
    Act::ClearCell,
    Act::MarkRow,
    Act::MarkAll,
    Act::Filter,
    Act::ClearFilter,
    Act::SaveFilter,
    Act::SortCol,
    Act::SortDialog,
    Act::GroupCol,
    Act::ClearGroup,
    Act::Columns,
    Act::ToggleDetail,
    Act::NextPane,
    Act::NextView,
    Act::PrevView,
    Act::Theme,
    Act::Density,
    Act::Lanes,
    Act::ZoomIn,
    Act::ZoomOut,
    Act::Today,
    Act::Help,
    Act::About,
    Act::SetPassword,
    Act::Logout,
];

fn palette_items(app: App) -> Vec<Pal> {
    let mut v: Vec<Pal> = Vec::new();
    for vid in ViewId::ALL {
        let a = Act::View(vid);
        v.push(Pal {
            label: a.label(),
            key: a.key(),
            detail: "view".into(),
            item: PalItem::Do(a),
        });
    }
    for a in PAL_ACTS {
        v.push(Pal {
            label: a.label(),
            key: a.key(),
            detail: "command".into(),
            item: PalItem::Do(*a),
        });
    }
    for i in 0..THEMES.len() {
        let a = Act::ThemeSet(i);
        v.push(Pal {
            label: a.label(),
            key: String::new(),
            detail: "theme".into(),
            item: PalItem::Do(a),
        });
    }
    app.saved.with_untracked(|s| {
        for (i, f) in s.iter().enumerate() {
            v.push(Pal {
                label: format!("★ {}", f.name),
                key: String::new(),
                detail: format!("saved filter · {}", f.view),
                item: PalItem::Saved(i),
            });
        }
    });
    app.rep.with_untracked(|r| {
        for p in r.projects.values() {
            v.push(Pal {
                label: format!("Go to project {} {}", p.code, p.name),
                key: String::new(),
                detail: r.program_name(p.program_id).to_string(),
                item: PalItem::Goto(Entity::Project, p.id),
            });
        }
        for t in r.tasks.values() {
            v.push(Pal {
                label: format!("Go to task {} {}", r.task_ref(t), t.title),
                key: String::new(),
                detail: t.status.label().into(),
                item: PalItem::Goto(Entity::Task, t.id),
            });
        }
    });
    v
}

#[component]
fn Palette() -> impl IntoView {
    let app = use_app();
    let all = StoredValue::new(palette_items(app));
    let q = RwSignal::new(String::new());
    let sel = RwSignal::new(0usize);
    let results = Memo::new(move |_| {
        let q = q.get();
        all.with_value(|all| {
            if q.trim().is_empty() {
                // commands first when nothing is typed
                return all
                    .iter()
                    .filter(|p| !matches!(p.item, PalItem::Goto(..)))
                    .take(60)
                    .cloned()
                    .map(|p| (p, vec![]))
                    .collect::<Vec<_>>();
            }
            // commands and projects outrank the (many) tasks on ties
            let mut ranked: Vec<(i32, usize, Vec<usize>)> =
                fuzzy::rank(&q, all.iter().map(|p| p.label.as_str()))
                    .into_iter()
                    .map(|(i, m)| {
                        let bonus = match all[i].item {
                            PalItem::Goto(Entity::Task, _) => 0,
                            PalItem::Goto(..) => 12,
                            _ => 20,
                        };
                        (m.score + bonus, i, m.indices)
                    })
                    .collect();
            ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            ranked
                .into_iter()
                .take(60)
                .map(|(_, i, m)| (all[i].clone(), m))
                .collect()
        })
    });
    let exec = move |i: usize| {
        let Some((p, _)) = results.with_untracked(|r| r.get(i).cloned()) else {
            return;
        };
        close(app);
        match p.item {
            PalItem::Do(a) => run(app, a),
            PalItem::Saved(i) => shell::apply_saved(app, i),
            PalItem::Goto(e, id) => app.goto(e, id),
        }
    };
    let list = NodeRef::<leptos::html::Div>::new();
    Effect::new(move |_| {
        let s = sel.get();
        if let Some(el) = list.get() {
            if let Some(c) = el.children().item(s as u32) {
                let o = web_sys::ScrollIntoViewOptions::new();
                o.set_block(web_sys::ScrollLogicalPosition::Nearest);
                c.scroll_into_view_with_scroll_into_view_options(&o);
            }
        }
    });
    autofocus(".palette input");
    view! {
        <Dialog title="Commands".into() class="palette">
            <input class="fin big" placeholder="Type a command, view, project code or task…" prop:value=move || q.get()
                on:input=move |e| { q.set(event_target_value(&e)); sel.set(0); }
                on:keydown=move |e: web_sys::KeyboardEvent| {
                    let n = results.with_untracked(|r| r.len());
                    match e.key().as_str() {
                        "ArrowDown" => sel.update(|s| *s = (*s + 1).min(n.saturating_sub(1))),
                        "ArrowUp" => sel.update(|s| *s = s.saturating_sub(1)),
                        "PageDown" => sel.update(|s| *s = (*s + 10).min(n.saturating_sub(1))),
                        "PageUp" => sel.update(|s| *s = s.saturating_sub(10)),
                        "Enter" => exec(sel.get_untracked()),
                        _ => return,
                    }
                    e.prevent_default();
                } />
            <div class="plist" node_ref=list>
                {move || results.get().into_iter().enumerate().map(|(i, (p, m))| view! {
                    <div class="pitem" class:sel=move || sel.get() == i on:mouseenter=move |_| sel.set(i) on:mousedown=move |e| { e.prevent_default(); exec(i); }>
                        <span class="pl">{highlight(&p.label, &m)}</span>
                        <span class="pd">{p.detail.clone()}</span>
                        <kbd class="pk">{p.key.clone()}</kbd>
                    </div>
                }).collect_view()}
            </div>
        </Dialog>
    }
}

// ---- help --------------------------------------------------------------------------------

const HELP: &[(&str, &[(&str, &str)])] = &[
    (
        "Global",
        &[
            ("F1  ^G", "This help"),
            ("F10  Alt+F/E/V/D/H", "Menu bar"),
            (
                "^K  ^P",
                "Command palette: commands, views, go to any project or task",
            ),
            ("^F  F3", "Focus the filter bar"),
            ("Alt+1 … Alt+9", "Go to view"),
            ("Alt+→  Alt+←", "Next / previous view"),
            ("F6", "Next pane (grid ⇄ detail)"),
            ("^Z  ^Y", "Undo / redo your own last change (server-side)"),
            ("Ins  Alt+N", "New row / record"),
            ("^E", "Edit the whole record in a form"),
            ("F8  ^Del", "Delete row(s)"),
            ("^B", "Toggle the detail pane"),
            ("F4", "Columns: show, hide, reorder, width"),
            ("^O", "Sort by current column (▲ ▼ off)"),
            ("^R", "Group by current column"),
            ("^T", "Next theme (View menu if the browser takes ^T)"),
            ("Shift+F10  Menu key  right-click", "Context menu"),
            ("Esc", "Close popup / cancel / back to the grid"),
        ],
    ),
    (
        "Grid",
        &[
            ("↑ ↓ ← →", "Move"),
            ("Tab  Shift+Tab", "Next / previous cell"),
            ("Home  End", "First / last column"),
            ("^Home  ^End", "First / last row"),
            ("PgUp  PgDn", "Page"),
            ("Enter  F2  double-click", "Edit cell"),
            (
                "type a letter",
                "Start editing with that letter / filter a list",
            ),
            ("Del", "Clear cell"),
            ("Space", "Mark row"),
            ("Shift+↑ ↓", "Extend marked range"),
            ("^A", "Mark all (again: none)"),
            ("^C", "Copy marked rows or cell as TSV"),
            ("^V", "Paste TSV at the cursor, filling right and down"),
            (
                "^D",
                "Fill down from the first marked row (or from the row above)",
            ),
            ("Enter / ← → on a group", "Collapse / expand"),
            ("click header  Shift+click", "Sort / add sort key"),
            ("drag header edge", "Resize column"),
        ],
    ),
    (
        "Editing",
        &[
            ("Enter", "Save and move down"),
            ("Tab  Shift+Tab", "Save and move right / left"),
            ("Esc", "Cancel"),
            (
                "dates",
                "fri, tom, +3d, -2w, eow, eom, 11/3, nov 3 · ↑↓ week · PgUp/Dn month · ^←→ day",
            ),
            ("numbers", "↑ ↓ ±1, Shift ±0.25"),
            ("lists", "type to fuzzy-filter, ↑ ↓, Enter"),
            (
                "chips",
                "Enter adds, Backspace removes, Enter on empty saves",
            ),
            (
                "conflict cell (⚠)",
                "Someone changed it first. Enter edits from their value, Esc dismisses",
            ),
        ],
    ),
    (
        "Board",
        &[
            ("← → ↑ ↓", "Move between cards"),
            ("Shift+← →  < >", "Move card to the previous / next status"),
            ("Enter  ^E", "Open card"),
            ("Ins", "New task in this column"),
            ("^R", "Swimlanes by assignee"),
            ("drag & drop", "Move a card"),
        ],
    ),
    (
        "Timeline",
        &[
            ("↑ ↓", "Rows"),
            ("→ Enter  ←", "Expand / collapse a project"),
            ("Shift+← →", "Move the bar ±1 day/week/month"),
            ("[  ]", "Shrink / extend the due date"),
            ("+  −", "Zoom"),
            ("t", "Jump to today"),
            ("drag bar / right edge", "Move / resize"),
        ],
    ),
    (
        "Dialogs",
        &[
            ("Tab  Shift+Tab", "Fields"),
            ("^S", "Save"),
            ("Esc", "Cancel"),
        ],
    ),
    (
        "Filter language",
        &[
            ("word  \"two words\"", "Free text"),
            ("status:active,on_hold", "Field match, comma = or"),
            ("due<+7d  est>=8", "Compare numbers and dates"),
            ("-status:done", "Exclude"),
            ("@me  @bob", "Owner / assignee"),
            ("#pilot", "Tag"),
            (
                "is:overdue is:open is:blocked is:mine is:unassigned",
                "Predicates",
            ),
            ("has:notes has:deps has:due", "Non-empty"),
            ("program:nwr  project:nwr-004", "By program / project"),
        ],
    ),
];

#[component]
fn Help() -> impl IntoView {
    let q = RwSignal::new(String::new());
    autofocus(".help input");
    view! {
        <Dialog title="Keyboard help".into() class="help">
            <p class="muted intro">"pt is keyboard-first: every action has a key, shown in the hint bar at the bottom, the menus and the command palette (^K). Your edits save immediately and appear on every other client (TUI or browser) live; ^Z undoes your own last change. Type below to search."</p>
            <input class="fin" placeholder="Search keys…" prop:value=move || q.get() on:input=move |e| q.set(event_target_value(&e)) />
            <div class="helpgrid">
                {move || {
                    let f = q.get().to_lowercase();
                    HELP.iter().filter_map(|(sec, rows)| {
                        let rows: Vec<&(&str, &str)> = rows.iter().filter(|(k, d)| f.is_empty() || k.to_lowercase().contains(&f) || d.to_lowercase().contains(&f) || sec.to_lowercase().contains(&f)).collect();
                        (!rows.is_empty()).then(|| view! {
                            <section>
                                <h4>{*sec}</h4>
                                {rows.into_iter().map(|(k, d)| view! { <div class="hrowk"><kbd>{*k}</kbd><span>{*d}</span></div> }).collect_view()}
                            </section>
                        })
                    }).collect_view()
                }}
            </div>
        </Dialog>
    }
}

#[component]
fn About() -> impl IntoView {
    let app = use_app();
    let info = app.rep.with_untracked(|r| r.server.clone());
    view! {
        <Dialog title="About pt".into() class="about">
            <dl class="kv">
                <dt>"Client"</dt><dd>{format!("web-rust {} · Leptos {} (WebAssembly)", pt_core::VERSION, "0.8")}</dd>
                <dt>"Server"</dt><dd>{info.map(|s| format!("{} {}", s.implementation, s.version)).unwrap_or_else(|| "—".into())}</dd>
                <dt>"Protocol"</dt><dd>{pt_core::protocol::PROTOCOL_VERSION}</dd>
                <dt>"Data"</dt><dd>{move || app.rep.with(|r| format!("{} projects · {} tasks · {} users", r.projects.len(), r.tasks.len(), r.users.len()))}</dd>
            </dl>
            <p class="muted">"One SQLite server, any number of TUI and browser clients, all live. See docs/pt in the repository."</p>
        </Dialog>
    }
}

#[component]
fn Confirm(title: String, body: String, act: ConfirmAct) -> impl IntoView {
    let app = use_app();
    let act = StoredValue::new(act);
    let ok = move || {
        close(app);
        match act.get_value() {
            ConfirmAct::Delete { entity, ids } => {
                let n = ids.len();
                let ops = ids
                    .into_iter()
                    .map(|id| Op::Delete { entity, id })
                    .collect();
                app.mutate(ops, move |r| {
                    if r.ok && r.error.is_none() {
                        app.say(
                            Level::Ok,
                            format!(
                                "Deleted {n} {}{}.",
                                entity.label(),
                                if n == 1 { "" } else { "s" }
                            ),
                        );
                    }
                });
                for g in [app.grids.projects, app.grids.tasks, app.grids.proj_tasks] {
                    g.marks.set(Default::default());
                }
            }
        }
    };
    autofocus(".confirm .btn.cancel");
    view! {
        <Dialog title=title class="confirm">
            <p>{body}</p>
            <div class="dlg-f" on:keydown=move |e: web_sys::KeyboardEvent| match e.key().as_str() {
                "y" | "Y" => { e.prevent_default(); ok(); }
                "n" | "N" => { e.prevent_default(); close(app); }
                _ => {}
            }>
                <button class="btn cancel" on:click=move |_| close(app)><u>"N"</u>"o, cancel"</button>
                <button class="btn danger" on:click=move |_| ok()><u>"Y"</u>"es, delete"</button>
            </div>
        </Dialog>
    }
}

// ---- columns & sort -------------------------------------------------------------------------

#[component]
fn Columns(k: GridKey) -> impl IntoView {
    let app = use_app();
    let g = app.grid(k);
    let sel = RwSignal::new(0usize);
    let toggle = move |key: String, hidden: bool| {
        g.layout.update(|l| {
            if hidden {
                l.hidden.remove(&key);
                l.shown.insert(key);
            } else {
                l.shown.remove(&key);
                l.hidden.insert(key);
            }
        })
    };
    let moveby = move |i: usize, d: i64| {
        let keys: Vec<String> = g
            .all_cols
            .with_untracked(|c| c.iter().map(|c| c.key.clone()).collect());
        let j = i as i64 + d;
        if j < 0 || j as usize >= keys.len() {
            return;
        }
        let mut keys = keys;
        keys.swap(i, j as usize);
        g.layout.update(|l| l.order = keys);
        sel.set(j as usize);
    };
    let width = move |key: String, d: i64| {
        let w = g
            .all_cols
            .with_untracked(|c| c.iter().find(|c| c.key == key).map_or(10, |c| c.width))
            as i64;
        g.layout.update(|l| {
            l.widths.insert(key, (w + d).clamp(3, 120) as u16);
        });
    };
    let on_key = move |e: web_sys::KeyboardEvent| {
        let cols = g.all_cols.get_untracked();
        let i = sel.get_untracked().min(cols.len().saturating_sub(1));
        let Some(c) = cols.get(i) else { return };
        match e.key().as_str() {
            "ArrowDown" if e.shift_key() => moveby(i, 1),
            "ArrowUp" if e.shift_key() => moveby(i, -1),
            "]" => moveby(i, 1),
            "[" => moveby(i, -1),
            "ArrowDown" => sel.set((i + 1).min(cols.len() - 1)),
            "ArrowUp" => sel.set(i.saturating_sub(1)),
            " " => toggle(c.key.clone(), c.hidden),
            "+" | "=" => width(c.key.clone(), 2),
            "-" => width(c.key.clone(), -2),
            "r" | "R" => g.layout.set(Default::default()),
            "Enter" => close(app),
            _ => return,
        }
        e.prevent_default();
    };
    autofocus(".colsdlg .collist");
    view! {
        <Dialog title=format!("Columns — {}", k.label()) class="colsdlg">
            <div class="collist" tabindex="0" on:keydown=on_key>
                {move || g.all_cols.get().into_iter().enumerate().map(|(i, c)| {
                    let key = c.key.clone();
                    let key2 = c.key.clone();
                    let key3 = c.key.clone();
                    let hidden = c.hidden;
                    view! {
                        <div class="colrow" class:sel=move || sel.get() == i on:mousedown=move |_| sel.set(i)>
                            <input type="checkbox" prop:checked=!hidden on:change=move |_| toggle(key.clone(), hidden) tabindex="-1" />
                            <span class="cn" class:muted=hidden>{c.title.clone()}</span>
                            <span class="cw2">{format!("{} ch", c.width)}</span>
                            <button class="btn sm" tabindex="-1" on:click=move |_| width(key2.clone(), -2)>"−"</button>
                            <button class="btn sm" tabindex="-1" on:click=move |_| width(key3.clone(), 2)>"+"</button>
                            <button class="btn sm" tabindex="-1" on:click=move |_| moveby(i, -1)>"▲"</button>
                            <button class="btn sm" tabindex="-1" on:click=move |_| moveby(i, 1)>"▼"</button>
                        </div>
                    }
                }).collect_view()}
            </div>
            <div class="dlg-f">
                <span class="muted">"Space show/hide · Shift+↑↓ or [ ] move · + − width · r reset"</span>
                <button class="btn" on:click=move |_| g.layout.set(Default::default())>"Reset"</button>
                <button class="btn primary" on:click=move |_| close(app)>"Done"</button>
            </div>
        </Dialog>
    }
}

#[component]
fn SortDialog(k: GridKey) -> impl IntoView {
    let app = use_app();
    let g = app.grid(k);
    let cols = g.all_cols.get_untracked();
    let cur = g.sort.get_untracked();
    let keys: Vec<RwSignal<String>> = (0..3)
        .map(|i| RwSignal::new(cur.get(i).map(|s| s.key.clone()).unwrap_or_default()))
        .collect();
    let descs: Vec<RwSignal<bool>> = (0..3)
        .map(|i| RwSignal::new(cur.get(i).is_some_and(|s| s.desc)))
        .collect();
    let (k2, d2) = (keys.clone(), descs.clone());
    let save = Callback::new(move |_| {
        let spec: Vec<SortSpec> = k2
            .iter()
            .zip(&d2)
            .filter(|(k, _)| !k.get_untracked().is_empty())
            .map(|(k, d)| SortSpec {
                key: k.get_untracked(),
                desc: d.get_untracked(),
            })
            .collect();
        g.sort.set(spec);
        close(app);
    });
    autofocus(".sortdlg select");
    view! {
        <Dialog title=format!("Sort — {}", k.label()) class="sortdlg" on_save=save>
            {keys.into_iter().zip(descs).enumerate().map(|(i, (kk, dd))| {
                let cols = cols.clone();
                view! {
                    <div class="sortrow">
                        <span class="flabel">{if i == 0 { "Sort by" } else { "then by" }}</span>
                        <select class="fin" on:change=move |e| kk.set(event_target_value(&e))>
                            <option value="" selected=kk.get_untracked().is_empty()>"(none)"</option>
                            {cols.iter().map(|c| view! { <option value=c.key.clone() selected=kk.get_untracked() == c.key>{c.title.clone()}</option> }).collect_view()}
                        </select>
                        <select class="fin" on:change=move |e| dd.set(event_target_value(&e) == "desc")>
                            <option value="asc" selected=!dd.get_untracked()>"▲ ascending"</option>
                            <option value="desc" selected=dd.get_untracked()>"▼ descending"</option>
                        </select>
                    </div>
                }
            }).collect_view()}
            <div class="dlg-f">
                <button class="btn" on:click=move |_| close(app)>"Cancel"</button>
                <button class="btn primary" on:click=move |_| save.run(false)>"Apply  ^S"</button>
            </div>
        </Dialog>
    }
}

// ---- small forms -----------------------------------------------------------------------------

#[component]
fn SetPassword(user: i64) -> impl IntoView {
    let app = use_app();
    let me = app.me_id();
    let admin = app.is_admin();
    let name = app
        .rep
        .with_untracked(|r| r.user_name(Some(user)).to_string());
    let old = RwSignal::new(Value::from(""));
    let new = RwSignal::new(Value::from(""));
    let again = RwSignal::new(Value::from(""));
    let err = RwSignal::new(None::<String>);
    let save = Callback::new(move |_| {
        let (n, a) = (
            value_text(&new.get_untracked()),
            value_text(&again.get_untracked()),
        );
        if n != a {
            let _ = err.try_set(Some("The new passwords don't match.".into()));
            return;
        }
        if n.chars().count() < 8 {
            let _ = err.try_set(Some("Use at least 8 characters.".into()));
            return;
        }
        let o = value_text(&old.get_untracked());
        let sent = net::request(
            move |req| ClientMsg::SetPassword {
                req,
                user_id: user,
                password: n,
                old_password: (!admin).then_some(o),
            },
            move |r| {
                if r.ok {
                    close(app);
                    app.toast(
                        Level::Ok,
                        "Password changed. Other sessions for that user were signed out.",
                    );
                } else {
                    let _ = err.try_set(r.error.map(|e| e.message));
                }
            },
        );
        if !sent {
            let _ = err.try_set(Some("Not connected.".into()));
        }
    });
    autofocus(".pwdlg input");
    let need_old = !admin || user == me && !admin;
    view! {
        <Dialog title=format!("Set password — {name}") class="pwdlg" on_save=save>
            <div class="form">
                {need_old.then(|| view! { <Field label="Current password".into() ctl=Ctl::Password value=old wide=true /> })}
                <Field label="New password".into() ctl=Ctl::Password value=new wide=true />
                <Field label="Repeat new password".into() ctl=Ctl::Password value=again wide=true err=Signal::derive(move || err.get()) />
            </div>
            <div class="dlg-f">
                <button class="btn" on:click=move |_| close(app)>"Cancel"</button>
                <button class="btn primary" on:click=move |_| save.run(false)>"Save  ^S"</button>
            </div>
        </Dialog>
    }
}

#[component]
fn SaveFilterDlg() -> impl IntoView {
    let app = use_app();
    let v = app.view.get_untracked();
    let name = RwSignal::new(Value::from(""));
    let q = app
        .q
        .of(v)
        .map(|(q, _)| q.get_untracked())
        .unwrap_or_default();
    let qs = q.clone();
    let save = Callback::new(move |_| {
        let n = value_text(&name.get_untracked()).trim().to_string();
        if n.is_empty() {
            return;
        }
        let g = app.grids.of_view(v).first().copied();
        let f = SavedFilter {
            name: n.clone(),
            view: v.key().into(),
            query: qs.clone(),
            sort: g
                .map(|g| {
                    g.sort
                        .get_untracked()
                        .into_iter()
                        .map(|s| (s.key, s.desc))
                        .collect()
                })
                .unwrap_or_default(),
            group: g.and_then(|g| g.group.get_untracked()),
        };
        app.saved.update(|s| {
            s.retain(|x| x.name != n);
            s.push(f);
        });
        close(app);
        app.say(Level::Ok, format!("Saved filter “{n}” (Data menu and ^K)."));
    });
    autofocus(".savedlg input");
    view! {
        <Dialog title="Save filter".into() class="savedlg" on_save=save>
            <div class="form">
                <Field label="Name".into() ctl=Ctl::Text value=name wide=true />
            </div>
            <p class="muted">{format!("View: {} · Filter: {}", v.label(), if q.is_empty() { "(none)".to_string() } else { q.clone() })}</p>
            <div class="dlg-f">
                <button class="btn" on:click=move |_| close(app)>"Cancel"</button>
                <button class="btn primary" on:click=move |_| save.run(false)>"Save  ^S"</button>
            </div>
        </Dialog>
    }
}

// ---- context menu -----------------------------------------------------------------------------

fn query_term(app: App, g: crate::app::Grid) -> Option<String> {
    let col = g.cur_column()?;
    let id = g.cur_id.get_untracked()?;
    let quote = |s: &str| {
        if s.contains(' ') {
            format!("\"{s}\"")
        } else {
            s.to_string()
        }
    };
    app.rep.with_untracked(|r| {
        let v = col
            .field
            .as_ref()
            .map(|f| r.field_value(g.entity, id, f))
            .unwrap_or(Value::Null);
        Some(match col.key.as_str() {
            "status" | "priority" | "health" => format!("{}:{}", col.key, v.as_str()?),
            "owner_id" | "assignee_id" => match v.as_i64() {
                Some(u) => format!("@{}", r.username(Some(u))),
                None => format!(
                    "{}:none",
                    if col.key == "owner_id" {
                        "owner"
                    } else {
                        "assignee"
                    }
                ),
            },
            "program_id" => format!(
                "program:{}",
                r.programs.get(&v.as_i64()?)?.prefix.to_lowercase()
            ),
            "project" => format!(
                "project:{}",
                r.tasks
                    .get(&id)
                    .map(|t| r.project_code(t.project_id).to_lowercase())?
            ),
            "tags" => format!("#{}", v.as_array()?.first()?.as_str()?),
            "phase" => format!("phase:{}", quote(v.as_str()?)),
            "due_date" | "start_date" => format!(
                "{}:{}",
                if col.key == "due_date" {
                    "due"
                } else {
                    "start"
                },
                v.as_str().unwrap_or("none")
            ),
            "code" | "name" | "title" => quote(v.as_str()?),
            _ => return None,
        })
    })
}

fn add_term(app: App, term: String) {
    if let Some((q, _)) = app.q.of(app.view.get_untracked()) {
        q.update(|s| {
            if !s.is_empty() && !s.ends_with(' ') {
                s.push(' ');
            }
            s.push_str(&term);
        });
    }
}

#[component]
fn ContextMenu(x: f64, y: f64, k: GridKey) -> impl IntoView {
    let app = use_app();
    let g = app.grid(k);
    let term = query_term(app, g);
    let colname = g.cur_column().map(|c| c.title).unwrap_or_default();
    let has_q = app.q.of(app.view.get_untracked()).is_some();
    type Item = (String, String, Box<dyn Fn() + Send + Sync>);
    let mut items: Vec<Item> = vec![
        (
            "Edit cell".into(),
            "F2".into(),
            Box::new(move || gridops::start_edit(app, g, None)),
        ),
        (
            "Record form…".into(),
            "^E".into(),
            Box::new(move || gridops::act(app, g, Act::EditRecord)),
        ),
        (
            "Copy".into(),
            "^C".into(),
            Box::new(move || gridops::copy(app, g)),
        ),
        (
            "Paste".into(),
            "^V".into(),
            Box::new(move || gridops::act(app, g, Act::Paste)),
        ),
        (
            "Fill down".into(),
            "^D".into(),
            Box::new(move || gridops::fill_down(app, g)),
        ),
        (
            "Clear cell".into(),
            "Del".into(),
            Box::new(move || gridops::act(app, g, Act::ClearCell)),
        ),
    ];
    if let (Some(t), true) = (term, has_q) {
        let t2 = t.clone();
        items.push((
            format!("Filter by {t}"),
            String::new(),
            Box::new(move || add_term(app, t.clone())),
        ));
        items.push((
            format!("Exclude -{t2}"),
            String::new(),
            Box::new(move || add_term(app, format!("-{t2}"))),
        ));
    }
    let key = g.cur_column().map(|c| c.key).unwrap_or_default();
    let (k1, k2, k3) = (key.clone(), key.clone(), key.clone());
    items.extend::<Vec<Item>>(vec![
        (
            format!("Sort by {colname} ▲"),
            "^O".into(),
            Box::new(move || {
                g.sort.set(vec![SortSpec {
                    key: k1.clone(),
                    desc: false,
                }])
            }),
        ),
        (
            format!("Sort by {colname} ▼"),
            String::new(),
            Box::new(move || {
                g.sort.set(vec![SortSpec {
                    key: k2.clone(),
                    desc: true,
                }])
            }),
        ),
        (
            format!("Group by {colname}"),
            "^R".into(),
            Box::new(move || gridops::group_col(app, g)),
        ),
        (
            format!("Hide {colname}"),
            String::new(),
            Box::new(move || {
                g.layout.update(|l| {
                    l.shown.remove(&k3);
                    l.hidden.insert(k3.clone());
                })
            }),
        ),
        (
            "Delete row(s)…".into(),
            "F8".into(),
            Box::new(move || gridops::delete_rows(app, g)),
        ),
    ]);
    if k == GridKey::Users {
        if let Some(uid) = g.cur_id.get_untracked() {
            items.push((
                "Set password…".into(),
                String::new(),
                Box::new(move || app.modal.set(Some(Modal::SetPassword { user: uid }))),
            ));
        }
    }
    let items = StoredValue::new(items);
    let n = items.with_value(|i| i.len());
    let sel = RwSignal::new(0usize);
    let run_i = move |i: usize| {
        close(app);
        items.with_value(|it| (it[i].2)());
    };
    let (vw, vh) = util::viewport();
    let style = format!(
        "left:{}px;top:{}px",
        x.min(vw - 260.0),
        y.min(vh - n as f64 * 26.0 - 12.0)
    );
    autofocus(".ctxmenu");
    view! {
        <div class="modal-back clear" on:mousedown=move |e| { if e.target() == e.current_target() { close(app); } }>
            <div class="ctxmenu drop" style=style tabindex="0" on:keydown=move |e: web_sys::KeyboardEvent| {
                match e.key().as_str() {
                    "ArrowDown" => sel.update(|s| *s = (*s + 1) % n),
                    "ArrowUp" => sel.update(|s| *s = (*s + n - 1) % n),
                    "Enter" => run_i(sel.get_untracked()),
                    "Escape" => close(app),
                    _ => return,
                }
                e.prevent_default();
                e.stop_propagation();
            }>
                {(0..n).map(|i| {
                    let (l, k) = items.with_value(|it| (it[i].0.clone(), it[i].1.clone()));
                    view! {
                        <div class="item" class:sel=move || sel.get() == i on:mouseenter=move |_| sel.set(i) on:mousedown=move |e| { e.prevent_default(); run_i(i); }>
                            <span class="il">{l}</span><span class="ik">{k}</span>
                        </div>
                    }
                }).collect_view()}
            </div>
        </div>
    }
}

// ---- record-creating dialogs ----------------------------------------------------------------

fn choices_sig(
    entity: Entity,
    field: &'static str,
    project_hint: Signal<Option<i64>>,
) -> Signal<Vec<Choice>> {
    let app = use_app();
    Signal::derive(move || {
        let hint = project_hint.get();
        app.rep.with(|r| grid::choices(r, entity, field, 0, hint))
    })
}

fn text(v: &RwSignal<Value>) -> String {
    value_text(&v.get_untracked())
}

/// Parse a form value for a field via pt-core (dates, hours, labels).
fn parsed(app: App, e: Entity, field: &str, v: &Value, ctl: Ctl) -> Result<Value, String> {
    match ctl {
        Ctl::Text | Ctl::Number | Ctl::Date | Ctl::Area | Ctl::Suggest | Ctl::Password => app
            .rep
            .with_untracked(|r| grid::parse_input(r, e, field, 0, &value_text(v))),
        _ => Ok(v.clone()),
    }
}

#[component]
fn NewProject(program: Option<i64>) -> impl IntoView {
    let app = use_app();
    let today = app.rep.with_untracked(|r| r.today);
    let program = RwSignal::new(program.map_or(Value::Null, Value::from));
    let code = RwSignal::new(Value::from(
        app.rep
            .with_untracked(|r| r.next_code(program.get_untracked().as_i64())),
    ));
    let name = RwSignal::new(Value::from(""));
    let first_tpl = app.rep.with_untracked(|r| {
        r.templates
            .values()
            .find(|t| t.name == "Standard Project")
            .or(r.templates.values().next())
            .map(|t| t.id)
    });
    let template = RwSignal::new(first_tpl.map_or(Value::Null, Value::from));
    let owner = RwSignal::new(Value::from(app.me_id()));
    let start = RwSignal::new(Value::from(today.iso()));
    let due = RwSignal::new(Value::Null);
    let budget = RwSignal::new(Value::Null);
    let (code_d, due_d, budget_d) = (
        RwSignal::new(false),
        RwSignal::new(false),
        RwSignal::new(false),
    );
    let err = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    // cascade: program → next free code
    Effect::new(move |_| {
        let p = program.get().as_i64();
        if !code_d.get_untracked() {
            code.set(Value::from(app.rep.with_untracked(|r| r.next_code(p))));
        }
    });
    // cascade: template + start → due and budget defaults
    Effect::new(move |_| {
        let t = template.get().as_i64();
        let s = pt_core::date::parse_natural(&value_text(&start.get()), today)
            .ok()
            .flatten()
            .unwrap_or(today);
        let (_, hours, span) = t
            .map(|t| app.rep.with_untracked(|r| r.template_stats(t)))
            .unwrap_or((0, 0.0, 30));
        if !due_d.get_untracked() {
            due.set(Value::from(s.add_days(span.max(1) as i32).iso()));
        }
        if !budget_d.get_untracked() {
            budget.set(if hours > 0.0 {
                Value::from(grid::fmt_hours(hours))
            } else {
                Value::Null
            });
        }
    });
    let tpl_hint = Signal::derive(move || {
        template
            .get()
            .as_i64()
            .map(|t| {
                let (n, h, span) = app.rep.with(|r| r.template_stats(t));
                format!("{n} tasks, {} h, {span} days", grid::fmt_hours(h))
            })
            .unwrap_or_else(|| "a blank project with no tasks".into())
    });
    let none = Signal::derive(|| None);
    let save = Callback::new(move |_| {
        if busy.get_untracked() {
            return;
        }
        let mut values = Map::new();
        let e = Entity::Project;
        let fields: [(&str, RwSignal<Value>, Ctl); 8] = [
            ("code", code, Ctl::Text),
            ("name", name, Ctl::Text),
            ("program_id", program, Ctl::Combo),
            ("template_id", template, Ctl::Combo),
            ("owner_id", owner, Ctl::Combo),
            ("start_date", start, Ctl::Date),
            ("due_date", due, Ctl::Date),
            ("budget_hours", budget, Ctl::Number),
        ];
        for (f, v, c) in fields {
            match parsed(app, e, f, &v.get_untracked(), c) {
                Ok(val) => {
                    values.insert(f.into(), val);
                }
                Err(m) => {
                    let _ = err.try_set(Some(format!(
                        "{}: {m}",
                        field_label(&app.rep.read_untracked(), e, f)
                    )));
                    return;
                }
            }
        }
        if text(&name).trim().is_empty() {
            let _ = err.try_set(Some("Give the project a name.".into()));
            return;
        }
        busy.set(true);
        app.mutate(
            vec![Op::Create {
                entity: e,
                reference: Some("p".into()),
                values,
            }],
            move |r| {
                let _ = busy.try_set(false);
                if let Some(id) = r.created.get("p") {
                    let id = *id;
                    close(app);
                    app.when_present(Entity::Project, id, move || {
                        let (code, n) = app.rep.with_untracked(|r| {
                            (r.project_code(id).to_string(), r.project_tasks(id).len())
                        });
                        app.toast(Level::Ok, format!("Created {code} with {n} tasks."));
                        app.goto(Entity::Project, id);
                    });
                } else if let Some(e) = &r.error {
                    let _ = err.try_set(Some(e.message.clone()));
                }
            },
        );
    });
    autofocus(".newproj input");
    view! {
        <Dialog title="New project".into() class="newproj form-dlg" on_save=save>
            <div class="form">
                <Field label="Program".into() ctl=Ctl::Combo value=program choices=choices_sig(Entity::Project, "program_id", none) />
                <Field label="Code".into() ctl=Ctl::Text value=code on_edit=Callback::new(move |_| code_d.set(true))
                    hint=Signal::derive(move || if code_d.get() { String::new() } else { "next free code for the program".into() }) />
                <Field label="Name".into() ctl=Ctl::Text value=name wide=true />
                <Field label="Template".into() ctl=Ctl::Combo value=template choices=choices_sig(Entity::Project, "template_id", none) hint=tpl_hint />
                <Field label="Owner".into() ctl=Ctl::Combo value=owner choices=choices_sig(Entity::Project, "owner_id", none) />
                <Field label="Start".into() ctl=Ctl::Date value=start />
                <Field label="Due".into() ctl=Ctl::Date value=due on_edit=Callback::new(move |_| due_d.set(true))
                    hint=Signal::derive(move || if due_d.get() { String::new() } else { "start + template span".into() }) />
                <Field label="Budget h".into() ctl=Ctl::Number value=budget on_edit=Callback::new(move |_| budget_d.set(true))
                    hint=Signal::derive(move || if budget_d.get() { String::new() } else { "template estimate".into() }) />
            </div>
            <Show when=move || err.get().is_some()><div class="dlg-err">"✗ "{move || err.get()}</div></Show>
            <div class="dlg-f">
                <span class="muted">"Tab moves · ^S creates · Esc cancels"</span>
                <button class="btn" on:click=move |_| close(app)>"Cancel"</button>
                <button class="btn primary" disabled=move || busy.get() on:click=move |_| save.run(false)>"Create  ^S"</button>
            </div>
        </Dialog>
    }
}

#[component]
fn NewTask(project: Option<i64>, status: Option<TaskStatus>) -> impl IntoView {
    let app = use_app();
    let prog0 = project.and_then(|p| {
        app.rep
            .with_untracked(|r| r.projects.get(&p).and_then(|p| p.program_id))
    });
    let program = RwSignal::new(prog0.map_or(Value::Null, Value::from));
    let proj = RwSignal::new(project.map_or(Value::Null, Value::from));
    let phase = RwSignal::new(Value::from(""));
    let title = RwSignal::new(Value::from(""));
    let assignee = RwSignal::new(Value::Null);
    let assignee_d = RwSignal::new(false);
    let st = RwSignal::new(Value::from(status.unwrap_or(TaskStatus::Todo).as_str()));
    let start = RwSignal::new(Value::Null);
    let due = RwSignal::new(Value::Null);
    let est = RwSignal::new(Value::Null);
    let err = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let pid = Signal::derive(move || proj.get().as_i64());
    // cascade: program filters projects
    let projects = Signal::derive(move || {
        let p = program.get().as_i64();
        app.rep.with(|r| {
            grid::choices(r, Entity::Task, "project_id", 0, None)
                .into_iter()
                .filter(|c| {
                    p.is_none()
                        || c.value
                            .as_i64()
                            .and_then(|id| r.projects.get(&id))
                            .is_some_and(|x| x.program_id == p)
                })
                .collect::<Vec<_>>()
        })
    });
    Effect::new(move |_| {
        let p = program.get().as_i64();
        let cur = proj.get_untracked().as_i64();
        let ok = cur.is_some_and(|c| {
            app.rep.with_untracked(|r| {
                r.projects
                    .get(&c)
                    .is_some_and(|x| p.is_none() || x.program_id == p)
            })
        });
        if !ok {
            proj.set(Value::Null);
        }
    });
    // cascade: project → default assignee (its owner) and phases
    Effect::new(move |_| {
        let p = pid.get();
        if !assignee_d.get_untracked() {
            let owner = p.and_then(|p| {
                app.rep
                    .with_untracked(|r| r.projects.get(&p).and_then(|x| x.owner_id))
            });
            assignee.set(owner.map_or(Value::Null, Value::from));
        }
    });
    let phases = Signal::derive(move || {
        let p = pid.get();
        app.rep
            .with(|r| grid::choices(r, Entity::Task, "phase", 0, p))
    });
    let none = Signal::derive(|| None);
    let save = Callback::new(move |_| {
        if busy.get_untracked() {
            return;
        }
        let Some(p) = pid.get_untracked() else {
            let _ = err.try_set(Some("Choose a project.".into()));
            return;
        };
        if text(&title).trim().is_empty() {
            let _ = err.try_set(Some("Give the task a title.".into()));
            return;
        }
        let e = Entity::Task;
        let mut values = Map::new();
        values.insert("project_id".into(), Value::from(p));
        let fields: [(&str, RwSignal<Value>, Ctl); 7] = [
            ("title", title, Ctl::Text),
            ("phase", phase, Ctl::Suggest),
            ("assignee_id", assignee, Ctl::Combo),
            ("status", st, Ctl::Combo),
            ("start_date", start, Ctl::Date),
            ("due_date", due, Ctl::Date),
            ("est_hours", est, Ctl::Number),
        ];
        for (f, v, c) in fields {
            match parsed(app, e, f, &v.get_untracked(), c) {
                Ok(val) => {
                    values.insert(f.into(), val);
                }
                Err(m) => {
                    let _ = err.try_set(Some(format!(
                        "{}: {m}",
                        field_label(&app.rep.read_untracked(), e, f)
                    )));
                    return;
                }
            }
        }
        busy.set(true);
        app.mutate(
            vec![Op::Create {
                entity: e,
                reference: Some("t".into()),
                values,
            }],
            move |r| {
                let _ = busy.try_set(false);
                if let Some(id) = r.created.get("t") {
                    let id = *id;
                    close(app);
                    app.when_present(Entity::Task, id, move || {
                        let label = app.rep.with_untracked(|r| {
                            r.tasks.get(&id).map(|t| r.task_ref(t)).unwrap_or_default()
                        });
                        app.toast(Level::Ok, format!("Created {label}."));
                        // put the cursor on it where it is visible
                        for g in [app.grids.tasks, app.grids.proj_tasks] {
                            g.cur_id.set(Some(id));
                            if let Some(i) = g.data_index_of(id) {
                                g.cur_row.set(i);
                            }
                        }
                        app.board.sel.set(Some(id));
                    });
                } else if let Some(e) = &r.error {
                    let _ = err.try_set(Some(e.message.clone()));
                }
            },
        );
    });
    autofocus(if project.is_some() {
        ".newtask .fld:nth-child(4) input"
    } else {
        ".newtask input"
    });
    view! {
        <Dialog title="New task".into() class="newtask form-dlg" on_save=save>
            <div class="form">
                <Field label="Program".into() ctl=Ctl::Combo value=program choices=choices_sig(Entity::Project, "program_id", none) />
                <Field label="Project".into() ctl=Ctl::Combo value=proj choices=projects
                    hint=Signal::derive(move || if program.get().is_null() { "all programs".to_string() } else { "filtered by program".to_string() }) />
                <Field label="Phase".into() ctl=Ctl::Suggest value=phase choices=phases
                    hint=Signal::derive(move || if pid.get().is_some() { "from the project's template, or type a new one".into() } else { String::new() }) />
                <Field label="Title".into() ctl=Ctl::Text value=title wide=true />
                <Field label="Assignee".into() ctl=Ctl::Combo value=assignee choices=choices_sig(Entity::Task, "assignee_id", none)
                    on_edit=Callback::new(move |_| assignee_d.set(true)) />
                <Field label="Status".into() ctl=Ctl::Combo value=st choices=choices_sig(Entity::Task, "status", none) />
                <Field label="Start".into() ctl=Ctl::Date value=start />
                <Field label="Due".into() ctl=Ctl::Date value=due />
                <Field label="Est h".into() ctl=Ctl::Number value=est />
            </div>
            <Show when=move || err.get().is_some()><div class="dlg-err">"✗ "{move || err.get()}</div></Show>
            <div class="dlg-f">
                <span class="muted">"Tab moves · ^S creates · Esc cancels"</span>
                <button class="btn" on:click=move |_| close(app)>"Cancel"</button>
                <button class="btn primary" disabled=move || busy.get() on:click=move |_| save.run(false)>"Create  ^S"</button>
            </div>
        </Dialog>
    }
}

/// One editable field of the record form.
#[derive(Clone)]
struct FormRow {
    field: String,
    label: String,
    ctl: Ctl,
    base: Value,
    value: RwSignal<Value>,
    conflict: RwSignal<Option<(String, Value)>>,
}

fn record_rows(app: App, e: Entity, id: i64) -> Vec<FormRow> {
    app.rep.with_untracked(|r| {
        let mut names: Vec<String> = fields::fields(e)
            .iter()
            .filter(|m| !m.create_only && m.kind != Kind::Password)
            .filter(|m| !(e == Entity::User && m.name == "username"))
            .map(|m| m.name.to_string())
            .collect();
        match e {
            Entity::Project => names.extend(
                r.custom_fields_for(CfEntity::Project)
                    .iter()
                    .map(|c| c.key()),
            ),
            Entity::Task => {
                names.extend(r.custom_fields_for(CfEntity::Task).iter().map(|c| c.key()))
            }
            _ => {}
        }
        names
            .into_iter()
            .map(|f| {
                let ctl = ctl_for(e, &f);
                let base = r.field_value(e, id, &f);
                let shown = match ctl {
                    Ctl::Text | Ctl::Number | Ctl::Date | Ctl::Area | Ctl::Suggest => {
                        Value::from(grid::edit_text(r, e, &f, id))
                    }
                    _ => base.clone(),
                };
                FormRow {
                    label: field_label(r, e, &f),
                    ctl,
                    base,
                    value: RwSignal::new(shown),
                    conflict: RwSignal::new(None),
                    field: f,
                }
            })
            .collect()
    })
}

#[component]
fn RecordForm(entity: Entity, id: i64) -> impl IntoView {
    let app = use_app();
    if !app.rep.with_untracked(|r| r.exists(entity, id)) {
        return view! { <Dialog title="Record".into()><p>"That record no longer exists."</p></Dialog> }.into_any();
    }
    let rows = record_rows(app, entity, id);
    let rows_s = StoredValue::new(rows.clone());
    let err = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let editable = app.rep.with_untracked(|r| {
        r.can_edit() && (matches!(entity, Entity::Project | Entity::Task) || r.is_admin())
    });
    let project_hint = app
        .rep
        .with_untracked(|r| r.tasks.get(&id).map(|t| t.project_id));
    let title = app.rep.with_untracked(|r| {
        let l = entity.label();
        let name = match entity {
            Entity::Project => r
                .projects
                .get(&id)
                .map(|p| format!("{} — {}", p.code, p.name)),
            Entity::Task => r
                .tasks
                .get(&id)
                .map(|t| format!("{} — {}", r.task_ref(t), t.title)),
            _ => None,
        }
        .unwrap_or_else(|| r.row_label(entity, id));
        format!("{}{} {name}", l[..1].to_uppercase(), &l[1..])
    });
    let save = Callback::new(move |force: bool| {
        if busy.get_untracked() || !editable {
            return;
        }
        let mut ops = Vec::new();
        let mut first_err = None;
        rows_s.with_value(|rows| {
            for row in rows {
                let v = match parsed(app, entity, &row.field, &row.value.get_untracked(), row.ctl) {
                    Ok(v) => v,
                    Err(m) => {
                        first_err.get_or_insert(format!("{}: {m}", row.label));
                        continue;
                    }
                };
                // a conflicting field: retry from their value only when forcing
                let conflict = row.conflict.get_untracked();
                let base = conflict
                    .as_ref()
                    .map(|(_, cur)| cur.clone())
                    .unwrap_or_else(|| row.base.clone());
                if values_equal(&base, &v) && conflict.is_none() {
                    continue;
                }
                if conflict.is_some() && !force {
                    continue;
                }
                ops.push(Op::Set {
                    entity,
                    id,
                    field: row.field.clone(),
                    base,
                    value: v,
                    force: conflict.is_some() && force,
                });
            }
        });
        if let Some(e) = first_err {
            let _ = err.try_set(Some(e));
            return;
        }
        if ops.is_empty() {
            close(app);
            return;
        }
        busy.set(true);
        app.mutate(ops.clone(), move |r| {
            let _ = busy.try_set(false);
            if !r.conflicts.is_empty() {
                // the form may have been closed before the reply arrived
                let _ = rows_s.try_with_value(|rows| {
                    for c in &r.conflicts {
                        if let Some(row) = rows.iter().find(|x| x.field == c.field) {
                            let who = c.by.map(|b| app.rep.with_untracked(|r| r.user_name(Some(b)).to_string())).unwrap_or_else(|| "Someone".into());
                            let shown = app.rep.with_untracked(|rp| grid::display_value(rp, entity, &c.field, &c.current));
                            let _ = row.conflict.try_set(Some((format!("{who} changed this to {shown}"), c.current.clone())));
                        }
                    }
                });
                let _ = err.try_set(Some("Some fields were changed by someone else (marked ⚠). Keep theirs, or press Ctrl+Shift+S to save yours anyway.".into()));
            } else if r.ok && r.error.is_none() {
                close(app);
            } else if let Some(e) = &r.error {
                let _ = err.try_set(Some(e.message.clone()));
            }
        });
    });
    let acts = app.rep.with_untracked(|r| {
        r.activity
            .values()
            .rev()
            .filter(|a| a.entity == entity.as_str() && a.entity_id == id)
            .take(6)
            .map(|a| {
                (
                    grid::activity_when(a, util::now_secs()),
                    r.user_name(a.user_id).to_string(),
                    a.label.clone(),
                )
            })
            .collect::<Vec<_>>()
    });
    autofocus(".record input, .record textarea");
    view! {
        <Dialog title=title class="record form-dlg" on_save=save>
            {(!editable).then(|| view! { <p class="lock">"🔒 Read-only for your role."</p> })}
            <div class="form">
                {rows.into_iter().map(|row| {
                    let FormRow { field, label, ctl, value, conflict, .. } = row;
                    let fc = field.clone();
                    let choices = if matches!(ctl, Ctl::Combo | Ctl::Suggest | Ctl::Chips) {
                        let ph = project_hint;
                        Signal::derive(move || app.rep.with(|r| {
                            let mut c = grid::choices(r, entity, &fc, id, ph);
                            if entity == Entity::Task && fc == "deps" {
                                c.retain(|x| x.value.as_i64() != Some(id));
                            }
                            c
                        }))
                    } else {
                        Signal::derive(Vec::new)
                    };
                    let wide = matches!(ctl, Ctl::Area | Ctl::Chips) || field == "name" || field == "title";
                    let errs = Signal::derive(move || conflict.get().map(|(m, _)| format!("⚠ {m}")));
                    let fk = field.clone();
                    let keep = move |_: web_sys::MouseEvent| {
                        if let Some((_, cur)) = conflict.get_untracked() {
                            let shown = app.rep.with_untracked(|r| match ctl {
                                Ctl::Text | Ctl::Number | Ctl::Date | Ctl::Area | Ctl::Suggest => { let _ = (r, &fk); Value::from(value_text(&cur)) },
                                _ => cur.clone(),
                            });
                            value.set(if cur.is_null() && matches!(ctl, Ctl::Text | Ctl::Number | Ctl::Date | Ctl::Area) { Value::from("") } else { shown });
                            conflict.set(None);
                        }
                    };
                    view! {
                        <div class="fwrap" class:wide=wide>
                            <Field label=label ctl=ctl value=value choices=choices err=errs wide=wide />
                            <Show when=move || conflict.get().is_some()>
                                <button class="btn sm" on:click=keep.clone()>"Keep theirs"</button>
                            </Show>
                        </div>
                    }
                }).collect_view()}
            </div>
            {(!acts.is_empty()).then(|| view! {
                <div class="recact">
                    <h4>"Recent changes"</h4>
                    {acts.into_iter().map(|(w, u, l)| view! { <div class="actline"><span class="muted">{w}</span>" "<b>{u}</b>" "{l}</div> }).collect_view()}
                </div>
            })}
            <Show when=move || err.get().is_some()><div class="dlg-err">{move || err.get()}</div></Show>
            <div class="dlg-f">
                <span class="muted">"^S saves changed fields · Esc cancels"</span>
                <button class="btn" on:click=move |_| close(app)>"Cancel"</button>
                <button class="btn primary" disabled=move || busy.get() || !editable on:click=move |_| save.run(false)>"Save  ^S"</button>
            </div>
        </Dialog>
    }
    .into_any()
}

/// Generic create form for admin entities and users (fields from pt-core metadata).
#[component]
fn CreateForm(entity: Entity, preset: Vec<(String, Value)>) -> impl IntoView {
    let app = use_app();
    let names: Vec<&'static str> = match entity {
        Entity::User => vec!["username", "display_name", "role", "password"],
        Entity::Program => vec!["name", "client", "prefix", "color"],
        Entity::Template => vec!["name", "description"],
        Entity::TemplateTask => vec![
            "title",
            "phase",
            "est_hours",
            "offset_days",
            "duration_days",
        ],
        Entity::CustomField => vec!["entity", "name", "kind", "options"],
        _ => fields::fields(entity)
            .iter()
            .filter(|m| m.required)
            .map(|m| m.name)
            .collect(),
    };
    let defaults = |f: &str| -> Value {
        match (entity, f) {
            (Entity::User, "role") => Value::from("member"),
            (Entity::Program, "color") => Value::from("#5aa9ff"),
            (Entity::TemplateTask, "duration_days") => Value::from("5"),
            (Entity::TemplateTask, "offset_days") => Value::from("0"),
            (Entity::CustomField, "entity") => Value::from("task"),
            (Entity::CustomField, "kind") => Value::from("text"),
            _ => Value::from(""),
        }
    };
    let rows: Vec<(&'static str, String, Ctl, RwSignal<Value>)> = names
        .iter()
        .map(|f| {
            let label = fields::field(entity, f).map_or(f.to_string(), |m| m.label.to_string());
            let mut ctl = ctl_for(entity, f);
            if *f == "phase" {
                ctl = Ctl::Suggest;
            }
            let init = if matches!(ctl, Ctl::Chips) {
                Value::Array(vec![])
            } else {
                defaults(f)
            };
            (*f, label, ctl, RwSignal::new(init))
        })
        .collect();
    let rows_s = StoredValue::new(rows.clone());
    let preset = StoredValue::new(preset);
    let err = RwSignal::new(None::<String>);
    let save = Callback::new(move |_| {
        let mut values = Map::new();
        for (k, v) in preset.get_value() {
            values.insert(k, v);
        }
        let mut bad = None;
        rows_s.with_value(|rows| {
            for (f, label, ctl, v) in rows {
                let raw = v.get_untracked();
                let val = match (ctl, f) {
                    // ints: parse_input handles them; password is plain text
                    (Ctl::Password, _) => Ok(raw.clone()),
                    _ => parsed(app, entity, f, &raw, *ctl),
                };
                match val {
                    Ok(Value::String(s)) if s.is_empty() && !matches!(*ctl, Ctl::Password) => {}
                    Ok(x) => {
                        values.insert(f.to_string(), x);
                    }
                    Err(m) => {
                        bad.get_or_insert(format!("{label}: {m}"));
                    }
                }
            }
        });
        if let Some(b) = bad {
            let _ = err.try_set(Some(b));
            return;
        }
        app.mutate(
            vec![Op::Create {
                entity,
                reference: Some("n".into()),
                values,
            }],
            move |r| {
                if r.created.contains_key("n") {
                    close(app);
                    app.toast(Level::Ok, format!("Created {}.", entity.label()));
                } else if let Some(e) = &r.error {
                    let _ = err.try_set(Some(e.message.clone()));
                }
            },
        );
    });
    autofocus(".createdlg input");
    let none = Signal::derive(|| None);
    view! {
        <Dialog title=format!("New {}", entity.label()) class="createdlg form-dlg" on_save=save>
            <div class="form">
                {rows.into_iter().map(|(f, label, ctl, v)| {
                    let choices = choices_sig(entity, f, none);
                    view! { <Field label=label ctl=ctl value=v choices=choices wide=matches!(ctl, Ctl::Chips) /> }
                }).collect_view()}
            </div>
            <Show when=move || err.get().is_some()><div class="dlg-err">"✗ "{move || err.get()}</div></Show>
            <div class="dlg-f">
                <button class="btn" on:click=move |_| close(app)>"Cancel"</button>
                <button class="btn primary" on:click=move |_| save.run(false)>"Create  ^S"</button>
            </div>
        </Dialog>
    }
}
