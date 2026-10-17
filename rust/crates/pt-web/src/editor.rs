//! In-cell editors (ui-spec §3 "Cell editors"): text, number, date with
//! calendar, select, fuzzy combobox, suggest, chips and notes.

use leptos::prelude::*;
use pt_core::date::parse_natural;
use pt_core::fields::{self, Kind};
use pt_core::grid::{self, Choice, Editor, filter_choices};
use pt_core::model::Entity;
use pt_core::protocol::Op;
use serde_json::Value;

use crate::app::{EditState, Grid, Level, use_app};
use crate::gridops::{self, Nav};
use crate::widgets::{Calendar, ChoiceList, popup_style};

fn nav_of(e: &web_sys::KeyboardEvent) -> Option<Nav> {
    match e.key().as_str() {
        "Enter" => Some(Nav::Down),
        "Tab" if e.shift_key() => Some(Nav::Left),
        "Tab" => Some(Nav::Right),
        _ => None,
    }
}

fn focus_input(r: NodeRef<leptos::html::Input>, select_all: bool) {
    request_animation_frame(move || {
        if let Some(i) = r.get() {
            let _ = i.focus();
            let n = i.value().chars().count() as u32;
            if select_all {
                i.select();
            } else {
                let _ = i.set_selection_range(n, n);
            }
        }
    });
}

#[component]
pub fn CellEditor(g: Grid, st: EditState) -> impl IntoView {
    match st.editor {
        Editor::Select | Editor::Combo | Editor::Bool | Editor::Suggest => {
            view! { <ChoiceEditor g=g st=st /> }.into_any()
        }
        Editor::Chips => view! { <ChipsEditor g=g st=st /> }.into_any(),
        Editor::Multiline => view! { <NotesEditor g=g st=st /> }.into_any(),
        _ => view! { <TextEditor g=g st=st /> }.into_any(),
    }
}

/// Text, number and date editors (date adds a preview and a calendar).
#[component]
fn TextEditor(g: Grid, st: EditState) -> impl IntoView {
    let app = use_app();
    let initial = st.seed.clone().unwrap_or_else(|| {
        app.rep
            .with_untracked(|r| grid::edit_text(r, g.entity, &st.field, st.id))
    });
    let select_all = st.seed.is_none();
    let text = RwSignal::new(initial);
    let err = RwSignal::new(None::<String>);
    let input = NodeRef::<leptos::html::Input>::new();
    focus_input(input, select_all);
    let is_date = st.editor == Editor::Date;
    let is_num = matches!(st.editor, Editor::Number | Editor::Int);
    let field = st.field.clone();
    let today = app.rep.with_untracked(|r| r.today);
    let parsed_date = Signal::derive(move || parse_natural(&text.get(), today).ok().flatten());
    let id = st.id;
    let parse = {
        let field = field.clone();
        move || {
            app.rep.with_untracked(|r| {
                grid::parse_input(r, g.entity, &field, id, &text.get_untracked())
            })
        }
    };
    let parse2 = parse.clone();
    let commit = move |nav: Nav| match parse() {
        Ok(v) => gridops::commit(app, g, v, nav),
        Err(e) => {
            err.set(Some(e.clone()));
            app.say(Level::Warn, e);
        }
    };
    let commit2 = commit.clone();
    let on_key = move |e: web_sys::KeyboardEvent| {
        if let Some(nav) = nav_of(&e) {
            e.prevent_default();
            e.stop_propagation();
            commit(nav);
            return;
        }
        let k = e.key();
        let ctrl = crate::util::ctrl(&e);
        let step = |days: i32| {
            let d = parsed_date.get_untracked().unwrap_or(today).add_days(days);
            text.set(d.iso());
        };
        match k.as_str() {
            "Escape" => {
                e.stop_propagation();
                gridops::cancel_edit(app, g);
            }
            "ArrowUp" | "ArrowDown" if is_date => {
                e.prevent_default();
                step(if k == "ArrowUp" { -7 } else { 7 });
            }
            "PageUp" | "PageDown" if is_date => {
                e.prevent_default();
                let d = parsed_date
                    .get_untracked()
                    .unwrap_or(today)
                    .add_months(if k == "PageUp" { -1 } else { 1 });
                text.set(d.iso());
            }
            "ArrowLeft" | "ArrowRight" if is_date && ctrl => {
                e.prevent_default();
                step(if k == "ArrowLeft" { -1 } else { 1 });
            }
            "ArrowUp" | "ArrowDown" if is_num => {
                e.prevent_default();
                let delta = if e.shift_key() { 0.25 } else { 1.0 }
                    * if k == "ArrowUp" { 1.0 } else { -1.0 };
                let cur: f64 = text
                    .get_untracked()
                    .trim()
                    .trim_end_matches('h')
                    .parse()
                    .unwrap_or(0.0);
                text.set(grid::fmt_hours((cur + delta).max(0.0)));
            }
            _ => {}
        }
        e.stop_propagation();
    };
    let rect = st.rect;
    let on_blur = move |_| {
        // blur also fires while the editor is being removed (view switch);
        // its signals may already be gone then
        if text.is_disposed() {
            g.edit.set(None);
            return;
        }
        // clicking elsewhere commits a valid value, otherwise cancels
        if g.edit.get_untracked().is_some() {
            match parse2() {
                Ok(v) => gridops::commit(app, g, v, Nav::Stay),
                Err(_) => {
                    g.edit.set(None);
                    app.ctx.set(crate::app::Ctx::Grid);
                }
            }
        }
    };
    let pick = Callback::new(move |d: pt_core::Date| {
        text.set(d.iso());
        commit2(Nav::Stay);
    });
    view! {
        <input class="ed" class:bad=move || err.get().is_some() node_ref=input
            prop:value=move || text.get()
            on:input=move |e| { text.set(event_target_value(&e)); err.set(None); }
            on:keydown=on_key on:blur=on_blur
            title=move || err.get().unwrap_or_default() />
        {is_date.then(|| view! {
            <div class="popup datepop" style=popup_style(rect, 230.0, 260.0)>
                <div class="preview">{move || {
                    let t = text.get();
                    match parse_natural(&t, today) {
                        Ok(Some(d)) => format!("→ {}", d.long()),
                        Ok(None) => "(empty)".to_string(),
                        Err(e) => e,
                    }
                }}</div>
                <Calendar cur=parsed_date today=today on_pick=pick />
                <div class="cal-foot">
                    <button class="btn sm" on:mousedown=move |e| { e.prevent_default(); text.set(today.iso()); }>"t Today"</button>
                    <span class="muted">"↑↓ week · PgUp/PgDn month · ^←/→ day"</span>
                </div>
            </div>
        })}
    }
}

