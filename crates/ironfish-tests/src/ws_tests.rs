use crate::helpers::TestServer;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

async fn send_json(
    sink: &mut futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        Message,
    >,
    msg: Value,
) {
    sink.send(Message::Text(msg.to_string().into()))
        .await
        .expect("send");
}

async fn recv_json(
    stream: &mut futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
) -> Value {
    let timeout = tokio::time::Duration::from_secs(5);
    let msg = tokio::time::timeout(timeout, stream.next())
        .await
        .expect("recv timeout")
        .expect("stream ended")
        .expect("ws error");
    match msg {
        Message::Text(text) => serde_json::from_str(&text).expect("parse json"),
        other => panic!("unexpected message: {:?}", other),
    }
}

#[tokio::test]
async fn test_ws_connect_and_auth() {
    let server = TestServer::new().await;
    let (mut sink, mut stream) = server.ws_connect(None).await;
    send_json(
        &mut sink,
        json!({"type": "auth", "id": "1", "token": server.token}),
    )
    .await;
    let resp = recv_json(&mut stream).await;
    assert_eq!(resp["type"], "auth_result");
    assert_eq!(resp["id"], "1");
    assert_eq!(resp["success"], true);
}

#[tokio::test]
async fn test_ws_query_param_auth() {
    let server = TestServer::new().await;
    let (mut sink, mut stream) = server.ws_connect(Some(&server.token)).await;
    send_json(&mut sink, json!({"type": "ping", "id": "1"})).await;
    let resp = recv_json(&mut stream).await;
    assert_eq!(resp["type"], "pong");
    assert_eq!(resp["id"], "1");
}

#[tokio::test]
async fn test_ws_auth_timeout() {
    let server = TestServer::new().await;
    let (_sink, mut stream) = server.ws_connect(None).await;
    let timeout = tokio::time::Duration::from_secs(10);
    let mut got_auth_error = false;
    let mut got_disconnect = false;
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout_at(deadline, stream.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                let resp: Value = serde_json::from_str(&text).expect("parse");
                if resp["type"] == "error" && resp["code"] == 401 {
                    got_auth_error = true;
                }
            }
            Ok(Some(Ok(Message::Close(_)))) | Ok(None) => {
                got_disconnect = true;
                break;
            }
            Ok(Some(Err(_))) => {
                got_disconnect = true;
                break;
            }
            Err(_) => break,
            _ => {}
        }
    }
    assert!(got_auth_error || got_disconnect);
}

#[tokio::test]
async fn test_ws_analyze_streaming() {
    let server = TestServer::new().await;
    let (mut sink, mut stream) = server.ws_connect(Some(&server.token)).await;
    send_json(
        &mut sink,
        json!({
            "type": "analyze",
            "id": "a1",
            "fen": "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "depth": 20,
            "multipv": 1
        }),
    )
    .await;
    let mut got_progress = false;
    let mut got_complete = false;
    let timeout = tokio::time::Duration::from_secs(10);
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout_at(deadline, stream.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                let resp: Value = serde_json::from_str(&text).expect("parse");
                match resp["type"].as_str() {
                    Some("analysis_progress") => {
                        got_progress = true;
                        assert!(resp["current_depth"].as_u64().unwrap() > 0);
                    }
                    Some("analysis_complete") => {
                        got_complete = true;
                        assert_eq!(resp["id"], "a1");
                        assert!(resp["result"]["best_move"].is_object());
                        break;
                    }
                    Some("pong") => continue,
                    _ => {}
                }
            }
            _ => break,
        }
    }
    assert!(got_progress, "should have received progress messages");
    assert!(got_complete, "should have received complete message");
}

#[tokio::test]
async fn test_ws_cancel_analysis() {
    let server = TestServer::new().await;
    let (mut sink, mut stream) = server.ws_connect(Some(&server.token)).await;
    send_json(
        &mut sink,
        json!({
            "type": "analyze",
            "id": "a1",
            "fen": "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "depth": 20,
            "multipv": 1
        }),
    )
    .await;
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    let first_msg = recv_json(&mut stream).await;
    let analysis_id = if first_msg["type"] == "analysis_progress" {
        first_msg["analysis_id"].as_str().unwrap().to_string()
    } else {
        return;
    };
    send_json(
        &mut sink,
        json!({
            "type": "cancel",
            "id": "c1",
            "analysis_id": analysis_id
        }),
    )
    .await;
    let timeout = tokio::time::Duration::from_secs(5);
    let deadline = tokio::time::Instant::now() + timeout;
    let mut got_cancelled = false;
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout_at(deadline, stream.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                let resp: Value = serde_json::from_str(&text).expect("parse");
                if resp["type"] == "analysis_cancelled" {
                    got_cancelled = true;
                    break;
                }
                if resp["type"] == "analysis_complete" {
                    break;
                }
            }
            _ => break,
        }
    }
    assert!(got_cancelled, "Expected to receive analysis_cancelled");
}

