//! Networking for the TUI: the WebSocket connection task (with reconnect
//! and backoff). Adapted from pt-tui's net.rs; demo has no login, so the
//! name travels in the `/ws` query string (protocol §1).
//!
//! TLS uses rustls with the `ring` provider, trusting the OS root store plus
//! an optional extra CA (`--ca-cert`) for self-signed LAN servers.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, anyhow, bail};
use demo_core::protocol::{ClientMsg, ServerMsg};
use futures_util::{SinkExt, StreamExt};
use rustls::pki_types::CertificateDer;
use rustls::pki_types::pem::PemObject;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::Connector;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::{self, Message};

pub const CLIENT_ID: &str = "tui-rust";

/// A parsed server base URL (`http://host:port` or `https://host[:port]`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerUrl {
    pub tls: bool,
    pub host: String,
    pub port: u16,
}

impl ServerUrl {
    pub fn parse(s: &str) -> anyhow::Result<ServerUrl> {
        let s = s.trim().trim_end_matches('/');
        let (tls, rest) = if let Some(r) = s.strip_prefix("https://") {
            (true, r)
        } else if let Some(r) = s.strip_prefix("http://") {
            (false, r)
        } else if s.contains("://") {
            bail!("the server URL must start with http:// or https://");
        } else {
            (false, s)
        };
        let authority = rest.split('/').next().unwrap_or("");
        if authority.is_empty() {
            bail!("missing host in server URL");
        }
        // [v6]:port | host:port | host
        let (host, port) = if let Some(r) = authority.strip_prefix('[') {
            let (h, after) = r
                .split_once(']')
                .ok_or_else(|| anyhow!("bad IPv6 address"))?;
            let port = after
                .strip_prefix(':')
                .map(|p| p.parse())
                .transpose()
                .context("bad port")?;
            (h.to_string(), port)
        } else {
            match authority.rsplit_once(':') {
                Some((h, p)) => (h.to_string(), Some(p.parse().context("bad port")?)),
                None => (authority.to_string(), None),
            }
        };
        if host.is_empty() {
            bail!("missing host in server URL");
        }
        let port = port.unwrap_or(if tls { 443 } else { 80 });
        Ok(ServerUrl { tls, host, port })
    }

    fn authority(&self) -> String {
        let h = if self.host.contains(':') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        let default = if self.tls { 443 } else { 80 };
        if self.port == default {
            h
        } else {
            format!("{h}:{}", self.port)
        }
    }

    /// Canonical base URL.
    pub fn base(&self) -> String {
        format!(
            "{}://{}",
            if self.tls { "https" } else { "http" },
            self.authority()
        )
    }

