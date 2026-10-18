//! Tab 3, Date: natural-language date fields with a live preview and a
//! month calendar under the focused field.

use demo_core::Date;
use demo_core::date::{parse_natural, preview};
use demo_core::keys;
use leptos::prelude::*;
use serde_json::{Value, json};
use wasm_bindgen::JsCast;

use super::{Field, TabFrame};
use crate::app::{Ctx, use_app};
use crate::util;
use crate::widgets::Calendar;

fn to_value(d: Option<Date>) -> Value {
    d.map_or(Value::Null, |d| json!(d.iso()))
}

#[component]
fn DateField(key: &'static str, other: &'static str) -> impl IntoView {
    let app = use_app();
    let today = util::today();
    let synced = Memo::new(move |_| app.vals.with(|v| v.date(key)));
    let other_d = Signal::derive(move || app.vals.with(|v| v.date(other)));
    let draft = RwSignal::new(None::<String>);
    let base = StoredValue::new(0u64);
    // an empty field's calendar opens on the other date of the pair, else today
    let home = move || {
        synced
            .get_untracked()
            .or_else(|| app.vals.with_untracked(|v| v.date(other)))
            .unwrap_or(today)
    };
    let cursor = RwSignal::new(home());
    let focused = RwSignal::new(false);
    // follow the synced value while not editing
    Effect::new(move |_| {
        let _ = synced.get();
        if draft.with_untracked(|d| d.is_none()) {
            cursor.set(home());
        }
    });
    Effect::new(move |_| {
        if app.revert.with(|(k, _)| k == key) {
            draft.set(None);
            cursor.set(home());
        }
    });
    let start_draft = move |text: String| {
        if draft.with_untracked(|d| d.is_none()) {
            base.set_value(app.version(key));
        }
        draft.set(Some(text));
    };
    let shown = move || {
        draft
            .get()
            .unwrap_or_else(|| synced.get().map(|d| d.iso()).unwrap_or_default())
    };
    // the parsed draft, or the error to show (parse error or due < start)
    let parsed = move || -> Result<Option<Date>, String> {
        let Some(d) = draft.get() else {
            return Ok(synced.get());
        };
        let p = parse_natural(&d, today)?;
        keys::validate_with(key, &to_value(p), |k| {
            app.vals.with(|v| v.value(k).cloned())
        })
        .map_err(|e| e.to_string())?;
        Ok(p)
    };
    let prev = move || match draft.get() {
        Some(d) => preview(&d, today).unwrap_or_default(),
        None => synced
            .get()
            .map(|d| format!("→ {}", d.long()))
            .unwrap_or_else(|| "→ none".into()),
    };
    let commit = move |d: Option<Date>| {
        if app.commit(key, to_value(d), Some(base.get_value())).is_ok() {
            draft.set(None);
            if let Some(d) = d {
                cursor.set(d);
            }
        }
    };
    let move_cursor = move |f: &dyn Fn(Date) -> Date| {
        let c = f(cursor.get_untracked());
        cursor.set(c);
        start_draft(c.iso());
    };
    let on_key = move |e: web_sys::KeyboardEvent| {
        let ctrl = util::ctrl(&e);
        match e.key().as_str() {
            "ArrowDown" => move_cursor(&|c| c.add_days(7)),
            "ArrowUp" => move_cursor(&|c| c.add_days(-7)),
            "ArrowRight" if ctrl => move_cursor(&|c| c.add_days(1)),
            "ArrowLeft" if ctrl => move_cursor(&|c| c.add_days(-1)),
            "PageDown" => move_cursor(&|c| c.add_months(1)),
            "PageUp" => move_cursor(&|c| c.add_months(-1)),
            "t" | "T" if ctrl => commit(Some(today)),
            "Enter" => match draft.get_untracked() {
                Some(d) => {
                    if let Ok(p) = parse_natural(&d, today) {
                        commit(p);
                    }
                }
                None => commit(Some(cursor.get_untracked())),
            },
            "Delete" if draft.with_untracked(|d| d.is_none()) => {
                base.set_value(app.version(key));
                commit(None);
            }
            "Escape" if draft.with_untracked(|d| d.is_some()) => {
                draft.set(None);
                cursor.set(home());
            }
            _ => return,
        }
        e.prevent_default();
    };
    let ctl = key;
    view! {
        <div class="datef">
            <div class="tin">
                <input class="fin date" class:bad=move || parsed().is_err() class:dirty=move || draft.with(|d| d.is_some())
                    data-control=ctl spellcheck="false" autocomplete="off" placeholder="fri, +3d, 23 oct…" prop:value=shown
                    on:input=move |e| start_draft(event_target_value(&e))
                    on:focus=move |e| {
                        focused.set(true);
                        app.ctx.set(Ctx::Body("date"));
                        // select all, so typing replaces the date
                        if let Some(i) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok()) { i.select(); }
                    }
                    on:blur=move |_| { let _ = focused.try_set(false); }
                    on:keydown=on_key />
                <button class="btn sm" tabindex="-1" title="Today (^T where the browser allows)" on:mousedown=|e| e.prevent_default()
                    on:click=move |_| { base.set_value(app.version(key)); commit(Some(today)); }>"Today"</button>
                <span class=move || if parsed().is_err() { "ferr" } else { "dprev2" }>
                    {move || match parsed() { Err(m) => format!("✗ {m}"), Ok(_) => prev() }}
                </span>
            </div>
            <Show when=move || focused.get()>
                <Calendar cursor=cursor.into() value=synced.into() other=other_d today=today
                    on_pick=Callback::new(move |d: Date| { base.set_value(app.version(key)); commit(Some(d)); }) />
            </Show>
        </div>
    }
}

#[component]
pub fn DateTab() -> impl IntoView {
    view! {
        <TabFrame id="date">
            <Field label="Start" key="date.start"><DateField key="date.start" other="date.due" /></Field>
            <Field label="Due" key="date.due"><DateField key="date.due" other="date.start" /></Field>
            <p class="fhint wide">"Try: today · tom · fri · +3d · -2w · +1m · eow · eom · 23 oct · 2026-11-02 · empty = no date"</p>
        </TabFrame>
    }
}
