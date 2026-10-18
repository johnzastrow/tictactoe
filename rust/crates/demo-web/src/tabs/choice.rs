//! Tab 8, Choices: a checkbox, a checkbox group, radio buttons and a toggle
//! switch. Each is one focus stop; Space toggles, arrows move in a group.

use demo_core::dataset::{CHANNELS, SIZES};
use leptos::prelude::*;
use serde_json::json;

use super::{Field, TabFrame};
use crate::app::{Ctx, Level, use_app};

#[component]
pub fn ChoiceTab() -> impl IntoView {
    let app = use_app();
    let notify = app.watch("choice.notify");
    let channels = app.watch("choice.channels");
    let size = app.watch("choice.size");
    let dark = app.watch("choice.dark");
    let on = move |m: Memo<serde_json::Value>| m.with(|v| v.as_bool().unwrap_or(false));
    let has =
        move |id: &str| channels.with(|v| v.as_array().is_some_and(|a| a.iter().any(|x| x == id)));
    let flip = move |key: &'static str| {
        let cur = app.latest(key).as_bool().unwrap_or(false);
        app.set_latest(key, json!(!cur));
    };
    let toggle_channel = move |i: usize| {
        if !on(notify) {
            app.say(Level::Info, "Turn on Notify first");
            return;
        }
        let id = CHANNELS[i].id;
        let mut list: Vec<String> =
            serde_json::from_value(app.latest("choice.channels")).unwrap_or_default();
        if list.iter().any(|c| c == id) {
            list.retain(|c| c != id);
        } else {
            list.push(id.into());
        }
        app.set_latest("choice.channels", json!(list));
    };
    let pick_size = move |i: usize| app.set_latest("choice.size", json!(SIZES[i].id));
    let size_idx = move || {
        size.with(|v| {
            SIZES
                .iter()
                .position(|s| Some(s.id) == v.as_str())
                .unwrap_or(0)
        })
    };
    let ch_sel = RwSignal::new(0usize);
    let space = |e: &web_sys::KeyboardEvent| e.key() == " " || e.key() == "Enter";
    view! {
        <TabFrame id="choice">
            <Field label="Notify me" key="choice.notify">
                <div class="check" tabindex="0" role="checkbox" aria-checked=move || on(notify).to_string() data-control="choice.notify"
                    on:focus=move |_| app.ctx.set(Ctx::Body("check"))
                    on:keydown=move |e| if space(&e) { e.prevent_default(); flip("choice.notify"); }
                    on:click=move |_| flip("choice.notify")>
                    <span class="box">{move || if on(notify) { "✓" } else { "" }}</span>" Notify me about changes"
                </div>
            </Field>
            <Field label="Channels" key="choice.channels">
                <div class="group" class:disabled=move || !on(notify) tabindex="0" role="group" data-control="choice.channels"
                    on:focus=move |_| app.ctx.set(Ctx::Body("group"))
                    on:keydown=move |e: web_sys::KeyboardEvent| {
                        let n = CHANNELS.len();
                        match e.key().as_str() {
                            "ArrowDown" | "ArrowRight" => ch_sel.update(|s| *s = (*s + 1) % n),
                            "ArrowUp" | "ArrowLeft" => ch_sel.update(|s| *s = (*s + n - 1) % n),
                            " " => toggle_channel(ch_sel.get_untracked()),
                            _ => return,
                        }
                        e.prevent_default();
                    }>
                    {CHANNELS.iter().enumerate().map(|(i, c)| view! {
                        <div class="gitem" class:cur=move || ch_sel.get() == i data-id=c.id
                            on:click=move |_| { ch_sel.set(i); toggle_channel(i); }>
                            <span class="box">{move || if has(c.id) { "✓" } else { "" }}</span>" "{c.label}
                        </div>
                    }).collect_view()}
                </div>
                {move || (!on(notify)).then(|| view! { <span class="fhint">"disabled while Notify is off"</span> })}
            </Field>
            <Field label="Size" key="choice.size">
                <div class="radios" tabindex="0" role="radiogroup" data-control="choice.size"
                    on:focus=move |_| app.ctx.set(Ctx::Body("radio"))
                    on:keydown=move |e: web_sys::KeyboardEvent| {
                        let n = SIZES.len();
                        let i = size_idx();
                        match e.key().as_str() {
                            "ArrowRight" | "ArrowDown" => pick_size((i + 1) % n),
                            "ArrowLeft" | "ArrowUp" => pick_size((i + n - 1) % n),
                            " " => pick_size(i),
                            _ => return,
                        }
                        e.prevent_default();
                    }>
                    {SIZES.iter().enumerate().map(|(i, s)| view! {
                        <div class="ritem" class:on=move || size_idx() == i data-id=s.id on:click=move |_| pick_size(i)>
                            <span class="rdot">{move || if size_idx() == i { "●" } else { "" }}</span>" "{s.label}
                        </div>
                    }).collect_view()}
                </div>
            </Field>
            <Field label="Dark mode" key="choice.dark">
                <div class="toggle" class:on=move || on(dark) tabindex="0" role="switch" aria-checked=move || on(dark).to_string() data-control="choice.dark"
                    on:focus=move |_| app.ctx.set(Ctx::Body("toggle"))
                    on:keydown=move |e| if space(&e) { e.prevent_default(); flip("choice.dark"); }
                    on:click=move |_| flip("choice.dark")>
                    <span class="track"><span class="knob"></span></span>
                    <span>{move || if on(dark) { " On" } else { " Off" }}</span>
                </div>
                <span class="fhint">"a shared setting only; it doesn't change this page's theme (View ▸ Theme does)"</span>
            </Field>
        </TabFrame>
    }
}
