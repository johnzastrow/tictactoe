//! Tab 6, Tags: chips plus an input with ranked suggestions; every add or
//! remove commits the whole list.

use demo_core::dataset::suggest_tags;
use demo_core::keys::{MAX_TAGS, valid_tag};
use leptos::prelude::*;
use serde_json::json;

use super::TabFrame;
use crate::app::{Ctx, use_app};
use crate::util;

const KEY: &str = "tags.tags";

#[component]
pub fn TagsTab() -> impl IntoView {
    let app = use_app();
    let raw = app.watch(KEY);
    let tags = Memo::new(move |_| {
        raw.with(|v| serde_json::from_value::<Vec<String>>(v.clone()).unwrap_or_default())
    });
    let text = RwSignal::new(String::new());
    let open = RwSignal::new(false);
    let sel = RwSignal::new(0usize);
    let chip = RwSignal::new(None::<usize>);
    Effect::new(move |prev: Option<bool>| {
        let o = open.get();
        if prev.is_some() || o {
            app.popup.set(o.then_some("tags"));
        }
        o
    });
    on_cleanup(move || {
        if open.try_get_untracked() == Some(true) {
            let _ = app.popup.try_set(None);
        }
    });
    let typed = move || text.get().trim().to_lowercase();
    let sugg = Memo::new(move |_| suggest_tags(&text.get(), &tags.get()));
    // the "✚ Create" entry, when the typed text is a new valid tag
    let create = Memo::new(move |_| {
        let t = typed();
        (!t.is_empty()
            && valid_tag(&t)
            && !tags.with(|v| v.contains(&t))
            && !sugg.with(|s| s.iter().any(|(s, _)| *s == t)))
        .then_some(t)
    });
    let invalid = move || {
        let t = typed();
        (!t.is_empty() && !valid_tag(&t))
            .then_some("a–z, 0–9, _ or -, starting with a letter or digit, ≤ 24")
    };
    let count = move || sugg.with(|s| s.len()) + usize::from(create.with(|c| c.is_some()));
    let save = move |list: Vec<String>| app.set_latest(KEY, json!(list));
    let add = move |t: String| {
        let mut list = serde_json::from_value::<Vec<String>>(app.latest(KEY)).unwrap_or_default();
        if list.len() >= MAX_TAGS {
            app.say(crate::app::Level::Err, format!("at most {MAX_TAGS} tags"));
            return;
        }
        if !list.contains(&t) {
            list.push(t);
            save(list);
        }
        text.set(String::new());
        sel.set(0);
    };
    let remove = move |i: usize| {
        // by name: the shown list is the server's (sorted), `latest` may be what we sent
        let Some(name) = tags.with_untracked(|t| t.get(i).cloned()) else {
            return;
        };
        let mut list = serde_json::from_value::<Vec<String>>(app.latest(KEY)).unwrap_or_default();
        list.retain(|t| *t != name);
        save(list);
        chip.set(None);
    };
    let pick = move |i: usize| {
        let n = sugg.with_untracked(|s| s.len());
        if i < n {
            add(sugg.with_untracked(|s| s[i].0.to_string()));
        } else if let Some(t) = create.get_untracked() {
            add(t);
        }
    };
    let on_key = move |e: web_sys::KeyboardEvent| {
        let empty = text.with_untracked(|t| t.is_empty());
        let n_tags = tags.with_untracked(|t| t.len());
        let n = count();
        match e.key().as_str() {
            "ArrowDown" if !open.get_untracked() => {
                open.set(true);
                sel.set(0);
            }
            "ArrowDown" => sel.update(|s| *s = (*s + 1).min(n.saturating_sub(1))),
            "ArrowUp" if open.get_untracked() => sel.update(|s| *s = s.saturating_sub(1)),
            "Enter" => {
                if open.get_untracked() && n > 0 {
                    pick(sel.get_untracked());
                } else if let Some(t) = create.get_untracked() {
                    add(t);
                } else if !empty {
                    // a typed suggestion name (exact) is added as is
                    let t = typed();
                    if valid_tag(&t) {
                        add(t);
                    }
                }
            }
            "Escape" if open.get_untracked() => open.set(false),
            "Escape" if chip.get_untracked().is_some() => chip.set(None),
            "Backspace" if empty && n_tags > 0 => match chip.get_untracked() {
                Some(i) => remove(i),
                None => remove(n_tags - 1),
            },
            "ArrowLeft" if empty && n_tags > 0 => {
                chip.update(|c| *c = Some(c.map_or(n_tags - 1, |i| i.saturating_sub(1))));
                open.set(false);
            }
            "ArrowRight" if empty && chip.get_untracked().is_some() => {
                chip.update(|c| *c = c.and_then(|i| (i + 1 < n_tags).then_some(i + 1)));
            }
            "Delete" if empty && chip.get_untracked().is_some() => {
                if let Some(i) = chip.get_untracked() {
                    remove(i);
                }
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
        <TabFrame id="tags">
            <div class="frow" class:flash=move || app.flash.with(|f| f.contains(KEY))>
                <label class="flabel2">"Tags"</label>
                <div class="fctl">
                    <div class="tagbox">
                        <div class="chips">
                            {move || tags.get().into_iter().enumerate().map(|(i, t)| view! {
                                <span class="chipx" class:sel=move || chip.get() == Some(i) data-tag=t.clone()>
                                    {t.clone()}
                                    <b title="Remove" on:mousedown=|e| e.prevent_default() on:click=move |_| remove(i)>"×"</b>
                                </span>
                            }).collect_view()}
                            <input class="chipin" class:bad=move || invalid().is_some() data-control=KEY spellcheck="false" autocomplete="off"
                                placeholder="add a tag…" prop:value=move || text.get()
                                on:input=move |e| { text.set(event_target_value(&e)); sel.set(0); chip.set(None); open.set(true); }
                                on:focus=move |_| app.ctx.set(Ctx::Body("tags"))
                                on:blur=move |_| { let _ = open.try_set(false); let _ = chip.try_set(None); }
                                on:mousedown=move |_| open.set(true)
                                on:keydown=on_key />
                        </div>
                        <Show when=move || open.get()>
                            <div class="choices dd-list" data-popup="tags" on:mousedown=|e| e.prevent_default()>
                                {move || sugg.get().into_iter().enumerate().map(|(i, (t, uses))| view! {
                                    <div class="choice" class:sel=move || sel.get() == i data-id=t on:mouseenter=move |_| sel.set(i) on:mousedown=move |_| pick(i)>
                                        <span class="lbl">{t}</span><span class="det">{util::plural(uses as usize, "use", "uses")}</span>
                                    </div>
                                }).collect_view()}
                                {move || create.get().map(|t| {
                                    let pos = sugg.with(|s| s.len());
                                    view! {
                                        <div class="choice create" class:sel=move || sel.get() == pos on:mouseenter=move |_| sel.set(pos) on:mousedown=move |_| pick(pos)>
                                            {format!("✚ Create \"{t}\"")}
                                        </div>
                                    }
                                })}
                                {move || (count() == 0).then(|| view! { <div class="choice empty">"No suggestions"</div> })}
                            </div>
                        </Show>
                    </div>
                    {move || invalid().map(|m| view! { <div class="ferr">"✗ "{m}</div> })}
                    <div class="fhint">{move || format!("{} / {MAX_TAGS} tags · stored sorted, lowercase", tags.with(|t| t.len()))}</div>
                </div>
            </div>
        </TabFrame>
    }
}
