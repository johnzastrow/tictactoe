//! The WebSocket to `/ws` (protocol §1), same-origin. Text frames in are
//! handed to the app as strings (demo-core parses them); frames out are
//! `ClientMsg`s. A generation counter makes sure only the newest socket's
//! close callback runs, so replacing or leaving never triggers a reconnect.

use std::cell::{Cell, RefCell};

use demo_core::ClientMsg;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use crate::util::window;

struct Socket {
    ws: web_sys::WebSocket,
    _on_open: Closure<dyn FnMut(web_sys::Event)>,
    _on_msg: Closure<dyn FnMut(web_sys::MessageEvent)>,
    _on_close: Closure<dyn FnMut(web_sys::CloseEvent)>,
}

thread_local! {
    static SOCKET: RefCell<Option<Socket>> = const { RefCell::new(None) };
    static GENERATION: Cell<u64> = const { Cell::new(0) };
}

/// Open the socket as `name`. `on_msg` gets every text frame; `on_close`
/// runs once when this socket ends (with `opened` = whether it ever opened),
/// not when it is replaced or closed by [`close`].
pub fn connect(
    name: &str,
    on_open: impl Fn() + 'static,
    on_msg: impl Fn(String) + 'static,
    on_close: impl Fn(bool) + 'static,
) {
    close();
    let loc = window().location();
    let proto = if loc.protocol().ok().as_deref() == Some("https:") {
        "wss"
    } else {
        "ws"
    };
    let host = loc.host().unwrap_or_default();
    let name = String::from(js_sys::encode_uri_component(name));
    let Ok(ws) =
        web_sys::WebSocket::new(&format!("{proto}://{host}/ws?client=web-rust&name={name}"))
    else {
        on_close(false);
        return;
    };
    let generation = GENERATION.with(|g| {
        g.set(g.get() + 1);
        g.get()
    });
    let opened = std::rc::Rc::new(Cell::new(false));
    let o2 = opened.clone();
    let on_open = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
        o2.set(true);
        on_open();
    });
    let on_msg =
        Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |e: web_sys::MessageEvent| {
            if let Some(text) = e.data().as_string() {
                on_msg(text);
            }
        });
    let on_close = Closure::<dyn FnMut(web_sys::CloseEvent)>::new(move |_| {
        if GENERATION.with(|g| g.get()) != generation {
            return;
        }
        SOCKET.with(|s| s.borrow_mut().take());
        on_close(opened.get());
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

/// Close without triggering `on_close` (leaving, or before reconnecting).
pub fn close() {
    GENERATION.with(|g| g.set(g.get() + 1));
    if let Some(s) = SOCKET.with(|s| s.borrow_mut().take()) {
        s.ws.set_onclose(None);
        s.ws.set_onmessage(None);
        s.ws.set_onopen(None);
        let _ = s.ws.close();
    }
}

/// Send one message; false if the socket isn't open.
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
