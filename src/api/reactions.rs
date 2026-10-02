//! Native message reaction properties. Writes affect one emoji for our user.

use anyhow::{ensure, Context, Result};
use serde_json::Value;

use super::{client::TeamsClient, names::same_user};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reaction {
    pub key: String,
    pub count: u32,
    pub users: Vec<String>,
}

impl Reaction {
    pub fn is_own(&self, current_user: Option<&str>) -> bool {
        current_user.is_some_and(|user| self.users.iter().any(|mri| same_user(mri, user)))
    }

    pub fn label(&self) -> &str {
        label(&self.key)
    }
}

pub(crate) fn label(key: &str) -> &str {
    match key {
        "like" => "+1",
        "heart" => "<3",
        "laugh" => "laugh",
        "surprised" => "surprised",
        "sad" => "sad",
        "angry" => "angry",
        _ => key,
    }
}

/// Both decoded arrays and JSON strings occur in native message properties.
/// Missing user identities never establish ownership or inflate user counts.
pub(super) fn parse_reactions(properties: Option<&Value>) -> Vec<Reaction> {
    let Some(value) = properties.and_then(|p| p.get("emotions")) else {
        return Vec::new();
    };
    let decoded;
    let value = if let Some(text) = value.as_str() {
        decoded = serde_json::from_str::<Value>(text).unwrap_or_default();
        &decoded
    } else {
        value
    };
    let mut reactions: Vec<Reaction> = Vec::new();
    for entry in value.as_array().into_iter().flatten() {
        let Some(key) = entry
            .get("key")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
        else {
            continue;
        };
        let index = reactions
            .iter()
            .position(|r| r.key == key)
            .unwrap_or_else(|| {
                reactions.push(Reaction {
                    key: key.into(),
                    count: 0,
                    users: Vec::new(),
                });
                reactions.len() - 1
            });
        let reaction = &mut reactions[index];
        for user in entry
            .get("users")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(mri) = user
                .get("mri")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
            else {
                continue;
            };
            if !reaction.users.iter().any(|other| same_user(mri, other)) {
                reaction.users.push(mri.into());
            }
        }
        let reported = entry
            .get("count")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .min(u32::MAX as u64) as u32;
        reaction.count = reaction
            .count
            .max(reported)
            .max(reaction.users.len() as u32);
    }
    reactions.retain(|r| r.count > 0);
    reactions
}

fn message_url(client: &TeamsClient, chat_id: &str, message_id: &str) -> Result<url::Url> {
    ensure!(
        !chat_id.trim().is_empty() && !message_id.trim().is_empty(),
        "Select a message first"
    );
    let mut url = url::Url::parse(&client.chat_service_url())?;
    url.path_segments_mut()
        .map_err(|_| anyhow::anyhow!("Invalid chat service URL"))?
        .pop_if_empty()
        .extend([
            "v1",
            "users",
            "ME",
            "conversations",
            chat_id,
            "messages",
            message_id,
        ]);
    Ok(url)
}

pub async fn change_reaction(
    client: &TeamsClient,
    chat_id: &str,
    message_id: &str,
    key: &str,
    remove: bool,
) -> Result<()> {
    ensure!(!key.trim().is_empty(), "Choose a reaction first");
    let mut url = message_url(client, chat_id, message_id)?;
    url.path_segments_mut().unwrap().push("properties");
    url.query_pairs_mut().append_pair("name", "emotions");
    let mut emotion = serde_json::json!({"key": key});
    if !remove {
        emotion["value"] = chrono::Utc::now().timestamp_millis().into();
    }
    client
        .chat_reaction(
            url.as_str(),
            &serde_json::json!({"emotions": emotion}),
            remove,
        )
        .await?;
    Ok(())
}

