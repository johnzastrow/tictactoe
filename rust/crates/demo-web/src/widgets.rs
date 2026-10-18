//! Reusable controls: highlighted fuzzy matches, coloured chips, the
//! dropdown (tabs 4 and 9, the grid's status editor) and the month calendar.

use demo_core::Date;
use demo_core::dataset::Choice;
use demo_core::date::MONTH_NAMES;
use leptos::prelude::*;

use crate::app::{Ctx, Level, use_app};
use crate::util;

/// Split `label` into (text, highlighted) runs for matched char indices.
pub fn highlight(label: &str, idx: &[usize]) -> impl IntoView + use<> {
    let mut runs: Vec<(String, bool)> = Vec::new();
    for (i, ch) in label.chars().enumerate() {
        let hit = idx.contains(&i);
        match runs.last_mut() {
            Some((s, h)) if *h == hit => s.push(ch),
            _ => runs.push((ch.to_string(), hit)),
        }
    }
    runs.into_iter()
        .map(|(s, hit)| {
            if hit {
                view! { <b class="hl">{s}</b> }.into_any()
            } else {
                view! { <span>{s}</span> }.into_any()
            }
        })
        .collect_view()
}

/// A coloured chip (`color` is a `#rrggbb` from the dataset, or empty).
pub fn chip(label: &str, color: &str) -> impl IntoView + use<> {
    let style = if color.is_empty() {
        String::new()
    } else {
        format!("--c:{color}")
    };
    view! { <span class="chip" style=style>{label.to_string()}</span> }
}

/// One dropdown entry.
#[derive(Clone, PartialEq, Debug)]
pub struct Opt {
    pub id: String,
    pub label: String,
    pub color: String,
}

pub fn opts(list: &[Choice]) -> Vec<Opt> {
    list.iter()
        .map(|c| Opt {
            id: c.id.into(),
            label: c.label.into(),
            color: c.color.into(),
        })
        .collect()
}

pub fn plain_opts<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<Opt> {
    names
        .into_iter()
        .map(|n| Opt {
            id: n.into(),
            label: n.into(),
            color: String::new(),
        })
        .collect()
}

