//! Microsoft Graph API: joined teams and channels

use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

use super::chat::{list_conversations, Conversation};

use super::client::TeamsClient;

#[derive(Debug, Deserialize)]
struct TeamsResponse {
    value: Vec<Team>,
}

#[derive(Debug, Deserialize)]
struct Team {
    id: String,
    #[serde(rename = "displayName")]
    display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChannelsResponse {
    value: Vec<Channel>,
}

#[derive(Debug, Deserialize)]
struct Channel {
    id: String,
    #[serde(rename = "displayName")]
    display_name: Option<String>,
}

/// List joined teams and channels (prints to stdout).
pub async fn list_teams() -> Result<()> {
    let client = TeamsClient::new().await?;
    let teams = list_teams_data(&client).await?;

    println!("\nTeams and Channels:");
    println!("{:-<60}", "");

    if teams.is_empty() {
        println!("  (no teams found)");
        return Ok(());
    }

    for team in &teams {
        println!("Team: {} ({} channels)", team.name, team.channels.len());
        for ch in &team.channels {
            println!("  {:<30} {}", ch.name, ch.id);
        }
        println!();
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Data-returning API functions for TUI integration
// ---------------------------------------------------------------------------

/// Team metadata for TUI display.
pub struct TeamInfo {
    pub id: String,
    pub name: String,
    pub channels: Vec<ChannelInfo>,
}

/// Channel metadata for TUI display.
pub struct ChannelInfo {
    pub id: String,
    pub name: String,
}

/// List joined teams with their channels and return structured data.
async fn graph_teams_data(client: &TeamsClient) -> Result<Vec<TeamInfo>> {
    tracing::debug!("Fetching joined teams...");
    let resp = client.graph_get("/me/joinedTeams").await?;
    let teams: TeamsResponse = resp
        .json()
        .await
        .context("Failed to parse joinedTeams response")?;

    let mut result = Vec::new();

    for team in &teams.value {
        let team_name = team.display_name.as_deref().unwrap_or(&team.id).to_string();
        tracing::debug!("Fetching channels for team: {} ({})", team_name, team.id);

        let path = format!("/teams/{}/channels", team.id);
        let channels = match client.graph_get(&path).await {
            Ok(resp) => {
                let channels_resp: ChannelsResponse = resp
                    .json()
                    .await
                    .context("Failed to parse channels response")?;
                channels_resp
                    .value
                    .into_iter()
                    .map(|ch| ChannelInfo {
                        name: ch.display_name.unwrap_or_else(|| ch.id.clone()),
                        id: ch.id,
                    })
                    .collect()
            }
            Err(e) => {
                tracing::warn!("Failed to fetch channels for {}: {:#}", team_name, e);
                Vec::new()
            }
        };

        result.push(TeamInfo {
            id: team.id.clone(),
            name: team_name,
            channels,
        });
    }

    Ok(result)
}

/// Supplement Graph's channel list with native recent-channel metadata. Some
/// private channels are absent from Graph's /channels response for this account.
pub async fn list_teams_data(client: &TeamsClient) -> Result<Vec<TeamInfo>> {
    let graph = graph_teams_data(client).await;
    let conversations = match list_conversations(client, 50).await {
        Ok(conversations) => conversations,
        Err(error) => {
            tracing::debug!("Native Teams hierarchy unavailable: {error}");
            return graph;
        }
    };
    let mut teams = match graph {
        Ok(teams) => teams,
        Err(error) => {
            tracing::debug!("Graph hierarchy unavailable; using native metadata: {error}");
            Vec::new()
        }
    };
    let mut parents = HashMap::new();
    let mut attempted = HashSet::new();
    for conversation in &conversations {
        let Some(props) = conversation.thread_properties.as_ref() else {
            continue;
        };
        let group = text(&props.extra, "groupId");
        if group.is_some_and(|id| teams.iter().any(|team| team.id == id)) {
            continue;
        }
        let Some(parent) = parent_thread(&props.extra) else {
            continue;
        };
        if !attempted.insert(parent.clone()) {
            continue;
        }
        let result = async {
            let mut url = url::Url::parse(&client.chat_service_url())?;
            url.path_segments_mut()
                .map_err(|_| anyhow::anyhow!("Invalid chat URL"))?
                .extend(["v1", "threads", &parent]);
            url.query_pairs_mut()
                .append_pair("view", "msnp24Equivalent");
            client
                .chat_get(url.as_str())
                .await?
                .json::<serde_json::Value>()
                .await
                .map_err(anyhow::Error::from)
        }
        .await;
        if let Ok(value) = result {
            if let Some(properties) = value.get("properties").and_then(|p| p.as_object()) {
                parents.insert(parent, properties.clone());
            }
        }
    }
    merge_native_teams(&mut teams, &conversations, &parents);
    Ok(teams)
}

type Properties = serde_json::Map<String, serde_json::Value>;
fn text<'a>(props: &'a Properties, key: &str) -> Option<&'a str> {
    props
        .get(key)?
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
}
fn parent_thread(props: &Properties) -> Option<String> {
    let value = props.get("parentSpaces")?;
    let parsed;
    let value = if let Some(raw) = value.as_str() {
        parsed = serde_json::from_str::<serde_json::Value>(raw).ok()?;
        &parsed
    } else {
        value
    };
    value
        .get("linkedSpaceInfoItems")?
        .as_array()?
        .iter()
        .find_map(|item| item.get("spaceThreadId")?.as_str().map(str::to_owned))
}

fn merge_native_teams(
    teams: &mut Vec<TeamInfo>,
    conversations: &[Conversation],
    parents: &HashMap<String, Properties>,
) {
    // Add roots first so channels can use their names even if Graph is unavailable.
    let mut roots = parents.clone();
    for conv in conversations {
        if let (Some(id), Some(props)) = (&conv.id, &conv.thread_properties) {
            if props.product_thread_type.as_deref() == Some("TeamsTeam") {
                roots.insert(id.clone(), props.extra.clone());
            }
        }
    }
    for conv in conversations {
        let (Some(id), Some(props)) = (&conv.id, &conv.thread_properties) else {
            continue;
        };
        let is_root = props.product_thread_type.as_deref() == Some("TeamsTeam");
        if !is_root
            && !matches!(
                props.product_thread_type.as_deref(),
                Some("TeamsChannel" | "TeamsPrivateChannel" | "TeamsSharedChannel")
            )
        {
            continue;
        }
        let parent = if is_root {
            Some(id.clone())
        } else {
            parent_thread(&props.extra)
        };
        let root = parent.as_ref().and_then(|id| roots.get(id));
        let Some(team_id) = root
            .and_then(|p| text(p, "groupId"))
            .or_else(|| text(&props.extra, "groupId"))
            .or(parent.as_deref())
        else {
            continue;
        };
        let team_name = root.and_then(|p| text(p, "spaceThreadTopic"));
        let index = teams
            .iter()
            .position(|team| team.id == team_id)
            .unwrap_or_else(|| {
                teams.push(TeamInfo {
                    id: team_id.to_owned(),
                    name: team_name.unwrap_or("Unnamed team").to_owned(),
                    channels: Vec::new(),
                });
                teams.len() - 1
            });
        let team = &mut teams[index];
        if let Some(name) = team_name {
            team.name = name.to_owned();
        }
        let channel_name = if is_root {
            text(&props.extra, "topicThreadTopic")
        } else {
            text(&props.extra, "spaceThreadTopic")
        };
        if let Some(channel) = team.channels.iter_mut().find(|channel| channel.id == *id) {
            if let Some(name) = channel_name {
                channel.name = name.to_owned();
            }
        } else {
            team.channels.push(ChannelInfo {
                id: id.clone(),
                name: channel_name
                    .unwrap_or(if is_root {
                        "General"
                    } else {
                        "Unnamed channel"
                    })
                    .to_owned(),
            });
        }
    }
}

#[cfg(test)]
mod native_tests {
    use super::*;

