//! Rendering pt-core cells: style hints → CSS classes, chips, bars, avatars.

use leptos::prelude::*;
use pt_core::grid::{Cell, Style};
use pt_core::model::*;

pub fn style_class(s: &Style) -> String {
    match s {
        Style::Plain => String::new(),
        Style::Muted => "muted".into(),
        Style::Strong => "strong".into(),
        Style::ProjectStatus(p) => format!("ps-{}", p.as_str()),
        Style::TaskStatus(t) => format!("ts-{}", t.as_str()),
        Style::Priority(p) => format!("pri-{}", p.as_str()),
        Style::Health(h) => format!("h-{}", h.as_str()),
        Style::Role(r) => format!("role-{}", r.as_str()),
        Style::Overdue => "c-err".into(),
        Style::DueSoon => "c-warn".into(),
        Style::Bar { .. } => "barcell".into(),
        Style::Bad => "c-err".into(),
        Style::Good => "c-ok".into(),
        Style::Tags => "tags".into(),
        Style::Color(_) => String::new(),
        Style::User(_) => String::new(),
        Style::Struck => "struck".into(),
    }
}

pub fn priority_mark(p: Priority) -> &'static str {
    match p {
        Priority::Low => "▽",
        Priority::Medium => "◇",
        Priority::High => "▲",
        Priority::Critical => "◆",
    }
}

pub fn initials(name: &str) -> String {
    let mut it = name.split_whitespace();
    let a = it.next().and_then(|w| w.chars().next());
    let b = it.last().and_then(|w| w.chars().next());
    match (a, b) {
        (Some(a), Some(b)) => format!("{a}{b}").to_uppercase(),
        (Some(a), None) => a.to_uppercase().to_string(),
        _ => "?".into(),
    }
}

pub fn avatar(user_id: i64, name: &str) -> impl IntoView + use<> {
    let style = format!("background:var(--u{})", user_id.rem_euclid(8));
    let ini = initials(name);
    let title = name.to_string();
    view! { <span class="av" style=style title=title>{ini}</span> }
}

/// The inner content of a grid cell.
pub fn render(c: Cell) -> AnyView {
    match &c.style {
        Style::ProjectStatus(_) | Style::TaskStatus(_) | Style::Role(_) => {
            let cls = format!("chip {}", style_class(&c.style));
            view! { <span class=cls>{c.text}</span> }.into_any()
        }
        Style::Priority(p) => {
            let cls = format!("pri {}", style_class(&c.style));
            let mark = priority_mark(*p);
            view! { <span class=cls><i>{mark}</i>" "{c.text}</span> }.into_any()
        }
        Style::Health(h) => {
            let cls = format!("health {}", style_class(&c.style));
            let _ = h;
            view! { <span class=cls><i>"●"</i>" "{c.text}</span> }.into_any()
        }
        Style::Bar { pct, level } => {
            let w = format!("width:{}%", pct.clamp(0.0, 100.0));
            let cls = format!("fill l{level}");
            view! {
                <span class="bar"><span class=cls style=w></span></span>
                <span class="bar-t">{c.text}</span>
            }
            .into_any()
        }
        Style::Tags => c
            .text
            .split_whitespace()
            .map(|t| view! { <span class="tag">{t.to_string()}</span> })
            .collect_view()
            .into_any(),
        Style::Color(col) => {
            let st = format!("background:{col}");
            view! { <i class="sw" style=st></i>{c.text} }.into_any()
        }
        Style::User(id) => {
            let id = *id;
            view! { {avatar(id, &c.text)}<span class="un">{c.text.clone()}</span> }.into_any()
        }
        other => {
            let cls = style_class(other);
            view! { <span class=cls>{c.text}</span> }.into_any()
        }
    }
}
