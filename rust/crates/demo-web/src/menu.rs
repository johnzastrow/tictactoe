//! Menu bars with drop-downs and submenus (ui-spec §3 "Menus", tab 11):
//! one model and one renderer shared by the app's File/View/Help bar and the
//! Menus tab's demo bar. State is two signals: which menu is open, and the
//! selected item at each level (`path`; a longer path = an open submenu).

use leptos::prelude::*;

use crate::app::Act;

#[derive(Clone, PartialEq, Debug, Default)]
pub struct MItem {
    pub label: String,
    pub key: String,
    pub act: Option<Act>,
    pub sub: Vec<MItem>,
    pub sep: bool,
    pub disabled: bool,
    pub checked: Option<bool>,
}

impl MItem {
    pub fn new(label: impl Into<String>, key: impl Into<String>) -> MItem {
        MItem {
            label: label.into(),
            key: key.into(),
            ..Default::default()
        }
    }
    pub fn act(a: Act) -> MItem {
        MItem {
            label: a.label(),
            key: a.key(),
            act: Some(a),
            ..Default::default()
        }
    }
    pub fn sep() -> MItem {
        MItem {
            sep: true,
            ..Default::default()
        }
    }
    pub fn sub(label: impl Into<String>, items: Vec<MItem>) -> MItem {
        MItem {
            label: label.into(),
            sub: items,
            ..Default::default()
        }
    }
    pub fn checked(mut self, on: bool) -> MItem {
        self.checked = Some(on);
        self
    }
    pub fn disabled(mut self) -> MItem {
        self.disabled = true;
        self
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct MenuDef {
    pub title: &'static str,
    pub items: Vec<MItem>,
}

/// What a key or click did to a menu.
pub enum Outcome {
    /// Consumed, nothing to run.
    Handled,
    /// An item was picked: its labels from the top (`Format ▸ Text ▸ Bold`) and action.
    Picked(Vec<String>, Option<Act>),
    /// Not a menu key.
    Ignored,
}

/// The items list at `level` of the open menu along `path`.
fn level_items<'a>(
    menus: &'a [MenuDef],
    m: usize,
    path: &[usize],
    level: usize,
) -> Option<&'a [MItem]> {
    let mut items: &[MItem] = &menus.get(m)?.items;
    for &i in path.iter().take(level) {
        items = &items.get(i)?.sub;
    }
    Some(items)
}

fn labels(menus: &[MenuDef], m: usize, path: &[usize]) -> Vec<String> {
    let mut out = vec![menus[m].title.to_string()];
    let mut items: &[MItem] = &menus[m].items;
    for &i in path {
        let Some(it) = items.get(i) else { break };
        out.push(it.label.clone());
        items = &it.sub;
    }
    out
}

/// Activate the item at the end of `path`: open its submenu or pick it.
fn activate(
    menus: &[MenuDef],
    open: RwSignal<Option<usize>>,
    path: RwSignal<Vec<usize>>,
) -> Outcome {
    let Some(m) = open.get_untracked() else {
        return Outcome::Ignored;
    };
    let p = path.get_untracked();
    let Some(items) = level_items(menus, m, &p, p.len() - 1) else {
        return Outcome::Handled;
    };
    let Some(it) = items.get(*p.last().unwrap_or(&0)) else {
        return Outcome::Handled;
    };
    if it.sep || it.disabled {
        return Outcome::Handled;
    }
    if !it.sub.is_empty() {
        path.update(|p| p.push(0));
        return Outcome::Handled;
    }
    let act = it.act;
    let l = labels(menus, m, &p);
    open.set(None);
    Outcome::Picked(l, act)
}

