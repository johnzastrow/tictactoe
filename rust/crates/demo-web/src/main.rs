//! demo web client: a Leptos (client-side rendered) app compiled to
//! WebAssembly and served by demo-server. All data rules (keys, validation,
//! dataset, dates, fuzzy matching, the value store) come from demo-core,
//! the crate the servers and TUIs use, so every client agrees.
//!
//! Text from peers is only ever set as text nodes or attributes: nothing
//! here uses `inner_html`.

#![recursion_limit = "512"]
// Nested `if let` guards around signals and DOM lookups read better than
// long let-chains here.
#![allow(clippy::collapsible_if)]

mod app;
mod dialogs;
mod help;
mod menu;
mod net;
mod shell;
mod tabs;
mod util;
mod widgets;

use std::time::Duration;

use demo_core::protocol::clean_name;
use leptos::prelude::*;
use wasm_bindgen::JsCast;

use app::{App, ConnState, THEMES, focus_sidebar};

fn main() {
    console_error_panic_hook::set_once();
    mount_to_body(Root);
}

#[component]
fn Root() -> impl IntoView {
    let app = App::new();
    provide_context(app);
    // ?name=alice joins directly
    if let Some(n) = util::query_param("name") {
        match clean_name(&n) {
            Some(n) => app.join(n),
            None => app
                .join_error
                .set(Some(format!("“{n}” isn't a valid name: pick another."))),
        }
    }
    let keys = window_event_listener(leptos::ev::keydown, move |e| shell::global_key(app, e));
    // An open menu bar owns the keyboard: take keys in the capture phase,
    // before the focused control (sidebar, grid, …) can act on them. Root
    // lives as long as the page, so the closure is leaked on purpose.
    let menu_keys = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(
        move |e: web_sys::KeyboardEvent| {
            if app.menu.get_untracked().is_some() && shell::menu_capture(app, &e) {
                e.stop_propagation();
            }
        },
    );
    let _ = util::window().add_event_listener_with_callback_and_bool(
        "keydown",
        menu_keys.as_ref().unchecked_ref(),
        true,
    );
    menu_keys.forget();
    // clicking outside the open app menu closes it
    let clicks = window_event_listener(leptos::ev::mousedown, move |e: web_sys::MouseEvent| {
        if app.menu.get_untracked().is_none() {
            return;
        }
        let inside = e
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            .is_some_and(|el| util::within(&el, ".menubar.top"));
        if !inside {
            app.menu.set(None);
        }
    });
    // "2m ago" in the Value panels
    let tick = set_interval_with_handle(
        move || _ = app.now.try_set(util::now_secs()),
        Duration::from_secs(5),
    )
    .ok();
    on_cleanup(move || {
        keys.remove();
        clicks.remove();
        if let Some(t) = tick {
            t.clear();
        }
    });
    Effect::new(move |_| {
        if let Some(root) = util::document().document_element() {
            let _ = root.set_attribute("data-theme", THEMES[app.theme.get()].0);
        }
    });
    move || {
        if app.name.get().is_none() {
            view! { <shell::Join /> }.into_any()
        } else {
            view! { <Main /> }.into_any()
        }
    }
}

#[component]
fn Main() -> impl IntoView {
    let app = app::use_app();
    focus_sidebar();
    view! {
        <div class="shell">
            <shell::TopBar />
            <Show when=move || app.conn.get() == ConnState::Offline>
                <div class="offline">"⚠ Connection lost: reconnecting. Values may be out of date and changes aren't sent until it's back."</div>
            </Show>
            <div class="mid">
                <shell::Sidebar />
                <main class="viewport">{move || tabs::render(app.tab.get())}</main>
            </div>
            <shell::StatusLine />
            <shell::HintBar />
            <shell::Toasts />
            <dialogs::Modals />
        </div>
    }
}