/// A closed list showing the value as a chip; Enter/Space/↓ or a click opens
/// it, typing filters, Enter picks, Esc closes (ui-spec tab 4).
#[component]
pub fn Dropdown(
    #[prop(into)] ctl: String,
    #[prop(into)] options: Signal<Vec<Opt>>,
    #[prop(into)] value: Signal<Option<String>>,
    on_pick: Callback<String>,
    #[prop(optional, into)] disabled: Signal<bool>,
    #[prop(optional, into)] disabled_msg: String,
    #[prop(optional)] on_clear: Option<Callback<()>>,
    #[prop(optional, into)] none_label: Option<String>,
    /// Open as soon as it mounts (the grid's status editor).
    #[prop(optional)]
    start_open: bool,
    /// Called when the list closes without a pick (grid editor cancel).
    #[prop(optional)]
    on_cancel: Option<Callback<()>>,
) -> impl IntoView {
    let app = use_app();
    let open = RwSignal::new(false);
    let filter = RwSignal::new(String::new());
    let sel = RwSignal::new(0usize);
    let list = NodeRef::<leptos::html::Div>::new();
    let shown = Memo::new(move |_| {
        let f = filter.get().to_lowercase();
        options
            .get()
            .into_iter()
            .filter(|o| f.is_empty() || o.label.to_lowercase().contains(&f))
            .collect::<Vec<_>>()
    });
    Effect::new(move |prev: Option<bool>| {
        let o = open.get();
        if prev.is_some() || o {
            app.popup.set(o.then_some("dropdown"));
        }
        o
    });
    on_cleanup(move || {
        if open.try_get_untracked() == Some(true) {
            let _ = app.popup.try_set(None);
        }
    });
    // keep the selected row in view
    Effect::new(move |_| {
        let s = sel.get();
        if let Some(el) = list.get() {
            if let Some(c) = el.children().item(s as u32) {
                util::scroll_into_view(&c);
            }
        }
    });
    let msg = StoredValue::new(disabled_msg);
    let do_open = move |f: String| {
        if disabled.get_untracked() {
            app.say(Level::Info, msg.get_value());
            return;
        }
        filter.set(f.clone());
        let cur = value.get_untracked();
        let pos = if f.is_empty() {
            shown.with_untracked(|s| s.iter().position(|o| Some(&o.id) == cur.as_ref()))
        } else {
            None
        };
        sel.set(pos.unwrap_or(0));
        open.set(true);
    };
    if start_open {
        do_open(String::new());
    }
    let close = move || {
        open.set(false);
        if let Some(c) = on_cancel {
            c.run(());
        }
    };
    let pick = move |i: usize| {
        if let Some(o) = shown.with_untracked(|s| s.get(i).cloned()) {
            open.set(false);
            on_pick.run(o.id);
        }
    };
    let on_key = move |e: web_sys::KeyboardEvent| {
        let k = e.key();
        if open.get_untracked() {
            let n = shown.with_untracked(|s| s.len());
            match k.as_str() {
                "ArrowDown" => sel.update(|s| *s = (*s + 1).min(n.saturating_sub(1))),
                "ArrowUp" => sel.update(|s| *s = s.saturating_sub(1)),
                "Home" | "PageUp" => sel.set(0),
                "End" | "PageDown" => sel.set(n.saturating_sub(1)),
                "Enter" => pick(sel.get_untracked()),
                " " if filter.with_untracked(|f| f.is_empty()) => pick(sel.get_untracked()),
                "Escape" => close(),
                "Backspace" => {
                    filter.update(|f| {
                        f.pop();
                    });
                    sel.set(0);
                }
                "Tab" => {
                    close();
                    return;
                }
                _ => match util::typed_char(&e) {
                    Some(c) => {
                        filter.update(|f| f.push(c));
                        sel.set(0);
                    }
                    None => return,
                },
            }
        } else {
            match k.as_str() {
                "Enter" | " " | "ArrowDown" => do_open(String::new()),
                "Delete" | "Backspace" if on_clear.is_some() => {
                    if let Some(c) = on_clear {
                        c.run(());
                    }
                }
                _ => match util::typed_char(&e) {
                    Some(c) if c.is_alphanumeric() => do_open(c.to_string()),
                    _ => return,
                },
            }
        }
        e.prevent_default();
    };
    let none = none_label.unwrap_or_else(|| "— none —".into());
    let current = move || {
        let v = value.get();
        let o = options.with(|os| os.iter().find(|o| Some(&o.id) == v.as_ref()).cloned());
        match o {
            Some(o) => chip(&o.label, &o.color).into_any(),
            None => view! { <span class="muted">{none.clone()}</span> }.into_any(),
        }
    };
    view! {
        <div class="dd-wrap">
            <div class="dd fin" class:disabled=move || disabled.get() class:open=move || open.get() tabindex="0" role="combobox"
                aria-expanded=move || open.get().to_string() data-control=ctl
                on:keydown=on_key
                on:focus=move |_| app.ctx.set(Ctx::Body("dropdown"))
                on:blur=move |_| { if open.try_get_untracked() == Some(true) { let _ = open.try_set(false); if let Some(c) = on_cancel { c.run(()); } } }
                on:mousedown=move |e| { if open.get_untracked() { e.prevent_default(); close(); } else { do_open(String::new()); } }>
                {current}
                <span class="caret">"▾"</span>
            </div>
            <Show when=move || open.get()>
                <div class="choices dd-list" data-popup="dropdown" node_ref=list on:mousedown=|e| e.prevent_default()>
                    {move || (!filter.get().is_empty()).then(|| view! { <div class="dd-filter">"filter: "{filter.get()}</div> })}
                    {move || {
                        let items = shown.get();
                        if items.is_empty() {
                            return view! { <div class="choice empty">"No matches"</div> }.into_any();
                        }
                        items.into_iter().enumerate().map(|(i, o)| {
                            let dot = if o.color.is_empty() { String::new() } else { format!("--c:{}", o.color) };
                            view! {
                                <div class="choice" class:sel=move || sel.get() == i data-id=o.id.clone()
                                    on:mouseenter=move |_| sel.set(i) on:mousedown=move |_| pick(i)>
                                    <i class="dot" class:none=o.color.is_empty() style=dot></i>
                                    <span class="lbl">{o.label.clone()}</span>
                                </div>
                            }
                        }).collect_view().into_any()
                    }}
                </div>
            </Show>
        </div>
    }
}

/// Month calendar for the date tab: the cursor, today, this field's value,
/// the other date of the pair and the range between them.
#[component]
pub fn Calendar(
    cursor: Signal<Date>,
    value: Signal<Option<Date>>,
    other: Signal<Option<Date>>,
    today: Date,
    on_pick: Callback<Date>,
) -> impl IntoView {
    view! {
        <div class="cal" data-popup="calendar" on:mousedown=|e| e.prevent_default()>
            <div class="cal-h">
                {move || { let m = cursor.get(); format!("{} {}", MONTH_NAMES[m.month() as usize - 1], m.year()) }}
            </div>
            <div class="cal-g">
                {["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"].map(|d| view! { <span class="cal-wd">{d}</span> })}
                {move || {
                    let cur = cursor.get();
                    let month = cur.month();
                    let (val, oth) = (value.get(), other.get());
                    // the range runs between the cursor and the other date of the pair
                    let range = oth.map(|o| if o < cur { (o, cur) } else { (cur, o) });
                    cur.calendar_grid()
                        .into_iter()
                        .map(|d| {
                            let mut cls = String::from("cal-d");
                            if d.month() != month { cls.push_str(" out"); }
                            if d.is_weekend() { cls.push_str(" we"); }
                            if d == today { cls.push_str(" today"); }
                            if Some(d) == val { cls.push_str(" val"); }
                            if Some(d) == oth { cls.push_str(" other"); }
                            if range.is_some_and(|(a, b)| d > a && d < b) { cls.push_str(" range"); }
                            if d == cur { cls.push_str(" cur"); }
                            view! { <span class=cls data-date=d.iso() on:mousedown=move |_| on_pick.run(d)>{d.day()}</span> }
                        })
                        .collect_view()
                }}
            </div>
            <div class="cal-foot muted">"■ today  ● other date  ░ range"</div>
        </div>
    }
}