/// Keyboard for an open menu bar (←/→ menus or submenus, ↑/↓ items, Enter, Esc, first letter).
pub fn menu_key(
    menus: &[MenuDef],
    open: RwSignal<Option<usize>>,
    path: RwSignal<Vec<usize>>,
    e: &web_sys::KeyboardEvent,
) -> Outcome {
    let Some(m) = open.get_untracked() else {
        return Outcome::Ignored;
    };
    let n_menus = menus.len();
    let p = path.get_untracked();
    let depth = p.len() - 1;
    let items = level_items(menus, m, &p, depth).unwrap_or(&[]);
    let n = items.len();
    let step = |dir: i64| {
        let mut s = p[depth] as i64;
        for _ in 0..n {
            s = (s + dir).rem_euclid(n.max(1) as i64);
            if !items[s as usize].sep {
                break;
            }
        }
        path.update(|p| p[depth] = s as usize);
    };
    let cur_has_sub = items
        .get(p[depth])
        .is_some_and(|it| !it.sub.is_empty() && !it.disabled);
    match e.key().as_str() {
        "ArrowDown" => step(1),
        "ArrowUp" => step(-1),
        "Home" => path.update(|p| p[depth] = 0),
        "End" => path.update(|p| p[depth] = n.saturating_sub(1)),
        "ArrowRight" if cur_has_sub => path.update(|p| p.push(0)),
        "ArrowRight" => {
            open.set(Some((m + 1) % n_menus));
            path.set(vec![0]);
        }
        "ArrowLeft" if depth > 0 => path.update(|p| {
            p.pop();
        }),
        "ArrowLeft" => {
            open.set(Some((m + n_menus - 1) % n_menus));
            path.set(vec![0]);
        }
        "Enter" | " " => return activate(menus, open, path),
        "Escape" | "F10" => open.set(None),
        "Tab" => open.set(None),
        _ => {
            // first letter of an item jumps to it
            if let Some(c) = crate::util::typed_char(e) {
                let c = c.to_lowercase().to_string();
                if let Some(i) = items
                    .iter()
                    .position(|it| !it.sep && it.label.to_lowercase().starts_with(&c))
                {
                    path.update(|p| p[depth] = i);
                }
            } else {
                return Outcome::Handled;
            }
        }
    }
    Outcome::Handled
}

/// The bar of titles plus the open drop-down. `on_pick` gets picked items.
#[component]
pub fn MenuBar(
    #[prop(into)] menus: Signal<Vec<MenuDef>>,
    open: RwSignal<Option<usize>>,
    path: RwSignal<Vec<usize>>,
    on_pick: Callback<(Vec<String>, Option<Act>)>,
) -> impl IntoView {
    menus
        .get_untracked()
        .into_iter()
        .enumerate()
        .map(|(i, def)| {
            let t = def.title;
            let (first, rest) = t.split_at(1);
            view! {
                <div class="menu" class:open=move || open.get() == Some(i) data-menu=t
                    on:mousedown=move |e| {
                        e.prevent_default();
                        if open.get_untracked() == Some(i) { open.set(None) } else { path.set(vec![0]); open.set(Some(i)); }
                    }
                    on:mouseenter=move |_| { if open.get_untracked().is_some_and(|o| o != i) { path.set(vec![0]); open.set(Some(i)); } }>
                    <u>{first.to_string()}</u>{rest.to_string()}
                    <Show when=move || open.get() == Some(i)>
                        {move || drop_view(menus, i, 0, open, path, on_pick)}
                    </Show>
                </div>
            }
        })
        .collect_view()
}

/// One drop-down level (recursive for submenus).
fn drop_view(
    menus: Signal<Vec<MenuDef>>,
    m: usize,
    level: usize,
    open: RwSignal<Option<usize>>,
    path: RwSignal<Vec<usize>>,
    on_pick: Callback<(Vec<String>, Option<Act>)>,
) -> AnyView {
    let items = menus.with(|ms| {
        let p = path.get_untracked();
        level_items(ms, m, &p, level)
            .map(|s| s.to_vec())
            .unwrap_or_default()
    });
    let cls = if level == 0 { "drop" } else { "drop sub" };
    view! {
        <div class=cls data-popup="menu" on:mousedown=|e| { e.stop_propagation(); e.prevent_default(); }>
            {items.into_iter().enumerate().map(|(i, it)| {
                if it.sep {
                    return view! { <div class="sep"></div> }.into_any();
                }
                let has_sub = !it.sub.is_empty();
                let sel = move || path.with(|p| p.len() > level && p[level] == i);
                let sub_open = move || path.with(|p| p.len() > level + 1 && p[level] == i);
                let chk = match it.checked { Some(true) => "✓", _ => "" };
                let dis = it.disabled;
                view! {
                    <div class="item" class:sel=sel class:disabled=dis
                        on:mouseenter=move |_| path.update(|p| { p.truncate(level + 1); if p.len() <= level { p.resize(level + 1, 0); } p[level] = i; if has_sub && !dis { p.push(0); } })
                        on:click=move |e| {
                            e.stop_propagation();
                            path.update(|p| { p.truncate(level + 1); p[level] = i; });
                            if !has_sub {
                                let out = menus.with_untracked(|ms| activate(ms, open, path));
                                if let Outcome::Picked(l, a) = out { on_pick.run((l, a)); }
                            }
                        }>
                        <span class="chk">{chk}</span>
                        <span class="il">{it.label.clone()}</span>
                        <span class="ik">{if has_sub { "▸".to_string() } else { it.key.clone() }}</span>
                        <Show when=sub_open>
                            {move || drop_view(menus, m, level + 1, open, path, on_pick)}
                        </Show>
                    </div>
                }.into_any()
            }).collect_view()}
        </div>
    }
    .into_any()
}
