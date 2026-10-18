//! Tab 2, Number: a decimal spinner, an integer spinner and a slider.

use std::time::Duration;

use demo_core::keys::{self, num_value, round2};
use leptos::prelude::*;
use serde_json::Value;
use wasm_bindgen::JsCast;

use super::{Field, TabFrame};
use crate::app::{Ctx, use_app};

fn fmt_num(x: f64) -> String {
    if x.fract() == 0.0 {
        format!("{}", x as i64)
    } else {
        format!("{x}")
    }
}

/// A number field with ▲▼: typing makes a draft committed by Enter;
/// ↑/↓ (Shift: ±0.25 when `fine`) and the buttons commit at once.
#[component]
fn Spinner(key: &'static str, min: f64, max: f64, #[prop(optional)] fine: bool) -> impl IntoView {
    let app = use_app();
    let synced = app.watch(key);
    let draft = RwSignal::new(None::<String>);
    let base = StoredValue::new(0u64);
    Effect::new(move |_| {
        if app.revert.with(|(k, _)| k == key) {
            draft.set(None);
        }
    });
    let shown = move || {
        draft
            .get()
            .unwrap_or_else(|| synced.with(|v| fmt_num(v.as_f64().unwrap_or(0.0))))
    };
    let error = move || {
        let d = draft.get()?;
        match d.trim().parse::<f64>() {
            Ok(x) => keys::validate(key, &num_value(x))
                .err()
                .map(|e| e.to_string()),
            Err(_) => Some("not a number".into()),
        }
    };
    let step = move |delta: f64| {
        let cur = draft
            .get_untracked()
            .and_then(|d| d.trim().parse::<f64>().ok())
            .unwrap_or_else(|| app.latest(key).as_f64().unwrap_or(0.0));
        let next = round2((cur + delta).clamp(min, max));
        draft.set(None);
        app.set_latest(key, num_value(next));
    };
    let on_key = move |e: web_sys::KeyboardEvent| {
        let small = if fine && e.shift_key() { 0.25 } else { 1.0 };
        match e.key().as_str() {
            "ArrowUp" => step(small),
            "ArrowDown" => step(-small),
            "Enter" => {
                if let Some(x) = draft
                    .get_untracked()
                    .and_then(|d| d.trim().parse::<f64>().ok())
                {
                    if app
                        .commit(key, num_value(round2(x)), Some(base.get_value()))
                        .is_ok()
                    {
                        draft.set(None);
                    }
                }
            }
            "Escape" if draft.with_untracked(|d| d.is_some()) => draft.set(None),
            _ => return,
        }
        e.prevent_default();
    };
    view! {
        <div class="tin">
            <div class="spin">
                <input class="fin num short" class:bad=move || error().is_some() class:dirty=move || draft.with(|d| d.is_some())
                    inputmode="decimal" data-control=key spellcheck="false" autocomplete="off" prop:value=shown
                    on:input=move |e| {
                        // digits, '.', '-' only (ui-spec tab 2)
                        let t: String = event_target_value(&e).chars().filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-').collect();
                        if draft.with_untracked(|d| d.is_none()) { base.set_value(app.version(key)); }
                        draft.set(Some(t));
                    }
                    on:focus=move |_| app.ctx.set(Ctx::Body("spinner"))
                    on:keydown=on_key />
                <div class="spinbtns">
                    <button class="sb" tabindex="-1" title="+1 (↑)" on:mousedown=|e| e.prevent_default() on:click=move |_| step(1.0)>"▲"</button>
                    <button class="sb" tabindex="-1" title="−1 (↓)" on:mousedown=|e| e.prevent_default() on:click=move |_| step(-1.0)>"▼"</button>
                </div>
            </div>
            {move || error().map(|m| view! { <span class="ferr">"✗ "{m}</span> })}
            <span class="fhint">{format!("{} – {}{}", fmt_num(min), fmt_num(max), if fine { " · Shift+↑↓ ±0.25" } else { " · whole numbers" })}</span>
        </div>
    }
}

