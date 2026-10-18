//! The frame around the tabs (ui-spec §1–3, §5): join page, menu bar,
//! sidebar with presence dots, status line, clickable hint bar, toasts, and
//! the window-level keyboard handler.

use demo_core::dataset::{TABS, initials};
use demo_core::protocol::clean_name;
use demo_core::safe_text;
use leptos::prelude::*;

use crate::app::{
    Act, App, ConnState, Ctx, THEMES, focus_body, focus_sidebar, run, stops, use_app,
};
use crate::help::{HA, Hint, hints};
use crate::menu::{MItem, MenuBar, MenuDef, Outcome, menu_key};
use crate::util;

// ---- join ------------------------------------------------------------------------------

#[component]
pub fn Join() -> impl IntoView {
    let app = use_app();
    let name = RwSignal::new(util::load("demo.name").unwrap_or_default());
    let submit = move |e: leptos::ev::SubmitEvent| {
        e.prevent_default();
        match clean_name(&name.get_untracked()) {
            Some(n) => app.join(n),
            None => app.join_error.set(Some(
                "A name starts with a letter, then up to 23 letters, digits, spaces or _ . -"
                    .into(),
            )),
        }
    };
    util::focus_later("#join-name");
    view! {
        <div class="join-wrap">
            <form class="join" on:submit=submit>
                <div class="brand">"demo"<span>" · control gallery"</span></div>
                <p class="muted">"Every control on its own tab, shared live with everyone connected (TUI or browser)."</p>
                <label>"Your name"
                    <input id="join-name" data-control="join.name" autocomplete="nickname" maxlength="24" spellcheck="false"
                        prop:value=move || name.get() on:input=move |e| name.set(event_target_value(&e)) />
                </label>
                <Show when=move || app.join_error.get().is_some()>
                    <div class="lerr">"✗ "{move || app.join_error.get()}</div>
                </Show>
                <button class="btn primary" type="submit" data-control="join.go">"Join"</button>
                <div class="lfoot">"No password: the name is only a label others see. Tip: add ?name=alice to the URL to join directly."</div>
            </form>
        </div>
    }
}

// ---- menu bar --------------------------------------------------------------------------

/// File / View / Help (ui-spec §3), with ✓ on the current tab, theme and panels.
pub fn app_menus(app: App) -> Vec<MenuDef> {
    let tab = app.tab.get();
    let theme = app.theme.get();
    let mut view: Vec<MItem> = (0..TABS.len())
        .map(|i| MItem::act(Act::GoTab(i)).checked(i == tab))
        .collect();
    view.extend([
        MItem::sep(),
        MItem::act(Act::NextTab),
        MItem::act(Act::PrevTab),
        MItem::sep(),
    ]);
    view.push(MItem::sub(
        "Theme",
        (0..THEMES.len())
            .map(|i| MItem::act(Act::Theme(i)).checked(i == theme))
            .collect(),
    ));
    view.extend([
        MItem::sep(),
        MItem::act(Act::ToggleKeys).checked(app.show_keys.get()),
        MItem::act(Act::ToggleValue).checked(app.show_value.get()),
    ]);
    vec![
        MenuDef {
            title: "File",
            items: vec![
                MItem::act(Act::ResetTab),
                MItem::act(Act::ResetAll),
                MItem::sep(),
                MItem::act(Act::Leave),
            ],
        },
        MenuDef {
            title: "View",
            items: view,
        },
        MenuDef {
            title: "Help",
            items: vec![
                MItem::act(Act::Help),
                MItem::act(Act::Palette),
                MItem::sep(),
                MItem::act(Act::About),
            ],
        },
    ]
}

#[component]
pub fn TopBar() -> impl IntoView {
    let app = use_app();
    let dot = move || match app.conn.get() {
        ConnState::Online => ("cdot dot-on", "connected"),
        ConnState::Connecting => ("cdot dot-wait", "reconnecting"),
        ConnState::Offline => ("cdot dot-off", "offline: reconnecting"),
    };
    let on_pick = Callback::new(move |(_, act): (Vec<String>, Option<Act>)| {
        if let Some(a) = act {
            run(app, a);
        }
    });
    view! {
        <div class="menubar top">
            <span class="logo">"demo"</span>
            <MenuBar menus=Signal::derive(move || app_menus(app)) open=app.menu path=app.menu_path on_pick=on_pick />
            <span class="spacer"></span>
            <span class="who">
                "demo · "{move || safe_text(&app.name.get().unwrap_or_default())}" · "
                <span class="conn" title=move || dot().1>
                    <i class=move || dot().0></i>
                    {move || app.vals.with(|v| v.server().map(|s| format!(" {} {}", s.implementation, s.version)).unwrap_or_default())}
                </span>
            </span>
        </div>
    }
}

