//! Modal popups (ui-spec §3, tab 13): command palette, searchable help,
//! about, confirm, message, one-line input, the profile form and the file
//! list's context menu. Every popup carries `data-popup="<kind>"`.

use demo_core::dataset::TABS;
use demo_core::fuzzy;
use demo_core::keys::{MAX_EMAIL, MAX_NAME, Profile, valid_email};
use leptos::prelude::*;
use serde_json::json;
use wasm_bindgen::JsCast;

use crate::app::{
    Act, App, ConfirmAct, InputAct, Level, Modal, THEMES, confirm, input_done, run, use_app,
};
use crate::help::{GETTING_AROUND, GLOBAL, HOW, Keys, tab_keys};
use crate::util;
use crate::widgets::highlight;

#[component]
pub fn Modals() -> impl IntoView {
    let app = use_app();
    move || {
        let m = app.modal.get()?;
        Some(match m {
            Modal::Palette => view! { <PaletteDlg /> }.into_any(),
            Modal::Help => view! { <Help /> }.into_any(),
            Modal::About => view! { <About /> }.into_any(),
            Modal::Confirm {
                title,
                body,
                ok,
                act,
            } => view! { <Confirm title=title body=body ok=ok act=act /> }.into_any(),
            Modal::Message { title, body } => {
                view! { <Message title=title body=body /> }.into_any()
            }
            Modal::Input {
                title,
                label,
                value,
                act,
            } => view! { <InputDlg title=title label=label value=value act=act /> }.into_any(),
            Modal::Profile => view! { <ProfileForm /> }.into_any(),
            Modal::Context { x, y, file } => view! { <ContextMenu x=x y=y file=file /> }.into_any(),
        })
    }
}

/// Focus the first match of `sel` once rendered.
fn autofocus(sel: &'static str) {
    util::focus_later(sel);
}

/// Keep Tab inside the dialog.
fn trap_tab(e: &web_sys::KeyboardEvent) {
    if e.key() != "Tab" || e.default_prevented() {
        return;
    }
    let els =
        util::query_all(".dialog input, .dialog textarea, .dialog button, .dialog [tabindex='0']");
    let (Some(first), Some(last)) = (els.first(), els.last()) else {
        return;
    };
    let active = util::active_element();
    let at = |el: &web_sys::Element| active.as_ref().is_some_and(|a| a.is_same_node(Some(el)));
    if e.shift_key() && at(first) {
        e.prevent_default();
        util::focus_el(last);
    } else if !e.shift_key() && at(last) {
        e.prevent_default();
        util::focus_el(first);
    } else {
        // let the browser move focus, but keep the window handler out of it
        e.stop_propagation();
    }
}

#[component]
fn Dialog(
    title: String,
    #[prop(optional)] class: &'static str,
    #[prop(optional)] kind: Option<&'static str>,
    #[prop(optional)] on_save: Option<Callback<()>>,
    children: Children,
) -> impl IntoView {
    let app = use_app();
    let on_key = move |e: web_sys::KeyboardEvent| {
        if util::ctrl(&e) && (e.key() == "s" || e.key() == "S") {
            e.prevent_default();
            e.stop_propagation();
            if let Some(cb) = on_save {
                cb.run(());
            }
            return;
        }
        if e.key() == "Escape" {
            e.prevent_default();
            e.stop_propagation();
            app.close_modal();
            return;
        }
        trap_tab(&e);
    };
    let label = title.clone();
    view! {
        <div class="modal-back" on:mousedown=move |e| {
            let is_back = e.target().zip(e.current_target()).is_some_and(|(t, c)| t == c);
            if is_back { app.close_modal(); }
        }>
            <div class=format!("dialog {class}") role="dialog" aria-label=label data-popup=kind.unwrap_or("dialog") on:keydown=on_key>
                <div class="dlg-h"><span>{title}</span><button class="btn ghost sm" tabindex="-1" title="Close (Esc)" on:click=move |_| app.close_modal()>"✕"</button></div>
                {children()}
            </div>
        </div>
    }
}

// ---- command palette -------------------------------------------------------------------

