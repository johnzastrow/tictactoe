//! Tab 9, Cascade: Program ▸ Project ▸ Phase, each list depending on the
//! one before; every pick commits the whole `cascade.pick` object.

use demo_core::dataset::{PROGRAMS, phases_of, projects_of};
use demo_core::keys::CascadePick;
use leptos::prelude::*;
use serde_json::json;

use super::{Field, TabFrame};
use crate::app::use_app;
use crate::util::plural;
use crate::widgets::{Dropdown, plain_opts};

#[component]
pub fn CascadeTab() -> impl IntoView {
    let app = use_app();
    let raw = app.watch("cascade.pick");
    let pick = Memo::new(move |_| {
        raw.with(|v| serde_json::from_value::<CascadePick>(v.clone()).unwrap_or_default())
    });
    let save = move |p: CascadePick| app.set("cascade.pick", json!(p));
    let program = Signal::derive(move || pick.with(|p| p.program.clone()));
    let project = Signal::derive(move || pick.with(|p| p.project.clone()));
    let phase = Signal::derive(move || pick.with(|p| p.phase.clone()));
    let projects = Signal::derive(move || {
        plain_opts(
            projects_of(&program.get().unwrap_or_default())
                .iter()
                .map(|p| p.name),
        )
    });
    let phases = Signal::derive(move || {
        plain_opts(
            phases_of(
                &program.get().unwrap_or_default(),
                &project.get().unwrap_or_default(),
            )
            .iter()
            .copied(),
        )
    });
    let crumb = move || {
        let p = pick.get();
        [p.program, p.project, p.phase]
            .into_iter()
            .map(|x| x.unwrap_or_else(|| "—".into()))
            .collect::<Vec<_>>()
            .join(" ▸ ")
    };
    view! {
        <TabFrame id="cascade">
            <Field label="Program" key="cascade.pick">
                <Dropdown ctl="cascade.program" options=Signal::derive(|| plain_opts(PROGRAMS.iter().map(|p| p.name))) value=program
                    on_pick=Callback::new(move |id: String| {
                        // changing Program clears Project and Phase
                        if program.get_untracked().as_deref() != Some(id.as_str()) {
                            save(CascadePick { program: Some(id), project: None, phase: None });
                        }
                    })
                    on_clear=Callback::new(move |_| save(CascadePick::default())) />
                <span class="fhint">{move || plural(PROGRAMS.len(), "program", "programs")}</span>
            </Field>
            <Field label="Project">
                <Dropdown ctl="cascade.project" options=projects value=project
                    disabled=Signal::derive(move || program.get().is_none()) disabled_msg="Pick a program first"
                    on_pick=Callback::new(move |id: String| {
                        if project.get_untracked().as_deref() != Some(id.as_str()) {
                            save(CascadePick { program: program.get_untracked(), project: Some(id), phase: None });
                        }
                    })
                    on_clear=Callback::new(move |_| save(CascadePick { program: program.get_untracked(), project: None, phase: None })) />
                <span class="fhint">{move || plural(projects.with(|p| p.len()), "project", "projects")}</span>
            </Field>
            <Field label="Phase">
                <Dropdown ctl="cascade.phase" options=phases value=phase
                    disabled=Signal::derive(move || project.get().is_none()) disabled_msg="Pick a project first"
                    on_pick=Callback::new(move |id: String| save(CascadePick { program: program.get_untracked(), project: project.get_untracked(), phase: Some(id) }))
                    on_clear=Callback::new(move |_| save(CascadePick { program: program.get_untracked(), project: project.get_untracked(), phase: None })) />
                <span class="fhint">{move || plural(phases.with(|p| p.len()), "phase", "phases")}</span>
            </Field>
            <div class="crumb">{crumb}</div>
        </TabFrame>
    }
}