/// Fetch the exact message, including targets older than the latest history page.
pub async fn read_reactions(
    client: &TeamsClient,
    chat_id: &str,
    message_id: &str,
) -> Result<Vec<Reaction>> {
    let url = message_url(client, chat_id, message_id)?;
    let message: Value = client
        .chat_get(url.as_str())
        .await?
        .json()
        .await
        .context("Invalid message response")?;
    // Do not silently clear reactions if the service returned an unrelated shape.
    ensure!(
        message.get("id").and_then(Value::as_str) == Some(message_id),
        "Reaction refresh returned a different message"
    );
    Ok(parse_reactions(message.get("properties")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn reactions_decode_arrays_and_strings_and_deduplicate_users_per_emoji() {
        let entries = serde_json::json!([
            {"key":"like", "users":[{"mri":"8:orgid:me"}, {"mri":"orgid:ME"}, {"mri":"other"}]},
            {"key":"heart", "users":[{"mri":"me"}]},
            {"key":"like", "users":[{"mri":"https://chat/contacts/8:orgid:other"}, {"mri":"third"}]}
        ]);
        for emotions in [entries.clone(), Value::String(entries.to_string())] {
            let actual = parse_reactions(Some(&serde_json::json!({"emotions":emotions})));
            assert_eq!(actual.len(), 2);
            assert_eq!(actual[0].count, 3);
            assert_eq!(actual[1].count, 1);
            assert!(actual.iter().all(|r| r.is_own(Some("ME"))));
            assert!(!actual[0].is_own(None));
            assert!(!actual[0].is_own(Some("unknown")));
        }
    }

    #[test]
    fn reactions_tolerate_missing_malformed_and_partial_properties() {
        for properties in [
            None,
            Some(Value::Null),
            Some(serde_json::json!({"emotions":"broken"})),
            Some(serde_json::json!({"emotions":{}})),
        ] {
            assert!(parse_reactions(properties.as_ref()).is_empty());
        }
        let actual = parse_reactions(Some(&serde_json::json!({"emotions":[
            null, {}, {"key":""}, {"key":"like", "users":[{}, {"mri":""}]},
            {"key":"custom", "count":4}, {"key":"custom", "count":2},
            {"key":"heart", "users":[{"mri":"someone"}, null]}
        ]})));
        assert_eq!(
            actual
                .iter()
                .map(|r| (r.key.as_str(), r.count))
                .collect::<Vec<_>>(),
            [("custom", 4), ("heart", 1)]
        );
        assert!(actual.iter().all(|r| !r.is_own(Some("me"))));
    }

    async fn mock_response(
        status: &str,
        body: &str,
    ) -> (TeamsClient, tokio::task::JoinHandle<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = TeamsClient::for_test(&format!("http://{}", listener.local_addr().unwrap()));
        let response = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        let request = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            let mut buffer = [0; 4096];
            loop {
                let read = socket.read(&mut buffer).await.unwrap();
                assert!(read > 0);
                bytes.extend_from_slice(&buffer[..read]);
                let text = String::from_utf8_lossy(&bytes);
                if let Some((headers, body)) = text.split_once("\r\n\r\n") {
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length: ")
                                .and_then(|value| value.parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if body.len() >= length {
                        break;
                    }
                }
            }
            socket.write_all(response.as_bytes()).await.unwrap();
            String::from_utf8(bytes).unwrap()
        });
        (client, request)
    }

    #[tokio::test]
    async fn reaction_http_uses_native_auth_scoped_methods_and_encoded_target() {
        for remove in [false, true] {
            let (client, request) = mock_response("200 OK", "{}").await;
            change_reaction(&client, "19:chat/part@thread.v2", "12?34", "heart", remove)
                .await
                .unwrap();
            let request = request.await.unwrap();
            let (headers, body) = request.split_once("\r\n\r\n").unwrap();
            assert!(headers.starts_with(if remove { "DELETE " } else { "PUT " }));
            assert!(headers.contains("/v1/users/ME/conversations/19:chat%2Fpart@thread.v2/messages/12%3F34/properties?name=emotions HTTP/1.1"));
            let headers = headers.to_ascii_lowercase();
            assert!(headers.contains("authentication: skypetoken=test-skype-token"));
            assert!(headers.contains(if remove {
                "x-ms-client-caller: updatemessagereactionremove"
            } else {
                "x-ms-client-caller: updatemessagereactionadd"
            }));
            let body: Value = serde_json::from_str(body).unwrap();
            assert_eq!(body["emotions"]["key"], "heart");
            assert_eq!(body.as_object().unwrap().len(), 1);
            if remove {
                assert_eq!(body["emotions"], serde_json::json!({"key":"heart"}));
            } else {
                assert!(body["emotions"]["value"].as_i64().unwrap() > 0);
            }
        }
    }

    #[tokio::test]
    async fn reaction_http_reads_exact_message_and_propagates_errors() {
        let (client, request) = mock_response(
            "200 OK",
            r#"{"id":"old","properties":{"emotions":[{"key":"like","users":[{"mri":"me"}]}]}}"#,
        )
        .await;
        assert_eq!(
            read_reactions(&client, "chat", "old").await.unwrap()[0].count,
            1
        );
        assert!(request
            .await
            .unwrap()
            .starts_with("GET /v1/users/ME/conversations/chat/messages/old HTTP/1.1"));
        for (status, expected) in [
            ("401 Unauthorized", "login"),
            ("403 Forbidden", "403"),
            ("429 Too Many Requests", "429"),
        ] {
            let (client, request) = mock_response(status, "denied").await;
            assert!(change_reaction(&client, "chat", "old", "like", false)
                .await
                .unwrap_err()
                .to_string()
                .contains(expected));
            request.await.unwrap();
        }
        let (client, request) = mock_response("200 OK", r#"{"id":"different"}"#).await;
        assert!(read_reactions(&client, "chat", "old")
            .await
            .unwrap_err()
            .to_string()
            .contains("different message"));
        request.await.unwrap();
        let client = TeamsClient::for_test("http://127.0.0.1:1");
        assert!(change_reaction(&client, "", "old", "like", false)
            .await
            .unwrap_err()
            .to_string()
            .contains("Select a message"));
    }
}
