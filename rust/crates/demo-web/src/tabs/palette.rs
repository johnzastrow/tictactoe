//! Tab 12, Palette: the command palette shown inline and always open; any
//! run (here or ^K) records its label in `palette.last`.

use leptos::prelude::*;

use super::TabFrame;
use crate::app::use_app;
use crate::dialogs::PaletteList;

#[component]
pub fn PaletteTab() -> impl IntoView {
    let app = use_app();
    let last = app.watch("palette.last");
    view! {
        <TabFrame id="palette">
            <div class="lastcmd" class:flash=move || app.flash.with(|f| f.contains("palette.last"))>
                "Last command: "
                <b>{move || { let s = last.with(|v| v.as_str().unwrap_or("").to_string()); if s.is_empty() { "—".to_string() } else { demo_core::safe_text(&s) } }}</b>
                {move || { let b = app.by("palette.last"); (!b.is_empty()).then(|| format!(" · by {}", demo_core::safe_text(&b))) }}
            </div>
            <PaletteList inline=true />
        </TabFrame>
    }
}
