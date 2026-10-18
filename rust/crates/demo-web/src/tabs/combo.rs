//! Tab 5, Combobox: type to fuzzy-search People, pick one (or none).

use demo_core::dataset::{person, search_people};
use leptos::prelude::*;
use serde_json::{Value, json};
use wasm_bindgen::JsCast;

use super::{Field, TabFrame};
use crate::app::{Ctx, use_app};
use crate::util;
use crate::widgets::highlight;

/// The match indices that fall in `[start, start + len)`, shifted to 0.
fn part(idx: &[usize], start: usize, len: usize) -> Vec<usize> {
    idx.iter()
        .filter(|&&i| i >= start && i < start + len)
        .map(|i| i - start)
        .collect()
}

#[component]
fn ComboBox(key: &'static str) -> impl IntoView {
    let app = use_app();
    let synced = app.watch(key);
    let current = Memo::new(move |_| synced.with(|v| v.as_str().map(String::from)));
    let query = RwSignal::new(None::<String>);
    let open = RwSignal::new(false);
    let sel = RwSignal::new(0usize);
    let list = NodeRef::<leptos::html::Div>::new();
    let results = Memo::new(move |_| search_people(&query.get().unwrap_or_default()));
    Effect::new(move |prev: Option<bool>| {
        let o = open.get();
        if prev.is_some() || o {
            app.popup.set(o.then_some("combobox"));
        }
        o
    });
    on_cleanup(move || {
        if open.try_get_untracked() == Some(true) {
            let _ = app.popup.try_set(None);
        }
    });
    Effect::new(move |_| {
        if app.revert.with(|(k, _)| k == key) {
            query.set(None);
        }
    });
    Effect::new(move |_| {
        let s = sel.get();
        if let Some(el) = list.get() {
            if let Some(c) = el.children().item(s as u32) {
                util::scroll_into_view(&c);
            }
        }
    });
    let shown = move || {
        query.get().unwrap_or_else(|| {
            current
                .get()
                .and_then(|u| person(&u))
                .map(|p| p.name.to_string())
                .unwrap_or_default()
        })
    };
    let pick = move |i: usize| {
        let Some(u) = results.with_untracked(|r| r.get(i).map(|(p, _)| p.username)) else {
            return;
        };
        open.set(false);
        query.set(None);
        app.set(key, json!(u));
    };
    let do_open = move || {
        let cur = current.get_untracked();
        sel.set(
            results
                .with_untracked(|r| {
                    r.iter()
                        .position(|(p, _)| Some(p.username) == cur.as_deref())
                })
                .unwrap_or(0),
        );
        open.set(true);
    };
    let on_key = move |e: web_sys::KeyboardEvent| {
        let n = results.with_untracked(|r| r.len());
        let is_open = open.get_untracked();
        match e.key().as_str() {
            "ArrowDown" if !is_open => do_open(),
            "ArrowDown" => sel.update(|s| *s = (*s + 1).min(n.saturating_sub(1))),
            "ArrowUp" if is_open => sel.update(|s| *s = s.saturating_sub(1)),
            "Enter" if is_open => pick(sel.get_untracked()),
            "Enter" => do_open(),
            "Escape" if is_open => open.set(false),
            "Escape" if query.with_untracked(|q| q.is_some()) => query.set(None),
            "Delete" if query.with_untracked(|q| q.as_deref().unwrap_or("").is_empty()) => {
                open.set(false);
                query.set(None);
                app.set(key, Value::Null);
            }
            "Tab" => {
                open.set(false);
                return;
            }
            _ => return,
        }
        e.prevent_default();
    };
    view! {
        <div class="combo">
            <input class="fin" data-control=key role="combobox" spellcheck="false" autocomplete="off" placeholder="— none —"
                aria-expanded=move || open.get().to_string() prop:value=shown
                on:input=move |e| { query.set(Some(event_target_value(&e))); sel.set(0); open.set(true); }
                on:focus=move |e| {
                    app.ctx.set(Ctx::Body("combo"));
                    if let Some(i) = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok()) { i.select(); }
                }
                on:blur=move |_| { let _ = open.try_set(false); let _ = query.try_set(None); }
                on:mousedown=move |_| if !open.get_untracked() { do_open() }
                on:keydown=on_key />
            <span class="caret">"▾"</span>
            <Show when=move || open.get()>
                <div class="choices dd-list" data-popup="combobox" node_ref=list on:mousedown=|e| e.prevent_default()>
                    {move || {
                        let r = results.get();
                        if r.is_empty() {
                            return view! { <div class="choice empty">"No one matches"</div> }.into_any();
                        }
                        r.into_iter().enumerate().map(|(i, (p, m))| {
                            let n = p.name.chars().count();
                            let u = p.username.chars().count();
                            view! {
                                <div class="choice" class:sel=move || sel.get() == i data-id=p.username
                                    on:mouseenter=move |_| sel.set(i) on:mousedown=move |_| pick(i)>
                                    <span class="lbl">{highlight(p.name, &part(&m.indices, 0, n))}</span>
                                    <span class="det">{highlight(p.username, &part(&m.indices, n + 1, u))}" · "{highlight(p.team, &part(&m.indices, n + u + 2, p.team.len()))}</span>
                                </div>
                            }
                        }).collect_view().into_any()
                    }}
                </div>
            </Show>
            <span class="fhint">{move || match current.get() {
                Some(u) => format!("= {u}"),
                None => "= null (none)".into(),
            }}</span>
        </div>
    }
}

#[component]
pub fn ComboTab() -> impl IntoView {
    view! {
        <TabFrame id="combo">
            <Field label="Owner" key="combo.owner"><ComboBox key="combo.owner" /></Field>
            <Field label="Reviewer" key="combo.reviewer"><ComboBox key="combo.reviewer" /></Field>
        </TabFrame>
    }
}
