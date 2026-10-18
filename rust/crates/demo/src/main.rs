//! `demo`: a control gallery. One binary: the server, reset, and the TUI
//! client (docs/demo/PLAN.md).

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use demo_core::protocol::{DEFAULT_BIND, DEFAULT_SERVER};
use demo_server::{ServeConfig, Store};

#[derive(Parser)]
#[command(name = "demo", version, about = "A control gallery: every TUI/web control on its own tab, shared live through one server")]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the server (web client at http://<bind>/)
    Serve {
        #[arg(long, default_value = "demo.db")]
        db: PathBuf,
        #[arg(long, default_value = DEFAULT_BIND)]
        bind: SocketAddr,
        #[arg(long, requires = "tls_key")]
        tls_cert: Option<PathBuf>,
        #[arg(long, requires = "tls_cert")]
        tls_key: Option<PathBuf>,
        /// Allow a non-loopback bind without TLS (trusted networks such as Tailscale)
        #[arg(long)]
        allow_plaintext: bool,
    },
    /// Restore every control to its default
    Reset {
        #[arg(long, default_value = "demo.db")]
        db: PathBuf,
    },
    /// Run the terminal client
    Tui {
        #[arg(long, default_value = DEFAULT_SERVER)]
        server: String,
        /// Display name (without it, a join form asks)
        #[arg(long)]
        name: Option<String>,
        /// Extra CA certificate (PEM) to trust for https servers
        #[arg(long)]
        ca_cert: Option<PathBuf>,
        /// midnight, daylight, solarized-dark or gruvbox
        #[arg(long)]
        theme: Option<String>,
        /// Force colours: 256 or truecolor (default: detect from the terminal)
        #[arg(long, value_parser = ["256", "truecolor"])]
        colors: Option<String>,
    },
}

fn init_logging() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).with_writer(std::io::stderr).init();
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Command::Serve { db, bind, tls_cert, tls_key, allow_plaintext } => {
            init_logging();
            let cfg = ServeConfig { db, bind, tls: tls_cert.zip(tls_key), allow_plaintext };
            demo_server::serve(cfg, async {
                let _ = tokio::signal::ctrl_c().await;
            })
            .await
        }
        Command::Reset { db } => {
            let mut store = Store::open(&db).with_context(|| format!("opening {}", db.display()))?;
            let n = store.reset_all()?;
            println!("Reset {n} controls in {} to their defaults.", db.display());
            println!("A running server keeps its own copy: restart it to pick this up.");
            Ok(())
        }
        Command::Tui { server, name, ca_cert, theme, colors } => {
            demo_tui::run(demo_tui::TuiOptions { server, name, ca_cert, theme, colors }).await
        }
    }
}
