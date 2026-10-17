//! Form controls for dialogs: text, number, notes, date (natural language +
//! calendar), fuzzy combobox (with optional free text), chips, checkbox.
//! Every control writes a `serde_json::Value` into its signal; text-like
//! controls store the raw text and the dialog parses it on save.

use leptos::prelude::*;
use pt_core::date::parse_natural;
use pt_core::fields::{self, Kind};
use pt_core::grid::{Choice, filter_choices};
use pt_core::model::{CfKind, Entity};
use serde_json::Value;

use crate::app::use_app;
use crate::widgets::{Calendar, ChoiceList};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ctl {
    Text,
    Password,
    Area,
    Number,
    Date,
    Combo,
    Suggest,
    Chips,
    Check,
}

/// Control for a built-in field.
pub fn ctl_for(e: Entity, field: &str) -> Ctl {
    if let Some(id) = fields::cf_id(field) {
        let kind = use_app()
            .rep
            .with_untracked(|r| r.custom_fields.get(&id).map(|c| c.kind));
        return match kind {
            Some(CfKind::Number) => Ctl::Number,
            Some(CfKind::Date) => Ctl::Date,
            Some(CfKind::Enum) => Ctl::Combo,
            _ => Ctl::Text,
        };
    }
    match fields::field(e, field).map(|m| m.kind) {
        Some(Kind::Text { max, .. }) if max > 2000 => Ctl::Area,
        Some(Kind::Text { .. }) if field == "phase" => Ctl::Suggest,
        Some(Kind::Enum(_) | Kind::Ref(_)) => Ctl::Combo,
        Some(Kind::Date) => Ctl::Date,
        Some(Kind::Hours | Kind::Int { .. }) => Ctl::Number,
        Some(Kind::Tags | Kind::Deps | Kind::Options) => Ctl::Chips,
        Some(Kind::Bool) => Ctl::Check,
        Some(Kind::Password) => Ctl::Password,
        _ => Ctl::Text,
    }
}

/// Raw text of a Value for text-like controls.
pub fn value_text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Number(n) => n.as_f64().map(pt_core::grid::fmt_hours).unwrap_or_default(),
        Value::Bool(b) => if *b { "yes" } else { "no" }.into(),
        other => other.to_string(),
    }
}

