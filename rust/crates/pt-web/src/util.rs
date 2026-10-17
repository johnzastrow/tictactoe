//! Small browser helpers: time, storage, clipboard, DOM lookups.

use pt_core::Date;
use wasm_bindgen::JsCast;

pub fn window() -> web_sys::Window {
    leptos::prelude::window()
}

pub fn document() -> web_sys::Document {
    leptos::prelude::document()
}

/// Milliseconds since the epoch.
pub fn now_ms() -> f64 {
    js_sys::Date::now()
}

pub fn now_secs() -> i64 {
    (now_ms() / 1000.0) as i64
}

/// Today's date in the browser's local time zone.
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

pub fn load_json<T: serde::de::DeserializeOwned>(key: &str) -> Option<T> {
    serde_json::from_str(&load(key)?).ok()
}

pub fn save_json<T: serde::Serialize>(key: &str, v: &T) {
    if let Ok(s) = serde_json::to_string(v) {
        save(key, &s);
    }
}

/// Write text to the system clipboard (needs a secure context; returns
/// false when unavailable so callers can fall back to the in-app clipboard).
pub fn clipboard_write(text: &str) -> bool {
    if !window().is_secure_context() {
        return false;
    }
    let clip = window().navigator().clipboard();
    let _ = clip.write_text(text);
    true
}

pub fn by_id(id: &str) -> Option<web_sys::HtmlElement> {
    document()
        .get_element_by_id(id)
        .and_then(|e| e.dyn_into().ok())
}

pub fn focus_id(id: &str) {
    if let Some(e) = by_id(id) {
        let _ = e.focus();
    }
}

/// Focus after the next render (elements created by a signal change).
pub fn focus_later(id: &'static str) {
    leptos::prelude::request_animation_frame(move || focus_id(id));
}

pub fn active_element() -> Option<web_sys::Element> {
    document().active_element()
}

/// True if keyboard focus is in a text field.
pub fn typing_in_field() -> bool {
    active_element().is_some_and(|e| {
        let t = e.tag_name();
        t == "INPUT" || t == "TEXTAREA" || t == "SELECT"
    })
}

pub fn rect_of(el: &web_sys::Element) -> (f64, f64, f64, f64) {
    let r = el.get_bounding_client_rect();
    (r.left(), r.top(), r.width(), r.height())
}

pub fn viewport() -> (f64, f64) {
    let w = window()
        .inner_width()
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(1200.0);
    let h = window()
        .inner_height()
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(800.0);
    (w, h)
}

/// Ctrl on Windows/Linux, Cmd also accepted on macOS.
pub fn ctrl(e: &web_sys::KeyboardEvent) -> bool {
    e.ctrl_key() || e.meta_key()
}

/// Parse "text with\ttabs\nand lines" into rows of cells.
pub fn parse_tsv(s: &str) -> Vec<Vec<String>> {
    let s = s.strip_suffix('\n').unwrap_or(s);
    let s = s.strip_suffix('\r').unwrap_or(s);
    s.split('\n')
        .map(|line| {
            line.trim_end_matches('\r')
                .split('\t')
                .map(|c| c.to_string())
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn tsv() {
        assert_eq!(
            super::parse_tsv("a\tb\r\nc\td\n"),
            vec![vec!["a", "b"], vec!["c", "d"]]
        );
    }
}