/// Every command the palette offers (ui-spec §3), in display order.
pub fn palette_acts() -> Vec<Act> {
    let mut v: Vec<Act> = (0..TABS.len()).map(Act::GoTab).collect();
    v.extend([Act::ResetTab, Act::ResetAll]);
    v.extend((0..THEMES.len()).map(Act::Theme));
    v.extend([
        Act::ToggleKeys,
        Act::ToggleValue,
        Act::Help,
        Act::About,
        Act::MenuBar,
        Act::Leave,
    ]);
    v
}

/// Run a palette command: records it in `palette.last` (tab 12), then runs it.
pub fn run_palette(app: App, act: Act) {
    app.set("palette.last", json!(act.palette_label()));
    run(app, act);
}

/// The palette: search box plus fuzzy list. `inline` = the always-open copy on tab 12.
#[component]
pub fn PaletteList(inline: bool) -> impl IntoView {
    let app = use_app();
    let all: Vec<(Act, String, String)> = palette_acts()
        .into_iter()
        .map(|a| (a, a.palette_label(), a.key()))
        .collect();
    let all = StoredValue::new(all);
    let q = RwSignal::new(String::new());
    let sel = RwSignal::new(0usize);
    let results = Memo::new(move |_| {
        let q = q.get();
        all.with_value(|all| {
            fuzzy::rank(&q, all.iter().map(|(_, l, _)| l.as_str()))
                .into_iter()
                .map(|(i, m)| (all[i].clone(), m.indices))
                .collect::<Vec<_>>()
        })
    });
    let list = NodeRef::<leptos::html::Div>::new();
    Effect::new(move |_| {
        let s = sel.get();
        if let Some(el) = list.get() {
            if let Some(c) = el.children().item(s as u32) {
                util::scroll_into_view(&c);
            }
        }
    });
    let exec = move |i: usize| {
        let Some(((act, _, _), _)) = results.with_untracked(|r| r.get(i).cloned()) else {
            return;
        };
        if !inline {
            // give focus back before the palette goes, so a dialog the command opens returns there
            util::restore_focus();
            app.modal.set(None);
        }
        run_palette(app, act);
    };
    let on_key = move |e: web_sys::KeyboardEvent| {
        let n = results.with_untracked(|r| r.len());
        match e.key().as_str() {
            "ArrowDown" => sel.update(|s| *s = (*s + 1).min(n.saturating_sub(1))),
            "ArrowUp" => sel.update(|s| *s = s.saturating_sub(1)),
            "PageDown" => sel.update(|s| *s = (*s + 8).min(n.saturating_sub(1))),
            "PageUp" => sel.update(|s| *s = s.saturating_sub(8)),
            "Enter" => exec(sel.get_untracked()),
            _ => return,
        }
        e.prevent_default();
    };
    let ctl = inline.then_some("palette");
    view! {
        <div class="palbox" class:inline=inline>
            <input class="fin big" placeholder="Type a command…" spellcheck="false" autocomplete="off" data-control=ctl
                prop:value=move || q.get()
                on:input=move |e| { q.set(event_target_value(&e)); sel.set(0); }
                on:focus=move |_| app.ctx.set(crate::app::Ctx::Body("palette"))
                on:keydown=on_key />
            <div class="plist" node_ref=list>
                {move || {
                    let r = results.get();
                    if r.is_empty() {
                        return view! { <div class="pitem empty muted">"No matching command"</div> }.into_any();
                    }
                    r.into_iter().enumerate().map(|(i, ((_, label, key), m))| view! {
                        <div class="pitem" class:sel=move || sel.get() == i on:mouseenter=move |_| sel.set(i)
                            on:mousedown=move |e| { e.prevent_default(); exec(i); }>
                            <span class="pl">{highlight(&label, &m)}</span>
                            <kbd class="pk">{key}</kbd>
                        </div>
                    }).collect_view().into_any()
                }}
            </div>
        </div>
    }
}

#[component]
fn PaletteDlg() -> impl IntoView {
    autofocus(".palette input");
    view! {
        <Dialog title="Commands".into() class="palette" kind="palette">
            <PaletteList inline=false />
        </Dialog>
    }
}