/// `number.percent` as a slider: keys and click/drag on the track.
#[component]
fn Slider() -> impl IntoView {
    const KEY: &str = "number.percent";
    let app = use_app();
    let synced = app.watch(KEY);
    // the value under the pointer while dragging (shown before it is saved)
    let live = RwSignal::new(None::<i64>);
    let dragging = StoredValue::new(false);
    let gen_ = StoredValue::new(0u64);
    let track = NodeRef::<leptos::html::Div>::new();
    let value = move || {
        live.get()
            .unwrap_or_else(|| synced.with(|v| v.as_i64().unwrap_or(0)))
    };
    let set = move |v: i64| app.set_latest(KEY, Value::from(v.clamp(0, 100)));
    // drags commit at most every 150 ms; the last value always lands
    let debounce = move |v: i64| {
        let g = gen_.get_value() + 1;
        gen_.set_value(g);
        set_timeout(
            move || {
                if gen_.try_get_value() == Some(g) {
                    set(v);
                }
            },
            Duration::from_millis(150),
        );
    };
    let at = move |x: f64| -> i64 {
        let Some(t) = track.get_untracked() else {
            return 0;
        };
        let r = t.get_bounding_client_rect();
        (((x - r.left()) / r.width().max(1.0)) * 100.0)
            .round()
            .clamp(0.0, 100.0) as i64
    };
    let on_key = move |e: web_sys::KeyboardEvent| {
        let cur = app.latest(KEY).as_i64().unwrap_or(0);
        let next = match e.key().as_str() {
            "ArrowRight" | "ArrowUp" => cur + 1,
            "ArrowLeft" | "ArrowDown" => cur - 1,
            "PageUp" => cur + 10,
            "PageDown" => cur - 10,
            "Home" => 0,
            "End" => 100,
            _ => return,
        };
        e.prevent_default();
        set(next);
    };
    view! {
        <div class="slider" tabindex="0" role="slider" aria-valuemin="0" aria-valuemax="100" aria-valuenow=move || value().to_string()
            data-control=KEY on:keydown=on_key on:focus=move |_| app.ctx.set(Ctx::Body("slider"))>
            <div class="strack" node_ref=track
                on:pointerdown=move |e: web_sys::PointerEvent| {
                    e.prevent_default();
                    if let Some(t) = e.current_target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) {
                        let _ = t.set_pointer_capture(e.pointer_id());
                    }
                    if let Some(s) = crate::util::query("[data-control='number.percent']") { crate::util::focus_el(&s); }
                    dragging.set_value(true);
                    let v = at(e.client_x() as f64);
                    live.set(Some(v));
                    debounce(v);
                }
                on:pointermove=move |e: web_sys::PointerEvent| {
                    if dragging.get_value() {
                        let v = at(e.client_x() as f64);
                        live.set(Some(v));
                        debounce(v);
                    }
                }
                on:pointerup=move |e: web_sys::PointerEvent| {
                    if dragging.get_value() {
                        dragging.set_value(false);
                        let v = at(e.client_x() as f64);
                        gen_.set_value(gen_.get_value() + 1);
                        set(v);
                        // keep showing it until the saved value arrives
                        set_timeout(move || { let _ = live.try_set(None); }, Duration::from_millis(400));
                    }
                }>
                <div class="sfill" style=move || format!("width:{}%", value())></div>
                <div class="sthumb" style=move || format!("left:{}%", value())></div>
                <div class="sticks">{[0, 25, 50, 75, 100].map(|t| view! { <span style=format!("left:{t}%")>{t}</span> })}</div>
            </div>
            <span class="sval">{move || format!("{}%", value())}</span>
        </div>
    }
}

#[component]
pub fn NumberTab() -> impl IntoView {
    view! {
        <TabFrame id="number">
            <Field label="Hours" key="number.hours"><Spinner key="number.hours" min=0.0 max=1000.0 fine=true /></Field>
            <Field label="Quantity" key="number.qty"><Spinner key="number.qty" min=0.0 max=99.0 /></Field>
            <Field label="Percent" key="number.percent"><Slider /></Field>
        </TabFrame>
    }
}
