//! Tab 14, Charts: `charts.series` drawn as SVG bars, a sparkline, a gauge
//! of the average and a heat strip; the selected month is highlighted in all
//! four. Keys change the selected value; bars can be dragged.

use demo_core::dataset::CHART_MONTHS;
use leptos::prelude::*;
use serde_json::json;

use super::TabFrame;
use crate::app::{Ctx, use_app};

const KEY: &str = "charts.series";
// bar chart geometry (viewBox units)
const W: f64 = 640.0;
const H: f64 = 230.0;
const TOP: f64 = 20.0;
const BASE: f64 = 200.0;
const SLOT: f64 = (W - 40.0) / 12.0;

fn y_of(v: i64) -> f64 {
    BASE - (v as f64 / 100.0) * (BASE - TOP)
}

#[component]
pub fn ChartsTab() -> impl IntoView {
    let app = use_app();
    let raw = app.watch(KEY);
    let stored = Memo::new(move |_| {
        raw.with(|v| serde_json::from_value::<Vec<i64>>(v.clone()).unwrap_or_default())
    });
    let sel = RwSignal::new(0usize);
    // (month, value) under the pointer while dragging a bar
    let drag = RwSignal::new(None::<(usize, i64)>);
    let series = move || {
        let mut s = stored.get();
        if let Some((i, v)) = drag.get() {
            if let Some(x) = s.get_mut(i) {
                *x = v;
            }
        }
        s
    };
    let svg = NodeRef::<leptos::svg::Svg>::new();
    let set_month = move |i: usize, v: i64| {
        let mut s: Vec<i64> = serde_json::from_value(app.latest(KEY)).unwrap_or_default();
        if let Some(x) = s.get_mut(i) {
            *x = v.clamp(0, 100);
            app.set_latest(KEY, json!(s));
        }
    };
    let on_key = move |e: web_sys::KeyboardEvent| {
        let i = sel.get_untracked();
        let cur = serde_json::from_value::<Vec<i64>>(app.latest(KEY))
            .unwrap_or_default()
            .get(i)
            .copied()
            .unwrap_or(0);
        match e.key().as_str() {
            "ArrowRight" => sel.set((i + 1).min(11)),
            "ArrowLeft" => sel.set(i.saturating_sub(1)),
            "Home" => sel.set(0),
            "End" => sel.set(11),
            "ArrowUp" => set_month(i, cur + 5),
            "ArrowDown" => set_month(i, cur - 5),
            "PageUp" => set_month(i, cur + 20),
            "PageDown" => set_month(i, cur - 20),
            _ => return,
        }
        e.prevent_default();
    };
    let value_at = move |client_y: f64| -> i64 {
        let Some(s) = svg.get_untracked() else {
            return 0;
        };
        let r = s.get_bounding_client_rect();
        let y = (client_y - r.top()) * H / r.height().max(1.0);
        (((BASE - y) / (BASE - TOP)) * 100.0)
            .round()
            .clamp(0.0, 100.0) as i64
    };
    let avg = move || {
        let s = series();
        if s.is_empty() {
            0.0
        } else {
            s.iter().sum::<i64>() as f64 / s.len() as f64
        }
    };
    let level = move || match avg() {
        a if a < 60.0 => "ok",
        a if a < 80.0 => "warn",
        _ => "err",
    };
    view! {
        <TabFrame id="charts">
            <div class="charts" tabindex="0" data-control=KEY class:flash=move || app.flash.with(|f| f.contains(KEY))
                on:keydown=on_key on:focus=move |_| app.ctx.set(Ctx::Body("chart"))>
                <div class="chart-h">
                    <b>{move || { let i = sel.get(); format!("{} = {}", CHART_MONTHS[i], series().get(i).copied().unwrap_or(0)) }}</b>
                    <span class="muted">" · ←/→ month · ↑/↓ ±5 · PgUp/PgDn ±20 · drag a bar"</span>
                </div>
                <svg class="bars" node_ref=svg viewBox=format!("0 0 {W} {H}") preserveAspectRatio="xMidYMid meet"
                    on:pointermove=move |e: web_sys::PointerEvent| {
                        if let Some((i, _)) = drag.get_untracked() { drag.set(Some((i, value_at(e.client_y() as f64)))); }
                    }
                    on:pointerup=move |e: web_sys::PointerEvent| {
                        if let Some((i, _)) = drag.get_untracked() {
                            let v = value_at(e.client_y() as f64);
                            set_month(i, v);
                            // show the dragged value until the saved one arrives
                            set_timeout(move || { let _ = drag.try_set(None); }, std::time::Duration::from_millis(300));
                        }
                    }>
                    {[0, 25, 50, 75, 100].map(|t| view! {
                        <line class="gridl" x1="30" x2=W y1=y_of(t) y2=y_of(t) />
                        <text class="axis" x="24" y=y_of(t) + 3.0 text-anchor="end">{t}</text>
                    })}
                    {move || series().into_iter().enumerate().map(|(i, v)| {
                        let x = 36.0 + i as f64 * SLOT;
                        let w = SLOT - 12.0;
                        let is_sel = sel.get() == i;
                        view! {
                            <g class="bar-g" class:sel=is_sel data-month=i
                                on:pointerdown=move |e: web_sys::PointerEvent| {
                                    e.prevent_default();
                                    sel.set(i);
                                    if let Some(s) = crate::util::query("[data-control='charts.series']") { crate::util::focus_el(&s); }
                                    // capture on the <svg>: the bars re-render while dragging
                                    if let Some(s) = svg.get_untracked() { let s: &web_sys::Element = s.as_ref(); let _ = s.set_pointer_capture(e.pointer_id()); }
                                    drag.set(Some((i, value_at(e.client_y() as f64))));
                                }>
                                <rect class="hit" x=x - 4.0 y=TOP width=w + 8.0 height=BASE - TOP />
                                <rect class="barr" x=x y=y_of(v) width=w height=BASE - y_of(v) rx="3" />
                                <rect class="grab" x=x y=y_of(v) - 3.0 width=w height="6" />
                                <text class="vlabel" x=x + w / 2.0 y=y_of(v) - 5.0 text-anchor="middle">{v}</text>
                                <text class="mlabel" x=x + w / 2.0 y=BASE + 16.0 text-anchor="middle">{CHART_MONTHS[i]}</text>
                            </g>
                        }
                    }).collect_view()}
                </svg>
                <div class="chart-row">
                    <div class="mini-card">
                        <div class="mini-h">"Sparkline"</div>
                        <svg class="spark" viewBox="0 0 240 60" preserveAspectRatio="none">
                            {move || {
                                let s = series();
                                let pt = |i: usize, v: i64| (6.0 + i as f64 * (228.0 / 11.0), 54.0 - v as f64 * 0.48);
                                let pts = s.iter().enumerate().map(|(i, &v)| { let (x, y) = pt(i, v); format!("{x:.1},{y:.1}") }).collect::<Vec<_>>().join(" ");
                                let i = sel.get();
                                let (cx, cy) = pt(i, s.get(i).copied().unwrap_or(0));
                                view! {
                                    <polyline class="sl" points=pts />
                                    <circle class="sdot" cx=cx cy=cy r="3.5" />
                                }
                            }}
                        </svg>
                    </div>
                    <div class="mini-card">
                        <div class="mini-h">"Gauge: average"</div>
                        <div class=move || format!("gauge2 {}", level())>
                            <div class="gfill" style=move || format!("width:{:.1}%", avg())></div>
                            <span class="gmark" style="left:60%"></span><span class="gmark" style="left:80%"></span>
                        </div>
                        <div class="gval">{move || format!("{:.1}", avg())}<span class="muted">" / 100"</span></div>
                        <div class="fhint">"green < 60 ≤ amber < 80 ≤ red"</div>
                    </div>
                    <div class="mini-card">
                        <div class="mini-h">"Heat strip"</div>
                        <div class="heatstrip">
                            {move || series().into_iter().enumerate().map(|(i, v)| view! {
                                <span class="hcell" class:sel=move || sel.get() == i title=format!("{} {v}", CHART_MONTHS[i])
                                    style=format!("--a:{:.2}", 0.08 + v as f64 / 100.0 * 0.92)
                                    on:mousedown=move |e| { e.prevent_default(); sel.set(i); }>
                                    {CHART_MONTHS[i].chars().next().unwrap_or(' ').to_string()}
                                </span>
                            }).collect_view()}
                        </div>
                    </div>
                </div>
            </div>
        </TabFrame>
    }
}