/// Select (enums), combobox (references) and suggest (phase) editors.
#[component]
fn ChoiceEditor(g: Grid, st: EditState) -> impl IntoView {
    let app = use_app();
    let field = st.field.clone();
    let id = st.id;
    let choices: Vec<Choice> = app
        .rep
        .with_untracked(|r| grid::choices(r, g.entity, &field, id, None));
    let free = st.editor == Editor::Suggest;
    let can_create_program = field == "program_id" && app.is_admin();
    let choices = Signal::derive({
        let c = choices.clone();
        move || c.clone()
    });
    let filter = RwSignal::new(st.seed.clone().unwrap_or_default());
    let items = Signal::derive(move || filter_choices(&choices.get(), &filter.get()));
    let current = st.base.clone();
    let sel = RwSignal::new(0usize);
    if st.seed.is_none() {
        let cur_pos = items.with_untracked(|its| {
            its.iter()
                .position(|(i, _)| choices.with_untracked(|c| c[*i].value == current))
        });
        sel.set(cur_pos.unwrap_or(0));
    }
    Effect::new(move |prev: Option<String>| {
        let f = filter.get();
        if prev.is_some_and(|p| p != f) {
            sel.set(0);
        }
        f
    });
    let extra = Signal::derive(move || {
        let f = filter.get();
        let f = f.trim();
        if f.is_empty() {
            return None;
        }
        let exact = choices.with(|c| c.iter().any(|c| c.label.eq_ignore_ascii_case(f)));
        if exact {
            None
        } else if free {
            Some(format!("Use “{f}”"))
        } else if can_create_program {
            Some(format!("Create program “{f}”"))
        } else {
            None
        }
    });
    let input = NodeRef::<leptos::html::Input>::new();
    focus_input(input, false);
    let base = st.base.clone();
    let field2 = field.clone();
    let pick = move |pos: usize, nav: Nav| {
        let n = items.with_untracked(|i| i.len());
        if pos < n {
            let v = items.with_untracked(|i| choices.with_untracked(|c| c[i[pos].0].value.clone()));
            gridops::commit(app, g, v, nav);
        } else if extra.get_untracked().is_some() {
            let text = filter.get_untracked().trim().to_string();
            if free {
                gridops::commit(app, g, Value::from(text), nav);
            } else if can_create_program {
                // create the program, then point the project at it
                g.edit.set(None);
                let mut values = serde_json::Map::new();
                values.insert("name".into(), Value::from(text.clone()));
                let base = base.clone();
                let field = field2.clone();
                app.mutate(
                    vec![Op::Create {
                        entity: Entity::Program,
                        reference: Some("p".into()),
                        values,
                    }],
                    move |r| {
                        if let Some(pid) = r.created.get("p") {
                            app.mutate(
                                vec![Op::set(g.entity, id, &field, base, Value::from(*pid))],
                                |_| {},
                            );
                        }
                    },
                );
                gridops::focus_grid(g);
            }
        } else if free && !filter.get_untracked().trim().is_empty() {
            gridops::commit(
                app,
                g,
                Value::from(filter.get_untracked().trim().to_string()),
                nav,
            );
        }
    };
    let pick2 = pick.clone();
    let on_key = move |e: web_sys::KeyboardEvent| {
        let k = e.key();
        let n = items.with_untracked(|i| i.len()) + usize::from(extra.get_untracked().is_some());
        match k.as_str() {
            "ArrowDown" => sel.update(|s| *s = (*s + 1).min(n.saturating_sub(1))),
            "ArrowUp" => sel.update(|s| *s = s.saturating_sub(1)),
            "PageDown" => sel.update(|s| *s = (*s + 8).min(n.saturating_sub(1))),
            "PageUp" => sel.update(|s| *s = s.saturating_sub(8)),
            "Escape" => gridops::cancel_edit(app, g),
            _ => {
                if let Some(nav) = nav_of(&e) {
                    if n == 0 && !free {
                        app.say(Level::Warn, "No matching choice.");
                    } else {
                        pick(sel.get_untracked(), nav);
                    }
                } else {
                    e.stop_propagation();
                    return;
                }
            }
        }
        e.prevent_default();
        e.stop_propagation();
    };
    let on_pick = Callback::new(move |pos: usize| pick2(pos, Nav::Stay));
    let placeholder = app
        .rep
        .with_untracked(|r| grid::display_value(r, g.entity, &field, &st.base));
    let rect = st.rect;
    view! {
        <input class="ed" node_ref=input placeholder=placeholder
            prop:value=move || filter.get()
            on:input=move |e| filter.set(event_target_value(&e))
            on:keydown=on_key
            on:blur=move |_| { if g.edit.get_untracked().is_some() { g.edit.set(None); app.ctx.set(crate::app::Ctx::Grid); } } />
        <ChoiceList choices=choices items=items sel=sel on_pick=on_pick
            style=Signal::derive(move || format!("{};position:fixed", popup_style(rect, 200.0, 300.0)))
            extra=extra />
    }
}

