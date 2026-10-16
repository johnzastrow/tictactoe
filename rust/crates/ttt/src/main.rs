//! `ttt`: tic-tac-toe in a rich TUI and a local web page, sharing one game.
//!
//!   ttt                 TUI + web server (default)
//!   ttt --no-web        TUI only
//!   ttt serve           web server only (headless)

mod app;
mod theme;
mod ui;

use std::{
    io::stdout,
    net::{Ipv4Addr, SocketAddr},
    time::Duration,
};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
};
use tokio::{net::TcpListener, sync::oneshot};
use ttt_core::Player;
use ttt_server::{HubHandle, spawn_hub};

const AI_DELAY: Duration = Duration::from_millis(350);

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Port for the web interface (always bound to 127.0.0.1).
    #[arg(long, short, default_value_t = 8787, global = true)]
    port: u16,

    /// Who the AI plays at startup.
    #[arg(long, value_enum, default_value_t = AiArg::O, global = true)]
    ai: AiArg,

    /// Run the TUI without starting the web interface.
    #[arg(long)]
    no_web: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run only the web interface (no TUI). Stop with Ctrl-C.
    Serve,
}

#[derive(Clone, Copy, ValueEnum)]
enum AiArg {
    Off,
    X,
    O,
}

impl From<AiArg> for Option<Player> {
    fn from(a: AiArg) -> Self {
        match a {
            AiArg::Off => None,
            AiArg::X => Some(Player::X),
            AiArg::O => Some(Player::O),
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let hub = spawn_hub(cli.ai.into(), AI_DELAY);

    match cli.command {
        Some(Command::Serve) => run_headless(hub, cli.port).await,
        None => run_tui(hub, cli.port, !cli.no_web).await,
    }
}

/// Bind before doing anything else so "port in use" is a clean error.
async fn bind(port: u16) -> Result<TcpListener> {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    TcpListener::bind(addr)
        .await
        .with_context(|| format!("could not listen on {addr} (in use? try --port <N>)"))
}

async fn run_headless(hub: HubHandle, port: u16) -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let listener = bind(port).await?;
    let url = format!("http://{}", listener.local_addr()?);
    tracing::info!("serving tic-tac-toe at {url}  (Ctrl-C to stop)");
    let shutdown = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    ttt_server::serve(listener, hub, shutdown).await?;
    Ok(())
}

async fn run_tui(hub: HubHandle, port: u16, web: bool) -> Result<()> {
    // No tracing subscriber in TUI mode: log lines would corrupt the screen.
    let mut stop_web = None;
    let mut web_url = None;
    let mut server = None;
    if web {
        let listener = bind(port).await?;
        web_url = Some(format!("http://{}", listener.local_addr()?));
        let (tx, rx) = oneshot::channel::<()>();
        stop_web = Some(tx);
        let hub = hub.clone();
        server = Some(tokio::spawn(async move {
            ttt_server::serve(listener, hub, async {
                let _ = rx.await;
            })
            .await
        }));
    }

    // Restore the terminal (including mouse mode) even if we panic.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(stdout(), DisableMouseCapture);
        default_hook(info);
    }));

    let mut terminal = ratatui::init();
    execute!(stdout(), EnableMouseCapture)?;
    let result = app::App::new(hub, web_url).run(&mut terminal).await;
    let _ = execute!(stdout(), DisableMouseCapture);
    ratatui::restore();

    if let Some(tx) = stop_web {
        let _ = tx.send(());
    }
    if let Some(server) = server {
        // Don't hang on exit if a browser keeps a socket open.
        let _ = tokio::time::timeout(Duration::from_secs(2), server).await;
    }
    result.context("TUI error")
}