// ---- sidebar ---------------------------------------------------------------------------

#[component]
pub fn Sidebar() -> impl IntoView {
    let app = use_app();
    let on_key = move |e: web_sys::KeyboardEvent| {
        let n = TABS.len();
        let cur = app.tab.get_untracked();
        match e.key().as_str() {
            "ArrowDown" if !e.alt_key() => app.go_tab((cur + 1).min(n - 1)),
            "ArrowUp" if !e.alt_key() => app.go_tab(cur.saturating_sub(1)),
            "Home" => app.go_tab(0),
            "End" => app.go_tab(n - 1),
            "Enter" | "ArrowRight" => focus_body(),
            "Tab" if !e.shift_key() => focus_body(),
            _ => return,
        }
        e.prevent_default();
    };
    view! {
        <nav id="sidebar" class="sidebar" tabindex="0" on:keydown=on_key on:focus=move |_| app.ctx.set(Ctx::Sidebar)>
            {TABS.iter().enumerate().map(|(i, t)| {
                let id = t.id;
                let cur = move || app.tab.get() == i;
                let dots = move || app.vals.with(|v| v.others_on(id).iter().map(|p| (p.color.clone(), safe_text(&p.name))).collect::<Vec<_>>());
                view! {
                    <div class="side-item" class:cur=cur data-tab-link=id title=format!("{} (Alt+{})", t.title, (i + 1) % 10)
                        on:mousedown=move |e| { e.prevent_default(); app.go_tab(i); util::focus_sel("#sidebar"); }>
                        <span class="mark">{move || if cur() { "▸" } else { "" }}</span>
                        <span class="num">{t.number}</span>
                        <span class="st">{t.title}</span>
                        <span class="pdots">
                            {move || dots().into_iter().map(|(c, n)| view! { <i class="pdot" style=format!("background:{c}") title=n></i> }).collect_view()}
                        </span>
                    </div>
                }
            }).collect_view()}
        </nav>
    }
}

// ---- status line, hint bar, toasts -----------------------------------------------------------

#[component]
pub fn StatusLine() -> impl IntoView {
    let app = use_app();
    let peers = move || app.vals.with(|v| v.peers().to_vec());
    view! {
        <div class="statusline">
            <span class=move || format!("smsg {}", app.status.get().0.class())>
                {move || { let (l, t) = app.status.get(); format!("{} {t}", l.icon()) }}
            </span>
            <span class="spacer"></span>
            <span class="avs">
                {move || peers().into_iter().map(|p| {
                    let title = format!("{} · {} · {}", safe_text(&p.name), p.client, p.tab.clone().unwrap_or_else(|| "—".into()));
                    view! { <span class="av" style=format!("background:{}", p.color) title=title>{initials(&p.name)}</span> }
                }).collect_view()}
            </span>
            <span class="muted online">{move || format!("online: {}", app.vals.with(|v| v.peers().len()))}</span>
        </div>
    }
}

#[component]
pub fn HintBar() -> impl IntoView {
    let app = use_app();
    let render = move |hs: Vec<Hint>| {
        hs.into_iter()
            .map(|(k, l, a)| {
                let click = !matches!(a, HA::None);
                view! {
                    <span class="hint" class:click=click on:mousedown=move |e| {
                        // keep focus where it is, then run the hint
                        e.prevent_default();
                        match a {
                            HA::Act(act) => run(app, act),
                            HA::Key(key, c, s) => util::press(key, c, s),
                            HA::None => {}
                        }
                    }>
                        <kbd>{k}</kbd>" "{l}
                    </span>
                }
            })
            .collect_view()
    };
    view! {
        <div class="hintbar">
            <div class="hrow">{move || render(hints(app).0)}</div>
            <div class="hrow">{move || render(hints(app).1)}</div>
        </div>
    }
}

