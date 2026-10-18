//! Tab 4, Dropdown: status and priority as closed lists with coloured chips.

use demo_core::dataset::{PRIORITIES, STATUSES};
use leptos::prelude::*;
use serde_json::json;

use super::{Field, TabFrame};
use crate::app::use_app;
use crate::widgets::{Dropdown, opts};

#[component]
pub fn SelectTab() -> impl IntoView {
    let app = use_app();
    let status = app.watch("select.status");
    let priority = app.watch("select.priority");
    view! {
        <TabFrame id="select">
            <Field label="Status" key="select.status">
                <Dropdown ctl="select.status" options=Signal::derive(|| opts(&STATUSES))
                    value=Signal::derive(move || status.with(|v| v.as_str().map(String::from)))
                    on_pick=Callback::new(move |id: String| app.set("select.status", json!(id))) />
            </Field>
            <Field label="Priority" key="select.priority">
                <Dropdown ctl="select.priority" options=Signal::derive(|| opts(&PRIORITIES))
                    value=Signal::derive(move || priority.with(|v| v.as_str().map(String::from)))
                    on_pick=Callback::new(move |id: String| app.set("select.priority", json!(id))) />
            </Field>
        </TabFrame>
    }
}