fn chip_label(app: crate::app::App, field: &str, v: &Value) -> String {
    app.rep.with_untracked(|r| match (field, v) {
        ("deps", Value::Number(n)) => n
            .as_i64()
            .and_then(|id| r.tasks.get(&id))
            .map_or(n.to_string(), |t| format!("#{} {}", t.seq, t.title)),
        (_, Value::String(s)) => s.clone(),
        (_, other) => other.to_string(),
    })
}

/// Tags, dependencies and enum options as chips.
#[component]
fn ChipsEditor(g: Grid, st: EditState) -> impl IntoView {
    let app = use_app();
    let field = st.field.clone();
    let id = st.id;
    let all: Vec<Choice> = app
        .rep
        .with_untracked(|r| grid::choices(r, g.entity, &field, id, None));
    let values = RwSignal::new(st.base.as_array().cloned().unwrap_or_default());
    let filter = RwSignal::new(st.seed.clone().unwrap_or_default());
    let choices = Signal::derive(move || {
        let vals = values.get();
        all.iter()
            .filter(|c| !vals.contains(&c.value))
            .cloned()
            .collect::<Vec<_>>()
    });
    let items = Signal::derive(move || filter_choices(&choices.get(), &filter.get()));
    let sel = RwSignal::new(0usize);
    let creatable = matches!(
        fields::field(g.entity, &field).map(|m| m.kind),
        Some(Kind::Tags | Kind::Options)
    );
    let extra = Signal::derive(move || {
        let f = filter.get().trim().to_string();
        if f.is_empty() || !creatable {
            return None;
        }
        let exact = choices.with(|c| c.iter().any(|c| c.label.eq_ignore_ascii_case(&f)));
        (!exact).then(|| format!("Add “{f}”"))
    });
    let input = NodeRef::<leptos::html::Input>::new();
    focus_input(input, false);
    let is_tags = matches!(
        fields::field(g.entity, &field).map(|m| m.kind),
        Some(Kind::Tags)
    );
    let add = move |pos: usize| {
        let n = items.with_untracked(|i| i.len());
        if pos < n {
            let v = items.with_untracked(|i| choices.with_untracked(|c| c[i[pos].0].value.clone()));
            values.update(|vs| vs.push(v));
        } else if extra.get_untracked().is_some() {
            let mut t = filter.get_untracked().trim().to_string();
            if is_tags {
                t = t.trim_start_matches('#').to_lowercase();
                if !fields::valid_tag(&t) {
                    app.say(Level::Warn, "Tags use a–z, 0–9, '_' and '-'.");
                    return;
                }
            }
            values.update(|vs| vs.push(Value::from(t)));
        }
        filter.set(String::new());
        sel.set(0);
    };
    let add2 = add;
    let commit = move |nav: Nav| gridops::commit(app, g, Value::Array(values.get_untracked()), nav);
    let on_key = move |e: web_sys::KeyboardEvent| {
        let k = e.key();
        let n = items.with_untracked(|i| i.len()) + usize::from(extra.get_untracked().is_some());
        let empty = filter.with_untracked(|f| f.is_empty());
        match k.as_str() {
            "ArrowDown" => sel.update(|s| *s = (*s + 1).min(n.saturating_sub(1))),
            "ArrowUp" => sel.update(|s| *s = s.saturating_sub(1)),
            "Escape" => gridops::cancel_edit(app, g),
            "Backspace" if empty => values.update(|v| {
                v.pop();
            }),
            "Enter" if !empty && !crate::util::ctrl(&e) => add(sel.get_untracked()),
            "Enter" => commit(Nav::Down),
            "Tab" => commit(if e.shift_key() { Nav::Left } else { Nav::Right }),
            _ => {
                e.stop_propagation();
                return;
            }
        }
        e.prevent_default();
        e.stop_propagation();
    };
    let on_pick = Callback::new(move |pos: usize| add2(pos));
    let rect = st.rect;
    let field_c = field.clone();
    view! {
        <div class="popup chipsed" style=popup_style(rect, 280.0, 340.0)>
            <div class="chips">
                {move || values.get().into_iter().enumerate().map(|(i, v)| {
                    let label = chip_label(app, &field_c, &v);
                    view! {
                        <span class="chipx">{label}
                            <b on:mousedown=move |e| { e.prevent_default(); values.update(|vs| { vs.remove(i); }); }>"×"</b>
                        </span>
                    }
                }).collect_view()}
                <input class="chipin" node_ref=input placeholder="type to add…"
                    prop:value=move || filter.get()
                    on:input=move |e| { filter.set(event_target_value(&e)); sel.set(0); }
                    on:keydown=on_key
                    on:blur=move |_| { if g.edit.get_untracked().is_some() { g.edit.set(None); app.ctx.set(crate::app::Ctx::Grid); } } />
            </div>
            <ChoiceList choices=choices items=items sel=sel on_pick=on_pick extra=extra />
            <div class="cal-foot muted">"Enter add · Backspace remove · Enter on empty / Tab save · Esc cancel"</div>
        </div>
    }
}

