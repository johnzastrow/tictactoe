//! Tab 13, Dialogs: one button per kind of popup, and the current
//! `dialog.profile` (edited by the Form… dialog).

use demo_core::keys::Profile;
use leptos::prelude::*;

use super::TabFrame;
use crate::app::{App, ConfirmAct, Ctx, InputAct, Level, Modal, use_app};

/// (data-control, label, what it opens)
const BUTTONS: [(&str, &str); 5] = [
    ("dialogs.confirm", "Confirm…"),
    ("dialogs.form", "Form…"),
    ("dialogs.message", "Message"),
    ("dialogs.toast", "Toast"),
    ("dialogs.broadcast", "Broadcast…"),
];

fn open_kind(app: App, i: usize) {
    match i {
        0 => app.open(Modal::Confirm {
            title: "Delete 3 files?".into(),
            body: "Budget 2027.xlsx, Roadmap.pdf and Logo.svg will be deleted. (A demo: nothing is deleted.)".into(),
            ok: "Delete".into(),
            act: ConfirmAct::Demo,
        }),
        1 => app.open(Modal::Profile),
        2 => app.open(Modal::Message {
            title: "Message".into(),
            body: "This is an information box with a single OK button.".into(),
        }),
        3 => app.toast(Level::Info, "A local toast: only you see this one."),
        _ => app.open(Modal::Input {
            title: "Broadcast a toast".into(),
            label: "Message to everyone connected (1–120 characters)".into(),
            value: String::new(),
            act: InputAct::Broadcast,
        }),
    }
}

#[component]
pub fn DialogsTab() -> impl IntoView {
    let app = use_app();
    let raw = app.watch("dialog.profile");
    let profile =
        move || raw.with(|v| serde_json::from_value::<Profile>(v.clone()).unwrap_or_default());
    let on_key = move |i: usize| {
        move |e: web_sys::KeyboardEvent| {
            let n = BUTTONS.len();
            let go =
                |j: usize| crate::util::focus_sel(&format!("[data-control='{}']", BUTTONS[j].0));
            match e.key().as_str() {
                "Enter" | " " => open_kind(app, i),
                "ArrowRight" => go((i + 1) % n),
                "ArrowLeft" => go((i + n - 1) % n),
                _ => return,
            }
            e.prevent_default();
        }
    };
    view! {
        <TabFrame id="dialogs">
            <div class="btnrow">
                {BUTTONS.iter().enumerate().map(|(i, (ctl, label))| view! {
                    <button class="btn" data-control=*ctl on:keydown=on_key(i) on:click=move |_| open_kind(app, i)
                        on:focus=move |_| app.ctx.set(Ctx::Body("button"))>{*label}</button>
                }).collect_view()}
            </div>
            <div class="frow" class:flash=move || app.flash.with(|f| f.contains("dialog.profile"))>
                <label class="flabel2">"Profile"</label>
                <div class="fctl">
                    <dl class="kv card1">
                        <dt>"Name"</dt><dd>{move || demo_core::safe_text(&profile().name)}</dd>
                        <dt>"Email"</dt><dd>{move || demo_core::safe_text(&profile().email)}</dd>
                    </dl>
                    <span class="fhint">"Form… edits it (shared); Broadcast… sends a toast to everyone."</span>
                </div>
            </div>
        </TabFrame>
    }
}