#[component]
pub fn Toasts() -> impl IntoView {
    let app = use_app();
    view! {
        <div class="toasts">
            <For each=move || app.toasts.get() key=|t| t.id children=move |t| {
                let cls = format!("toast {}", t.level.class());
                let id = t.id;
                view! {
                    <div class=cls on:click=move |_| app.toasts.update(|v| v.retain(|x| x.id != id))>
                        <b>{t.level.icon()}</b>" "{t.text.clone()}
                    </div>
                }
            } />
        </div>
    }
}

// ---- global keys -------------------------------------------------------------------------------

/// Window-level keydown: app-wide shortcuts (ui-spec §2–3, Web column).
/// Controls handle their own keys first and `preventDefault` what they use.
pub fn global_key(app: App, e: web_sys::KeyboardEvent) {
    if app.name.get_untracked().is_none() || e.default_prevented() {
        return;
    }
    if app.menu.get_untracked().is_some() && menu_capture(app, &e) {
        return;
    }
    let k = e.key();
    let code = e.code();
    let ctrl = util::ctrl(&e);
    let alt = e.alt_key();
    let shift = e.shift_key();
    let modal = app.modal.get_untracked();
    let act: Option<Act> = match (k.as_str(), ctrl, alt) {
        ("F1", _, _) => Some(Act::Help),
        ("g" | "G", true, false) => Some(Act::Help),
        _ if modal.is_some() => None,
        ("F10", false, false) if !shift => Some(Act::MenuBar),
        ("k" | "K" | "p" | "P", true, false) => Some(Act::Palette),
        ("r" | "R", true, false) => Some(Act::ResetTab),
        ("ArrowDown", false, true) => Some(Act::NextTab),
        ("ArrowUp", false, true) => Some(Act::PrevTab),
        ("F6", _, _) => {
            if in_sidebar() {
                focus_body();
            } else {
                focus_sidebar();
            }
            e.prevent_default();
            None
        }
        ("Tab", false, false) => {
            tab_focus(shift);
            e.prevent_default();
            None
        }
        ("Escape", _, _) => {
            app.popup.set(None);
            focus_sidebar();
            None
        }
        ("ArrowDown" | "ArrowUp" | "Enter", false, false) if no_focus() => {
            util::focus_sel("#sidebar");
            None
        }
        _ if alt && !ctrl => {
            // Alt+digit = tab, Alt+letter = menu (use `code`: Option on macOS types symbols)
            if let Some(d) = code
                .strip_prefix("Digit")
                .and_then(|d| d.parse::<usize>().ok())
            {
                Some(Act::GoTab(if d == 0 { 9 } else { d - 1 }))
            } else if let Some(i) = ["KeyF", "KeyV", "KeyH"].iter().position(|c| *c == code) {
                app.menu_path.set(vec![0]);
                app.menu.set(Some(i));
                e.prevent_default();
                None
            } else {
                None
            }
        }
        _ => None,
    };
    if let Some(a) = act {
        e.prevent_default();
        run(app, a);
    }
}

/// Keys for the open app menu bar; true if the key was the menu's.
pub fn menu_capture(app: App, e: &web_sys::KeyboardEvent) -> bool {
    let menus = app_menus(app);
    match menu_key(&menus, app.menu, app.menu_path, e) {
        Outcome::Picked(_, Some(a)) => run(app, a),
        Outcome::Ignored => return false,
        _ => {}
    }
    if e.key() != "Tab" {
        e.prevent_default();
    }
    true
}

fn in_sidebar() -> bool {
    util::active_element().is_some_and(|a| util::within(&a, "#sidebar"))
}

fn no_focus() -> bool {
    util::active_element().is_none_or(|a| a.tag_name() == "BODY")
}

/// Tab / Shift+Tab between the controls of the tab body; Shift+Tab from
/// the first control (or Tab from the sidebar) crosses to the other area.
fn tab_focus(back: bool) {
    let st = stops();
    let active = util::active_element();
    if in_sidebar() || no_focus() {
        if back {
            util::focus_sel("#sidebar");
        } else {
            focus_body();
        }
        return;
    }
    let idx = active.and_then(|a| {
        st.iter()
            .position(|s| s.is_same_node(Some(&a)) || s.contains(Some(&a)))
    });
    match idx {
        Some(0) if back => util::focus_sel("#sidebar"),
        Some(i) if back => util::focus_stop(&st[i - 1]),
        Some(i) => util::focus_stop(&st[(i + 1) % st.len()]),
        None => util::focus_sel("#sidebar"),
    }
}
