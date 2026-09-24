//! Native roster and directory lookup; no Graph directory consent required.
use super::{client::TeamsClient, ChatNameSource};
use anyhow::{Context, Result};
use serde::Deserialize;

pub const UNKNOWN: &str = "Unknown conversation";

pub fn same_user(left: &str, right: &str) -> bool {
    fn identity(value: &str) -> &str {
        let tail = value.rsplit('/').next().unwrap_or(value);
        tail.strip_prefix("8:orgid:")
            .or_else(|| tail.strip_prefix("orgid:"))
            .unwrap_or(tail)
    }
    !left.is_empty() && !right.is_empty() && identity(left).eq_ignore_ascii_case(identity(right))
}

pub fn valid_name(name: &str) -> bool {
    let name = name.trim();
    !name.is_empty()
        && !matches!(name, "?" | "[unknown]" | UNKNOWN)
        && !name.starts_with("8:")
        && !name.starts_with("19:")
        && !name.starts_with("orgid:")
        && !name.contains("://")
        && uuid::Uuid::parse_str(name).is_err()
}

#[derive(Deserialize)]
struct Thread {
    #[serde(default)]
    properties: serde_json::Value,
    #[serde(default)]
    members: Vec<Member>,
}
#[derive(Deserialize)]
struct Member {
    id: String,
}
#[derive(Deserialize)]
struct Profiles {
    value: Vec<Profile>,
}
#[derive(Deserialize)]
struct Profile {
    mri: String,
    #[serde(rename = "displayName")]
    display_name: Option<String>,
}

fn participant_label(members: &[String], profiles: &[Profile], user: &str) -> Option<String> {
    let peers: Vec<_> = members.iter().filter(|id| !same_user(id, user)).collect();
    if peers.is_empty() {
        return None;
    }
    // A partial group roster would look like a different conversation. Keep the
    // cached label until every participant can be named.
    let names: Option<Vec<_>> = peers
        .iter()
        .map(|id| {
            profiles
                .iter()
                .find(|p| same_user(&p.mri, id))
                .and_then(|p| p.display_name.as_deref())
                .filter(|name| valid_name(name))
                .map(str::trim)
        })
        .collect();
    names.map(|names| names.join(", "))
}

/// A native direct-chat ID contains both AAD object IDs. Only trust it when
/// both parts are UUIDs and one matches the signed-in user.
fn direct_chat_peer(chat: &str, user: &str) -> Option<String> {
    let pair = chat.strip_prefix("19:")?.strip_suffix("@unq.gbl.spaces")?;
    let (left, right) = pair.split_once('_')?;
    uuid::Uuid::parse_str(left).ok()?;
    uuid::Uuid::parse_str(right).ok()?;
    let peer = if same_user(left, user) {
        right
    } else if same_user(right, user) {
        left
    } else {
        return None;
    };
    if same_user(peer, user) {
        return None;
    }
    Some(format!("8:orgid:{peer}"))
}

fn history_label(
    messages: &[super::MessageInfo],
    peers: &[String],
    user: &str,
    is_group: bool,
) -> Option<String> {
    if is_group || peers.len() > 1 {
        return None;
    }
    let candidates: Vec<_> = messages
        .iter()
        .filter(|message| {
            !message.sender_id.is_empty()
                && !same_user(&message.sender_id, user)
                && peers
                    .first()
                    .is_none_or(|peer| same_user(&message.sender_id, peer))
        })
        .collect();
    // Without roster evidence, require a consistent peer identity throughout
    // the loaded direct-chat history instead of picking an arbitrary sender.
    let peer = candidates.first()?;
    if candidates
        .iter()
        .any(|message| !same_user(&message.sender_id, &peer.sender_id))
    {
        return None;
    }
    candidates
        .iter()
        .rev()
        .find(|message| valid_name(&message.sender))
        .map(|message| message.sender.trim().to_owned())
}

