//! Networking for the TUI: a tiny HTTP/1.1 client for login/logout/health,
//! the WebSocket connection task (with reconnect and backoff), and the
//! remembered-session file.
//!
//! TLS uses rustls with the `ring` provider, trusting the OS root store plus
//! an optional extra CA (`--ca-cert`) for self-signed LAN servers.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, anyhow, bail};
use futures_util::{SinkExt, StreamExt};
use pt_core::protocol::{ClientMsg, LoginRequest, LoginResponse, ServerMsg};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, ServerName};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
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
        } else {
            (false, s)
        };
        let authority = rest.split('/').next().unwrap_or("");
        if authority.is_empty() {
            bail!("missing host in server URL");
        }
        // [v6]:port | host:port | host
        let (host, port) = if let Some(r) = authority.strip_prefix('[') {
            let (h, after) = r.split_once(']').ok_or_else(|| anyhow!("bad IPv6 address"))?;
            let port = after.strip_prefix(':').map(|p| p.parse()).transpose().context("bad port")?;
            (h.to_string(), port)
        } else {
            match authority.rsplit_once(':') {
                Some((h, p)) => (h.to_string(), Some(p.parse().context("bad port")?)),
                None => (authority.to_string(), None),
            }
        };
        let port = port.unwrap_or(if tls { 443 } else { 80 });
        Ok(ServerUrl { tls, host, port })
    }

    fn authority(&self) -> String {
        let h = if self.host.contains(':') { format!("[{}]", self.host) } else { self.host.clone() };
        let default = if self.tls { 443 } else { 80 };
        if self.port == default { h } else { format!("{h}:{}", self.port) }
    }

    /// Canonical base URL; also the key in the sessions file.
    pub fn base(&self) -> String {
        format!("{}://{}", if self.tls { "https" } else { "http" }, self.authority())
    }

    pub fn ws_url(&self) -> String {
        format!("{}://{}/ws?client={CLIENT_ID}", if self.tls { "wss" } else { "ws" }, self.authority())
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
        for cert in CertificateDer::pem_file_iter(path).with_context(|| format!("reading {}", path.display()))? {
            roots.add(cert.context("bad PEM certificate")?).context("adding CA certificate")?;
        }
    }
    let cfg = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(Arc::new(cfg))
}

trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

async fn open(url: &ServerUrl, tls: &Arc<rustls::ClientConfig>) -> anyhow::Result<Box<dyn Io>> {
    let tcp = tokio::time::timeout(Duration::from_secs(10), TcpStream::connect((url.host.as_str(), url.port)))
        .await
        .map_err(|_| anyhow!("timed out connecting to {}", url.base()))?
        .with_context(|| format!("can't reach {}", url.base()))?;
    let _ = tcp.set_nodelay(true);
    if !url.tls {
        return Ok(Box::new(tcp));
    }
    let name = ServerName::try_from(url.host.clone()).context("bad server name")?;
    let s = tokio_rustls::TlsConnector::from(tls.clone()).connect(name, tcp).await.context("TLS handshake failed")?;
    Ok(Box::new(s))
}

/// One HTTP/1.1 request with `Connection: close`; returns (status, body).
pub async fn http(
    url: &ServerUrl,
    tls: &Arc<rustls::ClientConfig>,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<&str>,
) -> anyhow::Result<(u16, String)> {
    let mut s = open(url, tls).await?;
    let body = body.unwrap_or("");
    let mut req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nUser-Agent: pt-tui\r\nContent-Length: {}\r\n",
        url.authority(),
        body.len()
    );
    if !body.is_empty() {
        req.push_str("Content-Type: application/json\r\n");
    }
    if let Some(t) = token {
        req.push_str(&format!("Authorization: Bearer {t}\r\n"));
    }
    req.push_str("\r\n");
    req.push_str(body);
    s.write_all(req.as_bytes()).await?;
    s.flush().await?;
    let mut buf = Vec::new();
    // TLS peers may close without close_notify; whatever arrived is the response.
    let read = tokio::time::timeout(Duration::from_secs(15), s.read_to_end(&mut buf)).await;
    match read {
        Err(_) => bail!("server did not answer"),
        Ok(Err(e)) if buf.is_empty() => return Err(e.into()),
        _ => {}
    }
    parse_response(&buf)
}

