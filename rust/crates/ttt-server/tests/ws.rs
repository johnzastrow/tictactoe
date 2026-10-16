//! End-to-end: real server on an ephemeral port, real WebSocket client.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest, http::HeaderValue},
};
use ttt_core::Difficulty;
use ttt_server::{ServerMsg, spawn_hub};

async fn start() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let hub = spawn_hub(None, Difficulty::Hard, Duration::ZERO);
    tokio::spawn(ttt_server::serve(listener, hub, std::future::pending()));
    port
}

fn request(
    port: u16,
    origin: Option<&str>,
    host: Option<&str>,
) -> tokio_tungstenite::tungstenite::handshake::client::Request {
    let mut req = format!("ws://127.0.0.1:{port}/ws")
        .into_client_request()
        .unwrap();
    if let Some(o) = origin {
        req.headers_mut()
            .insert("Origin", HeaderValue::from_str(o).unwrap());
    }
    if let Some(h) = host {
        req.headers_mut()
            .insert("Host", HeaderValue::from_str(h).unwrap());
    }
    req
}

async fn next_msg<S>(ws: &mut S) -> ServerMsg
where
    S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    loop {
        if let Message::Text(t) = ws.next().await.unwrap().unwrap() {
            return serde_json::from_str(&t).unwrap();
        }
    }
}

#[tokio::test]
async fn move_is_broadcast_to_all_clients() {
    let port = start().await;
    let origin = format!("http://127.0.0.1:{port}");
    let (mut a, _) = connect_async(request(port, Some(&origin), None))
        .await
        .unwrap();
    let (mut b, _) = connect_async(request(port, Some(&origin), None))
        .await
        .unwrap();
    assert!(matches!(next_msg(&mut a).await, ServerMsg::State(_)));
    assert!(matches!(next_msg(&mut b).await, ServerMsg::State(_)));

    a.send(Message::Text(r#"{"type":"move","cell":4}"#.into()))
        .await
        .unwrap();
    // b may first see the client-count update; wait for the move.
    loop {
        if let ServerMsg::State(s) = next_msg(&mut b).await
            && s.board[4].is_some()
        {
            break;
        }
    }
}

#[tokio::test]
async fn bad_input_gets_error_reply() {
    let port = start().await;
    let (mut ws, _) = connect_async(request(port, None, None)).await.unwrap();
    next_msg(&mut ws).await;
    ws.send(Message::Text(r#"{"type":"move","cell":42}"#.into()))
        .await
        .unwrap();
    ws.send(Message::Text("not json".into())).await.unwrap();
    let mut errors = Vec::new();
    while errors.len() < 2 {
        if let ServerMsg::Error { message } = next_msg(&mut ws).await {
            errors.push(message);
        }
    }
    assert!(errors[0].contains("out of range"), "{errors:?}");
    assert_eq!(errors[1], "malformed message");
}

#[tokio::test]
async fn foreign_origin_is_rejected() {
    let port = start().await;
    let err = connect_async(request(port, Some("https://evil.example"), None))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("403"), "{err}");
}

#[tokio::test]
async fn foreign_host_is_rejected() {
    let port = start().await;
    let err = connect_async(request(port, None, Some("evil.example")))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("421"), "{err}");
}

#[tokio::test]
async fn set_difficulty_is_broadcast() {
    let port = start().await;
    let (mut ws, _) = connect_async(request(port, None, None)).await.unwrap();
    match next_msg(&mut ws).await {
        ServerMsg::State(s) => assert_eq!(s.difficulty, Difficulty::Hard),
        other => panic!("{other:?}"),
    }
    ws.send(Message::Text(
        r#"{"type":"set_difficulty","difficulty":"easy"}"#.into(),
    ))
    .await
    .unwrap();
    loop {
        if let ServerMsg::State(s) = next_msg(&mut ws).await
            && s.difficulty == Difficulty::Easy
        {
            break;
        }
    }
    ws.send(Message::Text(
        r#"{"type":"set_difficulty","difficulty":"impossible"}"#.into(),
    ))
    .await
    .unwrap();
    loop {
        if let ServerMsg::Error { message } = next_msg(&mut ws).await {
            assert_eq!(message, "malformed message");
            break;
        }
    }
}
