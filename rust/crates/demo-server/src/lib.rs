//! demo server: one process, one SQLite file holding the current value of
//! every control, any number of TUI and browser clients
//! (docs/demo/protocol.md).

pub mod hub;
pub mod store;
pub mod web;

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, bail};
use demo_core::protocol::ServerInfo;

pub use store::Store;

pub struct ServeConfig {
    pub db: PathBuf,
    pub bind: SocketAddr,
    pub tls: Option<(PathBuf, PathBuf)>,
    pub allow_plaintext: bool,
}

pub fn server_info() -> ServerInfo {
    ServerInfo { implementation: "rust".into(), version: demo_core::VERSION.into() }
}

/// Start the hub and build the router for an open store.
pub fn app(store: Store, tls: bool) -> anyhow::Result<axum::Router> {
    let hub = hub::spawn(store, server_info())?;
    Ok(web::router(web::AppState::new(hub, tls)))
}

/// Run until `shutdown` resolves.
pub async fn serve(cfg: ServeConfig, shutdown: impl std::future::Future<Output = ()> + Send + 'static) -> anyhow::Result<()> {
    if !cfg.bind.ip().is_loopback() && cfg.tls.is_none() && !cfg.allow_plaintext {
        bail!(
            "refusing to serve {} without TLS: pass --tls-cert/--tls-key, or --allow-plaintext on a trusted network (e.g. Tailscale)",
            cfg.bind
        );
    }
    let store = Store::open(&cfg.db).with_context(|| format!("opening {}", cfg.db.display()))?;
    let app = app(store, cfg.tls.is_some())?.into_make_service();
    match cfg.tls {
        Some((cert, key)) => {
            let _ = rustls::crypto::ring::default_provider().install_default();
            let tls = axum_server::tls_rustls::RustlsConfig::from_pem_file(&cert, &key)
                .await
                .with_context(|| format!("loading TLS cert {} / key {}", cert.display(), key.display()))?;
            let handle = axum_server::Handle::new();
            let h2 = handle.clone();
            tokio::spawn(async move {
                shutdown.await;
                h2.graceful_shutdown(Some(std::time::Duration::from_secs(3)));
            });
            tracing::info!("demo (rust) listening on https://{}", cfg.bind);
            axum_server::bind_rustls(cfg.bind, tls).handle(handle).serve(app).await?;
        }
        None => {
            let listener = tokio::net::TcpListener::bind(cfg.bind)
                .await
                .with_context(|| format!("binding {}", cfg.bind))?;
            tracing::info!("demo (rust) listening on http://{}", cfg.bind);
            axum::serve(listener, app).with_graceful_shutdown(shutdown).await?;
        }
    }
    Ok(())
}
