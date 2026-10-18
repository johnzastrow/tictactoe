//! Tab 16, Gantt: `gantt.bars` on a time axis with week ticks, weekend
//! shading and a today line; keys and drags move and resize bars (drags
//! commit on release), + / - zoom between day, week and month scales.

use demo_core::Date;
use demo_core::date::MONTH_NAMES;
use demo_core::keys::{GanttBar, MAX_BAR_DAYS};
use leptos::prelude::*;
use serde_json::json;

use super::TabFrame;
use crate::app::{App, Ctx, use_app};
use crate::util;

const KEY: &str = "gantt.bars";
const ROW: f64 = 30.0;
const AXIS: f64 = 38.0;
/// Pixels per day at each zoom: day, week, month.
const PPD: [f64; 3] = [30.0, 12.0, 4.0];
const ZOOMS: [&str; 3] = ["day", "week", "month"];

fn bars(app: App) -> Vec<GanttBar> {
    serde_json::from_value(app.latest(KEY)).unwrap_or_default()
}

/// Move a bar's start by `delta` days, or (resize) change its length.
fn shift(app: App, id: u64, delta: i32, resize: bool) {
    if delta == 0 {
        return;
    }
    let mut list = bars(app);
    let Some(b) = list.iter_mut().find(|b| b.id == id) else {
        return;
    };
    if resize {
        b.days = (b.days as i64 + delta as i64).clamp(1, MAX_BAR_DAYS) as u32;
    } else if let Some(s) = b.start_date() {
        b.start = s.add_days(delta).iso();
    }
    app.set_latest(KEY, json!(list));
}

#[derive(Clone, Copy, PartialEq, Debug)]
struct Drag {
    id: u64,
    resize: bool,
    x0: f64,
    dx: f64,
}

