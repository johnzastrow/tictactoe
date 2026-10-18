//! The 16 tabs (ui-spec §4). Each renders inside [`TabFrame`]: title, the
//! one-sentence explanation, its controls, then the Keys and Value panels.
//! DOM hooks for scripts: the frame is `data-tab="<id>"`, every focusable
//! control `data-control="<key or name>"`.

pub mod cascade;
pub mod charts;
pub mod choice;
pub mod combo;
pub mod date;
pub mod dialogs;
pub mod gantt;
pub mod grid;
pub mod kanban;
pub mod menus;
pub mod number;
pub mod palette;
pub mod select;
pub mod tags;
pub mod text;
pub mod textarea;

use demo_core::dataset::{TABS, Tab};
use leptos::prelude::*;

use crate::app::use_app;
use crate::help::tab_keys;

/// The current tab's body.
pub fn render(i: usize) -> AnyView {
    match TABS[i].id {
        "text" => view! { <text::TextTab /> }.into_any(),
        "number" => view! { <number::NumberTab /> }.into_any(),
        "date" => view! { <date::DateTab /> }.into_any(),
        "select" => view! { <select::SelectTab /> }.into_any(),
        "combo" => view! { <combo::ComboTab /> }.into_any(),
        "tags" => view! { <tags::TagsTab /> }.into_any(),
        "textarea" => view! { <textarea::TextareaTab /> }.into_any(),
        "choice" => view! { <choice::ChoiceTab /> }.into_any(),
        "cascade" => view! { <cascade::CascadeTab /> }.into_any(),
        "grid" => view! { <grid::GridTab /> }.into_any(),
        "menus" => view! { <menus::MenusTab /> }.into_any(),
        "palette" => view! { <palette::PaletteTab /> }.into_any(),
        "dialogs" => view! { <dialogs::DialogsTab /> }.into_any(),
        "charts" => view! { <charts::ChartsTab /> }.into_any(),
        "kanban" => view! { <kanban::KanbanTab /> }.into_any(),
        _ => view! { <gantt::GanttTab /> }.into_any(),
    }
}

pub fn tab(id: &str) -> &'static Tab {
    TABS.iter().find(|t| t.id == id).unwrap_or(&TABS[0])
}

#[component]
pub fn TabFrame(id: &'static str, children: Children) -> impl IntoView {
    let app = use_app();
    let t = tab(id);
    view! {
        <section class="tabbody" data-tab=t.id>
            <header class="th">
                <h1><span class="tnum">{t.number}</span>{t.heading}</h1>
                <p class="blurb">{t.blurb}</p>
            </header>
            <div class="ctlarea">{children()}</div>
            <div class="panels">
                <Show when=move || app.show_keys.get()><KeysPanel id=id /></Show>
                <Show when=move || app.show_value.get()><ValuePanel id=id /></Show>
            </div>
        </section>
    }
}

#[component]
fn KeysPanel(id: &'static str) -> impl IntoView {
    view! {
        <div class="panel keys">
            <div class="ptitle">"Keys"</div>
            <div class="pbody">
                {tab_keys(id).iter().map(|(k, d)| view! { <div class="krow"><kbd>{*k}</kbd><span>{*d}</span></div> }).collect_view()}
            </div>
        </div>
    }
}

/// Longest value text shown before clipping (lists can be long).
const MAX_SHOWN: usize = 900;

#[component]
fn ValuePanel(id: &'static str) -> impl IntoView {
    let app = use_app();
    let t = tab(id);
    view! {
        <div class="panel value">
            <div class="ptitle">"Value "<span class="muted">"(shared)"</span></div>
            <div class="pbody">
                {t.keys.iter().map(|&key| {
                    let text = move || {
                        let s = app.vals.with(|v| demo_core::safe_text(&v.value_text(key)));
                        if s.chars().count() > MAX_SHOWN { format!("{}…", s.chars().take(MAX_SHOWN).collect::<String>()) } else { s }
                    };
                    let meta = move || app.vals.with(|v| v.describe(key, app.now.get()));
                    view! {
                        <div class="vrow" class:flash=move || app.flash.with(|f| f.contains(key)) data-key=key>
                            <code class="vk">{key}" = "<span class="vv">{text}</span></code>
                            <div class="vmeta muted">{meta}</div>
                        </div>
                    }
                }).collect_view()}
            </div>
        </div>
    }
}

/// A labelled row in a tab's control area; flashes when a peer changes `key`.
#[component]
pub fn Field(
    #[prop(into)] label: String,
    #[prop(optional)] key: &'static str,
    children: Children,
) -> impl IntoView {
    let app = use_app();
    view! {
        <div class="frow" class:flash=move || !key.is_empty() && app.flash.with(|f| f.contains(key))>
            <label class="flabel2">{label}</label>
            <div class="fctl">{children()}</div>
        </div>
    }
}
