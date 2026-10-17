//! pt web client: a Leptos (client-side rendered) app compiled to
//! WebAssembly. All data logic comes from pt-core, the same crate the
//! server and the TUI use, so every client agrees on values and rules.

#![recursion_limit = "512"]
// Nested `if let` guards around signals and DOM lookups read better than
// long let-chains here.
#![allow(clippy::collapsible_if)]

mod app;
mod board;
mod cells;
mod dashboard;
mod dialogs;
mod editor;
mod forms;
mod gridops;
mod gridview;
mod net;
mod shell;
mod timeline;
mod util;
mod views;
mod widgets;

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use app::{App, ConnState, THEMES, ViewId};

fn main() {
    console_error_panic_hook::set_once();
    mount_to_body(Root);
}

#[component]
fn Root() -> impl IntoView {
    let app = App::new();
    provide_context(app);
    app.boot();
    let keys = window_event_listener(leptos::ev::keydown, move |e| shell::global_key(app, e));
    // clicking outside an open menu closes it
    let clicks = window_event_listener(leptos::ev::mousedown, move |e: web_sys::MouseEvent| {
        if app.menu.get_untracked().is_none() {
            return;
        }
        let inside = e
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            .and_then(|el| el.closest(".menubar").ok().flatten())
            .is_some();
        if !inside {
            app.menu.set(None);
        }
    });
    on_cleanup(move || {
        keys.remove();
        clicks.remove();
    });
    Effect::new(move |_| {
        let t = THEMES[app.theme.get()].0;
        if let Some(root) = util::document().document_element() {
            let _ = root.set_attribute("data-theme", t);
            let _ = root.set_attribute(
                "data-density",
                if app.compact.get() {
                    "compact"
                } else {
                    "comfortable"
                },
            );
        }
    });
    move || {
        if app.booting.get() {
            view! { <div class="splash">"pt"</div> }.into_any()
        } else if app.user.get().is_none() {
            view! { <shell::Login /> }.into_any()
        } else {
            view! { <Main /> }.into_any()
        }
    }
}

#[component]
fn Main() -> impl IntoView {
    let app = app::use_app();
    let body = move || match app.view.get() {
        ViewId::Projects => view! { <views::ProjectsView /> }.into_any(),
        ViewId::Tasks => view! { <views::TasksView /> }.into_any(),
        ViewId::Board => view! { <board::BoardView /> }.into_any(),
        ViewId::Timeline => view! { <timeline::TimelineView /> }.into_any(),
        ViewId::Dashboard => view! { <dashboard::DashboardView /> }.into_any(),
        ViewId::Activity => view! { <views::ActivityView /> }.into_any(),
        ViewId::Templates => view! { <views::TemplatesView /> }.into_any(),
        ViewId::Fields => view! { <views::FieldsView /> }.into_any(),
        ViewId::Users => view! { <views::UsersView /> }.into_any(),
    };
    util::focus_later("g-projects");
    view! {
        <div class="shell">
            <shell::MenuBar />
            <shell::Tabs />
            <shell::QueryBar />
            <Show when=move || app.conn.get() == ConnState::Offline>
                <div class="offline">"⚠ Connection lost — reconnecting. Edits are paused; your view is read-only until the connection is back."</div>
            </Show>
            <main class="viewport">{body}</main>
            <shell::StatusLine />
            <shell::HintBar />
            <shell::Toasts />
            <dialogs::Modals />
        </div>
    }
}