// ---- help --------------------------------------------------------------------------------

#[component]
fn Help() -> impl IntoView {
    let app = use_app();
    let q = RwSignal::new(String::new());
    let body = NodeRef::<leptos::html::Div>::new();
    let tab = TABS[app.tab.get_untracked()];
    autofocus(".help input");
    let section = move |title: String, intro: Option<&'static str>, keys: Keys, f: &str| {
        let hit = |s: &str| f.is_empty() || s.to_lowercase().contains(f);
        let rows: Vec<(&str, &str)> = keys
            .iter()
            .copied()
            .filter(|(k, d)| hit(k) || hit(d) || hit(&title))
            .collect();
        let intro = intro.filter(|i| hit(i) || hit(&title));
        (!rows.is_empty() || intro.is_some()).then(|| {
            view! {
                <section>
                    <h4>{title}</h4>
                    {intro.map(|i| view! { <p class="hintro">{i}</p> })}
                    {rows.into_iter().map(|(k, d)| view! { <div class="hrowk"><kbd>{k}</kbd><span>{d}</span></div> }).collect_view()}
                </section>
            }
        })
    };
    let on_key = move |e: web_sys::KeyboardEvent| {
        let Some(b) = body.get_untracked() else {
            return;
        };
        let page = b.client_height() as f64 * 0.9;
        let dy = match e.key().as_str() {
            "ArrowDown" => 40.0,
            "ArrowUp" => -40.0,
            "PageDown" => page,
            "PageUp" => -page,
            _ => return,
        };
        e.prevent_default();
        b.scroll_by_with_x_and_y(0.0, dy);
    };
    view! {
        <Dialog title="Keyboard help".into() class="help" kind="help">
            <input class="fin" placeholder="Search the help…" spellcheck="false" prop:value=move || q.get()
                on:input=move |e| q.set(event_target_value(&e)) on:keydown=on_key />
            <div class="helpgrid" node_ref=body>
                {move || {
                    let f = q.get().to_lowercase();
                    let f = f.trim().to_string();
                    let mut out = vec![
                        section(format!("This tab: {}", tab.title), Some(tab.blurb), tab_keys(tab.id), &f),
                        section("Getting around".into(), None, GETTING_AROUND, &f),
                        section("Global keys".into(), None, GLOBAL, &f),
                    ];
                    out.extend(TABS.iter().map(|t| section(format!("{} {}", t.number, t.title), None, tab_keys(t.id), &f)));
                    out.push(section("How demo works".into(), Some(HOW), &[], &f));
                    let empty = out.iter().all(|s| s.is_none());
                    view! {
                        {out.into_iter().flatten().collect_view()}
                        {empty.then(|| view! { <p class="muted">"Nothing matches."</p> })}
                    }
                }}
            </div>
        </Dialog>
    }
}

#[component]
fn About() -> impl IntoView {
    let app = use_app();
    let srv = app.vals.with_untracked(|v| {
        v.server()
            .map(|s| format!("{} {}", s.implementation, s.version))
    });
    autofocus(".about button.primary");
    view! {
        <Dialog title="About demo".into() class="about">
            <dl class="kv">
                <dt>"Server"</dt><dd>{srv.unwrap_or_else(|| "—".into())}</dd>
                <dt>"Client"</dt><dd>{format!("web-rust {} · Leptos 0.8 (WebAssembly)", demo_core::VERSION)}</dd>
                <dt>"Protocol"</dt><dd>{demo_core::protocol::PROTOCOL_VERSION}</dd>
            </dl>
            <p class="muted pad">"A control gallery: every control on its own tab, shared live through one server."</p>
            <div class="dlg-f"><button class="btn primary" on:click=move |_| app.close_modal()>"OK"</button></div>
        </Dialog>
    }
}

