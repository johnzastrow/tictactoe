//! Tab 7, Text area: a multi-line editor; ^S saves, Esc reverts (asking
//! first if edited), leaving keeps the draft marked "● unsaved".

use demo_core::keys::MAX_NOTES;
use leptos::prelude::*;
use serde_json::Value;
use wasm_bindgen::JsCast;

use super::TabFrame;
use crate::app::{ConfirmAct, Ctx, Modal, use_app};
use crate::util;

const KEY: &str = "textarea.notes";

/// `Ln 2, Col 14` for a char offset.
fn line_col(text: &str, caret: usize) -> (usize, usize) {
    let before: String = text.chars().take(caret).collect();
    let ln = before.matches('\n').count() + 1;
    let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    (ln, col)
}

#[component]
pub fn TextareaTab() -> impl IntoView {
    let app = use_app();
    let synced = app.watch(KEY);
    let draft = RwSignal::new(None::<String>);
    let base = StoredValue::new(0u64);
    let caret = RwSignal::new(0usize);
    let focused = RwSignal::new(false);
    let area = NodeRef::<leptos::html::Textarea>::new();
    Effect::new(move |_| {
        if app.revert.with(|(k, _)| k == KEY) {
            draft.set(None);
        }
    });
    let text = move || {
        draft
            .get()
            .unwrap_or_else(|| synced.with(|v| v.as_str().unwrap_or("").to_string()))
    };
    let footer = move || {
        let t = text();
        let (ln, col) = line_col(&t, caret.get());
        let words = t.split_whitespace().count();
        format!(
            "Ln {ln}, Col {col} · {} · {}/{MAX_NOTES}",
            util::plural(words, "word", "words"),
            t.chars().count()
        )
    };
    let too_long = move || text().chars().count() > MAX_NOTES;
    let update_caret = move || {
        if let Some(a) = area.get_untracked() {
            // selectionStart counts UTF-16 units; close enough for the footer
            let _ = caret.try_set(a.selection_start().ok().flatten().unwrap_or(0) as usize);
        }
    };
    let on_key = move |e: web_sys::KeyboardEvent| {
        if util::ctrl(&e) && (e.key() == "s" || e.key() == "S") {
            e.prevent_default();
            if let Some(d) = draft.get_untracked() {
                if app
                    .commit(KEY, Value::String(d), Some(base.get_value()))
                    .is_ok()
                {
                    draft.set(None);
                }
            }
            return;
        }
        match e.key().as_str() {
            // ^Home / ^End: start / end of the text on every platform (macOS
            // has no native Cmd+End in a textarea)
            "Home" | "End" if util::ctrl(&e) => {
                e.prevent_default();
                if let Some(a) = area.get_untracked() {
                    let pos = if e.key() == "Home" {
                        0
                    } else {
                        a.value().encode_utf16().count() as u32
                    };
                    let _ = a.set_selection_range(pos, pos);
                    if pos == 0 {
                        a.set_scroll_top(0);
                    } else {
                        a.set_scroll_top(a.scroll_height());
                    }
                }
                update_caret();
            }
            "Escape" if draft.with_untracked(|d| d.is_some()) => {
                e.prevent_default();
                app.open(Modal::Confirm {
                    title: "Discard your edits?".into(),
                    body: "Revert the notes to the shared version? Your unsaved text is lost."
                        .into(),
                    ok: "Revert".into(),
                    act: ConfirmAct::RevertNotes,
                });
            }
            // Tab leaves the editor (no tab characters), keeping the draft
            "Tab" => {}
            _ => {}
        }
    };
    view! {
        <TabFrame id="textarea">
            <div class="frow" class:flash=move || app.flash.with(|f| f.contains(KEY))>
                <label class="flabel2">"Notes"
                    {move || (draft.with(|d| d.is_some()) && !focused.get()).then(|| view! { <span class="unsaved">" ● unsaved"</span> })}
                </label>
                <div class="fctl wide">
                    <textarea class="fin area notes" class:bad=too_long class:dirty=move || draft.with(|d| d.is_some())
                        node_ref=area data-control=KEY spellcheck="false" prop:value=text
                        on:input=move |e| {
                            if draft.with_untracked(|d| d.is_none()) { base.set_value(app.version(KEY)); }
                            let v = e.target().and_then(|t| t.dyn_into::<web_sys::HtmlTextAreaElement>().ok()).map(|t| t.value()).unwrap_or_default();
                            draft.set(Some(v));
                            update_caret();
                        }
                        on:keydown=on_key
                        on:keyup=move |_| update_caret()
                        on:click=move |_| update_caret()
                        on:focus=move |_| { focused.set(true); app.ctx.set(Ctx::Body("textarea")); }
                        on:blur=move |_| { let _ = focused.try_set(false); }></textarea>
                    <div class="tafoot">
                        <span>{footer}</span>
                        <span class="spacer"></span>
                        {move || draft.with(|d| d.is_some()).then(|| view! { <span class="fwarn">"● unsaved · ^S saves · Esc reverts"</span> })}
                        {move || too_long().then(|| view! { <span class="ferr">{format!("✗ at most {MAX_NOTES} characters")}</span> })}
                    </div>
                </div>
            </div>
        </TabFrame>
    }
}