#[tokio::test]
async fn test_ws_bestmove() {
    let server = TestServer::new().await;
    let (mut sink, mut stream) = server.ws_connect(Some(&server.token)).await;
    send_json(
        &mut sink,
        json!({
            "type": "bestmove",
            "id": "b1",
            "fen": "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "movetime": 1000
        }),
    )
    .await;
    let timeout = tokio::time::Duration::from_secs(10);
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        match tokio::time::timeout_at(deadline, stream.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                let resp: Value = serde_json::from_str(&text).expect("parse");
                if resp["type"] == "bestmove_result" {
                    assert_eq!(resp["id"], "b1");
                    assert!(resp["result"]["best_move"].is_object());
                    break;
                }
                if resp["type"] == "pong" {
                    continue;
                }
            }
            _ => panic!("did not receive bestmove result"),
        }
    }
}

#[tokio::test]
async fn test_ws_cluster_subscribe() {
    let server = TestServer::new().await;
    let (mut sink, mut stream) = server.ws_connect(Some(&server.token)).await;
    send_json(
        &mut sink,
        json!({"type": "subscribe", "id": "s1", "topics": ["cluster"]}),
    )
    .await;
    let resp = recv_json(&mut stream).await;
    assert_eq!(resp["type"], "subscribed");
    assert_eq!(resp["id"], "s1");
    assert_eq!(resp["topics"], json!(["cluster"]));
}

#[tokio::test]
async fn test_ws_ping_pong() {
    let server = TestServer::new().await;
    let (mut sink, mut stream) = server.ws_connect(Some(&server.token)).await;
    send_json(&mut sink, json!({"type": "ping", "id": "p1"})).await;
    let resp = recv_json(&mut stream).await;
    assert_eq!(resp["type"], "pong");
    assert_eq!(resp["id"], "p1");
}

#[tokio::test]
async fn test_ws_invalid_message() {
    let server = TestServer::new().await;
    let (mut sink, mut stream) = server.ws_connect(Some(&server.token)).await;
    sink.send(Message::Text("not json at all".into()))
        .await
        .expect("send");
    let resp = recv_json(&mut stream).await;
    assert_eq!(resp["type"], "error");
    assert_eq!(resp["code"], 400);
}

#[tokio::test]
async fn test_ws_concurrent_analyses() {
    let server = TestServer::new().await;
    let (mut sink, mut stream) = server.ws_connect(Some(&server.token)).await;
    for i in 0..2 {
        send_json(
            &mut sink,
            json!({
                "type": "analyze",
                "id": format!("a{}", i),
                "fen": "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
                "depth": 20,
                "multipv": 1
            }),
        )
        .await;
    }
    let mut completed = std::collections::HashSet::new();
    let timeout = tokio::time::Duration::from_secs(15);
    let deadline = tokio::time::Instant::now() + timeout;
    while completed.len() < 2 && tokio::time::Instant::now() < deadline {
        match tokio::time::timeout_at(deadline, stream.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                let resp: Value = serde_json::from_str(&text).expect("parse");
                if resp["type"] == "analysis_complete" {
                    completed.insert(resp["id"].as_str().unwrap().to_string());
                }
            }
            _ => break,
        }
    }
    assert_eq!(completed.len(), 2);
}

#[tokio::test]
async fn test_ws_auth_with_invalid_token() {
    let server = TestServer::new().await;
    let (mut sink, mut stream) = server.ws_connect(None).await;
    send_json(
        &mut sink,
        json!({"type": "auth", "id": "1", "token": "iff_totally_invalid_token"}),
    )
    .await;
    let resp = recv_json(&mut stream).await;
    assert_eq!(resp["type"], "auth_result");
    assert_eq!(resp["success"], false);
    assert!(resp["error"].as_str().is_some());
}

#[tokio::test]
async fn test_ws_unauthenticated_analyze_rejected() {
    let server = TestServer::new().await;
    let (mut sink, mut stream) = server.ws_connect(None).await;
    send_json(
        &mut sink,
        json!({
            "type": "analyze",
            "id": "a1",
            "fen": "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "depth": 10,
            "multipv": 1
        }),
    )
    .await;
    let resp = recv_json(&mut stream).await;
    assert_eq!(resp["type"], "error");
    assert_eq!(resp["code"], 401);
}

#[tokio::test]
async fn test_ws_unauthenticated_bestmove_rejected() {
    let server = TestServer::new().await;
    let (mut sink, mut stream) = server.ws_connect(None).await;
    send_json(
        &mut sink,
        json!({
            "type": "bestmove",
            "id": "b1",
            "fen": "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"
        }),
    )
    .await;
    let resp = recv_json(&mut stream).await;
    assert_eq!(resp["type"], "error");
    assert_eq!(resp["code"], 401);
}

