//! Reusable controls: fuzzy choice lists with highlighted matches, the month
//! calendar, chips, and helpers to position popups.

use leptos::prelude::*;
use pt_core::date::{Date, MONTH_NAMES, days_in_month};
use pt_core::grid::{Choice, Style};

use crate::cells::style_class;

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

/// Fixed-position style for a popup under (or above) a rectangle.
pub fn popup_style(rect: (f64, f64, f64, f64), min_w: f64, h: f64) -> String {
    let (x, y, w, rh) = rect;
    let (vw, vh) = crate::util::viewport();
    let width = w.max(min_w);
    let left = x.min(vw - width - 8.0).max(4.0);
    let below = y + rh + 2.0;
    let top = if below + h > vh - 8.0 && y - h - 2.0 > 0.0 {
        y - h - 2.0
    } else {
        below
    };
    format!("left:{left}px;top:{top}px;min-width:{width}px;max-height:{h}px")
}

/// A list of choices filtered by the caller. `items` = (index into choices, matched char indices).
#[component]
pub fn ChoiceList(
    choices: Signal<Vec<Choice>>,
    items: Signal<Vec<(usize, Vec<usize>)>>,
    sel: RwSignal<usize>,
    on_pick: Callback<usize>,
    #[prop(optional, into)] style: Signal<String>,
    #[prop(optional, into)] extra: Option<Signal<Option<String>>>,
) -> impl IntoView {
    let list = NodeRef::<leptos::html::Div>::new();
    // keep the selected row in view
    Effect::new(move |_| {
        let s = sel.get();
        if let Some(el) = list.get() {
            if let Some(child) = el.children().item(s as u32) {
                let opts = web_sys::ScrollIntoViewOptions::new();
                opts.set_block(web_sys::ScrollLogicalPosition::Nearest);
                child.scroll_into_view_with_scroll_into_view_options(&opts);
            }
        }
    });
    view! {
        <div class="choices" node_ref=list style=move || style.get() on:mousedown=|e| e.prevent_default()>
            {move || {
                let ch = choices.get();
                let its = items.get();
                if its.is_empty() && extra.and_then(|x| x.get()).is_none() {
                    return view! { <div class="choice empty">"No matches"</div> }.into_any();
                }
                its.into_iter()
                    .enumerate()
                    .map(|(pos, (i, m))| {
                        let c = ch[i].clone();
                        let cls = move || if sel.get() == pos { "choice sel" } else { "choice" };
                        let dot = style_dot(&c.style);
                        view! {
                            <div class=cls on:mousedown=move |_| on_pick.run(pos) on:mouseenter=move |_| sel.set(pos)>
                                {dot}
                                <span class="lbl">{highlight(&c.label, &m)}</span>
                                <span class="det">{c.detail.clone()}</span>
                            </div>
                        }
                    })
                    .collect_view()
                    .into_any()
            }}
            {move || extra.and_then(|x| x.get()).map(|t| {
                let pos = items.with(|i| i.len());
                let cls = move || if sel.get() == pos { "choice sel create" } else { "choice create" };
                view! { <div class=cls on:mousedown=move |_| on_pick.run(pos)>"✚ "{t}</div> }
            })}
        </div>
    }
}

/// A coloured marker for a choice (status chip colour, user colour, …).
pub fn style_dot(s: &Style) -> AnyView {
    match s {
        Style::User(id) => {
            let c = format!("background:var(--u{})", id.rem_euclid(8));
            view! { <i class="dot" style=c></i> }.into_any()
        }
        Style::Color(c) => {
            let c = format!("background:{c}");
            view! { <i class="dot" style=c></i> }.into_any()
        }
        Style::Plain | Style::Muted | Style::Tags => view! { <i class="dot none"></i> }.into_any(),
        other => {
            let cls = format!("dot {}", style_class(other));
            view! { <i class=cls></i> }.into_any()
        }
    }
}

/// Month calendar. `cur` is the highlighted date; clicking a day calls `on_pick`.
#[component]
pub fn Calendar(cur: Signal<Option<Date>>, today: Date, on_pick: Callback<Date>) -> impl IntoView {
    let month = move || cur.get().unwrap_or(today).first_of_month();
    view! {
        <div class="cal" on:mousedown=|e| e.prevent_default()>
            <div class="cal-h">
                {move || { let m = month(); format!("{} {}", MONTH_NAMES[m.month() as usize - 1], m.year()) }}
            </div>
            <div class="cal-g">
                {["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"].map(|d| view! { <span class="cal-wd">{d}</span> })}
                {move || {
                    let first = month();
                    let lead = first.weekday() as i32;
                    let n = days_in_month(first.year(), first.month()) as i32;
                    let sel = cur.get();
                    (0..42)
                        .map(|i| {
                            let d = first.add_days(i - lead);
                            let inside = i >= lead && i < lead + n;
                            let mut cls = String::from("cal-d");
                            if !inside { cls.push_str(" out"); }
                            if d == today { cls.push_str(" today"); }
                            if Some(d) == sel { cls.push_str(" sel"); }
                            if d.weekday() >= 5 { cls.push_str(" we"); }
                            view! { <span class=cls on:mousedown=move |_| on_pick.run(d)>{d.day()}</span> }
                        })
                        .collect_view()
                }}
            </div>
        </div>
    }
}