/// Multi-line notes in a popup. Ctrl+Enter or Tab saves.
#[component]
fn NotesEditor(g: Grid, st: EditState) -> impl IntoView {
    let app = use_app();
    let text = RwSignal::new(st.seed.clone().unwrap_or_else(|| {
        app.rep
            .with_untracked(|r| grid::edit_text(r, g.entity, &st.field, st.id))
    }));
    let area = NodeRef::<leptos::html::Textarea>::new();
    request_animation_frame(move || {
        if let Some(a) = area.get() {
            let _ = a.focus();
        }
    });
    let commit = move |nav| gridops::commit(app, g, Value::from(text.get_untracked()), nav);
    let rect = st.rect;
    view! {
        <div class="popup notesed" style=popup_style(rect, 420.0, 240.0)>
            <textarea node_ref=area prop:value=move || text.get() on:input=move |e| text.set(event_target_value(&e))
                on:keydown=move |e: web_sys::KeyboardEvent| {
                    match e.key().as_str() {
                        "Escape" => gridops::cancel_edit(app, g),
                        "Enter" if crate::util::ctrl(&e) => commit(Nav::Down),
                        "Tab" => commit(if e.shift_key() { Nav::Left } else { Nav::Right }),
                        _ => { e.stop_propagation(); return; }
                    }
                    e.prevent_default();
                    e.stop_propagation();
                }></textarea>
            <div class="cal-foot muted">"Ctrl+Enter or Tab saves · Esc cancels"</div>
        </div>
    }
}