#[tokio::test]
async fn test_ws_ping_works_without_auth() {
    let server = TestServer::new().await;
    let (mut sink, mut stream) = server.ws_connect(None).await;
    send_json(&mut sink, json!({"type": "ping", "id": "p1"})).await;
    let resp = recv_json(&mut stream).await;
    assert_eq!(resp["type"], "pong");
    assert_eq!(resp["id"], "p1");
}

#[tokio::test]
async fn test_ws_invalid_fen_returns_error() {
    let server = TestServer::new().await;
    let (mut sink, mut stream) = server.ws_connect(Some(&server.token)).await;
    send_json(
        &mut sink,
        json!({
            "type": "analyze",
            "id": "a1",
            "fen": "not-a-valid-fen",
            "depth": 10,
            "multipv": 1
        }),
    )
    .await;
    let timeout = tokio::time::Duration::from_secs(5);
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        match tokio::time::timeout_at(deadline, stream.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                let resp: Value = serde_json::from_str(&text).expect("parse");
                if resp["type"] == "error" {
                    assert_eq!(resp["code"], 500);
                    assert!(resp["message"].as_str().unwrap().contains("invalid FEN"));
                    break;
                }
                if resp["type"] == "pong" {
                    continue;
                }
            }
            _ => panic!("did not receive error for invalid FEN"),
        }
    }
}

#[tokio::test]
async fn test_ws_subscribe_and_unsubscribe() {
    let server = TestServer::new().await;
    let (mut sink, mut stream) = server.ws_connect(Some(&server.token)).await;
    send_json(
        &mut sink,
        json!({"type": "subscribe", "id": "s1", "topics": ["cluster", "metrics"]}),
    )
    .await;
    let resp = recv_json(&mut stream).await;
    assert_eq!(resp["type"], "subscribed");
    let topics = resp["topics"].as_array().unwrap();
    assert_eq!(topics.len(), 2);

    send_json(
        &mut sink,
        json!({"type": "unsubscribe", "id": "u1", "topics": ["metrics"]}),
    )
    .await;

    send_json(
        &mut sink,
        json!({"type": "subscribe", "id": "s2", "topics": ["cluster"]}),
    )
    .await;
    let resp = recv_json(&mut stream).await;
    assert_eq!(resp["type"], "subscribed");
    assert_eq!(resp["topics"], json!(["cluster"]));
}

#[tokio::test]
async fn test_ws_multiple_pings() {
    let server = TestServer::new().await;
    let (mut sink, mut stream) = server.ws_connect(Some(&server.token)).await;
    for i in 0..5 {
        send_json(&mut sink, json!({"type": "ping", "id": format!("p{}", i)})).await;
    }
    for i in 0..5 {
        let resp = recv_json(&mut stream).await;
        assert_eq!(resp["type"], "pong");
        assert_eq!(resp["id"], format!("p{}", i));
    }
}

#[tokio::test]
async fn test_ws_analyze_progress_has_evaluation() {
    let server = TestServer::new().await;
    let (mut sink, mut stream) = server.ws_connect(Some(&server.token)).await;
    send_json(
        &mut sink,
        json!({
            "type": "analyze",
            "id": "a1",
            "fen": "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "depth": 20,
            "multipv": 1
        }),
    )
    .await;
    let timeout = tokio::time::Duration::from_secs(10);
    let deadline = tokio::time::Instant::now() + timeout;
    let mut progress_count = 0;
    let mut complete_result: Option<Value> = None;
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout_at(deadline, stream.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                let resp: Value = serde_json::from_str(&text).expect("parse");
                match resp["type"].as_str() {
                    Some("analysis_progress") => {
                        progress_count += 1;
                        assert!(resp["target_depth"].as_u64().unwrap() == 20);
                        assert!(resp["nodes_per_second"].is_u64());
                        if resp["evaluation"].is_object() {
                            let eval = &resp["evaluation"];
                            assert!(eval["score_type"].is_string());
                            assert!(eval["value"].is_i64());
                        }
                        if resp["principal_variations"].is_array() {
                            let pvs = resp["principal_variations"].as_array().unwrap();
                            if !pvs.is_empty() {
                                assert!(pvs[0]["rank"].is_u64());
                                assert!(pvs[0]["moves"].is_array());
                            }
                        }
                    }
                    Some("analysis_complete") => {
                        complete_result = Some(resp);
                        break;
                    }
                    Some("pong") => continue,
                    _ => {}
                }
            }
            _ => break,
        }
    }
    assert!(
        progress_count >= 2,
        "should have at least 2 progress messages, got {}",
        progress_count
    );
    let result = complete_result.expect("should have received analysis_complete");
    let analysis = &result["result"];
    assert!(analysis["best_move"]["from"].is_string());
    assert!(analysis["best_move"]["to"].is_string());
    assert!(analysis["evaluation"]["score_type"].is_string());
    assert!(analysis["principal_variations"].is_array());
    assert!(analysis["depth_reached"].is_u64());
    assert!(analysis["nodes_searched"].is_u64());
    assert!(analysis["time_ms"].is_u64());
    assert!(analysis["completed_at"].is_string());
}