#[component]
pub fn Field(
    label: String,
    ctl: Ctl,
    value: RwSignal<Value>,
    #[prop(optional, into)] choices: Option<Signal<Vec<Choice>>>,
    #[prop(optional, into)] err: Option<Signal<Option<String>>>,
    #[prop(optional)] id: Option<String>,
    #[prop(optional)] wide: bool,
    #[prop(optional, into)] hint: Option<Signal<String>>,
    #[prop(optional)] on_edit: Option<Callback<()>>,
) -> impl IntoView {
    let choices = choices.unwrap_or_else(|| Signal::derive(Vec::new));
    let id = id.unwrap_or_default();
    let edited = move || {
        if let Some(cb) = on_edit {
            cb.run(());
        }
    };
    let control = match ctl {
        Ctl::Text | Ctl::Number | Ctl::Password => {
            let ty = if ctl == Ctl::Password { "password" } else { "text" };
            let num = ctl == Ctl::Number;
            view! {
                <input class="fin" class:num=num id=id.clone() type=ty prop:value=move || value_text(&value.get())
                    on:input=move |e| { value.set(Value::from(event_target_value(&e))); edited(); }
                    on:keydown=move |e: web_sys::KeyboardEvent| {
                        if num && (e.key() == "ArrowUp" || e.key() == "ArrowDown") {
                            e.prevent_default();
                            let cur: f64 = value_text(&value.get_untracked()).trim().parse().unwrap_or(0.0);
                            let d = if e.shift_key() { 0.25 } else { 1.0 } * if e.key() == "ArrowUp" { 1.0 } else { -1.0 };
                            value.set(Value::from(pt_core::grid::fmt_hours((cur + d).max(0.0))));
                            edited();
                        }
                    } />
            }
            .into_any()
        }
        Ctl::Area => view! {
            <textarea class="fin area" id=id.clone() prop:value=move || value_text(&value.get())
                on:input=move |e| { value.set(Value::from(event_target_value(&e))); edited(); }
                on:keydown=|e: web_sys::KeyboardEvent| { if e.key() == "Enter" { e.stop_propagation(); } }></textarea>
        }
        .into_any(),
        Ctl::Check => view! {
            <label class="fcheck">
                <input type="checkbox" id=id.clone() prop:checked=move || value.get() == Value::Bool(true)
                    on:change=move |e| { value.set(Value::Bool(event_target_checked(&e))); edited(); } />
                {move || if value.get() == Value::Bool(true) { "Yes" } else { "No" }}
            </label>
        }
        .into_any(),
        Ctl::Date => view! { <DateField value=value id=id.clone() on_edit=Callback::new(move |_| edited()) /> }.into_any(),
        Ctl::Combo | Ctl::Suggest => view! {
            <ComboField value=value choices=choices free=ctl == Ctl::Suggest id=id.clone() on_edit=Callback::new(move |_| edited()) />
        }
        .into_any(),
        Ctl::Chips => view! { <ChipsField value=value choices=choices id=id.clone() on_edit=Callback::new(move |_| edited()) /> }.into_any(),
    };
    view! {
        <div class="fld" class:wide=wide class:bad=move || err.is_some_and(|e| e.get().is_some())>
            <span class="flabel">{label}</span>
            {control}
            {move || hint.map(|h| h.get()).filter(|h| !h.is_empty()).map(|h| view! { <span class="fhint">{h}</span> })}
            {move || err.and_then(|e| e.get()).map(|m| view! { <span class="ferr">"✗ "{m}</span> })}
        </div>
    }
}

/// Combobox: shows the chosen label; typing filters (fuzzy); Enter/click picks.
#[component]
fn ComboField(
    value: RwSignal<Value>,
    choices: Signal<Vec<Choice>>,
    free: bool,
    id: String,
    on_edit: Callback<()>,
) -> impl IntoView {
    let text = RwSignal::new(String::new());
    let dirty = RwSignal::new(false);
    let open = RwSignal::new(false);
    let sel = RwSignal::new(0usize);
    let label_of = move |v: &Value| {
        if free {
            return value_text(v);
        }
        choices
            .with_untracked(|c| c.iter().find(|c| c.value == *v).map(|c| c.label.clone()))
            .unwrap_or_else(|| value_text(v))
    };
    // show the current label whenever not typing
    Effect::new(move |_| {
        let v = value.get();
        let _ = choices.get();
        if !dirty.get_untracked() {
            text.set(label_of(&v));
        }
    });
    let items = Signal::derive(move || {
        let f = if dirty.get() {
            text.get()
        } else {
            String::new()
        };
        filter_choices(&choices.get(), &f)
    });
    let pick = move |pos: usize| {
        if let Some(v) = items.with_untracked(|i| {
            i.get(pos)
                .map(|(i, _)| choices.with_untracked(|c| c[*i].value.clone()))
        }) {
            value.set(v.clone());
            dirty.set(false);
            text.set(label_of(&v));
            on_edit.run(());
        } else if free {
            value.set(Value::from(text.get_untracked().trim().to_string()));
            dirty.set(false);
            on_edit.run(());
        }
        open.set(false);
    };
    let on_key = move |e: web_sys::KeyboardEvent| {
        let n = items.with_untracked(|i| i.len());
        match e.key().as_str() {
            "ArrowDown" => {
                open.set(true);
                sel.update(|s| *s = (*s + 1).min(n.saturating_sub(1)));
            }
            "ArrowUp" => sel.update(|s| *s = s.saturating_sub(1)),
            "Enter" => {
                if open.get_untracked() {
                    pick(sel.get_untracked());
                } else {
                    open.set(true);
                }
            }
            "Tab" => {
                if dirty.get_untracked() {
                    pick(sel.get_untracked());
                }
                open.set(false);
                return;
            }
            "Escape" if open.get_untracked() => {
                dirty.set(false);
                text.set(label_of(&value.get_untracked()));
                open.set(false);
            }
            k => {
                // the first character typed over the shown label replaces it
                let printable = k.chars().count() == 1 && !crate::util::ctrl(&e) && !e.alt_key();
                if printable && !dirty.get_untracked() && !free {
                    use wasm_bindgen::JsCast;
                    if let Some(i) = e
                        .target()
                        .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                    {
                        i.set_value("");
                    }
                }
                return;
            }
        }
        e.prevent_default();
        e.stop_propagation();
    };
    let on_pick = Callback::new(move |pos: usize| pick(pos));
    view! {
        <div class="combo">
            <input class="fin" id=id prop:value=move || text.get() autocomplete="off"
                on:input=move |e| { text.set(event_target_value(&e)); dirty.set(true); open.set(true); sel.set(0); }
                on:focus=move |e| { open.set(true); if let Some(t) = e.target() { use wasm_bindgen::JsCast; if let Ok(i) = t.dyn_into::<web_sys::HtmlInputElement>() { i.select(); } } }
                on:blur=move |_| {
                    // also fires during removal, after the signals are disposed
                    if dirty.is_disposed() {
                        return;
                    }
                    if dirty.get_untracked() {
                        if free { value.set(Value::from(text.get_untracked().trim().to_string())); on_edit.run(()); }
                        dirty.set(false);
                        text.set(label_of(&value.get_untracked()));
                    }
                    open.set(false);
                }
                on:keydown=on_key />
            <span class="caret" on:mousedown=move |e| { e.prevent_default(); open.update(|o| *o = !*o); }>"▾"</span>
            <Show when=move || open.get()>
                <ChoiceList choices=choices items=items sel=sel on_pick=on_pick style=Signal::derive(|| "position:absolute;top:100%;left:0;right:0".to_string()) />
            </Show>
        </div>
    }
}