pub async fn resolve(
    client: &TeamsClient,
    chat: &str,
    user: &str,
    is_group: bool,
) -> Result<(String, ChatNameSource)> {
    let mut url = url::Url::parse(&client.chat_service_url())?;
    url.path_segments_mut()
        .map_err(|_| anyhow::anyhow!("Invalid chat URL"))?
        .extend(["v1", "threads", chat]);
    url.query_pairs_mut()
        .append_pair("view", "msnp24Equivalent");
    let roster = async {
        client
            .chat_get(url.as_str())
            .await?
            .json::<Thread>()
            .await
            .map_err(anyhow::Error::from)
    }
    .await;
    let thread = match roster {
        Ok(thread) => thread,
        Err(error) if !is_group => {
            tracing::debug!("Direct-chat roster unavailable; trying identity/history: {error}");
            Thread {
                properties: Default::default(),
                members: Vec::new(),
            }
        }
        Err(error) => return Err(error),
    };
    if let Some(topic) = thread
        .properties
        .get("topic")
        .and_then(|v| v.as_str())
        .filter(|s| valid_name(s))
    {
        return Ok((topic.trim().to_owned(), ChatNameSource::Topic));
    }
    let mut members: Vec<_> = thread.members.into_iter().map(|m| m.id).collect();
    members.sort();
    members.dedup();
    if !is_group {
        if let Some(peer) = direct_chat_peer(chat, user) {
            if !members.iter().any(|member| same_user(member, &peer)) {
                members.push(peer);
            }
        }
    }
    let peers: Vec<_> = members
        .iter()
        .filter(|id| !same_user(id, user))
        .cloned()
        .collect();
    // Chunk large groups to keep native profile requests bounded.
    let mut profiles = Vec::new();
    for chunk in peers.chunks(50) {
        match client
            .fetch_profiles(chunk)
            .await
            .and_then(|value| Ok(serde_json::from_value::<Profiles>(value)?))
        {
            Ok(response) => profiles.extend(response.value),
            Err(error) => {
                tracing::debug!("Participant directory lookup unavailable: {error}");
                break;
            }
        }
    }
    if let Some(name) = participant_label(&members, &profiles, user) {
        return Ok((name, ChatNameSource::Participants));
    }
    // Even an empty/self-only roster can recover a direct chat from history.
    if !is_group {
        let messages = super::read_messages_data(client, chat, 50).await?;
        if let Some(name) = history_label(&messages, &peers, user, is_group) {
            return Ok((name, ChatNameSource::LastSender));
        }
    }
    None.context("Participant names unavailable")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn profile(id: &str, name: &str) -> Profile {
        Profile {
            mri: id.into(),
            display_name: Some(name.into()),
        }
    }
    #[test]
    fn roster_labels_exclude_self_and_require_complete_groups() {
        let ids = vec!["8:orgid:me".into(), "8:orgid:a".into(), "8:orgid:b".into()];
        let profiles = vec![
            profile("8:orgid:b", "Bob"),
            profile("8:orgid:me", "Me"),
            profile("8:orgid:a", "Alice"),
        ];
        assert_eq!(
            participant_label(&ids, &profiles, "ME").as_deref(),
            Some("Alice, Bob")
        );
        assert!(participant_label(&ids, &profiles[1..], "me").is_none());
        assert_eq!(
            participant_label(&ids[..2], &profiles, "me").as_deref(),
            Some("Alice")
        );
    }
    #[test]
    fn identifiers_and_placeholders_are_not_names() {
        for name in [
            "",
            "?",
            UNKNOWN,
            "19:thread@thread.v2",
            "8:orgid:abc",
            "00000000-0000-0000-0000-000000000001",
        ] {
            assert!(!valid_name(name));
        }
        assert!(valid_name(" Alice "));
    }

    #[test]
    fn incomplete_roster_peer_is_validated_against_the_account() {
        let me = "00000000-0000-0000-0000-000000000001";
        let peer = "00000000-0000-0000-0000-000000000002";
        for pair in [format!("{me}_{peer}"), format!("{peer}_{me}")] {
            assert_eq!(
                direct_chat_peer(
                    &format!("19:{pair}@unq.gbl.spaces"),
                    &format!("8:orgid:{me}")
                ),
                Some(format!("8:orgid:{peer}"))
            );
        }
        assert!(direct_chat_peer(&format!("19:{me}_{peer}@unq.gbl.spaces"), "unrelated").is_none());
        assert!(direct_chat_peer(&format!("19:{me}_malformed@unq.gbl.spaces"), me).is_none());
        assert!(direct_chat_peer(&format!("19:{me}_{peer}@thread.v2"), me).is_none());
        assert!(direct_chat_peer(&format!("19:{me}_{me}@unq.gbl.spaces"), me).is_none());
    }

    #[test]
    fn self_only_direct_roster_can_use_history_but_groups_and_ambiguous_history_cannot() {
        let message = |id: &str, name: &str| super::super::MessageInfo {
            id: "1".into(),
            sender_id: id.into(),
            sender: name.into(),
            timestamp: String::new(),
            content: String::new(),
            mentions: Vec::new(),
        };
        let messages = vec![
            message("8:orgid:alice", "Alice"),
            message("8:orgid:me", "Me"),
        ];
        assert_eq!(
            history_label(&messages, &[], "me", false).as_deref(),
            Some("Alice")
        );
        assert_eq!(
            history_label(&messages, &["alice".into()], "me", false).as_deref(),
            Some("Alice")
        );
        assert!(history_label(&messages, &["bob".into()], "me", false).is_none());
        assert!(history_label(&messages, &[], "me", true).is_none());
        assert!(history_label(&messages[1..], &[], "me", false).is_none());
        let mut ambiguous = messages;
        ambiguous.push(message("bob", "Bob"));
        assert!(history_label(&ambiguous, &[], "me", false).is_none());
    }
}

#[cfg(test)]
mod live_tests {
    #[tokio::test]
    #[ignore = "requires existing Teams login; reads roster and profiles only"]
    async fn resolve_recent_chat_names_live() {
        let client = super::TeamsClient::new().await.unwrap();
        let user = crate::api::whoami_data(&client).await.unwrap().id;
        let recent = crate::api::list_recent_data(&client, 50).await.unwrap();
        let chats = recent.chats;
        assert!(chats
            .iter()
            .all(|chat| !crate::api::is_activity_stream(&chat.id)
                && !chat.id.ends_with("@thread.tacv2")));
        let teams = crate::api::list_teams_data(&client).await.unwrap();
        for channel in &recent.channels {
            assert!(
                teams
                    .iter()
                    .flat_map(|team| &team.channels)
                    .any(|item| item.id == channel.id),
                "Recent channel absent from Teams hierarchy"
            );
        }
        let mut resolved = 0;
        let mut unresolved = 0;
        for chat in chats
            .iter()
            .filter(|c| c.name_source != super::ChatNameSource::Topic)
        {
            if let Ok((name, _)) = super::resolve(&client, &chat.id, &user, chat.is_group).await {
                assert!(super::valid_name(&name));
                resolved += 1;
            } else {
                unresolved += 1;
            }
        }
        println!("{} real chats; {} channel summaries retained under {} teams; {resolved} untitled chats resolved; {unresolved} unresolved", chats.len(), recent.channels.len(), teams.len());
        assert!(resolved > 0);
    }
}
