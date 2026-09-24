//! Native Teams chat API (chatsvcagg / chat service)
//!
//! Uses the Skype token with `Authentication: skypetoken={token}` header,
//! bypassing Graph API which requires tenant admin consent for Chat.Read.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::client::TeamsClient;

// -- Response types for the native chat API --

#[derive(Debug, Deserialize)]
struct ConversationsResponse {
    conversations: Option<Vec<Conversation>>,
}

#[derive(Debug, Deserialize)]
pub(super) struct Conversation {
    pub(super) id: Option<String>,
    #[serde(rename = "threadProperties")]
    pub(super) thread_properties: Option<ThreadProperties>,
    #[serde(rename = "lastMessage")]
    last_message: Option<NativeMessage>,
    properties: Option<ConversationProperties>,
}

#[derive(Debug, Deserialize)]
struct ConversationProperties {
    /// Native read watermark: message ID; timestamp; service flags.
    consumptionhorizon: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ThreadProperties {
    topic: Option<String>,
    #[serde(rename = "productThreadType")]
    pub(super) product_thread_type: Option<String>,
    #[serde(flatten)]
    pub(super) extra: serde_json::Map<String, serde_json::Value>,
    #[serde(rename = "lastjoinat")]
    last_join_at: Option<String>,
    /// For 1:1 chats, contains member MRIs
    members: Option<String>,
}

#[derive(Debug, Deserialize)]
struct NativeMessage {
    id: Option<String>,
    #[serde(rename = "composetime")]
    compose_time: Option<String>,
    #[serde(rename = "originalarrivaltime")]
    original_arrival_time: Option<String>,
    #[serde(rename = "imdisplayname")]
    im_display_name: Option<String>,
    content: Option<String>,
    messagetype: Option<String>,
    from: Option<String>,
    properties: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct MessagesResponse {
    messages: Option<Vec<NativeMessage>>,
}

/// Strip HTML tags from content for CLI display.
fn strip_html(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }
    // Decode common HTML entities
    result
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

/// Display name for a conversation.
fn conversation_name(conv: &Conversation) -> (String, ChatNameSource) {
    if let Some(ref props) = conv.thread_properties {
        if let Some(ref topic) = props.topic {
            if !topic.trim().is_empty() {
                return (topic.trim().to_owned(), ChatNameSource::Topic);
            }
        }
    }
    // Fall back to last message sender or the thread ID
    if let Some(ref msg) = conv.last_message {
        if let Some(ref name) = msg.im_display_name {
            if !name.trim().is_empty() {
                return (name.trim().to_owned(), ChatNameSource::LastSender);
            }
        }
    }
    (
        conv.id.as_deref().unwrap_or("[unknown]").to_string(),
        ChatNameSource::Identifier,
    )
}

/// List recent chats using the native Teams API (prints to stdout).
pub async fn list_chats(limit: usize) -> Result<()> {
    let client = TeamsClient::new().await?;
    let chats = list_chats_data(&client, limit).await?;

    println!("\nRecent Chats:");
    println!("{:-<60}", "");

    if chats.is_empty() {
        println!("  (no chats found)");
        return Ok(());
    }

    for chat in &chats {
        println!("{}", chat.name);
        println!("  ID: {}", chat.id);

        if let Some(ref time) = chat.last_message_time {
            println!("  Last: {}", time);
        }
        if let Some(ref preview) = chat.last_message_preview {
            if !preview.trim().is_empty() {
                let sender = chat.last_message_sender.as_deref().unwrap_or("?");
                println!("  [{}]: {}", sender, preview.trim());
            }
        }

        println!();
    }

    Ok(())
}

/// Read messages from a specific chat thread (prints to stdout).
pub async fn read_messages(chat_id: &str, limit: usize) -> Result<()> {
    let client = TeamsClient::new().await?;
    let msgs = read_messages_data(&client, chat_id, limit).await?;

    if msgs.is_empty() {
        println!("(no messages)");
        return Ok(());
    }

    for msg in &msgs {
        println!("[{}] {}: {}", msg.timestamp, msg.sender, msg.content);
    }

    Ok(())
}

/// Send a message to a chat thread using the native API.
pub async fn send_message(chat_id: &str, message: &str) -> Result<()> {
    let client = TeamsClient::new().await?;
    send_message_with_client(&client, chat_id, message).await?;
    println!("Message sent.");
    Ok(())
}

/// HTML-escape text for embedding in Teams RichText/Html messages.
fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Send a message using an existing client (shared helper).
pub async fn send_message_with_client(
    client: &TeamsClient,
    chat_id: &str,
    message: &str,
) -> Result<()> {
    let base = client.chat_service_url();
    let url = format!("{}/v1/users/ME/conversations/{}/messages", base, chat_id);

    let escaped = html_escape(message);
    let body = serde_json::json!({
        "content": format!("<p>{}</p>", escaped),
        "messagetype": "RichText/Html",
        "contenttype": "text"
    });

    tracing::debug!("Sending message to {}", url);
    client.chat_post(&url, &body).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Data-returning API functions for TUI integration
// ---------------------------------------------------------------------------

/// A message sender is only a naming hint, never a conversation rename.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChatNameSource {
    Topic,
    Participants,
    LastSender,
    Identifier,
}

/// Chat metadata for TUI display.
#[allow(dead_code)]
pub struct ChatInfo {
    pub id: String,
    pub name: String,
    pub name_source: ChatNameSource,
    pub is_group: bool,
    pub last_message_id: Option<String>,
    pub last_message_sender_id: Option<String>,
    pub last_message_type: Option<String>,
    /// None when the native API supplies only a read watermark.
    pub unread_count: Option<u32>,
    pub has_unread: Option<bool>,
    pub last_message_time: Option<String>,
    pub last_message_sender: Option<String>,
    pub last_message_preview: Option<String>,
}

/// A single message for TUI display.
#[derive(Clone, Debug)]
pub struct MessageInfo {
    pub id: String,
    pub sender_id: String,
    pub sender: String,
    pub timestamp: String,
    pub content: String,
    pub mentions: Vec<String>,
}

/// The native consumption horizon identifies the newest consumed message. It
/// establishes only some unread activity, never a count of historical messages.
fn native_has_unread(conversation: &Conversation) -> Option<bool> {
    let horizon = conversation
        .properties
        .as_ref()?
        .consumptionhorizon
        .as_deref()?;
    let read_id = horizon.split(';').next()?.trim().parse::<u64>().ok()?;
    let message_id = conversation
        .last_message
        .as_ref()?
        .id
        .as_deref()?
        .parse::<u64>()
        .ok()?;
    Some(message_id > read_id)
}

/// Internal account feeds are not conversations, even when they contain text.
pub(crate) fn is_activity_stream(id: &str) -> bool {
    id.rsplit('/').next().unwrap_or(id).starts_with("48:")
}

/// Only actual chats belong in Chats. Older responses may omit product type.
fn chat_is_group(id: &str, product_type: Option<&str>) -> Option<bool> {
    if is_activity_stream(id) || id.ends_with("@thread.tacv2") {
        return None;
    }
    match product_type {
        Some("OneToOneChat") => Some(false),
        Some("Chat" | "Meeting") => Some(true),
        None if id.ends_with("@unq.gbl.spaces") => Some(false),
        None if id.ends_with("@thread.v2") || id.ends_with("@thread.skype") => Some(true),
        _ => None,
    }
}

/// List recent chats and return structured data.
pub(super) async fn list_conversations(
    client: &TeamsClient,
    limit: usize,
) -> Result<Vec<Conversation>> {
    // Strategy 1: CSA AFD endpoint with Bearer auth
    let csa_url = format!(
        "https://teams.microsoft.com/api/csa/api/v1/teams/users/ME/conversations?view=mychats&pageSize={}",
        limit
    );
    tracing::debug!("Trying CSA endpoint: {}", csa_url);
    let resp = match client.csa_get(&csa_url).await {
        Ok(r) => r,
        Err(e) => {
            tracing::debug!("CSA endpoint failed: {:#}, trying chatsvcagg", e);
            // Strategy 2: chatsvcagg with skypetoken auth
            let base = client.chatsvcagg_url();
            let url = format!(
                "{}/api/v2/users/ME/conversations?view=mychats&pageSize={}",
                base, limit
            );
            tracing::debug!("Trying chatsvcagg: {}", url);
            match client.chat_get(&url).await {
                Ok(r) => r,
                Err(e2) => {
                    tracing::debug!("chatsvcagg failed: {:#}, trying chat service", e2);
                    // Strategy 3: chat service (amer.ng.msg) with skypetoken auth
                    let base = client.chat_service_url();
                    let url = format!(
                        "{}/v1/users/ME/conversations?view=mychats&pageSize={}",
                        base, limit
                    );
                    client.chat_get(&url).await?
                }
            }
        }
    };

    let body: ConversationsResponse = resp
        .json()
        .await
        .context("Failed to parse conversations response")?;

    Ok(body.conversations.unwrap_or_default())
}

pub async fn list_chats_data(client: &TeamsClient, limit: usize) -> Result<Vec<ChatInfo>> {
    Ok(list_recent_data(client, limit).await?.chats)
}

pub(crate) struct RecentConversations {
    pub chats: Vec<ChatInfo>,
    pub channels: Vec<ChatInfo>,
}

pub(crate) async fn list_recent_data(
    client: &TeamsClient,
    limit: usize,
) -> Result<RecentConversations> {
    let conversations = list_conversations(client, limit).await?;
    Ok(RecentConversations {
        chats: conversation_infos(&conversations, false),
        channels: conversation_infos(&conversations, true),
    })
}

fn conversation_infos(conversations: &[Conversation], channels: bool) -> Vec<ChatInfo> {
    let mut chats = Vec::new();
    for conv in conversations {
        let id = conv.id.as_deref().unwrap_or("").to_string();
        if id.is_empty() {
            continue;
        }

        let props = conv.thread_properties.as_ref();
        let product_type = props.and_then(|p| p.product_thread_type.as_deref());
        let (is_group, name, name_source) = if channels {
            if !matches!(
                product_type,
                Some("TeamsTeam" | "TeamsChannel" | "TeamsPrivateChannel" | "TeamsSharedChannel")
            ) {
                continue;
            }
            let name = props
                .and_then(|p| {
                    p.extra
                        .get("topicThreadTopic")
                        .or_else(|| p.extra.get("spaceThreadTopic"))
                })
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or("Unnamed channel");
            (true, name.to_owned(), ChatNameSource::Topic)
        } else {
            let Some(is_group) = chat_is_group(&id, product_type) else {
                continue;
            };
            let (name, source) = conversation_name(conv);
            (is_group, name, source)
        };

        let (last_time, last_sender, last_preview) = if let Some(ref msg) = conv.last_message {
            let time = msg
                .original_arrival_time
                .as_deref()
                .or(msg.compose_time.as_deref())
                .map(String::from);
            let sender = msg.im_display_name.clone();
            let preview = msg.content.as_deref().map(|c| {
                let text = strip_html(c);
                if text.len() > 80 {
                    let end = text
                        .char_indices()
                        .map(|(i, _)| i)
                        .take_while(|&i| i <= 77)
                        .last()
                        .unwrap_or(0);
                    format!("{}...", &text[..end])
                } else {
                    text
                }
            });
            (time, sender, preview)
        } else {
            (None, None, None)
        };

        chats.push(ChatInfo {
            id,
            name,
            name_source,
            is_group,
            last_message_id: conv.last_message.as_ref().and_then(|m| m.id.clone()),
            last_message_sender_id: conv.last_message.as_ref().and_then(|m| m.from.clone()),
            last_message_type: conv
                .last_message
                .as_ref()
                .and_then(|m| m.messagetype.clone()),
            unread_count: None,
            has_unread: native_has_unread(conv),
            last_message_time: last_time,
            last_message_sender: last_sender,
            last_message_preview: last_preview,
        });
    }

    chats
}

/// Read messages from a specific chat thread and return structured data.
pub async fn read_messages_data(
    client: &TeamsClient,
    chat_id: &str,
    limit: usize,
) -> Result<Vec<MessageInfo>> {
    let base = client.chat_service_url();
    let url = format!(
        "{}/v1/users/ME/conversations/{}/messages?pageSize={}",
        base, chat_id, limit
    );

    tracing::debug!("Reading messages from {}", url);
    let resp = client.chat_get(&url).await?;
    let body: MessagesResponse = resp
        .json()
        .await
        .context("Failed to parse messages response")?;

    let messages = body.messages.unwrap_or_default();

    // Messages come newest-first; reverse for chronological display
    let mut msgs: Vec<&NativeMessage> = messages.iter().collect();
    msgs.reverse();

    let mut result = Vec::new();
    for msg in &msgs {
        let msgtype = msg.messagetype.as_deref().unwrap_or("");
        // Skip non-text messages (e.g. ThreadActivity/*)
        if !msgtype.contains("Text") && !msgtype.contains("RichText") {
            continue;
        }

        let sender = msg.im_display_name.as_deref().unwrap_or("?").to_string();
        let time = msg
            .original_arrival_time
            .as_deref()
            .or(msg.compose_time.as_deref())
            .unwrap_or("")
            .to_string();
        let content = msg.content.as_deref().unwrap_or("");
        let text = strip_html(content);

        if text.trim().is_empty() {
            continue;
        }

        result.push(MessageInfo {
            id: msg.id.clone().unwrap_or_default(),
            sender_id: msg.from.clone().unwrap_or_default(),
            sender,
            timestamp: time,
            content: text.trim().to_string(),
            mentions: explicit_mentions(msg.properties.as_ref()),
        });
    }

    // Never rely on page order or repeat duplicate service IDs in the view.
    result.sort_by(|left, right| {
        message_sort_key(&left.timestamp, &left.id)
            .cmp(&message_sort_key(&right.timestamp, &right.id))
    });
    let mut seen = std::collections::HashSet::new();
    result.retain(|message| message.id.is_empty() || seen.insert(message.id.clone()));
    Ok(result)
}

/// Chronological key with a numeric ID tie-breaker (service IDs are milliseconds).
pub(crate) fn message_sort_key(timestamp: &str, id: &str) -> (i64, u64, String) {
    let time = chrono::DateTime::parse_from_rfc3339(timestamp)
        .ok()
        .map(|time| time.timestamp_millis())
        .or_else(|| id.parse().ok())
        .unwrap_or(0);
    (time, id.parse().unwrap_or(0), id.to_string())
}

/// Native Teams encodes properties.mentions as either an array or a JSON string.
/// Numeric itemid/<at id> values are local indices, never user identities.
fn explicit_mentions(properties: Option<&serde_json::Value>) -> Vec<String> {
    let Some(value) = properties.and_then(|value| value.get("mentions")) else {
        return Vec::new();
    };
    let decoded;
    let value = if let Some(text) = value.as_str() {
        decoded = serde_json::from_str::<serde_json::Value>(text).unwrap_or_default();
        &decoded
    } else {
        value
    };
    let mut mentions: Vec<_> = value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|mention| mention.get("mri").and_then(serde_json::Value::as_str))
        .filter(|mri| !mri.is_empty())
        .map(String::from)
        .collect();
    mentions.sort();
    mentions.dedup();
    mentions
}

#[cfg(test)]
mod live_message_tests {
    use super::*;
    #[test]
    fn conversation_names_distinguish_titles_from_sender_hints_and_missing_metadata() {
        let conversation = |value| serde_json::from_value::<Conversation>(value).unwrap();
        assert_eq!(
            conversation_name(&conversation(serde_json::json!({
                "id":"thread", "threadProperties":{"topic":" Project room "},
                "lastMessage":{"imdisplayname":"Someone"}
            }))),
            ("Project room".into(), ChatNameSource::Topic)
        );
        assert_eq!(
            conversation_name(&conversation(serde_json::json!({
                "id":"thread", "threadProperties":{"topic":" "},
                "lastMessage":{"imdisplayname":" Colleague "}
            }))),
            ("Colleague".into(), ChatNameSource::LastSender)
        );
        for last_message in [
            serde_json::json!({}),
            serde_json::json!({"imdisplayname":" "}),
        ] {
            assert_eq!(
                conversation_name(&conversation(serde_json::json!({
                    "id":"thread", "lastMessage":last_message
                }))),
                ("thread".into(), ChatNameSource::Identifier)
            );
        }
    }

