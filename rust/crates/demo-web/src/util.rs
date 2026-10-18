//! Small browser helpers: time, storage, DOM lookups, focus.

use std::cell::RefCell;

use demo_core::Date;
use wasm_bindgen::JsCast;

pub fn window() -> web_sys::Window {
    leptos::prelude::window()
}

pub fn document() -> web_sys::Document {
    leptos::prelude::document()
}

pub fn now_secs() -> i64 {
    (js_sys::Date::now() / 1000.0) as i64
}

/// Today's date in the browser's local time zone (natural dates are local).
pub fn today() -> Date {
    let d = js_sys::Date::new_0();
    Date::from_ymd(d.get_full_year() as i32, d.get_month() + 1, d.get_date())
        .unwrap_or(Date::from_days(0))
}

/// localStorage can be missing or throw (private mode, blocked storage):
/// every access is fallible and callers fall back to defaults.
fn storage() -> Option<web_sys::Storage> {
    window().local_storage().ok().flatten()
}

pub fn load(key: &str) -> Option<String> {
    storage()?.get_item(key).ok().flatten()
}

pub fn save(key: &str, value: &str) {
    if let Some(s) = storage() {
        let _ = s.set_item(key, value);
    }
}

/// A query-string parameter of the page URL (`?name=alice`).
pub fn query_param(name: &str) -> Option<String> {
    let search = window().location().search().ok()?;
    web_sys::UrlSearchParams::new_with_str(&search)
        .ok()?
        .get(name)
}

/// Drop the query string without reloading (File ▸ Leave must not rejoin on reload).
pub fn clear_query() {
    if let Ok(h) = window().history() {
        let path = window()
            .location()
            .pathname()
            .unwrap_or_else(|_| "/".into());
        let _ = h.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&path));
    }
}

pub fn active_element() -> Option<web_sys::Element> {
    document().active_element()
}

pub fn focus_el(el: &web_sys::Element) {
    if let Ok(h) = el.clone().dyn_into::<web_sys::HtmlElement>() {
        let _ = h.focus();
    }
}

/// Focus a control reached by keyboard (Enter from the sidebar, Tab, F6).
/// A multi-line editor starts with the caret at the very start of its text,
/// as in both TUIs; a mouse click still puts the caret where clicked.
pub fn focus_stop(el: &web_sys::Element) {
    focus_el(el);
    if let Ok(t) = el.clone().dyn_into::<web_sys::HtmlTextAreaElement>() {
        let _ = t.set_selection_range(0, 0);
        t.set_scroll_top(0);
    }
}

pub fn query(sel: &str) -> Option<web_sys::Element> {
    document().query_selector(sel).ok().flatten()
}

pub fn query_all(sel: &str) -> Vec<web_sys::Element> {
    let Ok(list) = document().query_selector_all(sel) else {
        return vec![];
    };
    (0..list.length())
        .filter_map(|i| list.item(i))
        .filter_map(|n| n.dyn_into().ok())
        .collect()
}

pub fn focus_sel(sel: &str) {
    if let Some(e) = query(sel) {
        focus_el(&e);
    }
}

/// Focus once the current render is mounted (elements created by a signal
/// change). A microtask, not an animation frame, so keys typed right after
/// (a script, a fast typist) already land in the new element.
pub fn focus_later(sel: &'static str) {
    leptos::prelude::queue_microtask(move || focus_sel(sel));
}

/// True if `el` is `sel` or inside it.
pub fn within(el: &web_sys::Element, sel: &str) -> bool {
    el.closest(sel).ok().flatten().is_some()
}

/// Ctrl on Windows/Linux; Cmd also accepted on macOS.
pub fn ctrl(e: &web_sys::KeyboardEvent) -> bool {
    e.ctrl_key() || e.meta_key()
}

/// A single printable character typed without Ctrl/Alt (for type-to-filter).
pub fn typed_char(e: &web_sys::KeyboardEvent) -> Option<char> {
    if ctrl(e) || e.alt_key() {
        return None;
    }
    let k = e.key();
    let mut it = k.chars();
    match (it.next(), it.next()) {
        (Some(c), None) if !c.is_control() => Some(c),
        _ => None,
    }
}

pub fn rect_of(el: &web_sys::Element) -> (f64, f64, f64, f64) {
    let r = el.get_bounding_client_rect();
    (r.left(), r.top(), r.width(), r.height())
}

pub fn scroll_into_view(el: &web_sys::Element) {
    let o = web_sys::ScrollIntoViewOptions::new();
    o.set_block(web_sys::ScrollLogicalPosition::Nearest);
    o.set_inline(web_sys::ScrollLogicalPosition::Nearest);
    el.scroll_into_view_with_scroll_into_view_options(&o);
}

thread_local! {
    /// Where focus was when a dialog opened, to give it back on close.
    static RETURN_FOCUS: RefCell<Option<web_sys::Element>> = const { RefCell::new(None) };
}

pub fn remember_focus() {
    let el = active_element().filter(|e| e.tag_name() != "BODY");
    RETURN_FOCUS.with(|r| *r.borrow_mut() = el);
}

/// Refocus what had focus before the dialog; false if it is gone.
pub fn restore_focus() -> bool {
    match RETURN_FOCUS.with(|r| r.borrow_mut().take()) {
        Some(el) if el.is_connected() => {
            focus_el(&el);
            true
        }
        _ => false,
    }
}

/// Send a synthetic keydown to the focused element (a clicked hint "presses" its key).
pub fn press(key: &str, ctrl: bool, shift: bool) {
    let init = web_sys::KeyboardEventInit::new();
    init.set_key(key);
    init.set_ctrl_key(ctrl);
    init.set_shift_key(shift);
    init.set_bubbles(true);
    init.set_cancelable(true);
    let Ok(ev) = web_sys::KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &init) else {
        return;
    };
    let target: web_sys::EventTarget = match active_element() {
        Some(e) => e.into(),
        None => document().into(),
    };
    let _ = target.dispatch_event(&ev);
}

/// "1 project" / "3 projects".
pub fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}
