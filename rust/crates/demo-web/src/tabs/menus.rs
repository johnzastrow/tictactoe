//! Tab 11, Menus: (a) a demo menu bar with submenus, keys, separators, a
//! disabled and a checkable item; (b) the `menus.files` list with a context
//! menu (Open · Rename… · Duplicate · Star · Delete…).

use demo_core::keys::{FileEntry, MAX_FILE_NAME, MAX_FILES};
use leptos::prelude::*;
use serde_json::json;

use super::TabFrame;
use crate::app::{Act, App, ConfirmAct, Ctx, InputAct, Level, Modal, run, use_app};
use crate::menu::{MItem, MenuBar, MenuDef, Outcome, menu_key};
use crate::util;

const KEY: &str = "menus.files";

pub fn files(app: App) -> Vec<FileEntry> {
    serde_json::from_value(app.latest(KEY)).unwrap_or_default()
}

fn demo_menus(wrap: bool) -> Vec<MenuDef> {
    let leaf = MItem::new;
    vec![
        MenuDef {
            title: "Document",
            items: vec![
                leaf("New", "^N"),
                leaf("Open…", "^O"),
                leaf("Save", "^S"),
                MItem::sep(),
                MItem::sub(
                    "Export",
                    vec![leaf("PDF", ""), leaf("PNG", ""), leaf("Markdown", "")],
                ),
                MItem::sep(),
                leaf("Print…", "^P").disabled(),
                leaf("Close", "^W"),
            ],
        },
        MenuDef {
            title: "Format",
            items: vec![
                MItem::sub(
                    "Text",
                    vec![
                        leaf("Bold", "^B"),
                        leaf("Italic", "^I"),
                        leaf("Underline", "^U"),
                        MItem::sep(),
                        leaf("Strikethrough", ""),
                    ],
                ),
                MItem::sub(
                    "Paragraph",
                    vec![
                        leaf("Align left", ""),
                        leaf("Center", ""),
                        leaf("Align right", ""),
                        leaf("Justify", ""),
                    ],
                ),
                MItem::sep(),
                MItem {
                    act: Some(Act::DemoWrap),
                    ..leaf("Word wrap", "")
                }
                .checked(wrap),
                leaf("Clear formatting", ""),
            ],
        },
        MenuDef {
            title: "Arrange",
            items: vec![
                leaf("Bring to front", ""),
                leaf("Send to back", ""),
                MItem::sep(),
                leaf("Group", "^G"),
                leaf("Ungroup", "").disabled(),
                MItem::sep(),
                MItem::sub(
                    "Align",
                    vec![leaf("Left", ""), leaf("Centre", ""), leaf("Right", "")],
                ),
            ],
        },
    ]
}

fn save(app: App, list: Vec<FileEntry>) {
    app.set_latest(KEY, json!(list));
}

/// A free name like "Logo copy.svg", "Logo copy 2.svg".
fn copy_name(list: &[FileEntry], name: &str) -> String {
    let (stem, ext) = match name.rfind('.') {
        Some(p) if p > 0 => (&name[..p], &name[p..]),
        _ => (name, ""),
    };
    (1..)
        .map(|i| {
            if i == 1 {
                format!("{stem} copy{ext}")
            } else {
                format!("{stem} copy {i}{ext}")
            }
        })
        .find(|n| !list.iter().any(|f| f.name == *n))
        .unwrap_or_default()
}

