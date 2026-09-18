//! Local unread observations. This module never sends Teams read receipts.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::activity::IncomingMessage;
use crate::api::{ChatInfo, MessageInfo};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Badge {
    pub count: u32,
    pub unknown: bool,
}

impl Badge {
    pub fn any(self) -> bool {
        self.count > 0 || self.unknown
    }

    pub fn label(self) -> String {
        match (self.count, self.unknown) {
            (0, false) => String::new(),
            (0, true) => "●".into(),
            (count, true) => format!("{count}+"),
            (count, false) => count.to_string(),
        }
    }

    pub fn combine(self, other: Self) -> Self {
        Self {
            count: self.count.saturating_add(other.count),
            unknown: self.unknown || other.unknown,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageStamp {
    pub id: String,
    millis: Option<i64>,
}

impl MessageStamp {
    pub fn new(id: &str, timestamp: &str) -> Self {
        Self {
            id: id.to_owned(),
            millis: chrono::DateTime::parse_from_rfc3339(timestamp)
                .ok()
                .map(|date| date.timestamp_millis()),
        }
    }

    fn at_or_before(&self, other: &Self) -> bool {
        self.id == other.id || other.after(self)
    }

    fn after(&self, other: &Self) -> bool {
        match (self.millis, other.millis) {
            (Some(a), Some(b)) if a != b => a > b,
            _ => {
                matches!((self.id.parse::<u64>(), other.id.parse::<u64>()), (Ok(a), Ok(b)) if a > b)
            }
        }
    }
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Conversation {
    latest: Option<MessageStamp>,
    read_horizon: Option<MessageStamp>,
    seen: BTreeMap<String, MessageStamp>,
    pending: BTreeMap<String, MessageStamp>,
    /// Unknown backlog is a dot, never a fabricated historical count.
    unknown: bool,
    unknown_horizon: Option<MessageStamp>,
    server_count: Option<u32>,
    server_snapshot: Option<MessageStamp>,
    acknowledged_snapshot: Option<MessageStamp>,
}

#[derive(Default, Serialize, Deserialize)]
pub struct UnreadState {
    conversations: BTreeMap<String, Conversation>,
    #[serde(skip)]
    path: Option<PathBuf>,
    #[serde(skip)]
    dirty: bool,
}

/// Normalize an MRI or sender resource URL without using a display name.
pub fn same_user(left: &str, right: &str) -> bool {
    fn identity(value: &str) -> &str {
        let tail = value.rsplit('/').next().unwrap_or(value);
        tail.strip_prefix("8:orgid:")
            .or_else(|| tail.strip_prefix("orgid:"))
            .unwrap_or(tail)
    }
    !left.is_empty() && !right.is_empty() && identity(left).eq_ignore_ascii_case(identity(right))
}

impl UnreadState {
    #[cfg(not(test))]
    pub fn load_for_account(tenant: &str, user: &str) -> Result<Self> {
        let dirs = directories::ProjectDirs::from("com", "teams-cli", "teams-cli")
            .context("Could not determine unread state directory")?;
        Self::load_path(&account_path(dirs.config_dir(), tenant, user))
    }

    fn load_path(path: &Path) -> Result<Self> {
        let mut state = match fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).context("Invalid local unread state")?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(error) => return Err(error).context("Failed to read local unread state"),
        };
        state.path = Some(path.to_owned());
        Ok(state)
    }

    pub fn save(&mut self) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }
        let Some(path) = &self.path else {
            return Ok(());
        };
        fs::create_dir_all(
            path.parent()
                .context("Unread state path has no directory")?,
        )?;
        // A unique, restrictive temporary file avoids exposing account metadata or
        // truncating the last good state if the process stops while writing.
        let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let result = (|| -> Result<()> {
            let mut file = options.open(&temporary)?;
            file.write_all(&serde_json::to_vec(self)?)?;
            file.sync_all()?;
            fs::rename(&temporary, path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        } else {
            self.dirty = false;
        }
        result.context("Failed to save local unread state")
    }

    pub fn badge(&self, chat_id: &str) -> Badge {
        let Some(conversation) = self.conversations.get(chat_id) else {
            return Badge::default();
        };
        let beyond_snapshot = conversation
            .pending
            .values()
            .filter(|stamp| {
                !conversation
                    .server_snapshot
                    .as_ref()
                    .is_some_and(|s| stamp.at_or_before(s))
            })
            .count() as u32;
        Badge {
            count: conversation
                .server_count
                .unwrap_or(0)
                .saturating_add(beyond_snapshot)
                .max(conversation.pending.len() as u32),
            unknown: conversation.unknown,
        }
    }

    /// Snapshot metadata supplies a baseline on first use, not old-message alerts.
    pub fn observe_chat(&mut self, chat: &ChatInfo, current_user: Option<&str>) {
        let state = self.conversations.entry(chat.id.clone()).or_default();
        let before = state.clone();
        let latest = chat
            .last_message_id
            .as_deref()
            .filter(|id| !id.is_empty())
            .map(|id| MessageStamp::new(id, chat.last_message_time.as_deref().unwrap_or("")));
        if let Some(latest) = &latest {
            let changed = state.latest.as_ref().is_some_and(|old| latest.after(old));
            let own = current_user
                .zip(chat.last_message_sender_id.as_deref())
                .is_some_and(|(user, sender)| same_user(user, sender));
            let text = matches!(
                chat.last_message_type.as_deref(),
                Some("Text" | "RichText" | "RichText/Html")
            );
            let read = state
                .read_horizon
                .as_ref()
                .is_some_and(|h| latest.at_or_before(h));
            if changed && !own && text && !read && !state.pending.contains_key(&latest.id) {
                set_unknown(state, Some(latest));
            }
            if state
                .latest
                .as_ref()
                .is_none_or(|old| !latest.at_or_before(old))
            {
                state.latest = Some(latest.clone());
            }
            remember(state, latest.clone());
        }
        // Do not resurrect a server snapshot that this terminal already displayed.
        let acknowledged = latest.as_ref().is_some_and(|stamp| {
            state
                .acknowledged_snapshot
                .as_ref()
                .is_some_and(|old| stamp.at_or_before(old))
        });
        if !acknowledged {
            if let Some(count) = chat.unread_count {
                state.server_count = Some(count);
                state.server_snapshot = latest.clone();
            } else if chat.has_unread == Some(true)
                && !current_user
                    .zip(chat.last_message_sender_id.as_deref())
                    .is_some_and(|(u, s)| same_user(u, s))
                && matches!(
                    chat.last_message_type.as_deref(),
                    Some("Text" | "RichText" | "RichText/Html")
                )
            {
                set_unknown(state, latest.as_ref());
            }
        }
        if chat.has_unread == Some(false) {
            if let Some(stamp) = &latest {
                acknowledge_state(state, stamp);
            }
        }
        self.dirty |= *state != before;
    }

    /// Full history seeds identities. Only a saved baseline allows counting newer
    /// messages after an application restart; initial history never creates counts.
    pub fn observe_history(
        &mut self,
        chat_id: &str,
        messages: &[MessageInfo],
        current_user: Option<&str>,
    ) {
        let state = self.conversations.entry(chat_id.to_owned()).or_default();
        let before = state.clone();
        let baseline = state.latest.clone();
        for message in messages {
            if message.id.is_empty() {
                continue;
            }
            let stamp = MessageStamp::new(&message.id, &message.timestamp);
            let own = current_user.is_some_and(|user| same_user(user, &message.sender_id));
            let new = baseline.as_ref().is_some_and(|base| stamp.after(base));
            let read = state
                .read_horizon
                .as_ref()
                .is_some_and(|h| stamp.at_or_before(h));
            if new && current_user.is_some() && !own && !read && !state.seen.contains_key(&stamp.id)
            {
                state.pending.insert(stamp.id.clone(), stamp.clone());
            }
            remember(state, stamp.clone());
            if state
                .latest
                .as_ref()
                .is_none_or(|old| !stamp.at_or_before(old))
            {
                state.latest = Some(stamp);
            }
        }
        self.dirty |= *state != before;
    }

    /// Returns true only for a distinct, incoming, locally unread message.
    pub fn incoming(&mut self, message: &IncomingMessage, current_user: Option<&str>) -> bool {
        if message.id.is_empty()
            || current_user.is_some_and(|user| same_user(user, &message.sender_id))
        {
            return false;
        }
        let state = self
            .conversations
            .entry(message.chat_id.clone())
            .or_default();
        let stamp = MessageStamp::new(&message.id, &message.timestamp);
        if state.pending.contains_key(&stamp.id)
            || state
                .read_horizon
                .as_ref()
                .is_some_and(|h| stamp.at_or_before(h))
        {
            return false;
        }
        // Chat snapshots may have seen an ID without its content being displayed.
        let already_seen = state.seen.contains_key(&stamp.id);
        if already_seen
            && !(state.unknown && state.latest.as_ref().is_some_and(|s| s.id == stamp.id))
        {
            return false;
        }
        state.pending.insert(stamp.id.clone(), stamp.clone());
        remember(state, stamp.clone());
        if state
            .latest
            .as_ref()
            .is_none_or(|old| !stamp.at_or_before(old))
        {
            state.latest = Some(stamp);
        }
        self.dirty = true;
        true
    }

    /// Call only after a successful draw has exposed the newest loaded content.
    pub fn acknowledge(&mut self, chat_id: &str, displayed: &MessageStamp) {
        let Some(state) = self.conversations.get_mut(chat_id) else {
            return;
        };
        let before = state.clone();
        acknowledge_state(state, displayed);
        self.dirty |= *state != before;
    }
}

fn set_unknown(state: &mut Conversation, stamp: Option<&MessageStamp>) {
    state.unknown = true;
    if let Some(stamp) = stamp {
        if state
            .unknown_horizon
            .as_ref()
            .is_none_or(|old| stamp.after(old))
        {
            state.unknown_horizon = Some(stamp.clone());
        }
    }
}

fn acknowledge_state(state: &mut Conversation, displayed: &MessageStamp) {
    state
        .pending
        .retain(|_, stamp| !stamp.at_or_before(displayed));
    let covers_latest = state
        .latest
        .as_ref()
        .is_none_or(|last| last.at_or_before(displayed));
    if covers_latest
        || state
            .unknown_horizon
            .as_ref()
            .is_some_and(|s| s.at_or_before(displayed))
    {
        state.unknown = false;
        state.unknown_horizon = None;
    }
    if covers_latest
        || state
            .server_snapshot
            .as_ref()
            .is_some_and(|s| s.at_or_before(displayed))
    {
        state.server_count = None;
        state.server_snapshot = None;
    }
    if state
        .acknowledged_snapshot
        .as_ref()
        .is_none_or(|h| !displayed.at_or_before(h))
    {
        state.acknowledged_snapshot = Some(displayed.clone());
    }
    if state
        .read_horizon
        .as_ref()
        .is_none_or(|h| !displayed.at_or_before(h))
    {
        state.read_horizon = Some(displayed.clone());
    }
}

fn remember(state: &mut Conversation, stamp: MessageStamp) {
    state.seen.insert(stamp.id.clone(), stamp);
    while state.seen.len() > 256 {
        let oldest = state
            .seen
            .iter()
            .min_by_key(|(_, stamp)| stamp.millis)
            .map(|(id, _)| id.clone())
            .unwrap();
        state.seen.remove(&oldest);
    }
}

fn account_path(base: &Path, tenant: &str, user: &str) -> PathBuf {
    let digest =
        Sha256::digest(format!("{}\0{}", tenant.to_lowercase(), user.to_lowercase()).as_bytes());
    base.join("unread").join(format!("{digest:x}.json"))
}

/// Extract account identifiers from the already stored token, only for local
/// state namespacing. This does not authenticate or refresh anything.
pub fn configured_account() -> Option<(String, String)> {
    let config = crate::config::Config::load().ok()?;
    for token in [config.access_token, config.graph_token]
        .into_iter()
        .flatten()
    {
        let Some(payload) = token.token.split('.').nth(1) else {
            continue;
        };
        let Ok(bytes) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload) else {
            continue;
        };
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            continue;
        };
        if let Some(user) = value.get("oid").and_then(|v| v.as_str()) {
            let tenant = value
                .get("tid")
                .and_then(|v| v.as_str())
                .or(config.tenant_id.as_deref())
                .unwrap_or("");
            return Some((tenant.to_owned(), user.to_owned()));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(chat: &str, id: &str) -> IncomingMessage {
        IncomingMessage {
            chat_id: chat.into(),
            id: id.into(),
            sender_id: "8:orgid:other".into(),
            sender: "Same display name".into(),
            timestamp: "2026-09-18T10:00:00Z".into(),
            content: "private message text".into(),
            mentions: vec![],
        }
    }

    fn message(id: &str) -> MessageInfo {
        let event = event("chat", id);
        MessageInfo {
            id: event.id,
            sender_id: event.sender_id,
            sender: event.sender,
            timestamp: event.timestamp,
            content: event.content,
            mentions: vec![],
        }
    }

    fn chat(id: &str, last: &str, unread: Option<bool>) -> ChatInfo {
        ChatInfo {
            id: id.into(),
            name: "Chat".into(),
            is_group: false,
            last_message_id: Some(last.into()),
            last_message_sender_id: Some("8:orgid:other".into()),
            last_message_type: Some("Text".into()),
            last_message_time: Some("2026-09-18T10:00:00Z".into()),
            last_message_sender: None,
            last_message_preview: None,
            unread_count: None,
            has_unread: unread,
        }
    }

    #[test]
    fn initial_history_is_a_baseline_and_identity_excludes_self_duplicates() {
        let mut state = UnreadState::default();
        state.observe_history("chat", &[message("1"), message("2")], Some("self"));
        assert!(!state.badge("chat").any());
        assert!(!state.incoming(&event("chat", "2"), Some("self")));
        let mut own = event("chat", "3");
        own.sender_id = "https://example.invalid/users/8:orgid:SELF".into();
        assert!(!state.incoming(&own, Some("self")));
        assert!(state.incoming(&event("chat", "3"), Some("self")));
        assert!(!state.incoming(&event("chat", "3"), Some("self")));
        assert!(state.incoming(&event("other-chat", "3"), Some("self")));
        assert_eq!(state.badge("chat").count, 1);
    }

    #[test]
    fn native_unread_is_a_dot_and_stale_snapshots_do_not_resurrect_reads() {
        let mut state = UnreadState::default();
        let snapshot = chat("chat", "2", Some(true));
        state.observe_chat(&snapshot, Some("self"));
        assert_eq!(state.badge("chat").label(), "●");
        state.observe_history("chat", &[message("1"), message("2")], Some("self"));
        assert_eq!(state.badge("chat").label(), "●");
        state.acknowledge("chat", &MessageStamp::new("2", "2026-09-18T10:00:00Z"));
        assert!(!state.badge("chat").any());
        state.observe_chat(&snapshot, Some("self"));
        assert!(!state.badge("chat").any());
        assert!(!state.incoming(&event("chat", "1"), Some("self")));
        state.observe_chat(&chat("chat", "3", Some(true)), Some("self"));
        assert_eq!(state.badge("chat").label(), "●");
    }

    #[test]
    fn reload_preserves_pending_and_only_newer_history_adds_counts() {
        let mut state = UnreadState::default();
        state.observe_history("chat", &[message("1")], Some("self"));
        state.incoming(&event("chat", "2"), Some("self"));
        state.observe_chat(&chat("chat", "2", None), Some("self"));
        state.observe_history(
            "chat",
            &[message("1"), message("2"), message("3")],
            Some("self"),
        );
        assert_eq!(state.badge("chat").count, 2);
        state.acknowledge("chat", &MessageStamp::new("2", "2026-09-18T10:00:00Z"));
        assert_eq!(state.badge("chat").count, 1);
        state.acknowledge("chat", &MessageStamp::new("3", "2026-09-18T10:00:00Z"));
        assert!(!state.badge("chat").any());
    }

    #[test]
    fn history_does_not_classify_self_until_account_identity_is_known() {
        let mut state = UnreadState::default();
        state.observe_history("chat", &[message("1")], None);
        state.observe_history("chat", &[message("1"), message("2")], None);
        assert!(!state.badge("chat").any());
    }

    #[test]
    fn unavailable_ordering_does_not_invent_history_counts() {
        let mut state = UnreadState::default();
        let mut first = message("opaque-one");
        first.timestamp.clear();
        let mut second = message("opaque-two");
        second.timestamp.clear();
        state.observe_history("chat", &[first.clone()], Some("self"));
        state.observe_history("chat", &[first, second], Some("self"));
        assert!(!state.badge("chat").any());
    }

    #[test]
    fn server_count_does_not_double_count_matching_snapshot_messages() {
        let mut state = UnreadState::default();
        let mut snapshot = chat("chat", "5", None);
        snapshot.unread_count = Some(4);
        state.observe_chat(&snapshot, Some("self"));
        state.incoming(&event("chat", "6"), Some("self"));
        assert_eq!(state.badge("chat").count, 5);
        snapshot.last_message_id = Some("6".into());
        snapshot.unread_count = Some(5);
        state.observe_chat(&snapshot, Some("self"));
        assert_eq!(state.badge("chat").count, 5);
    }

    #[test]
    fn authoritative_read_snapshot_clears_only_covered_activity() {
        let mut state = UnreadState::default();
        state.observe_chat(&chat("chat", "2", Some(true)), Some("self"));
        state.observe_chat(&chat("chat", "2", None), Some("self"));
        assert!(state.badge("chat").unknown);
        state.incoming(&event("chat", "3"), Some("self"));
        state.observe_chat(&chat("chat", "2", Some(false)), Some("self"));
        assert_eq!(state.badge("chat").count, 1);
        // The old unknown backlog was covered by the server read horizon.
        assert!(!state.badge("chat").unknown);
        state.observe_chat(&chat("chat", "3", Some(false)), Some("self"));
        assert!(!state.badge("chat").any());
        assert!(!state.incoming(&event("chat", "2"), Some("self")));
    }

    #[test]
    fn repeated_reads_and_identical_snapshots_do_not_dirty_saved_state() {
        let mut state = UnreadState::default();
        let snapshot = chat("chat", "2", Some(true));
        state.observe_chat(&snapshot, Some("self"));
        state.dirty = false;
        state.observe_chat(&snapshot, Some("self"));
        assert!(!state.dirty);
        let stamp = MessageStamp::new("2", "2026-09-18T10:00:00Z");
        state.acknowledge("chat", &stamp);
        state.dirty = false;
        state.acknowledge("chat", &stamp);
        assert!(!state.dirty);
        state.incoming(&event("chat", "3"), Some("self"));
        state.dirty = false;
        state.acknowledge("chat", &stamp);
        assert!(!state.dirty);
    }

    #[test]
    fn persistence_retains_unread_and_read_horizons_without_message_content() {
        let dir = std::env::temp_dir().join(format!("ost-unread-{}", uuid::Uuid::new_v4()));
        let path = account_path(&dir, "tenant", "user");
        let mut state = UnreadState::load_path(&path).unwrap();
        state.incoming(&event("chat", "1"), Some("self"));
        state.save().unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains("private message text"));
        assert!(!text.contains("Same display name"));
        let mut restored = UnreadState::load_path(&path).unwrap();
        assert_eq!(restored.badge("chat").count, 1);
        restored.acknowledge("chat", &MessageStamp::new("1", "2026-09-18T10:00:00Z"));
        restored.save().unwrap();
        let mut restored = UnreadState::load_path(&path).unwrap();
        assert!(!restored.badge("chat").any());
        assert!(!restored.incoming(&event("chat", "1"), Some("self")));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert_ne!(path, account_path(&dir, "other-tenant", "user"));
        assert_ne!(path, account_path(&dir, "tenant", "other-user"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn corrupt_saved_state_is_reported_without_overwriting_it() {
        let path =
            std::env::temp_dir().join(format!("ost-bad-unread-{}.json", uuid::Uuid::new_v4()));
        fs::write(&path, b"not json").unwrap();
        assert!(UnreadState::load_path(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"not json");
        fs::remove_file(path).unwrap();
    }
}
