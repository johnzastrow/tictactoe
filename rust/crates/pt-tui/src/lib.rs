//! pt terminal client: a ratatui front end over the pt WebSocket protocol
//! (docs/pt/ui-spec.md). It holds a replica of the server's data, derives
//! every view from it locally, and sends `mutate` / `undo` / `focus`.

mod app;
mod dialogs;
mod grid;
mod keymap;
mod net;
mod theme;
mod ui;
mod views;
mod widgets;

#[cfg(test)]
mod testdata;
#[cfg(test)]
mod tests;

use std::io::stdout;
use std::path::PathBuf;

use anyhow::Context;
use crossterm::event::{
    DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste, EnableFocusChange, EnableMouseCapture,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use tokio::sync::mpsc;

pub struct Options {
    pub server: String,
    pub user: Option<String>,
    pub ca_cert: Option<PathBuf>,
    pub theme: Option<String>,
    /// Force the colour depth: "256" or "truecolor" (default: detect).
    pub colors: Option<String>,
}

fn restore_terminal(kitty: bool) {
    if kitty {
        let _ = execute!(stdout(), PopKeyboardEnhancementFlags);
    }
    let _ = execute!(stdout(), DisableMouseCapture, DisableBracketedPaste, DisableFocusChange);
    ratatui::restore();
}

pub async fn run(opts: Options) -> anyhow::Result<()> {
    let url = net::ServerUrl::parse(&opts.server).context("bad --server URL")?;
    let tls = net::tls_config(opts.ca_cert.as_deref())?;
    theme::detect_color_depth(opts.colors.as_deref());
    let theme_idx = match opts.theme.as_deref() {
        Some(name) => theme::theme_index(name).with_context(|| {
            format!("unknown theme '{name}' (try: {})", theme::PALETTES.iter().map(|p| p.name).collect::<Vec<_>>().join(", "))
        })?,
        None => 0,
    };
    let (itx, irx) = mpsc::unbounded_channel();
    let mut app = app::App::new(url.clone(), tls, theme_idx, itx);
    if let Some(u) = &opts.user {
        app.username = u.clone();
    }
    // A remembered session for this server (and user, if one was given) skips the login form.
    match net::load_sessions().get(&url.base()) {
        Some(saved) if opts.user.as_ref().is_none_or(|u| *u == saved.username) => {
            app.username = saved.username.clone();
            app.connect(saved.token.clone());
        }
        _ => app.show_login(None),
    }

    // Restore the terminal even if we panic.
    let kitty = matches!(crossterm::terminal::supports_keyboard_enhancement(), Ok(true));
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal(kitty);
        hook(info);
    }));
    let mut terminal = ratatui::init();
    execute!(stdout(), EnableMouseCapture, EnableBracketedPaste, EnableFocusChange)?;
    if kitty {
        // tells ^I from Tab, ^M from Enter, etc. where the terminal supports it
        let _ = execute!(stdout(), PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES));
    }
    let result = app.run(&mut terminal, irx).await;
    restore_terminal(kitty);
    result
}

/// `pt logout`: end the remembered session on the server and forget it locally.
pub fn forget_session(server: &str) -> anyhow::Result<()> {
    let url = net::ServerUrl::parse(server)?;
    match net::forget(&url)? {
        Some(saved) => {
            let tls = net::tls_config(None)?;
            let rt = tokio::runtime::Handle::try_current();
            let fut = async move { net::logout(&url, &tls, &saved.token).await };
            match rt {
                Ok(h) => {
                    let _ = tokio::task::block_in_place(|| h.block_on(fut));
                }
                Err(_) => {
                    let _ = tokio::runtime::Runtime::new()?.block_on(fut);
                }
            }
            println!("Logged out of {server}.");
        }
        None => println!("No saved session for {server}."),
    }
    Ok(())
}