fn parse_response(buf: &[u8]) -> anyhow::Result<(u16, String)> {
    let text = String::from_utf8_lossy(buf);
    let (head, rest) = text.split_once("\r\n\r\n").ok_or_else(|| anyhow!("bad HTTP response"))?;
    let status: u16 = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| anyhow!("bad HTTP status line"))?;
    let chunked = head.to_ascii_lowercase().contains("transfer-encoding: chunked");
    let body = if chunked { dechunk(rest) } else { rest.to_string() };
    Ok((status, body))
}

fn dechunk(mut r: &str) -> String {
    let mut out = String::new();
    while let Some((size, tail)) = r.split_once("\r\n") {
        let n = usize::from_str_radix(size.trim(), 16).unwrap_or(0);
        if n == 0 || tail.len() < n {
            break;
        }
        out.push_str(&tail[..n]);
        r = tail.get(n + 2..).unwrap_or("");
    }
    out
}

fn error_text(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(String::from))
        .unwrap_or_else(|| body.chars().take(120).collect())
}

pub async fn login(url: &ServerUrl, tls: &Arc<rustls::ClientConfig>, user: &str, password: &str) -> anyhow::Result<LoginResponse> {
    let body = serde_json::to_string(&LoginRequest {
        username: user.to_string(),
        password: password.to_string(),
        client: CLIENT_ID.to_string(),
    })?;
    let (status, body) = http(url, tls, "POST", "/api/login", None, Some(&body)).await?;
    match status {
        200 => Ok(serde_json::from_str(&body).context("unexpected login reply")?),
        401 => bail!("Wrong username or password."),
        429 => bail!("Too many attempts. Wait a few minutes and try again."),
        s => bail!("Login failed ({s}): {}", error_text(&body)),
    }
}