#[tokio::test]
async fn test_ws_bestmove_invalid_fen() {
    let server = TestServer::new().await;
    let (mut sink, mut stream) = server.ws_connect(Some(&server.token)).await;
    send_json(
        &mut sink,
        json!({
            "type": "bestmove",
            "id": "b1",
            "fen": "garbage-fen-string"
        }),
    )
    .await;
    let timeout = tokio::time::Duration::from_secs(5);
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        match tokio::time::timeout_at(deadline, stream.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                let resp: Value = serde_json::from_str(&text).expect("parse");
                if resp["type"] == "error" {
                    assert_eq!(resp["code"], 500);
                    assert!(resp["message"].as_str().unwrap().contains("invalid FEN"));
                    break;
                }
                if resp["type"] == "pong" {
                    continue;
                }
            }
            _ => panic!("did not receive error for invalid FEN bestmove"),
        }
    }
}

#[tokio::test]
async fn test_ws_auth_then_operations() {
    let server = TestServer::new().await;
    let (mut sink, mut stream) = server.ws_connect(None).await;
    send_json(
        &mut sink,
        json!({"type": "auth", "id": "1", "token": server.token}),
    )
    .await;
    let resp = recv_json(&mut stream).await;
    assert_eq!(resp["success"], true);

    send_json(&mut sink, json!({"type": "ping", "id": "p1"})).await;
    let resp = recv_json(&mut stream).await;
    assert_eq!(resp["type"], "pong");

    send_json(
        &mut sink,
        json!({"type": "subscribe", "id": "s1", "topics": ["cluster"]}),
    )
    .await;
    let resp = recv_json(&mut stream).await;
    assert_eq!(resp["type"], "subscribed");

    send_json(
        &mut sink,
        json!({
            "type": "bestmove",
            "id": "b1",
            "fen": "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "movetime": 500
        }),
    )
    .await;
    let timeout = tokio::time::Duration::from_secs(10);
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        match tokio::time::timeout_at(deadline, stream.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                let resp: Value = serde_json::from_str(&text).expect("parse");
                if resp["type"] == "bestmove_result" {
                    assert_eq!(resp["id"], "b1");
                    break;
                }
                if resp["type"] == "pong" {
                    continue;
                }
            }
            _ => panic!("did not receive bestmove result"),
        }
    }
}

#[tokio::test]
async fn test_ws_query_param_auth_invalid_token() {
    let server = TestServer::new().await;
    let url = format!("ws://{}/v1/ws?token=iff_bad_token", server.addr);
    let (ws_stream, _) = tokio_tungstenite::connect_async(&url)
        .await
        .expect("ws connect");
    use futures_util::StreamExt;
    let (_sink, mut stream) = ws_stream.split();
    let timeout = tokio::time::Duration::from_secs(10);
    let deadline = tokio::time::Instant::now() + timeout;
    let mut got_auth_error_or_disconnect = false;
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout_at(deadline, stream.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                let resp: Value = serde_json::from_str(&text).expect("parse");
                if resp["type"] == "error" && resp["code"] == 401 {
                    got_auth_error_or_disconnect = true;
                    break;
                }
            }
            Ok(Some(Ok(Message::Close(_)))) | Ok(None) | Ok(Some(Err(_))) => {
                got_auth_error_or_disconnect = true;
                break;
            }
            Err(_) => break,
            _ => {}
        }
    }
    assert!(got_auth_error_or_disconnect);
}

#[tokio::test]
async fn test_ws_analyze_with_defaults() {
    let server = TestServer::new().await;
    let (mut sink, mut stream) = server.ws_connect(Some(&server.token)).await;
    send_json(
        &mut sink,
        json!({
            "type": "analyze",
            "id": "a1",
            "fen": "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"
        }),
    )
    .await;
    let timeout = tokio::time::Duration::from_secs(10);
    let deadline = tokio::time::Instant::now() + timeout;
    let mut got_complete = false;
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout_at(deadline, stream.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                let resp: Value = serde_json::from_str(&text).expect("parse");
                if resp["type"] == "analysis_complete" {
                    got_complete = true;
                    assert_eq!(resp["id"], "a1");
                    break;
                }
            }
            _ => break,
        }
    }
    assert!(
        got_complete,
        "should receive analysis_complete with default depth/multipv"
    );
}