    #[test]
    fn native_channels_join_graph_team_without_duplicates_or_losing_other_channels() {
        let mut teams = vec![TeamInfo {
            id: "group".into(),
            name: "Graph team".into(),
            channels: vec![
                ChannelInfo {
                    id: "root".into(),
                    name: "General localized".into(),
                },
                ChannelInfo {
                    id: "graph-only".into(),
                    name: "Older channel".into(),
                },
            ],
        }];
        let conversations: Vec<Conversation> = serde_json::from_value(serde_json::json!([
            {"id":"private", "threadProperties":{
                "productThreadType":"TeamsPrivateChannel", "spaceThreadTopic":"Private channel",
                "groupId":"group", "parentSpaces":"{\"linkedSpaceInfoItems\":[{\"spaceThreadId\":\"root\"}]}"
            }},
            {"id":"root", "threadProperties":{
                "productThreadType":"TeamsTeam", "spaceThreadTopic":"Native team", "groupId":"group"
            }},
            {"id":"48:notifications", "threadProperties":{"productThreadType":"StreamOfNotifications"}}
        ])).unwrap();
        merge_native_teams(&mut teams, &conversations, &HashMap::new());
        merge_native_teams(&mut teams, &conversations, &HashMap::new());
        assert_eq!(teams.len(), 1);
        assert_eq!(teams[0].name, "Native team");
        assert_eq!(teams[0].channels.len(), 3);
        assert_eq!(teams[0].channels[0].name, "General localized");
        assert_eq!(teams[0].channels[1].name, "Older channel");
        assert_eq!(teams[0].channels[2].name, "Private channel");

        let mut native_only = Vec::new();
        merge_native_teams(&mut native_only, &conversations, &HashMap::new());
        assert_eq!(native_only.len(), 1);
        assert_eq!(native_only[0].name, "Native team");
        assert_eq!(native_only[0].channels.len(), 2);
    }

    #[test]
    fn parent_metadata_names_a_team_absent_from_the_recent_list() {
        let conversations = serde_json::from_value::<Vec<Conversation>>(serde_json::json!([
            {"id":"private", "threadProperties":{
                "productThreadType":"TeamsPrivateChannel", "spaceThreadTopic":"Research",
                "parentSpaces":{"linkedSpaceInfoItems":[{"spaceThreadId":"parent"}]}
            }}
        ]))
        .unwrap();
        let parents = HashMap::from([(
            "parent".into(),
            serde_json::from_value(serde_json::json!({
                "groupId":"group", "spaceThreadTopic":"Research team"
            }))
            .unwrap(),
        )]);
        let mut teams = Vec::new();
        merge_native_teams(&mut teams, &conversations, &parents);
        assert_eq!(teams[0].id, "group");
        assert_eq!(teams[0].name, "Research team");
        assert_eq!(teams[0].channels[0].id, "private");
    }
}