pub async fn logout(url: &ServerUrl, tls: &Arc<rustls::ClientConfig>, token: &str) -> anyhow::Result<()> {
    http(url, tls, "POST", "/api/logout", Some(token), None).await?;
    Ok(())
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
    /// The session was rejected (expired, logged out, deactivated).
    AuthFailed,
    Error(String),
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

pub fn connect(url: ServerUrl, tls: Arc<rustls::ClientConfig>, token: String) -> Connection {
    let (tx, mut out_rx) = mpsc::unbounded_channel::<ClientMsg>();
    let (ev_tx, rx) = mpsc::unbounded_channel::<NetEvent>();
    let task = tokio::spawn(async move {
        let mut attempt = 0u32;
        loop {
            let _ = ev_tx.send(NetEvent::State(if attempt == 0 { ConnState::Connecting } else { ConnState::Reconnecting }));
            match ws_connect(&url, &tls, &token).await {
                Ok(mut ws) => {
                    attempt = 0;
                    // Anything queued while offline is stale; the UI blocks edits offline.
                    while out_rx.try_recv().is_ok() {}
                    let _ = ev_tx.send(NetEvent::State(ConnState::Online));
                    loop {
                        tokio::select! {
                            msg = ws.next() => match msg {
                                Some(Ok(Message::Text(t))) => match serde_json::from_str::<ServerMsg>(t.as_str()) {
                                    Ok(m) => { let _ = ev_tx.send(NetEvent::Msg(Box::new(m))); }
                                    Err(e) => { let _ = ev_tx.send(NetEvent::Error(format!("unreadable server message: {e}"))); }
                                },
                                // Reconnect after any close; a dead session then gets a 401 upgrade.
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
                            }
                        }
                    }
                }
                Err(WsError::Auth) => {
                    let _ = ev_tx.send(NetEvent::AuthFailed);
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
    Auth,
    Other(String),
}

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>;

async fn ws_connect(url: &ServerUrl, tls: &Arc<rustls::ClientConfig>, token: &str) -> Result<Ws, WsError> {
    let mut req = url.ws_url().into_client_request().map_err(|e| WsError::Other(e.to_string()))?;
    let auth = format!("Bearer {token}").parse().map_err(|_| WsError::Auth)?;
    req.headers_mut().insert("Authorization", auth);
    let connector = if url.tls { Connector::Rustls(tls.clone()) } else { Connector::Plain };
    let fut = tokio_tungstenite::connect_async_tls_with_config(req, None, true, Some(connector));
    match tokio::time::timeout(Duration::from_secs(10), fut).await {
        Err(_) => Err(WsError::Other("timed out".into())),
        Ok(Ok((ws, _))) => Ok(ws),
        Ok(Err(tungstenite::Error::Http(resp))) if resp.status().as_u16() == 401 => Err(WsError::Auth),
        Ok(Err(e)) => Err(WsError::Other(e.to_string())),
    }
}

// ---- remembered sessions ------------------------------------------------------------------

/// `<config dir>/pt/sessions.json` (mode 0600 on Unix).
pub fn sessions_path() -> Option<PathBuf> {
    let base = if let Ok(x) = std::env::var("PT_CONFIG_DIR") {
        PathBuf::from(x)
    } else if cfg!(windows) {
        PathBuf::from(std::env::var_os("APPDATA")?).join("pt")
    } else if cfg!(target_os = "macos") {
        PathBuf::from(std::env::var_os("HOME")?).join("Library/Application Support/pt")
    } else {
        match std::env::var_os("XDG_CONFIG_HOME") {
            Some(x) if !x.is_empty() => PathBuf::from(x).join("pt"),
            _ => PathBuf::from(std::env::var_os("HOME")?).join(".config/pt"),
        }
    };
    Some(base.join("sessions.json"))
}

/// Stored per server: the token and the username it belongs to.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Saved {
    pub token: String,
    pub username: String,
}

pub fn load_sessions() -> BTreeMap<String, Saved> {
    sessions_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_sessions(map: &BTreeMap<String, Saved>) -> anyhow::Result<()> {
    let path = sessions_path().ok_or_else(|| anyhow!("no config directory"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string_pretty(map)?;
    write_private(&path, text.as_bytes())
}

/// Write a file readable only by the owner (0600 on Unix).
pub fn write_private(path: &Path, data: &[u8]) -> anyhow::Result<()> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(path).with_context(|| format!("writing {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    f.write_all(data)?;
    Ok(())
}

pub fn remember(url: &ServerUrl, username: &str, token: &str) -> anyhow::Result<()> {
    let mut m = load_sessions();
    m.insert(url.base(), Saved { token: token.to_string(), username: username.to_string() });
    save_sessions(&m)
}

pub fn forget(url: &ServerUrl) -> anyhow::Result<Option<Saved>> {
    let mut m = load_sessions();
    let old = m.remove(&url.base());
    if old.is_some() {
        save_sessions(&m)?;
    }
    Ok(old)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        let u = ServerUrl::parse("http://127.0.0.1:8788/").unwrap();
        assert_eq!((u.tls, u.host.as_str(), u.port), (false, "127.0.0.1", 8788));
        assert_eq!(u.base(), "http://127.0.0.1:8788");
        assert_eq!(u.ws_url(), "ws://127.0.0.1:8788/ws?client=tui-rust");
        let u = ServerUrl::parse("https://pt.lan").unwrap();
        assert_eq!((u.tls, u.port), (true, 443));
        assert_eq!(u.ws_url(), "wss://pt.lan/ws?client=tui-rust");
        let u = ServerUrl::parse("[::1]:9000").unwrap();
        assert_eq!((u.host.as_str(), u.port), ("::1", 9000));
        assert_eq!(u.base(), "http://[::1]:9000");
        assert!(ServerUrl::parse("http://").is_err());
    }

    #[test]
    fn responses() {
        let r = parse_response(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\nok").unwrap();
        assert_eq!(r, (200, "ok".into()));
        let r = parse_response(b"HTTP/1.1 401 X\r\ntransfer-encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n").unwrap();
        assert_eq!(r, (401, "abcde".into()));
        assert_eq!(error_text(r#"{"error":"invalid credentials"}"#), "invalid credentials");
    }

    #[test]
    fn private_files() {
        let dir = std::env::temp_dir().join(format!("pt-tui-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("s.json");
        write_private(&p, b"{}").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