    #[test]
    fn native_mentions_use_identity_not_index() {
        let props = serde_json::json!({"mentions": "[{\"mri\":\"8:orgid:other\",\"itemid\":\"0\"},{\"itemid\":\"1\"}]"});
        assert_eq!(explicit_mentions(Some(&props)), vec!["8:orgid:other"]);
        let props = serde_json::json!({"mentions":[{"mri":"8:orgid:other"}]});
        assert_eq!(explicit_mentions(Some(&props)), vec!["8:orgid:other"]);
        assert!(explicit_mentions(None).is_empty());
    }
    #[test]
    fn numeric_message_ids_break_timestamp_ties() {
        assert!(
            message_sort_key("2026-01-01T00:00:00Z", "9")
                < message_sort_key("2026-01-01T00:00:00Z", "10")
        );
    }
}

#[cfg(test)]
mod unread_metadata_tests {
    use super::*;

    #[test]
    fn native_consumption_horizon_distinguishes_unread_without_counting_history() {
        let conversation = |horizon: &str, message: &str| {
            serde_json::from_value::<Conversation>(serde_json::json!({
                "id": "chat", "properties": { "consumptionhorizon": horizon },
                "lastMessage": { "id": message }
            }))
            .unwrap()
        };
        assert_eq!(
            native_has_unread(&conversation(
                "1726653600000;1726653600000; 1",
                "1726653600001"
            )),
            Some(true)
        );
        assert_eq!(
            native_has_unread(&conversation(
                "1726653600001;1726653600000;0",
                "1726653600001"
            )),
            Some(false)
        );
        assert_eq!(
            native_has_unread(&conversation("invalid", "1726653600001")),
            None
        );
        assert_eq!(
            native_has_unread(&conversation("1726653600000;0;0", "opaque")),
            None
        );
        let missing: Conversation =
            serde_json::from_value(serde_json::json!({"id":"chat"})).unwrap();
        assert_eq!(native_has_unread(&missing), None);
    }
}

#[cfg(test)]
mod conversation_type_tests {
    use super::*;

