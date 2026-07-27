use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use tokio_tungstenite::{accept_async, connect_async, tungstenite::Message};
use uc_daemon_contract::api::types::DaemonWsEvent;
use uc_daemon_contract::constants::{ws_event, ws_topic};
use uc_engine::{EngineEvent, InboundNoticeEvent};
use uc_webserver::api::event_emitter::engine_event_to_ws;

#[derive(Serialize)]
struct InboundNoticeFixture {
    from_device: &'static str,
    snapshot_hash: &'static str,
    plaintext: Vec<u8>,
    text_preview: Option<&'static str>,
    representations: Vec<serde_json::Value>,
    action: &'static str,
    at_ms: i64,
}

fn large_inbound_notice() -> InboundNoticeEvent {
    let encoded = serde_json::to_vec(&InboundNoticeFixture {
        from_device: "peer-1",
        snapshot_hash: "hash-1",
        plaintext: vec![0; 20 * 1024 * 1024],
        text_preview: Some("large clipboard item"),
        representations: Vec::new(),
        action: "new_entry",
        at_ms: 42,
    })
    .expect("serialize inbound notice fixture");
    serde_json::from_slice(&encoded).expect("deserialize inbound notice fixture")
}

#[tokio::test]
async fn large_inbound_payload_keeps_default_client_connection_usable() {
    let inbound_notice = engine_event_to_ws(EngineEvent::InboundNotice(large_inbound_notice()))
        .expect("inbound notice websocket event");
    let follow_up = DaemonWsEvent {
        topic: ws_topic::CLIPBOARD.to_string(),
        event_type: ws_event::CLIPBOARD_NEW_CONTENT.to_string(),
        session_id: None,
        ts: 43,
        payload: serde_json::json!({ "entryId": "entry-2" }),
    };

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind websocket test listener");
    let address = listener.local_addr().expect("websocket test address");
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept websocket client");
        let mut socket = accept_async(stream)
            .await
            .expect("accept websocket handshake");
        for event in [inbound_notice, follow_up] {
            let payload = serde_json::to_string(&event).expect("serialize websocket event");
            socket
                .send(Message::Text(payload))
                .await
                .expect("send websocket event");
        }
        socket.close(None).await.expect("close websocket server");
    });

    let (mut client, _) = connect_async(format!("ws://{address}"))
        .await
        .expect("connect websocket client with default limits");

    let first = receive_event(&mut client).await;
    assert_eq!(first.event_type, ws_event::CLIPBOARD_INBOUND_NOTICE);
    assert!(first.payload.get("plaintextBase64").is_none());

    let second = receive_event(&mut client).await;
    assert_eq!(second.event_type, ws_event::CLIPBOARD_NEW_CONTENT);
    assert_eq!(second.payload["entryId"], "entry-2");

    server.await.expect("websocket test server task");
}

async fn receive_event(
    client: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> DaemonWsEvent {
    let message = client
        .next()
        .await
        .expect("websocket event")
        .expect("read websocket event");
    let Message::Text(payload) = message else {
        panic!("expected text websocket event");
    };
    serde_json::from_str(&payload).expect("deserialize websocket event")
}