/// Run a context-menu item on a file.
pub fn file_action(app: App, name: &str, label: &str) {
    let mut list = files(app);
    let Some(pos) = list.iter().position(|f| f.name == name) else {
        return;
    };
    match label {
        "Open" => app.open(Modal::Message {
            title: "Open".into(),
            body: format!("Opened \"{name}\" (a demo: nothing really opens)."),
        }),
        "Rename…" => app.open(Modal::Input {
            title: "Rename file".into(),
            label: "New name".into(),
            value: name.into(),
            act: InputAct::RenameFile(name.into()),
        }),
        "Duplicate" => {
            if list.len() >= MAX_FILES {
                app.say(Level::Err, format!("at most {MAX_FILES} files"));
                return;
            }
            let copy = copy_name(&list, name);
            list.insert(
                pos + 1,
                FileEntry {
                    name: copy,
                    starred: false,
                },
            );
            save(app, list);
        }
        "Star" | "Unstar" => {
            list[pos].starred = !list[pos].starred;
            save(app, list);
        }
        "Delete…" => app.open(Modal::Confirm {
            title: "Delete file?".into(),
            body: format!("Delete \"{name}\" for everyone?"),
            ok: "Delete".into(),
            act: ConfirmAct::DeleteFile(name.into()),
        }),
        _ => {}
    }
}

pub fn rename_file(app: App, old: &str, new: &str) -> Result<(), String> {
    let new = new.trim();
    let mut list = files(app);
    if new.is_empty() || new.chars().count() > MAX_FILE_NAME {
        return Err(format!("1–{MAX_FILE_NAME} characters"));
    }
    if new != old && list.iter().any(|f| f.name == new) {
        return Err("a file with that name exists".into());
    }
    if let Some(f) = list.iter_mut().find(|f| f.name == old) {
        f.name = new.to_string();
    }
    save(app, list);
    Ok(())
}

pub fn delete_file(app: App, name: &str) {
    let mut list = files(app);
    list.retain(|f| f.name != name);
    save(app, list);
}