    #[test]
    fn activity_feeds_and_channels_never_become_chats() {
        let fixtures = [
            ("48:notifications", "StreamOfNotifications"),
            ("48:mentions", "StreamOfMentions"),
            ("48:threads", "StreamOfThreads"),
            ("48:notes", "StreamOfNotes"),
            ("19:team@thread.tacv2", "TeamsTeam"),
            ("19:private@thread.tacv2", "TeamsPrivateChannel"),
            ("19:shared@thread.tacv2", "TeamsSharedChannel"),
            ("19:direct@unq.gbl.spaces", "OneToOneChat"),
            ("19:group@thread.v2", "Chat"),
            ("19:meeting@thread.v2", "Meeting"),
        ];
        let conversations: Vec<Conversation> = fixtures
            .iter()
            .map(|(id, kind)| {
                serde_json::from_value(serde_json::json!({
                "id": id,
                "threadProperties": {"productThreadType": kind, "spaceThreadTopic": "Native title"},
                "lastMessage": {"imdisplayname": "Sender", "id": "42"}
            })).unwrap()
            })
            .collect();
        let chats = conversation_infos(&conversations, false);
        assert_eq!(
            chats.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
            vec![fixtures[7].0, fixtures[8].0, fixtures[9].0]
        );
        assert!(!chats[0].is_group);
        assert!(chats[1].is_group && chats[2].is_group);
        let channels = conversation_infos(&conversations, true);
        assert_eq!(channels.len(), 3);
        assert!(channels
            .iter()
            .all(|c| c.name == "Native title" && c.last_message_id.as_deref() == Some("42")));
        assert_eq!(chat_is_group("48:notifications", Some("Chat")), None);
        assert_eq!(chat_is_group("19:private@thread.tacv2", None), None);
        assert_eq!(chat_is_group("19:direct@unq.gbl.spaces", None), Some(false));
        assert_eq!(chat_is_group("19:group@thread.v2", None), Some(true));
        assert_eq!(
            chat_is_group("19:unknown@thread.v2", Some("FutureSystemType")),
            None
        );
    }
}