/// Date: natural-language text with a live preview and a calendar.
#[component]
fn DateField(value: RwSignal<Value>, id: String, on_edit: Callback<()>) -> impl IntoView {
    let app = use_app();
    let today = app.rep.with_untracked(|r| r.today);
    let open = RwSignal::new(false);
    let text = Signal::derive(move || value_text(&value.get()));
    let parsed = Signal::derive(move || parse_natural(&text.get(), today).ok().flatten());
    let set = move |d: pt_core::Date| {
        value.set(Value::from(d.iso()));
        on_edit.run(());
    };
    let on_key = move |e: web_sys::KeyboardEvent| {
        let base = parsed.get_untracked().unwrap_or(today);
        match e.key().as_str() {
            "ArrowUp" => set(base.add_days(-7)),
            "ArrowDown" => set(base.add_days(7)),
            "PageUp" => set(base.add_months(-1)),
            "PageDown" => set(base.add_months(1)),
            "ArrowLeft" if crate::util::ctrl(&e) => set(base.add_days(-1)),
            "ArrowRight" if crate::util::ctrl(&e) => set(base.add_days(1)),
            "Enter" => {
                if let Some(d) = parsed.get_untracked() {
                    set(d);
                }
                open.set(false);
            }
            _ => return,
        }
        e.prevent_default();
        e.stop_propagation();
    };
    view! {
        <div class="combo">
            <input class="fin" id=id prop:value=move || text.get() placeholder="e.g. fri, +2w, nov 3"
                on:input=move |e| { value.set(Value::from(event_target_value(&e))); on_edit.run(()); open.set(true); }
                on:focus=move |_| open.set(true)
                on:blur=move |_| {
                    if open.is_disposed() {
                        return;
                    }
                    // normalise to ISO when it parses
                    if let Some(d) = parsed.get_untracked() { value.set(Value::from(d.iso())); }
                    open.set(false);
                }
                on:keydown=on_key />
            <span class="dprev">{move || {
                let t = text.get();
                match parse_natural(&t, today) { Ok(Some(d)) => d.long(), Ok(None) => String::new(), Err(_) => "?".into() }
            }}</span>
            <Show when=move || open.get()>
                <div class="popup inline">
                    <Calendar cur=parsed today=today on_pick=Callback::new(move |d| { set(d); }) />
                    <div class="cal-foot muted">"↑↓ week · PgUp/Dn month · ^←/→ day"</div>
                </div>
            </Show>
        </div>
    }
}

