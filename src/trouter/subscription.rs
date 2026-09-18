//! Quiet, message-only subscription for the TUI. No call dispatch or stdout.

use std::time::Duration;

use anyhow::{bail, Context, Result};
use base64::Engine;
use serde_json::Value;
use tokio::{sync::mpsc, time};

use super::{registrar, session, websocket::TrouterSocket};
use crate::api::client::TeamsClient;

#[derive(Debug)]
pub enum Event {
    Connected,
    Reconnecting {
        retry_in_secs: u64,
    },
    Degraded,
    /// Empty means the envelope was unfamiliar: reconcile recent conversations.
    Refresh(Vec<String>),
}

/// Runs until its owner cancels the task or drops the receiving channel.
pub async fn run(tx: mpsc::Sender<Event>) {
    let mut backoff = 1;
    loop {
        let started = time::Instant::now();
        match connected_session(&tx).await {
            Ok(()) => return,
            Err(error) => {
                // Endpoint URLs can contain session credentials. The UI gets only
                // a static description; debug logs retain the error category.
                tracing::warn!(
                    "Live subscription interrupted: {}",
                    error
                        .to_string()
                        .split(':')
                        .next()
                        .unwrap_or("connection error")
                );
                if tx.send(Event::Degraded).await.is_err() {
                    return;
                }
            }
        }
        if started.elapsed() >= Duration::from_secs(60) {
            backoff = 1;
        }
        if tx
            .send(Event::Reconnecting {
                retry_in_secs: backoff,
            })
            .await
            .is_err()
        {
            return;
        }
        tokio::select! {
            _ = tx.closed() => return,
            _ = time::sleep(Duration::from_secs(backoff)) => {}
        }
        backoff = (backoff * 2).min(60);
    }
}

async fn connected_session(tx: &mpsc::Sender<Event>) -> Result<()> {
    let (mut ws, lifetime) = time::timeout(Duration::from_secs(60), async {
        // Reload on every reconnect, and renew AAD/Skype credentials if needed.
        let client = TeamsClient::new()
            .await
            .context("Authentication unavailable")?;
        let token = client
            .skype_token()
            .context("Messaging token unavailable")?;
        let lifetime = client.push_session_lifetime();
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()?;
        let (session, epid) = session::negotiate(&http, &token)
            .await
            .context("Negotiation failed")?;
        let sid = session::get_session_id(&http, &session, &token, &epid)
            .await
            .context("Session handshake failed")?;
        let mut ws = TrouterSocket::connect(&session, &sid, &epid)
            .await
            .context("WebSocket connection failed")?;
        let frame = ws.recv_frame().await?.context("Handshake closed")?;
        anyhow::ensure!(frame.starts_with("1::"), "Unexpected handshake");
        let registrar_url = session
            .registrar_url
            .as_deref()
            .context("Missing message registrar")?;
        registrar::register_messages(&http, &token, registrar_url, &session.surl)
            .await
            .context("Message registration failed")?;
        let lifetime = lifetime.min(Duration::from_secs(session.ttl.saturating_sub(30).max(1)));
        Ok::<_, anyhow::Error>((ws, lifetime))
    })
    .await
    .context("Subscription setup timed out")??;

    // Connected means both WebSocket and messaging registration succeeded.
    if tx.send(Event::Connected).await.is_err() {
        return Ok(());
    }
    let mut heartbeat = time::interval(Duration::from_secs(30));
    heartbeat.set_missed_tick_behavior(time::MissedTickBehavior::Delay);
    heartbeat.tick().await;
    let deadline = time::sleep(lifetime);
    tokio::pin!(deadline);
    let mut received_at = time::Instant::now();
    loop {
        tokio::select! {
            _ = tx.closed() => return Ok(()),
            _ = &mut deadline => bail!("Refreshing session credentials"),
            _ = heartbeat.tick() => {
                // Negotiated Socket.IO heartbeat is typically 180 seconds.
                anyhow::ensure!(received_at.elapsed() < Duration::from_secs(210), "Heartbeat timed out");
                time::timeout(Duration::from_secs(10), ws.send_text("2::")).await.context("Heartbeat write timed out")??;
            }
            frame = ws.recv_frame() => {
                let frame = frame?.context("WebSocket closed")?;
                received_at = time::Instant::now();
                anyhow::ensure!(!frame.starts_with("0:") && !frame.starts_with("7:"), "Server ended session");
                if let Some(ids) = invalidated_chats(&frame) {
                    if tx.send(Event::Refresh(ids)).await.is_err() {
                        return Ok(());
                    }
                }
            }
        }
    }
}

