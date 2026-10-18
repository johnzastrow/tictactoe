//! demo terminal client: a ratatui front end over the demo WebSocket
//! protocol (docs/demo/ui-spec.md). It keeps a `demo_core::Values` replica
//! of every control's value, draws one control per tab, and sends `set` /
//! `reset` / `focus` / `toast`.

mod app;
mod dialogs;
mod help;
mod net;
mod palette;
mod tabs;
mod theme;
mod ui;
mod widgets;

#[cfg(test)]
mod tests;

use std::io::stdout;
use std::path::PathBuf;

use anyhow::Context;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use demo_core::protocol::clean_name;

/// Options for `demo tui` (docs/demo/PLAN.md CLI).
#[derive(Clone, Debug, Default)]
pub struct TuiOptions {
    /// Server base URL, e.g. `http://127.0.0.1:8790`.
    pub server: String,
    /// Display name; `None` shows the join form.
    pub name: Option<String>,
    /// Extra CA certificate (PEM) to trust for https servers.
    pub ca_cert: Option<PathBuf>,
    /// midnight, daylight, solarized-dark or gruvbox.
    pub theme: Option<String>,
    /// Force the colour depth: "256" or "truecolor" (default: detect).
    pub colors: Option<String>,
}

fn restore_terminal(kitty: bool) {
    if kitty {
        let _ = execute!(stdout(), PopKeyboardEnhancementFlags);
    }
    let _ = execute!(stdout(), DisableMouseCapture, DisableBracketedPaste);
    ratatui::restore();
}

pub async fn run(opts: TuiOptions) -> anyhow::Result<()> {
    let url = net::ServerUrl::parse(&opts.server).context("bad --server URL")?;
    let tls = net::tls_config(opts.ca_cert.as_deref())?;
    theme::detect_color_depth(opts.colors.as_deref());
    let theme_idx = match opts.theme.as_deref() {
        Some(name) => theme::theme_index(name).with_context(|| {
            format!(
                "unknown theme '{name}' (try: {})",
                theme::PALETTES
                    .iter()
                    .map(|p| p.name)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?,
        None => 0,
    };
    let mut app = app::App::new(url, tls, theme_idx);
    match &opts.name {
        Some(n) => {
            let name = clean_name(n)
                .context("bad --name: 1–24 letters, digits, space, _ . - starting with a letter")?;
            app.connect(name);
        }
        None => app.phase = app::Phase::Join,
    }

    // Restore the terminal even if we panic.
    let kitty = matches!(
        crossterm::terminal::supports_keyboard_enhancement(),
        Ok(true)
    );
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal(kitty);
        hook(info);
    }));
    let mut terminal = ratatui::init();
    execute!(stdout(), EnableMouseCapture, EnableBracketedPaste)?;
    if kitty {
        // tells ^I from Tab, ^M from Enter, etc. where the terminal supports it
        let _ = execute!(
            stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        );
    }
    let result = app.run(&mut terminal).await;
    restore_terminal(kitty);
    result
}