#[component]
pub fn MenusTab() -> impl IntoView {
    let app = use_app();
    // demo menu bar state, local to the tab
    let open = RwSignal::new(None::<usize>);
    let path = RwSignal::new(vec![0usize]);
    let title = RwSignal::new(0usize);
    let menus = Signal::derive(move || demo_menus(app.demo_wrap.get()));
    Effect::new(move |prev: Option<bool>| {
        let o = open.get().is_some();
        if prev.is_some() || o {
            app.popup.set(o.then_some("menu"));
        }
        o
    });
    on_cleanup(move || {
        if open.try_get_untracked().flatten().is_some() {
            let _ = app.popup.try_set(None);
        }
    });
    let picked = move |(labels, act): (Vec<String>, Option<Act>)| match act {
        Some(Act::DemoWrap) => {
            run(app, Act::DemoWrap);
            app.say(
                Level::Info,
                format!(
                    "Word wrap {}",
                    if app.demo_wrap.get_untracked() {
                        "on"
                    } else {
                        "off"
                    }
                ),
            );
        }
        _ => app.say(Level::Info, format!("Picked {}", labels.join(" ▸ "))),
    };
    let on_pick = Callback::new(picked);
    let bar = NodeRef::<leptos::html::Div>::new();
    let on_bar_key = move |e: web_sys::KeyboardEvent| {
        if let Some(m) = open.get_untracked() {
            title.set(m);
            let ms = menus.get_untracked();
            match menu_key(&ms, open, path, &e) {
                Outcome::Picked(l, a) => picked((l, a)),
                Outcome::Ignored => return,
                Outcome::Handled => {}
            }
            if e.key() != "Tab" {
                e.prevent_default();
            }
            if let Some(m) = open.get_untracked() {
                title.set(m);
            }
            return;
        }
        let n = 3;
        match e.key().as_str() {
            "Enter" | "ArrowDown" | " " => {
                path.set(vec![0]);
                open.set(Some(title.get_untracked()));
            }
            "ArrowRight" => title.update(|t| *t = (*t + 1) % n),
            "ArrowLeft" => title.update(|t| *t = (*t + n - 1) % n),
            _ => return,
        }
        e.prevent_default();
    };

    // file list
    let raw = app.watch(KEY);
    let list = Memo::new(move |_| {
        raw.with(|v| serde_json::from_value::<Vec<FileEntry>>(v.clone()).unwrap_or_default())
    });
    let sel = RwSignal::new(0usize);
    Effect::new(move |_| {
        let n = list.with(|l| l.len());
        sel.update(|s| *s = (*s).min(n.saturating_sub(1)));
    });
    let context_at = move |x: f64, y: f64| {
        if let Some(f) = list.with_untracked(|l| l.get(sel.get_untracked()).map(|f| f.name.clone()))
        {
            app.open(Modal::Context { x, y, file: f });
        }
    };
    let context_kbd = move || {
        let el = util::query(".files .frow2.sel");
        let (x, y) = el
            .map(|e| {
                let (x, y, _, h) = util::rect_of(&e);
                (x + 24.0, y + h)
            })
            .unwrap_or((200.0, 200.0));
        context_at(x, y);
    };
    let on_files_key = move |e: web_sys::KeyboardEvent| {
        let n = list.with_untracked(|l| l.len());
        match e.key().as_str() {
            "ArrowDown" => sel.update(|s| *s = (*s + 1).min(n.saturating_sub(1))),
            "ArrowUp" => sel.update(|s| *s = s.saturating_sub(1)),
            "Home" => sel.set(0),
            "End" => sel.set(n.saturating_sub(1)),
            "Enter" | "ContextMenu" => context_kbd(),
            "F10" if e.shift_key() => context_kbd(),
            _ => return,
        }
        e.prevent_default();
    };
    Effect::new(move |_| {
        let s = sel.get();
        request_animation_frame(move || {
            if let Some(el) = util::query(&format!(".files .frow2:nth-child({})", s + 1)) {
                util::scroll_into_view(&el);
            }
        });
    });
    view! {
        <TabFrame id="menus">
            <div class="frow">
                <label class="flabel2">"Menu bar"</label>
                <div class="fctl">
                    <div class="demobar menubar" class:focusring=true tabindex="0" node_ref=bar data-control="menus.bar"
                        on:keydown=on_bar_key
                        on:focus=move |_| app.ctx.set(Ctx::Body("demo-menu"))
                        on:blur=move |_| { let _ = open.try_set(None); }
                        on:mousedown=move |_| { if let Some(b) = bar.get_untracked() { let _ = b.focus(); } }>
                        <MenuBar menus=menus open=open path=path on_pick=on_pick />
                        <span class="spacer"></span>
                        <span class="fhint">{move || if open.get().is_none() { format!("focused: {} · Enter opens", ["Document", "Format", "Arrange"][title.get()]) } else { String::new() }}</span>
                    </div>
                    <span class="fhint">"Picking an item only reports it in the status line."</span>
                </div>
            </div>
            <div class="frow" class:flash=move || app.flash.with(|f| f.contains(KEY))>
                <label class="flabel2">"Files"</label>
                <div class="fctl">
                    <div class="files" tabindex="0" data-control=KEY on:keydown=on_files_key on:focus=move |_| app.ctx.set(Ctx::Body("files"))>
                        {move || list.get().into_iter().enumerate().map(|(i, f)| {
                            let name = f.name.clone();
                            view! {
                                <div class="frow2" class:sel=move || sel.get() == i data-name=name.clone()
                                    on:mousedown=move |_| sel.set(i)
                                    on:dblclick=move |_| { sel.set(i); file_action(app, &name, "Open"); }
                                    on:contextmenu=move |e: web_sys::MouseEvent| { e.prevent_default(); sel.set(i); context_at(e.client_x() as f64, e.client_y() as f64); }>
                                    <span class="star">{if f.starred { "★" } else { "" }}</span>
                                    <span class="fname">{f.name.clone()}</span>
                                </div>
                            }
                        }).collect_view()}
                    </div>
                    <span class="fhint">{move || format!("{} · Enter, Shift+F10 or right-click for the context menu", util::plural(list.with(|l| l.len()), "file", "files"))}</span>
                </div>
            </div>
        </TabFrame>
    }
}