#[component]
fn Confirm(title: String, body: String, ok: String, act: ConfirmAct) -> impl IntoView {
    let app = use_app();
    let act = StoredValue::new(act);
    let yes = move || {
        app.close_modal();
        confirm(app, act.get_value());
    };
    let no = move || {
        app.close_modal();
        if act.get_value() == ConfirmAct::Demo {
            app.say(Level::Info, "Confirm: you chose Cancel");
        }
    };
    autofocus(".confirm .btn.cancel");
    view! {
        <Dialog title=title class="confirm">
            <div on:keydown=move |e: web_sys::KeyboardEvent| match e.key().as_str() {
                "y" | "Y" => { e.prevent_default(); yes(); }
                "n" | "N" => { e.prevent_default(); no(); }
                _ => {}
            }>
                <p class="pad">{body}</p>
                <div class="dlg-f">
                    <button class="btn cancel" on:click=move |_| no()><u>"N"</u>"o, cancel"</button>
                    <button class="btn danger" on:click=move |_| yes()><u>"Y"</u>"es: "{ok}</button>
                </div>
            </div>
        </Dialog>
    }
}

#[component]
fn Message(title: String, body: String) -> impl IntoView {
    let app = use_app();
    autofocus(".message button.primary");
    view! {
        <Dialog title=title class="message">
            <p class="pad">"ℹ "{body}</p>
            <div class="dlg-f"><button class="btn primary" on:click=move |_| app.close_modal()>"OK"</button></div>
        </Dialog>
    }
}

#[component]
fn InputDlg(title: String, label: String, value: String, act: InputAct) -> impl IntoView {
    let app = use_app();
    let text = RwSignal::new(value);
    let err = RwSignal::new(None::<String>);
    let act = StoredValue::new(act);
    let done = move || {
        // close first so the dialog's own focus restore doesn't fight a follow-up
        match input_done(app, act.get_value(), text.get_untracked()) {
            Ok(()) => app.close_modal(),
            Err(e) => err.set(Some(e)),
        }
    };
    queue_microtask(|| {
        if let Some(i) = util::query(".inputdlg input")
            .and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok())
        {
            let _ = i.focus();
            i.select();
        }
    });
    view! {
        <Dialog title=title class="inputdlg">
            <div class="form1">
                <label class="flabel">{label}</label>
                <input class="fin" class:bad=move || err.get().is_some() spellcheck="false" prop:value=move || text.get()
                    on:input=move |e| { text.set(event_target_value(&e)); err.set(None); }
                    on:keydown=move |e: web_sys::KeyboardEvent| if e.key() == "Enter" { e.prevent_default(); done(); } />
                <Show when=move || err.get().is_some()><div class="ferr">"✗ "{move || err.get()}</div></Show>
            </div>
            <div class="dlg-f">
                <span class="muted">"Enter OK · Esc Cancel"</span>
                <button class="btn" on:click=move |_| app.close_modal()>"Cancel"</button>
                <button class="btn primary" on:click=move |_| done()>"OK"</button>
            </div>
        </Dialog>
    }
}

/// `dialog.profile` (tab 13): two validated fields; ^S or Enter saves.
#[component]
fn ProfileForm() -> impl IntoView {
    let app = use_app();
    let cur: Profile = app
        .vals
        .with_untracked(|v| v.get("dialog.profile").unwrap_or_default());
    let base = app.version("dialog.profile");
    let name = RwSignal::new(cur.name);
    let email = RwSignal::new(cur.email);
    let tried = RwSignal::new(false);
    let name_err = move || {
        let n = name.get();
        let n = n.trim();
        if n.is_empty() {
            Some("required".to_string())
        } else if n.chars().count() > MAX_NAME {
            Some(format!("at most {MAX_NAME} characters"))
        } else {
            None
        }
    };
    let email_err = move || {
        let e = email.get();
        let e = e.trim();
        if e.is_empty() {
            Some("required".to_string())
        } else if !valid_email(e) || e.len() > MAX_EMAIL {
            Some("must look like name@example.com".to_string())
        } else {
            None
        }
    };
    let save = Callback::new(move |_| {
        tried.set(true);
        if name_err().is_some() || email_err().is_some() {
            return;
        }
        let v = json!({"name": name.get_untracked(), "email": email.get_untracked()});
        if app.commit("dialog.profile", v, Some(base)).is_ok() {
            app.close_modal();
        }
    });
    autofocus(".profile input");
    let enter = move |e: web_sys::KeyboardEvent| {
        if e.key() == "Enter" {
            e.prevent_default();
            save.run(());
        }
    };
    view! {
        <Dialog title="Edit profile".into() class="profile" on_save=save>
            <div class="form">
                <div class="fld" class:bad=move || tried.get() && name_err().is_some()>
                    <label class="flabel">"Name"</label>
                    <input class="fin" data-field="name" prop:value=move || name.get() on:input=move |e| name.set(event_target_value(&e)) on:keydown=enter />
                    {move || name_err().filter(|_| tried.get()).map(|m| view! { <div class="ferr">"✗ "{m}</div> })}
                </div>
                <div class="fld" class:bad=move || tried.get() && email_err().is_some()>
                    <label class="flabel">"Email"</label>
                    <input class="fin" data-field="email" prop:value=move || email.get() on:input=move |e| email.set(event_target_value(&e)) on:keydown=enter />
                    {move || email_err().filter(|_| tried.get()).map(|m| view! { <div class="ferr">"✗ "{m}</div> })}
                </div>
            </div>
            <div class="dlg-f">
                <span class="muted">"Tab fields · ^S save · Esc cancel"</span>
                <button class="btn" on:click=move |_| app.close_modal()>"Cancel"</button>
                <button class="btn primary" on:click=move |_| save.run(())>"Save"</button>
            </div>
        </Dialog>
    }
}

