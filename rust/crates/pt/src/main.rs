//! `pt`: projects & tasks. One binary: the server, the seeder, user admin,
//! and the TUI client (docs/pt/PLAN.md).

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use pt_core::model::Role;
use pt_server::{ServeConfig, Store, auth, seed};

#[derive(Parser)]
#[command(name = "pt", version, about = "Projects & tasks: a TUI + web demo backed by one SQLite server")]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the server (web client at http://<bind>/)
    Serve {
        #[arg(long, default_value = "pt.db")]
        db: PathBuf,
        #[arg(long, default_value = "127.0.0.1:8788")]
        bind: SocketAddr,
        #[arg(long, requires = "tls_key")]
        tls_cert: Option<PathBuf>,
        #[arg(long, requires = "tls_cert")]
        tls_key: Option<PathBuf>,
        /// Allow a non-loopback bind without TLS (trusted networks such as Tailscale)
        #[arg(long)]
        allow_plaintext: bool,
    },
    /// Fill an empty database with demo data
    Seed {
        #[arg(long, default_value = "pt.db")]
        db: PathBuf,
        /// Password for every demo user (default: random, printed once)
        #[arg(long)]
        password: Option<String>,
        /// Wipe existing data first
        #[arg(long)]
        force: bool,
    },
    /// Manage users
    User {
        #[command(subcommand)]
        cmd: UserCmd,
    },
    /// Run the terminal client
    Tui {
        #[arg(long, default_value = "http://127.0.0.1:8788")]
        server: String,
        #[arg(long)]
        user: Option<String>,
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
    /// Forget the saved session for a server
    Logout {
        #[arg(long, default_value = "http://127.0.0.1:8788")]
        server: String,
    },
}

#[derive(Subcommand)]
enum UserCmd {
    /// Add a user (password from PT_PASSWORD or a prompt)
    Add {
        username: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long, default_value = "member")]
        role: String,
        #[arg(long, default_value = "pt.db")]
        db: PathBuf,
    },
    /// Set a user's password (from PT_PASSWORD or a prompt)
    Passwd {
        username: String,
        #[arg(long, default_value = "pt.db")]
        db: PathBuf,
    },
    /// List users
    List {
        #[arg(long, default_value = "pt.db")]
        db: PathBuf,
    },
}

fn read_password(confirm: bool) -> anyhow::Result<String> {
    if let Ok(p) = std::env::var("PT_PASSWORD") {
        return Ok(p);
    }
    let p = rpassword::prompt_password("Password: ")?;
    if confirm && rpassword::prompt_password("Again: ")? != p {
        bail!("passwords don't match");
    }
    Ok(p)
}

fn check_password(p: &str) -> anyhow::Result<()> {
    if !(8..=256).contains(&p.chars().count()) {
        bail!("password must be 8–256 characters");
    }
    Ok(())
}

fn init_logging(stderr: bool) {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    if stderr {
        tracing_subscriber::fmt().with_env_filter(filter).with_writer(std::io::stderr).init();
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Command::Serve { db, bind, tls_cert, tls_key, allow_plaintext } => {
            init_logging(true);
            let cfg = ServeConfig { db, bind, tls: tls_cert.zip(tls_key), allow_plaintext };
            pt_server::serve(cfg, async {
                let _ = tokio::signal::ctrl_c().await;
            })
            .await
        }
        Command::Seed { db, password, force } => {
            let mut store = Store::open(&db).with_context(|| format!("opening {}", db.display()))?;
            if force {
                seed::wipe(&mut store)?;
            }
            let (password, generated) = match password.or_else(|| std::env::var("PT_SEED_PASSWORD").ok()) {
                Some(p) => (p, false),
                None => (auth::random_password(12), true),
            };
            check_password(&password)?;
            let r = seed::seed(&mut store, &password)?;
            println!("Seeded {}: {} users, {} projects, {} tasks.", db.display(), r.users, r.projects, r.tasks);
            println!("Users: admin (admin), alice … heidi (members), victor (viewer).");
            if generated {
                println!("Password for every user: {password}");
            }
            Ok(())
        }
        Command::User { cmd } => match cmd {
            UserCmd::Add { username, name, role, db } => {
                let role = Role::parse(&role).context("role must be admin, member or viewer")?;
                let username = username.trim().to_lowercase();
                if !pt_core::fields::valid_username(&username) {
                    bail!("username must be 2–32 of a–z, 0–9, '_', '.', '-' starting with a letter");
                }
                let store = Store::open(&db)?;
                if store.user_by_name(&username)?.is_some() {
                    bail!("user {username} already exists");
                }
                let pw = read_password(true)?;
                check_password(&pw)?;
                let id = store.add_user(&username, name.as_deref().unwrap_or(&username), role, &auth::hash_password(&pw)?)?;
                println!("Added {username} (id {id}, {}).", role.as_str());
                Ok(())
            }
            UserCmd::Passwd { username, db } => {
                let store = Store::open(&db)?;
                let user = store.user_by_name(&username)?.context("no such user")?;
                let pw = read_password(true)?;
                check_password(&pw)?;
                store.set_password_hash(user.id, &auth::hash_password(&pw)?)?;
                store.delete_user_sessions(user.id, None)?;
                println!("Password changed for {}; their sessions were ended.", user.username);
                Ok(())
            }
            UserCmd::List { db } => {
                let store = Store::open(&db)?;
                println!("{:<4} {:<12} {:<22} {:<7} active", "id", "username", "name", "role");
                for u in store.list_users()? {
                    println!("{:<4} {:<12} {:<22} {:<7} {}", u.id, u.username, u.display_name, u.role.as_str(), u.active);
                }
                Ok(())
            }
        },
        Command::Tui { server, user, ca_cert, theme, colors } => {
            pt_tui::run(pt_tui::Options { server, user, ca_cert, theme, colors }).await
        }
        Command::Logout { server } => pt_tui::forget_session(&server),
    }
}