#[component]
fn ChipsField(
    value: RwSignal<Value>,
    choices: Signal<Vec<Choice>>,
    id: String,
    on_edit: Callback<()>,
) -> impl IntoView {
    let filter = RwSignal::new(String::new());
    let open = RwSignal::new(false);
    let sel = RwSignal::new(0usize);
    let vals = move || value.get().as_array().cloned().unwrap_or_default();
    let avail = Signal::derive(move || {
        let v = vals();
        choices
            .get()
            .into_iter()
            .filter(|c| !v.contains(&c.value))
            .collect::<Vec<_>>()
    });
    let items = Signal::derive(move || filter_choices(&avail.get(), &filter.get()));
    let label = move |v: &Value| {
        choices
            .with_untracked(|c| c.iter().find(|c| c.value == *v).map(|c| c.label.clone()))
            .unwrap_or_else(|| value_text(v))
    };
    let add = move |pos: usize| {
        let picked = items.with_untracked(|i| {
            i.get(pos)
                .map(|(i, _)| avail.with_untracked(|a| a[*i].value.clone()))
        });
        let v = picked.or_else(|| {
            let t = filter
                .get_untracked()
                .trim()
                .trim_start_matches('#')
                .to_lowercase();
            (!t.is_empty()).then(|| Value::from(t))
        });
        if let Some(v) = v {
            value.update(|cur| {
                let mut a = cur.as_array().cloned().unwrap_or_default();
                a.push(v);
                *cur = Value::Array(a);
            });
            on_edit.run(());
        }
        filter.set(String::new());
        sel.set(0);
    };
    let on_key = move |e: web_sys::KeyboardEvent| {
        let n = items.with_untracked(|i| i.len());
        match e.key().as_str() {
            "ArrowDown" => sel.update(|s| *s = (*s + 1).min(n.saturating_sub(1))),
            "ArrowUp" => sel.update(|s| *s = s.saturating_sub(1)),
            "Enter" if !filter.with_untracked(|f| f.is_empty()) => add(sel.get_untracked()),
            "Backspace" if filter.with_untracked(|f| f.is_empty()) => {
                value.update(|cur| {
                    let mut a = cur.as_array().cloned().unwrap_or_default();
                    a.pop();
                    *cur = Value::Array(a);
                });
                on_edit.run(());
            }
            _ => return,
        }
        e.prevent_default();
        e.stop_propagation();
    };
    view! {
        <div class="combo">
            <div class="chips inline">
                {move || vals().into_iter().enumerate().map(|(i, v)| {
                    let l = label(&v);
                    view! { <span class="chipx">{l}<b on:mousedown=move |e| { e.prevent_default(); value.update(|cur| { if let Some(a) = cur.as_array_mut() { a.remove(i); } }); on_edit.run(()); }>"×"</b></span> }
                }).collect_view()}
                <input class="chipin" id=id prop:value=move || filter.get() placeholder="type to add…"
                    on:input=move |e| { filter.set(event_target_value(&e)); open.set(true); sel.set(0); }
                    on:focus=move |_| open.set(true) on:blur=move |_| { let _ = open.try_set(false); } on:keydown=on_key />
            </div>
            <Show when=move || open.get()>
                <ChoiceList choices=avail items=items sel=sel on_pick=Callback::new(add) style=Signal::derive(|| "position:absolute;top:100%;left:0;right:0".to_string()) />
            </Show>
        </div>
    }
}