    /// `ws(s)://host/ws?client=tui-rust&name=<name>`. The name is already
    /// validated (`clean_name`), so only spaces need escaping.
    pub fn ws_url(&self, name: &str) -> String {
        let enc: String = name
            .bytes()
            .map(|b| {
                if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.') {
                    (b as char).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect();
        format!(
            "{}://{}/ws?client={CLIENT_ID}&name={enc}",
            if self.tls { "wss" } else { "ws" },
            self.authority()
        )
    }
}

/// Client TLS configuration (ring provider; OS roots + optional extra CA).
pub fn tls_config(ca_cert: Option<&Path>) -> anyhow::Result<Arc<rustls::ClientConfig>> {
    let mut roots = rustls::RootCertStore::empty();
    let native = rustls_native_certs::load_native_certs();
    for c in native.certs {
        let _ = roots.add(c);
    }
    if let Some(path) = ca_cert {
        for cert in CertificateDer::pem_file_iter(path)
            .with_context(|| format!("reading {}", path.display()))?
        {
            roots
                .add(cert.context("bad PEM certificate")?)
                .context("adding CA certificate")?;
        }
    }
    let cfg = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_root_certificates(roots)
    .with_no_client_auth();
    Ok(Arc::new(cfg))
}

// ---- WebSocket connection task -------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnState {
    Connecting,
    Online,
    Reconnecting,
}

#[derive(Debug)]
pub enum NetEvent {
    State(ConnState),
    Msg(Box<ServerMsg>),
    /// A connection problem worth showing (first failure only).
    Error(String),
    /// The server refused us for good (bad name: HTTP 400/403); no retry.
    Refused(String),
}

/// Handle to the background connection.
pub struct Connection {
    pub tx: mpsc::UnboundedSender<ClientMsg>,
    pub rx: mpsc::UnboundedReceiver<NetEvent>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub fn connect(url: ServerUrl, tls: Arc<rustls::ClientConfig>, name: String) -> Connection {
    let (tx, mut out_rx) = mpsc::unbounded_channel::<ClientMsg>();
    let (ev_tx, rx) = mpsc::unbounded_channel::<NetEvent>();
    let task = tokio::spawn(async move {
        let mut attempt = 0u32;
        let ping = Duration::from_secs(demo_core::protocol::PING_INTERVAL_SECS);
        loop {
            let _ = ev_tx.send(NetEvent::State(if attempt == 0 {
                ConnState::Connecting
            } else {
                ConnState::Reconnecting
            }));
            match ws_connect(&url, &tls, &name).await {
                Ok(mut ws) => {
                    attempt = 0;
                    // Anything queued while offline is stale; `welcome` replaces every value.
                    while out_rx.try_recv().is_ok() {}
                    let _ = ev_tx.send(NetEvent::State(ConnState::Online));
                    let mut keepalive = tokio::time::interval(ping);
                    keepalive.tick().await;
                    loop {
                        tokio::select! {
                            msg = ws.next() => match msg {
                                Some(Ok(Message::Text(t))) => match serde_json::from_str::<ServerMsg>(t.as_str()) {
                                    Ok(m) => { let _ = ev_tx.send(NetEvent::Msg(Box::new(m))); }
                                    Err(e) => { let _ = ev_tx.send(NetEvent::Error(format!("unreadable server message: {e}"))); }
                                },
                                Some(Ok(Message::Close(_))) => break,
                                Some(Ok(_)) => {}
                                Some(Err(_)) | None => break,
                            },
                            out = out_rx.recv() => match out {
                                Some(m) => {
                                    let text = serde_json::to_string(&m).unwrap_or_default();
                                    if ws.send(Message::Text(text.into())).await.is_err() { break; }
                                }
                                None => return, // app gone
                            },
                            _ = keepalive.tick() => {
                                let text = serde_json::to_string(&ClientMsg::Ping).unwrap_or_default();
                                if ws.send(Message::Text(text.into())).await.is_err() { break; }
                            }
                        }
                    }
                }
                Err(WsError::Refused(e)) => {
                    let _ = ev_tx.send(NetEvent::Refused(e));
                    return;
                }
                Err(WsError::Other(e)) => {
                    if attempt == 0 {
                        let _ = ev_tx.send(NetEvent::Error(e));
                    }
                }
            }
            attempt += 1;
            let _ = ev_tx.send(NetEvent::State(ConnState::Reconnecting));
            let wait = (500u64 << attempt.min(4)).min(5000);
            tokio::time::sleep(Duration::from_millis(wait)).await;
        }
    });
    Connection { tx, rx, task }
}

enum WsError {
    Refused(String),
    Other(String),
}

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>;

async fn ws_connect(
    url: &ServerUrl,
    tls: &Arc<rustls::ClientConfig>,
    name: &str,
) -> Result<Ws, WsError> {
    let req = url
        .ws_url(name)
        .into_client_request()
        .map_err(|e| WsError::Other(e.to_string()))?;
    let connector = if url.tls {
        Connector::Rustls(tls.clone())
    } else {
        Connector::Plain
    };
    let fut = tokio_tungstenite::connect_async_tls_with_config(req, None, true, Some(connector));
    match tokio::time::timeout(Duration::from_secs(10), fut).await {
        Err(_) => Err(WsError::Other(format!(
            "timed out connecting to {}",
            url.base()
        ))),
        Ok(Ok((ws, _))) => Ok(ws),
        Ok(Err(tungstenite::Error::Http(resp))) => {
            let status = resp.status().as_u16();
            let body = resp
                .body()
                .as_ref()
                .map(|b| String::from_utf8_lossy(b).to_string())
                .unwrap_or_default();
            let detail = error_text(&body);
            match status {
                400 | 403 => Err(WsError::Refused(format!(
                    "server refused the connection ({status}): {detail}"
                ))),
                503 => Err(WsError::Other("server is full (503)".into())),
                s => Err(WsError::Other(format!("server answered {s}: {detail}"))),
            }
        }
        Ok(Err(e)) => Err(WsError::Other(format!("can't reach {}: {e}", url.base()))),
    }
}

/// `{"error":"…"}` → the message; anything else → the first 120 chars.
fn error_text(body: &str) -> String {
    let text = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(String::from))
        .unwrap_or_else(|| body.chars().take(120).collect());
    demo_core::safe_text(text.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        let u = ServerUrl::parse("http://127.0.0.1:8790/").unwrap();
        assert_eq!((u.tls, u.host.as_str(), u.port), (false, "127.0.0.1", 8790));
        assert_eq!(u.base(), "http://127.0.0.1:8790");
        assert_eq!(
            u.ws_url("alice"),
            "ws://127.0.0.1:8790/ws?client=tui-rust&name=alice"
        );
        assert_eq!(
            u.ws_url("Mary Jo"),
            "ws://127.0.0.1:8790/ws?client=tui-rust&name=Mary%20Jo"
        );
        let u = ServerUrl::parse("https://demo.lan").unwrap();
        assert_eq!((u.tls, u.port), (true, 443));
        assert_eq!(u.ws_url("a"), "wss://demo.lan/ws?client=tui-rust&name=a");
        let u = ServerUrl::parse("[::1]:9000").unwrap();
        assert_eq!((u.host.as_str(), u.port), ("::1", 9000));
        assert!(ServerUrl::parse("http://").is_err());
        assert!(ServerUrl::parse("ftp://x").is_err());
        assert_eq!(error_text(r#"{"error":"bad name"}"#), "bad name");
    }
}
