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

pub async fn resolve(
    client: &TeamsClient,
    chat: &str,
    user: &str,
) -> Result<(String, ChatNameSource)> {
    let mut url = url::Url::parse(&client.chat_service_url())?;
    url.path_segments_mut()
        .map_err(|_| anyhow::anyhow!("Invalid chat URL"))?
        .extend(["v1", "threads", chat]);
    url.query_pairs_mut()
        .append_pair("view", "msnp24Equivalent");
    let thread: Thread = client.chat_get(url.as_str()).await?.json().await?;
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
    let peers: Vec<_> = members
        .iter()
        .filter(|id| !same_user(id, user))
        .cloned()
        .collect();
    anyhow::ensure!(!peers.is_empty(), "No peer roster available");
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
    // History can identify a sole peer, never the title of an unnamed group.
    if peers.len() == 1 {
        let messages = super::read_messages_data(client, chat, 50).await?;
        if let Some(message) = messages
            .iter()
            .rev()
            .find(|m| same_user(&m.sender_id, &peers[0]) && valid_name(&m.sender))
        {
            return Ok((message.sender.trim().to_owned(), ChatNameSource::LastSender));
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
}

#[cfg(test)]
mod live_tests {
    #[tokio::test]
    #[ignore = "requires existing Teams login; reads roster and profiles only"]
    async fn resolve_recent_chat_names_live() {
        let client = super::TeamsClient::new().await.unwrap();
        let user = crate::api::whoami_data(&client).await.unwrap().id;
        let chats = crate::api::list_chats_data(&client, 10).await.unwrap();
        let mut resolved = 0;
        for chat in chats
            .iter()
            .filter(|c| c.name_source != super::ChatNameSource::Topic)
        {
            if let Ok((name, _)) = super::resolve(&client, &chat.id, &user).await {
                assert!(super::valid_name(&name));
                resolved += 1;
            }
        }
        println!("Resolved {resolved} recent untitled chats without opening them");
        assert!(resolved > 0);
    }
}