// ---- context menu (tab 11 file list) -------------------------------------------------------------

#[component]
fn ContextMenu(x: f64, y: f64, file: String) -> impl IntoView {
    let app = use_app();
    let starred = crate::tabs::menus::files(app)
        .iter()
        .any(|f| f.name == file && f.starred);
    let items: Vec<&'static str> = vec![
        "Open",
        "Rename…",
        "Duplicate",
        if starred { "Unstar" } else { "Star" },
        "Delete…",
    ];
    let n = items.len();
    let sel = RwSignal::new(0usize);
    let file = StoredValue::new(file);
    let items = StoredValue::new(items);
    let exec = move |i: usize| {
        let label = items.with_value(|it| it[i]);
        app.close_modal();
        crate::tabs::menus::file_action(app, &file.get_value(), label);
    };
    let (vw, vh) = (
        util::window()
            .inner_width()
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(1200.0),
        util::window()
            .inner_height()
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(800.0),
    );
    let style = format!(
        "left:{}px;top:{}px",
        x.min(vw - 220.0).max(4.0),
        y.min(vh - 180.0).max(4.0)
    );
    autofocus(".ctxmenu");
    view! {
        <div class="modal-back clear" on:mousedown=move |e| {
            let is_back = e.target().zip(e.current_target()).is_some_and(|(t, c)| t == c);
            if is_back { app.close_modal(); }
        }>
            <div class="drop ctxmenu" tabindex="0" style=style data-popup="context-menu"
                on:keydown=move |e: web_sys::KeyboardEvent| {
                    match e.key().as_str() {
                        "ArrowDown" => sel.update(|s| *s = (*s + 1) % n),
                        "ArrowUp" => sel.update(|s| *s = (*s + n - 1) % n),
                        "Enter" | " " => exec(sel.get_untracked()),
                        "Escape" | "Tab" => app.close_modal(),
                        _ => {
                            let Some(c) = util::typed_char(&e) else { return };
                            let c = c.to_ascii_lowercase();
                            match items.with_value(|it| it.iter().position(|l| l.to_lowercase().starts_with(c))) {
                                Some(i) => sel.set(i),
                                None => return,
                            }
                        }
                    }
                    e.prevent_default();
                    e.stop_propagation();
                }>
                <div class="ctxh">{file.get_value()}</div>
                {items.get_value().into_iter().enumerate().map(|(i, l)| view! {
                    <div class="item" class:sel=move || sel.get() == i class:danger=l.starts_with("Delete")
                        on:mouseenter=move |_| sel.set(i) on:mousedown=move |e| { e.prevent_default(); exec(i); }>
                        <span class="il">{l}</span>
                    </div>
                }).collect_view()}
            </div>
        </div>
    }
}