#[component]
pub fn GanttTab() -> impl IntoView {
    let app = use_app();
    let today = util::today();
    let raw = app.watch(KEY);
    let list = Memo::new(move |_| {
        raw.with(|v| serde_json::from_value::<Vec<GanttBar>>(v.clone()).unwrap_or_default())
    });
    let zoom = RwSignal::new(1usize);
    let sel = RwSignal::new(0usize);
    let drag = RwSignal::new(None::<Drag>);
    let scroller = NodeRef::<leptos::html::Div>::new();
    let ppd = move || PPD[zoom.get()];
    // the date range: every bar and today, padded, from a Monday
    let range = Memo::new(move |_| {
        let l = list.get();
        let min = l
            .iter()
            .filter_map(|b| b.start_date())
            .min()
            .unwrap_or(today)
            .min(today)
            .add_days(-7)
            .week_start();
        let max = l
            .iter()
            .filter_map(|b| b.end_date())
            .max()
            .unwrap_or(today)
            .max(today)
            .add_days(21);
        (min, max)
    });
    let x_of = move |d: Date| (range.get().0.days_until(d)) as f64 * ppd();
    Effect::new(move |_| {
        let n = list.with(|l| l.len());
        sel.update(|s| *s = (*s).min(n.saturating_sub(1)));
    });
    // also runs from requestAnimationFrame, possibly after the tab is gone: try_ only
    let scroll_to = move |d: Date| {
        let (Some(Some(s)), Some((a, _)), Some(z)) = (
            scroller.try_get_untracked(),
            range.try_get_untracked(),
            zoom.try_get_untracked(),
        ) else {
            return;
        };
        let x = a.days_until(d) as f64 * PPD[z];
        s.set_scroll_left((x - s.client_width() as f64 / 3.0).max(0.0) as i32);
    };
    // start with today in view
    request_animation_frame(move || scroll_to(today));
    let sel_bar = move || list.with(|l| l.get(sel.get()).cloned());
    let on_key = move |e: web_sys::KeyboardEvent| {
        let n = list.with_untracked(|l| l.len());
        let id = list.with_untracked(|l| l.get(sel.get_untracked()).map(|b| b.id));
        let k = e.key();
        let shift_k = e.shift_key();
        match k.as_str() {
            "ArrowDown" => sel.update(|s| *s = (*s + 1).min(n.saturating_sub(1))),
            "ArrowUp" => sel.update(|s| *s = s.saturating_sub(1)),
            "ArrowRight" | ">" if shift_k || k == ">" => {
                if let Some(id) = id {
                    shift(app, id, 1, false)
                }
            }
            "ArrowLeft" | "<" if shift_k || k == "<" => {
                if let Some(id) = id {
                    shift(app, id, -1, false)
                }
            }
            "]" => {
                if let Some(id) = id {
                    shift(app, id, 1, true)
                }
            }
            "[" => {
                if let Some(id) = id {
                    shift(app, id, -1, true)
                }
            }
            "+" | "=" => zoom.update(|z| *z = z.saturating_sub(1)),
            "-" | "_" => zoom.update(|z| *z = (*z + 1).min(2)),
            "t" | "T" => scroll_to(today),
            "ArrowRight" | "ArrowLeft" => {
                if let Some(s) = scroller.get_untracked() {
                    let step = (ppd() * 7.0).max(60.0) as i32;
                    s.set_scroll_left(
                        s.scroll_left() + if k == "ArrowRight" { step } else { -step },
                    );
                }
            }
            _ => return,
        }
        e.prevent_default();
    };
    // keep the zoom anchored on the selected bar (or today)
    Effect::new(move |prev: Option<usize>| {
        let z = zoom.get();
        if prev.is_some() {
            let d = sel_bar().and_then(|b| b.start_date()).unwrap_or(today);
            request_animation_frame(move || scroll_to(d));
        }
        z
    });
    let on_move = move |e: web_sys::PointerEvent| {
        if let Some(mut d) = drag.get_untracked() {
            d.dx = e.client_x() as f64 - d.x0;
            drag.set(Some(d));
        }
    };
    let on_up = move |_e: web_sys::PointerEvent| {
        if let Some(d) = drag.get_untracked() {
            drag.set(None);
            shift(app, d.id, (d.dx / ppd()).round() as i32, d.resize);
        }
    };
    let width = move || {
        let (a, b) = range.get();
        a.days_until(b) as f64 * ppd()
    };
    let height = move || AXIS + list.with(|l| l.len()) as f64 * ROW + 6.0;
    view! {
        <TabFrame id="gantt">
            <div class="gantt-bar">
                <span class="gsel">{move || sel_bar().map(|b| {
                    let (s, e) = (b.start_date(), b.end_date());
                    format!("{} · {} → {} · {}", b.label,
                        s.map(|d| d.long()).unwrap_or_default(), e.map(|d| d.long()).unwrap_or_default(),
                        util::plural(b.days as usize, "day", "days"))
                }).unwrap_or_else(|| "no bars".into())}</span>
                <span class="spacer"></span>
                <span class="muted">{move || format!("zoom: {}", ZOOMS[zoom.get()])}</span>
                <button class="btn sm" tabindex="-1" on:mousedown=|e| e.prevent_default() on:click=move |_| zoom.update(|z| *z = z.saturating_sub(1))>"+ in"</button>
                <button class="btn sm" tabindex="-1" on:mousedown=|e| e.prevent_default() on:click=move |_| zoom.update(|z| *z = (*z + 1).min(2))>"− out"</button>
                <button class="btn sm" tabindex="-1" on:mousedown=|e| e.prevent_default() on:click=move |_| scroll_to(today)>"t today"</button>
            </div>
            <div class="gantt" tabindex="0" data-control=KEY class:flash=move || app.flash.with(|f| f.contains(KEY))
                on:keydown=on_key on:focus=move |_| app.ctx.set(Ctx::Body("gantt"))>
                <div class="gleft" style=move || format!("padding-top:{AXIS}px")>
                    {move || list.get().into_iter().enumerate().map(|(i, b)| view! {
                        <div class="glabel" class:sel=move || sel.get() == i data-id=b.id on:mousedown=move |_| sel.set(i)>{b.label.clone()}</div>
                    }).collect_view()}
                </div>
                <div class="gright" node_ref=scroller on:pointermove=on_move on:pointerup=on_up>
                    <svg class="gsvg" width=move || width().to_string() height=move || height().to_string()>
                        // weekend shading and week ticks
                        {move || {
                            let (a, b) = range.get();
                            let p = ppd();
                            let h = height();
                            (0..a.days_until(b)).map(|i| {
                                let d = a.add_days(i);
                                let x = i as f64 * p;
                                let we = (d.is_weekend() && p >= 8.0).then(|| view! { <rect class="we" x=x y=AXIS width=p height=h - AXIS /> });
                                let first = d.day() == 1;
                                let tick = (d.weekday() == 0 && p >= 8.0) || (p < 8.0 && first);
                                let label = if p >= 8.0 { format!("{} {}", d.day(), MONTH_NAMES[d.month() as usize - 1]) } else { format!("{} {}", MONTH_NAMES[d.month() as usize - 1], d.year() % 100) };
                                let tick = tick.then(|| view! {
                                    <line class="wline" x1=x x2=x y1=AXIS - 8.0 y2=h />
                                    <text class="dtext" x=x + 3.0 y=AXIS - 12.0>{label}</text>
                                });
                                let day = (p >= 24.0).then(|| view! { <text class="daytext" x=x + p / 2.0 y=AXIS - 1.0 text-anchor="middle">{d.day()}</text> });
                                view! { {we}{tick}{day} }
                            }).collect_view()
                        }}
                        {move || {
                            let x = x_of(today) + ppd() / 2.0;
                            view! {
                                <line class="today" x1=x x2=x y1=AXIS - 4.0 y2=height() />
                                <text class="todayt" x=x + 3.0 y=AXIS + 10.0>"today"</text>
                            }
                        }}
                        {move || {
                            let p = ppd();
                            let dr = drag.get();
                            list.get().into_iter().enumerate().map(|(i, b)| {
                                let Some(s) = b.start_date() else { return ().into_any() };
                                let mut x = x_of(s);
                                let mut w = b.days as f64 * p;
                                if let Some(d) = dr.filter(|d| d.id == b.id) {
                                    if d.resize { w = (w + d.dx).max(p); } else { x += d.dx; }
                                }
                                let y = AXIS + i as f64 * ROW + 6.0;
                                let id = b.id;
                                let start_drag = move |resize: bool| move |e: web_sys::PointerEvent| {
                                    e.prevent_default();
                                    e.stop_propagation();
                                    sel.set(i);
                                    if let Some(g) = util::query("[data-control='gantt.bars']") { util::focus_el(&g); }
                                    if let Some(sc) = scroller.get_untracked() { let _ = sc.set_pointer_capture(e.pointer_id()); }
                                    drag.set(Some(Drag { id, resize, x0: e.client_x() as f64, dx: 0.0 }));
                                };
                                let title = format!("{}: {} → {} · drag to move, drag the right edge to resize", b.label, s.iso(), b.end_date().map(|d| d.iso()).unwrap_or_default());
                                let text = (w > 50.0).then(|| view! { <text class="btext" x=x + 6.0 y=y + ROW - 17.0>{b.label.chars().take((w / 7.5) as usize).collect::<String>()}</text> });
                                view! {
                                    <g class="gbar" class:sel=move || sel.get() == i data-id=id>
                                        <rect class="gb" x=x y=y width=w height=ROW - 12.0 rx="4" on:pointerdown=start_drag(false)><title>{title}</title></rect>
                                        {text}
                                        <rect class="grip" x=x + w - 6.0 y=y width="8" height=ROW - 12.0 on:pointerdown=start_drag(true) />
                                    </g>
                                }.into_any()
                            }).collect_view()
                        }}
                    </svg>
                </div>
            </div>
        </TabFrame>
    }
}
