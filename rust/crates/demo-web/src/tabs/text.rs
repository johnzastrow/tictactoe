//! Tab 1, Text: three synced single-line inputs with live validation and a
//! local-only masked password field.

use demo_core::keys;
use leptos::prelude::*;
use serde_json::{Value, json};

use super::{Field, TabFrame};
use crate::app::{Ctx, use_app};
use crate::util;

/// A single-line input bound to a string key. Typing makes a draft (the
/// synced value stays untouched); Enter commits it with the version shown
/// when editing began; Esc reverts. Invalid drafts are red and not sent.
#[component]
pub fn SyncedInput(
    key: &'static str,
    #[prop(optional)] class: &'static str,
    #[prop(optional)] placeholder: &'static str,
) -> impl IntoView {
    let app = use_app();
    let synced = app.watch(key);
    let draft = RwSignal::new(None::<String>);
    let base = StoredValue::new(0u64);
    // a conflict or reset for this key drops the draft
    Effect::new(move |_| {
        if app.revert.with(|(k, _)| k == key) {
            draft.set(None);
        }
    });
    let shown = move || {
        draft
            .get()
            .unwrap_or_else(|| synced.with(|v| v.as_str().unwrap_or("").to_string()))
    };
    let error = move || {
        let d = draft.get()?;
        keys::validate(key, &json!(d)).err().map(|e| e.to_string())
    };
    // someone else changed the key while we hold a draft
    let changed_by = move || {
        draft.with(|d| d.is_some()).then_some(())?;
        let _ = synced.get();
        (app.vals.with(|v| v.version(key)) > base.get_value()).then(|| app.by(key))
    };
    let on_key = move |e: web_sys::KeyboardEvent| {
        match e.key().as_str() {
            "Enter" => {
                if let Some(d) = draft.get_untracked() {
                    if keys::validate(key, &Value::String(d.clone())).is_ok()
                        && app
                            .commit(key, Value::String(d), Some(base.get_value()))
                            .is_ok()
                    {
                        draft.set(None);
                    }
                }
            }
            "Escape" if draft.with_untracked(|d| d.is_some()) => draft.set(None),
            "u" | "U" if util::ctrl(&e) => {
                if draft.with_untracked(|d| d.is_none()) {
                    base.set_value(app.version(key));
                }
                draft.set(Some(String::new()));
            }
            _ => return,
        }
        e.prevent_default();
    };
    view! {
        <div class="tin">
            <input class=format!("fin {class}") class:bad=move || error().is_some() class:dirty=move || draft.with(|d| d.is_some())
                data-control=key spellcheck="false" autocomplete="off" placeholder=placeholder
                prop:value=shown
                on:input=move |e| {
                    if draft.with_untracked(|d| d.is_none()) { base.set_value(app.version(key)); }
                    draft.set(Some(event_target_value(&e)));
                }
                on:focus=move |_| app.ctx.set(Ctx::Body("text"))
                on:keydown=on_key />
            {move || error().map(|m| view! { <span class="ferr">"✗ "{m}</span> })}
            {move || changed_by().map(|b| view! { <span class="fwarn">"⚠ changed by "{demo_core::safe_text(&b)}" (Esc takes theirs)"</span> })}
            {move || (draft.with(|d| d.is_some()) && error().is_none() && changed_by().is_none()).then(|| view! { <span class="fhint">"Enter saves · Esc reverts"</span> })}
        </div>
    }
}

#[component]
pub fn TextTab() -> impl IntoView {
    let app = use_app();
    let pw = RwSignal::new(String::new());
    let show = RwSignal::new(false);
    view! {
        <TabFrame id="text">
            <Field label="Name" key="text.name"><SyncedInput key="text.name" /></Field>
            <Field label="Email" key="text.email"><SyncedInput key="text.email" placeholder="name@example.com" /></Field>
            <Field label="Code" key="text.code"><SyncedInput key="text.code" class="upper short" placeholder="ABC-123" /></Field>
            <Field label="Password">
                <div class="tin">
                    <input class="fin short" data-control="password" autocomplete="off" spellcheck="false"
                        type=move || if show.get() { "text" } else { "password" }
                        prop:value=move || pw.get() on:input=move |e| pw.set(event_target_value(&e))
                        on:focus=move |_| app.ctx.set(Ctx::Body("password"))
                        on:keydown=move |e: web_sys::KeyboardEvent| {
                            if util::ctrl(&e) && (e.key() == "o" || e.key() == "O") { e.prevent_default(); show.update(|s| *s = !*s); }
                        } />
                    <button class="btn sm eye" tabindex="-1" title="Show / hide (^O)" on:mousedown=|e| e.prevent_default()
                        on:click=move |_| show.update(|s| *s = !*s)>{move || if show.get() { "🙈 hide" } else { "👁 show" }}</button>
                    <span class="fhint">"local only: never sent"</span>
                </div>
            </Field>
        </TabFrame>
    }
}
