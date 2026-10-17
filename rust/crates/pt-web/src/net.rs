//! HTTP (login/logout/me) and the WebSocket, all same-origin. The session
//! is an HttpOnly cookie, so this code never sees or stores the token.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use pt_core::model::User;
use pt_core::protocol::{ClientMsg, LoginRequest, ResultMsg, ServerMsg};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use crate::util::window;

type ResultCb = Box<dyn FnOnce(ResultMsg)>;

struct Socket {
    ws: web_sys::WebSocket,
    _on_open: Closure<dyn FnMut(web_sys::Event)>,
    _on_msg: Closure<dyn FnMut(web_sys::MessageEvent)>,
    _on_close: Closure<dyn FnMut(web_sys::CloseEvent)>,
}

thread_local! {
    static SOCKET: RefCell<Option<Socket>> = const { RefCell::new(None) };
    static PENDING: RefCell<HashMap<u64, ResultCb>> = RefCell::new(HashMap::new());
    static REQ: Cell<u64> = const { Cell::new(0) };
    static GENERATION: Cell<u64> = const { Cell::new(0) };
}

async fn fetch(method: &str, path: &str, body: Option<String>) -> Result<(u16, String), String> {
    let init = web_sys::RequestInit::new();
    init.set_method(method);
    init.set_credentials(web_sys::RequestCredentials::SameOrigin);
    init.set_mode(web_sys::RequestMode::SameOrigin);
    let headers = web_sys::Headers::new().map_err(|_| "headers")?;
    if let Some(b) = &body {
        let _ = headers.set("Content-Type", "application/json");
        init.set_body(&JsValue::from_str(b));
    }
    init.set_headers(&headers);
    let req = web_sys::Request::new_with_str_and_init(path, &init).map_err(|_| "bad request")?;
    let resp = JsFuture::from(window().fetch_with_request(&req))
        .await
        .map_err(|_| "the server can't be reached".to_string())?;
    let resp: web_sys::Response = resp.dyn_into().map_err(|_| "bad response")?;
    let status = resp.status();
    let text = match resp.text() {
        Ok(p) => JsFuture::from(p)
            .await
            .ok()
            .and_then(|v| v.as_string())
            .unwrap_or_default(),
        Err(_) => String::new(),
    };
    Ok((status, text))
}

pub async fn login(username: &str, password: &str) -> Result<User, String> {
    let body = serde_json::to_string(&LoginRequest {
        username: username.to_string(),
        password: password.to_string(),
        client: "web-rust".into(),
    })
    .map_err(|e| e.to_string())?;
    let (status, text) = fetch("POST", "/api/login", Some(body)).await?;
    match status {
        200 => {
            #[derive(serde::Deserialize)]
            struct R {
                user: User,
            }
            serde_json::from_str::<R>(&text)
                .map(|r| r.user)
                .map_err(|e| e.to_string())
        }
        401 => Err("Wrong username or password.".into()),
        429 => Err("Too many attempts. Wait a minute and try again.".into()),
        s => Err(format!("Login failed (HTTP {s}).")),
    }
}

/// Some(user) if the cookie session is valid; None if logged out; Err if offline.
pub async fn me() -> Result<Option<User>, String> {
    let (status, text) = fetch("GET", "/api/me", None).await?;
    match status {
        200 => Ok(serde_json::from_str(&text).ok()),
        401 => Ok(None),
        s => Err(format!("HTTP {s}")),
    }
}

pub async fn logout() {
    close();
    let _ = fetch("POST", "/api/logout", None).await;
}

/// Open the WebSocket. `on_msg` gets every server message; `on_close` runs
/// once when this socket ends (not when replaced by a newer one).
pub fn connect(
    on_open: impl Fn() + 'static,
    on_msg: impl Fn(ServerMsg) + 'static,
    on_close: impl Fn() + 'static,
) {
    close();
    let loc = window().location();
    let proto = if loc.protocol().ok().as_deref() == Some("https:") {
        "wss"
    } else {
        "ws"
    };
    let host = loc.host().unwrap_or_default();
    let Ok(ws) = web_sys::WebSocket::new(&format!("{proto}://{host}/ws?client=web-rust")) else {
        on_close();
        return;
    };
    let generation = GENERATION.with(|g| {
        g.set(g.get() + 1);
        g.get()
    });
    let on_open = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| on_open());
    let on_msg =
        Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |e: web_sys::MessageEvent| {
            let Some(text) = e.data().as_string() else {
                return;
            };
            match serde_json::from_str::<ServerMsg>(&text) {
                Ok(ServerMsg::Result(r)) => {
                    let cb = PENDING.with(|p| p.borrow_mut().remove(&r.req));
                    if let Some(cb) = cb {
                        cb(r);
                    } else {
                        on_msg(ServerMsg::Result(r));
                    }
                }
                Ok(m) => on_msg(m),
                Err(err) => leptos::logging::warn!("unreadable server message: {err}"),
            }
        });
    let on_close = Closure::<dyn FnMut(web_sys::CloseEvent)>::new(move |_| {
        if GENERATION.with(|g| g.get()) != generation {
            return;
        }
        // Requests in flight will never be answered.
        let pending: Vec<ResultCb> =
            PENDING.with(|p| p.borrow_mut().drain().map(|(_, cb)| cb).collect());
        for cb in pending {
            cb(ResultMsg {
                ok: false,
                error: Some(pt_core::protocol::ErrorBody::new(
                    "offline",
                    "connection lost; change not saved",
                )),
                ..Default::default()
            });
        }
        SOCKET.with(|s| s.borrow_mut().take());
        on_close();
    });
    ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));
    ws.set_onmessage(Some(on_msg.as_ref().unchecked_ref()));
    ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));
    SOCKET.with(|s| {
        *s.borrow_mut() = Some(Socket {
            ws,
            _on_open: on_open,
            _on_msg: on_msg,
            _on_close: on_close,
        })
    });
}

pub fn close() {
    GENERATION.with(|g| g.set(g.get() + 1));
    if let Some(s) = SOCKET.with(|s| s.borrow_mut().take()) {
        s.ws.set_onclose(None);
        s.ws.set_onmessage(None);
        let _ = s.ws.close();
    }
}

pub fn send(msg: &ClientMsg) -> bool {
    let Ok(text) = serde_json::to_string(msg) else {
        return false;
    };
    SOCKET.with(|s| match s.borrow().as_ref() {
        Some(s) if s.ws.ready_state() == web_sys::WebSocket::OPEN => {
            s.ws.send_with_str(&text).is_ok()
        }
        _ => false,
    })
}

/// Send a request carrying `req`; `cb` gets the matching result.
pub fn request(make: impl FnOnce(u64) -> ClientMsg, cb: impl FnOnce(ResultMsg) + 'static) -> bool {
    let req = REQ.with(|r| {
        r.set(r.get() + 1);
        r.get()
    });
    PENDING.with(|p| p.borrow_mut().insert(req, Box::new(cb)));
    if send(&make(req)) {
        true
    } else {
        PENDING.with(|p| p.borrow_mut().remove(&req));
        false
    }
}