/// Decode Socket.IO 3/5 frames, including nested JSON/base64 bodies.
/// We fetch authoritative messages after invalidation instead of trusting the
/// many different push message representations as complete message history.
fn invalidated_chats(frame: &str) -> Option<Vec<String>> {
    let kind = frame.split(':').next()?;
    if kind != "3" && kind != "5" {
        return None;
    }
    let payload = frame.splitn(4, ':').nth(3).unwrap_or("");
    if payload.contains("NGCallManagerWin") || payload.contains("NextGenCalling") {
        return None;
    }
    let mut ids = Vec::new();
    if let Ok(value) = serde_json::from_str::<Value>(payload) {
        collect_chat_ids(&value, &mut ids, 0);
    }
    ids.sort();
    ids.dedup();
    // Unknown data still triggers bounded fallback; never silently ignore it.
    Some(ids)
}

fn collect_chat_ids(value: &Value, ids: &mut Vec<String>, depth: usize) {
    if depth > 12 || ids.len() >= 64 {
        return;
    }
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                let key = key.to_ascii_lowercase();
                if matches!(
                    key.as_str(),
                    "conversationid" | "chatid" | "threadid" | "conversationlink"
                ) {
                    if let Some(id) = value.as_str().and_then(chat_id) {
                        ids.push(id);
                    }
                }
                collect_chat_ids(value, ids, depth + 1);
            }
        }
        Value::Array(values) => {
            for value in values.iter().take(128) {
                collect_chat_ids(value, ids, depth + 1);
            }
        }
        Value::String(text) if text.len() < 512 * 1024 => {
            if let Ok(value) = serde_json::from_str::<Value>(text) {
                collect_chat_ids(&value, ids, depth + 1);
            } else if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(text) {
                if let Ok(value) = serde_json::from_slice::<Value>(&decoded) {
                    collect_chat_ids(&value, ids, depth + 1);
                }
            }
        }
        _ => {}
    }
}

fn chat_id(text: &str) -> Option<String> {
    let id = text
        .split("/conversations/")
        .nth(1)
        .unwrap_or(text)
        .split('/')
        .next()?;
    // Percent escapes occur in conversationLink URLs.
    let encoded = format!("id={}", id.replace('+', "%2B"));
    let id = url::form_urlencoded::parse(encoded.as_bytes())
        .next()?
        .1
        .into_owned();
    if id.starts_with("19:") || id.starts_with("48:") {
        Some(id)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalidates_http_and_socketio_envelopes() {
        let frame = r#"3:::{"id":12,"body":"{\"resource\":{\"conversationLink\":\"https://chat/v1/users/ME/conversations/19%3Achat%40thread.v2/messages\"}}"}"#;
        assert_eq!(
            invalidated_chats(frame),
            Some(vec!["19:chat@thread.v2".into()])
        );
        let frame = r#"5:77:: {"args":[{"threadId":"19:group@thread.v2"}]}"#;
        assert_eq!(
            invalidated_chats(frame),
            Some(vec!["19:group@thread.v2".into()])
        );
    }

    #[test]
    fn base64_envelopes_and_unknown_delivery_trigger_refresh() {
        let body =
            base64::engine::general_purpose::STANDARD.encode(r#"{"conversationId":"19:chat"}"#);
        assert_eq!(
            invalidated_chats(&format!("3:::{{\"body\":\"{body}\"}}")),
            Some(vec!["19:chat".into()])
        );
        assert_eq!(invalidated_chats("3:::unrecognized"), Some(vec![]));
        assert_eq!(invalidated_chats("5:::{}"), Some(vec![]));
    }

    #[test]
    fn ignores_protocol_frames_and_calls() {
        assert_eq!(invalidated_chats("2::"), None);
        assert_eq!(invalidated_chats("6:1::"), None);
        assert_eq!(
            invalidated_chats("5:::{\"name\":\"NGCallManagerWin\"}"),
            None
        );
    }
}
