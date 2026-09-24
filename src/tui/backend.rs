//! Async backend with a quiet push subscription and bounded reconciliation.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    time::Duration,
};

use anyhow::Result;
use tokio::{
    sync::mpsc,
    task::{JoinHandle, JoinSet},
    time,
};

use super::activity::{ConnectionState, IncomingMessage};
use crate::{api, api::client::TeamsClient, trouter::subscription};

pub enum BackendCommand {
    LoadTeams,
    LoadChats { limit: usize },
    LoadMessages { chat_id: String, limit: usize },
    SendMessage { chat_id: String, message: String },
    LoadUserInfo,
    LoadPresence,
}

pub enum BackendResponse {
    IncomingMessage(IncomingMessage),
    ConnectionState(ConnectionState),
    Teams(Result<Vec<api::TeamInfo>>),
    Chats(Result<Vec<api::ChatInfo>>),
    ChannelSummaries(Vec<api::ChatInfo>),
    ChatName {
        chat_id: String,
        name: String,
        source: api::ChatNameSource,
    },
    Messages {
        chat_id: String,
        result: Result<Vec<api::MessageInfo>>,
    },
    MessageSent {
        chat_id: String,
        result: Result<()>,
    },
    UserInfo(Result<api::UserInfo>),
    Presence(Result<api::PresenceInfo>),
    ClientError(String),
}

pub struct Backend {
    cmd_tx: mpsc::UnboundedSender<BackendCommand>,
    resp_rx: mpsc::UnboundedReceiver<BackendResponse>,
    task: Option<JoinHandle<()>>,
}

impl Drop for Backend {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            // Cancels pending network work; backend's JoinSet cancels push too.
            task.abort();
        }
    }
}

impl Backend {
    #[cfg(test)]
    pub fn for_test() -> (Self, mpsc::UnboundedReceiver<BackendCommand>) {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (_, resp_rx) = mpsc::unbounded_channel();
        (
            Self {
                cmd_tx,
                resp_rx,
                task: None,
            },
            cmd_rx,
        )
    }

    pub fn start() -> Self {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (resp_tx, resp_rx) = mpsc::unbounded_channel();
        let task = tokio::spawn(backend_loop(cmd_rx, resp_tx));
        Self {
            cmd_tx,
            resp_rx,
            task: Some(task),
        }
    }

    pub fn send(&self, cmd: BackendCommand) {
        if self.cmd_tx.send(cmd).is_err() {
            tracing::error!("Backend channel closed -- command dropped");
        }
    }

    pub async fn recv(&mut self) -> Option<BackendResponse> {
        self.resp_rx.recv().await
    }

    pub async fn shutdown(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
            let _ = task.await;
        }
    }
}

const HISTORY_LIMIT: usize = 50;
const CHAT_LIMIT: usize = 50;
const MAX_PENDING: usize = 64;
const MAX_SEEN: usize = 1024;

#[derive(Default)]
struct PendingChats {
    order: VecDeque<String>,
    ids: HashSet<String>,
}

impl PendingChats {
    fn insert(&mut self, id: String) {
        if api::is_activity_stream(&id) {
            return;
        }
        if self.ids.len() < MAX_PENDING && self.ids.insert(id.clone()) {
            self.order.push_back(id);
        }
    }
    fn pop(&mut self) -> Option<String> {
        let id = self.order.pop_front()?;
        self.ids.remove(&id);
        Some(id)
    }
}

#[derive(Default)]
struct Cursor {
    watermark: i64,
    highest_id: u64,
    seen: VecDeque<String>,
}

struct ActivityTracker {
    started_at: i64,
    initialized: bool,
    summaries: HashMap<String, String>,
    cursors: HashMap<String, Cursor>,
}

impl ActivityTracker {
    fn new() -> Self {
        Self {
            started_at: chrono::Utc::now().timestamp_millis(),
            initialized: false,
            summaries: HashMap::new(),
            cursors: HashMap::new(),
        }
    }

    fn reconcile_chats<'a>(
        &mut self,
        chats: impl IntoIterator<Item = &'a api::ChatInfo>,
        pending: &mut PendingChats,
    ) {
        for chat in chats {
            let signature = format!(
                "{:?}|{:?}|{:?}|{:?}",
                chat.last_message_id,
                chat.last_message_time,
                chat.last_message_sender,
                chat.last_message_preview
            );
            let previous = self.summaries.insert(chat.id.clone(), signature.clone());
            if !self.initialized {
                self.cursors
                    .entry(chat.id.clone())
                    .or_insert_with(|| Cursor {
                        watermark: chat
                            .last_message_time
                            .as_deref()
                            .and_then(timestamp_millis)
                            .unwrap_or(self.started_at)
                            .max(self.started_at),
                        highest_id: chat
                            .last_message_id
                            .as_deref()
                            .and_then(|id| id.parse().ok())
                            .unwrap_or(0),
                        ..Cursor::default()
                    });
            } else if previous.as_deref() != Some(&signature) {
                pending.insert(chat.id.clone());
            }
        }
        self.initialized = true;
    }

    fn observe(
        &mut self,
        chat_id: &str,
        messages: &[api::MessageInfo],
        opening: bool,
    ) -> Vec<IncomingMessage> {
        // Opening an untracked old channel establishes its history baseline.
        let baseline = if opening && !self.cursors.contains_key(chat_id) {
            messages
                .iter()
                .filter_map(message_time)
                .max()
                .unwrap_or(self.started_at)
        } else {
            self.started_at
        };
        let initial_highest_id = if opening {
            messages
                .iter()
                .filter_map(|message| message.id.parse::<u64>().ok())
                .max()
                .unwrap_or(0)
        } else {
            0
        };
        let cursor = self
            .cursors
            .entry(chat_id.to_string())
            .or_insert_with(|| Cursor {
                watermark: baseline,
                highest_id: initial_highest_id,
                ..Cursor::default()
            });
        let previous_watermark = cursor.watermark;
        let previous_id = cursor.highest_id;
        let mut incoming = Vec::new();
        for message in messages {
            let time = message_time(message).unwrap_or(previous_watermark);
            if !message.id.is_empty() && !cursor.seen.contains(&message.id) {
                if time > previous_watermark
                    || (time == previous_watermark
                        && message.id.parse::<u64>().is_ok_and(|id| id > previous_id))
                {
                    incoming.push(IncomingMessage {
                        chat_id: chat_id.to_string(),
                        id: message.id.clone(),
                        sender_id: message.sender_id.clone(),
                        sender: message.sender.clone(),
                        timestamp: message.timestamp.clone(),
                        content: message.content.clone(),
                        mentions: message.mentions.clone(),
                    });
                }
                cursor.seen.push_back(message.id.clone());
                if cursor.seen.len() > MAX_SEEN {
                    cursor.seen.pop_front();
                }
            }
            cursor.watermark = cursor.watermark.max(time);
            cursor.highest_id = cursor.highest_id.max(message.id.parse().unwrap_or(0));
        }
        incoming
    }
}

fn timestamp_millis(value: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|time| time.timestamp_millis())
        .or_else(|| value.parse::<i64>().ok())
}

fn message_time(message: &api::MessageInfo) -> Option<i64> {
    timestamp_millis(&message.timestamp).or_else(|| message.id.parse().ok())
}

async fn client() -> Result<TeamsClient> {
    time::timeout(Duration::from_secs(60), TeamsClient::new())
        .await
        .map_err(|_| anyhow::anyhow!("Authentication refresh timed out; retrying automatically"))?
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Auxiliary {
    Teams,
    User,
    Presence,
}

type AuxiliaryResult = (Auxiliary, bool, BackendResponse);

fn spawn_auxiliary(
    kind: Auxiliary,
    tasks: &mut JoinSet<AuxiliaryResult>,
    running: &mut HashSet<Auxiliary>,
) {
    if !running.insert(kind) {
        return;
    }
    tasks.spawn(async move {
        macro_rules! fetch {
            ($api:path, $variant:ident) => {{
                let result = time::timeout(Duration::from_secs(60), async {
                    let client = client().await?;
                    $api(&client).await
                })
                .await
                .unwrap_or_else(|_| {
                    Err(anyhow::anyhow!("Request timed out; retrying automatically"))
                });
                (kind, result.is_ok(), BackendResponse::$variant(result))
            }};
        }
        match kind {
            Auxiliary::Teams => fetch!(api::list_teams_data, Teams),
            Auxiliary::User => fetch!(api::whoami_data, UserInfo),
            Auxiliary::Presence => fetch!(api::get_presence_data, Presence),
        }
    });
}

/// A separate worker keeps directory latency out of the message command loop.
/// Latest-snapshot channel and one lookup at a time bound both memory and traffic.
#[derive(Clone)]
struct NameCandidate {
    id: String,
    is_group: bool,
}

async fn resolve_chat_names(
    mut snapshots: tokio::sync::watch::Receiver<Vec<NameCandidate>>,
    responses: mpsc::UnboundedSender<BackendResponse>,
) {
    let mut due: HashMap<String, time::Instant> = HashMap::new();
    let mut tick = time::interval(Duration::from_millis(750));
    tick.set_missed_tick_behavior(time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            changed = snapshots.changed() => { if changed.is_err() { break; } }
            _ = tick.tick() => {
                let ids = snapshots.borrow().clone();
                due.retain(|id, _| ids.iter().any(|candidate| candidate.id == *id));
                let now = time::Instant::now();
                let Some(candidate) = ids.into_iter().filter(|candidate| due.get(&candidate.id).is_none_or(|at| *at <= now)).min_by_key(|candidate| due.get(&candidate.id).copied()) else { continue; };
                let id = candidate.id;
                let result = time::timeout(Duration::from_secs(30), async {
                    let client = client().await?;
                    let user = match super::unread::configured_account() {
                        Some((_, user)) => user,
                        None => api::whoami_data(&client).await?.id,
                    };
                    api::names::resolve(&client, &id, &user, candidate.is_group).await
                }).await;
                let success = matches!(&result, Ok(Ok(_)));
                due.insert(id.clone(), time::Instant::now() + Duration::from_secs(if success { 300 } else { 60 }));
                match result {
                    Ok(Ok((name, source))) => {
                        if responses.send(BackendResponse::ChatName { chat_id: id, name, source }).is_err() { break; }
                    }
                    _ => tracing::debug!("Chat name lookup unavailable; retaining cached label"),
                }
            }
        }
    }
}

fn name_candidates(chats: &[api::ChatInfo]) -> Vec<NameCandidate> {
    chats
        .iter()
        .filter(|c| c.name_source != api::ChatNameSource::Topic)
        .take(CHAT_LIMIT)
        .map(|c| NameCandidate {
            id: c.id.clone(),
            is_group: c.is_group,
        })
        .collect()
}

async fn refresh_chats(
    client: &TeamsClient,
    limit: usize,
    tracker: &mut ActivityTracker,
    pending: &mut PendingChats,
    responses: &mpsc::UnboundedSender<BackendResponse>,
    names: &tokio::sync::watch::Sender<Vec<NameCandidate>>,
) -> bool {
    match api::list_recent_data(client, limit).await {
        Ok(recent) => {
            tracker.reconcile_chats(recent.chats.iter().chain(&recent.channels), pending);
            let candidates = name_candidates(&recent.chats);
            let _ = responses.send(BackendResponse::ChannelSummaries(recent.channels));
            let _ = responses.send(BackendResponse::Chats(Ok(recent.chats)));
            names.send_replace(candidates);
            true
        }
        Err(error) => {
            let _ = responses.send(BackendResponse::Chats(Err(error)));
            false
        }
    }
}

async fn backend_loop(
    mut cmd_rx: mpsc::UnboundedReceiver<BackendCommand>,
    resp_tx: mpsc::UnboundedSender<BackendResponse>,
) {
    let (push_tx, mut push_rx) = mpsc::channel(64);
    let mut tasks = JoinSet::new();
    tasks.spawn(subscription::run(push_tx));
    let (name_tx, name_rx) = tokio::sync::watch::channel(Vec::new());
    tasks.spawn(resolve_chat_names(name_rx, resp_tx.clone()));
    let mut auxiliary_tasks = JoinSet::new();
    let mut auxiliary_running = HashSet::new();
    let mut auxiliary_retry = HashSet::new();
    let _ = resp_tx.send(BackendResponse::ConnectionState(
        ConnectionState::Connecting,
    ));
    let mut tracker = ActivityTracker::new();
    let mut pending = PendingChats::default();
    let mut failed_chats = HashSet::new();
    let mut current_chat: Option<String> = None;
    let mut chats_due = false;
    let mut push_connected = false;
    let mut fallback = time::interval(Duration::from_secs(30));
    fallback.set_missed_tick_behavior(time::MissedTickBehavior::Delay);
    fallback.tick().await;
    let mut reconcile = time::interval(Duration::from_millis(750));
    reconcile.set_missed_tick_behavior(time::MissedTickBehavior::Delay);

    // Requests are serialized deliberately: responses cannot race and replace a
    // newer history with an older snapshot. The terminal loop remains async.
    loop {
        tokio::select! {
            biased;
            _ = resp_tx.closed() => break,
            command = cmd_rx.recv() => {
                let Some(command) = command else { break };
                let auxiliary = match &command {
                    BackendCommand::LoadTeams => Some(Auxiliary::Teams),
                    BackendCommand::LoadUserInfo => Some(Auxiliary::User),
                    BackendCommand::LoadPresence => Some(Auxiliary::Presence),
                    _ => None,
                };
                if let Some(kind) = auxiliary {
                    spawn_auxiliary(kind, &mut auxiliary_tasks, &mut auxiliary_running);
                    continue;
                }
                if let BackendCommand::LoadMessages { chat_id, .. } = &command {
                    current_chat = Some(chat_id.clone());
                }
                let client = match client().await {
                    Ok(client) => client,
                    Err(error) => {
                        let _ = resp_tx.send(BackendResponse::ClientError(error.to_string()));
                        chats_due = true;
                        continue;
                    }
                };
                match command {
                    BackendCommand::LoadTeams => { let _ = resp_tx.send(BackendResponse::Teams(api::list_teams_data(&client).await)); }
                    BackendCommand::LoadChats { limit } => {
                        refresh_chats(&client, limit.min(CHAT_LIMIT), &mut tracker, &mut pending, &resp_tx, &name_tx).await;
                    }
                    BackendCommand::LoadMessages { chat_id, limit } => {
                        current_chat = Some(chat_id.clone());
                        let result = api::read_messages_data(&client, &chat_id, limit.min(HISTORY_LIMIT)).await;
                        deliver_messages(&resp_tx, &mut tracker, &chat_id, result, true, true);
                    }
                    BackendCommand::SendMessage { chat_id, message } => {
                        let result = api::send_message_with_client(&client, &chat_id, &message).await;
                        if result.is_ok() {
                            // Reconcile the originating chat without changing the active subscription.
                            pending.insert(chat_id.clone());
                        }
                        let _ = resp_tx.send(BackendResponse::MessageSent { chat_id, result });
                        chats_due = true;
                    }
                    BackendCommand::LoadUserInfo => { let _ = resp_tx.send(BackendResponse::UserInfo(api::whoami_data(&client).await)); }
                    BackendCommand::LoadPresence => { let _ = resp_tx.send(BackendResponse::Presence(api::get_presence_data(&client).await)); }
                }
            }
            Some(result) = auxiliary_tasks.join_next(), if !auxiliary_tasks.is_empty() => {
                if let Ok((kind, success, response)) = result {
                    auxiliary_running.remove(&kind);
                    if success { auxiliary_retry.remove(&kind); } else { auxiliary_retry.insert(kind); }
                    let _ = resp_tx.send(response);
                }
            }
            Some(event) = push_rx.recv() => {
                match event {
                    subscription::Event::Connected => {
                        push_connected = true;
                        chats_due = true;
                        if let Some(id) = &current_chat { pending.insert(id.clone()); }
                        let _ = resp_tx.send(BackendResponse::ConnectionState(ConnectionState::Connected));
                    }
                    subscription::Event::Degraded => {
                        push_connected = false;
                        let _ = resp_tx.send(BackendResponse::ConnectionState(ConnectionState::Degraded("Push unavailable; checking every 30s".into())));
                    }
                    subscription::Event::Reconnecting { retry_in_secs } => {
                        let _ = resp_tx.send(BackendResponse::ConnectionState(ConnectionState::Reconnecting { retry_in_secs }));
                    }
                    subscription::Event::Refresh(ids) => {
                        chats_due = true;
                        for id in ids { pending.insert(id); }
                        if let Some(id) = &current_chat { pending.insert(id.clone()); }
                    }
                }
            }
            _ = fallback.tick() => {
                for id in failed_chats.drain() { pending.insert(id); }
                for kind in auxiliary_retry.iter().copied() {
                    spawn_auxiliary(kind, &mut auxiliary_tasks, &mut auxiliary_running);
                }
                chats_due = true;
                if let Some(id) = &current_chat { pending.insert(id.clone()); }
            }
            _ = reconcile.tick(), if chats_due || !pending.ids.is_empty() => {
                let client = match client().await {
                    Ok(client) => client,
                    Err(error) => {
                        let _ = resp_tx.send(BackendResponse::ConnectionState(ConnectionState::Degraded("Authentication unavailable; retrying".into())));
                        tracing::warn!("Background authentication unavailable: {}", error);
                        // Do not spin at the coalescing frequency after auth failure.
                        chats_due = false;
                        // Keep channel invalidations through auth recovery, even
                        // when that channel is absent from the recent-chat list.
                        for id in pending.ids.drain() {
                            if failed_chats.len() < MAX_PENDING { failed_chats.insert(id); }
                        }
                        pending = PendingChats::default();
                        continue;
                    }
                };
                let mut healthy = true;
                if chats_due {
                    chats_due = false;
                    healthy = refresh_chats(&client, CHAT_LIMIT, &mut tracker, &mut pending, &resp_tx, &name_tx).await;
                }
                // One conversation per tick bounds traffic even during a burst.
                if let Some(chat_id) = pending.pop() {
                    let result = api::read_messages_data(&client, &chat_id, HISTORY_LIMIT).await;
                    healthy &= result.is_ok();
                    if result.is_err() && failed_chats.len() < MAX_PENDING { failed_chats.insert(chat_id.clone()); }
                    deliver_messages(&resp_tx, &mut tracker, &chat_id, result, false, current_chat.as_deref() == Some(&chat_id));
                }
                if !healthy {
                    let _ = resp_tx.send(BackendResponse::ConnectionState(ConnectionState::Degraded("Message refresh failed; retrying".into())));
                } else if push_connected {
                    let _ = resp_tx.send(BackendResponse::ConnectionState(ConnectionState::Connected));
                }
            }
        }
    }
    tasks.abort_all();
    auxiliary_tasks.abort_all();
    while tasks.join_next().await.is_some() {}
    while auxiliary_tasks.join_next().await.is_some() {}
}

fn deliver_messages(
    tx: &mpsc::UnboundedSender<BackendResponse>,
    tracker: &mut ActivityTracker,
    chat_id: &str,
    result: Result<Vec<api::MessageInfo>>,
    opening: bool,
    visible: bool,
) {
    if let Ok(messages) = &result {
        for event in tracker.observe(chat_id, messages, opening) {
            let _ = tx.send(BackendResponse::IncomingMessage(event));
        }
    }
    if visible {
        let _ = tx.send(BackendResponse::Messages {
            chat_id: chat_id.to_string(),
            result,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_feed_invalidations_are_not_polled_as_messages() {
        let mut pending = PendingChats::default();
        for id in ["48:notifications", "48:mentions", "48:notes", "48:threads"] {
            pending.insert(id.into());
        }
        pending.insert("19:private@thread.tacv2".into());
        pending.insert("19:direct@unq.gbl.spaces".into());
        assert_eq!(pending.pop().as_deref(), Some("19:private@thread.tacv2"));
        assert_eq!(pending.pop().as_deref(), Some("19:direct@unq.gbl.spaces"));
        assert!(pending.pop().is_none());
    }
    fn message(id: &str, timestamp: &str) -> api::MessageInfo {
        api::MessageInfo {
            id: id.into(),
            sender_id: "8:orgid:other".into(),
            sender: "Other".into(),
            timestamp: timestamp.into(),
            content: "Hello".into(),
            mentions: vec![],
        }
    }
    #[test]
    fn opening_history_is_silent_and_reconnect_replays_are_deduplicated() {
        let mut tracker = ActivityTracker::new();
        let old = message("1000", "2026-01-01T00:00:00Z");
        assert!(tracker
            .observe("chat", std::slice::from_ref(&old), true)
            .is_empty());
        let new = message("2000", "2026-01-01T00:00:01Z");
        let events = tracker.observe("chat", &[old.clone(), new.clone()], false);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, "2000");
        assert!(tracker.observe("chat", &[old, new], false).is_empty());
        assert!(tracker
            .observe("chat", &[message("500", "2025-01-01T00:00:00Z")], false)
            .is_empty());
    }
    #[test]
    fn invalidations_are_bounded_coalesced_and_fifo() {
        let mut pending = PendingChats::default();
        pending.insert("first".into());
        pending.insert("first".into());
        for i in 0..100 {
            pending.insert(i.to_string());
        }
        assert_eq!(pending.ids.len(), MAX_PENDING);
        assert_eq!(pending.pop(), Some("first".into()));
        assert_eq!(pending.pop(), Some("0".into()));
    }
    #[test]
    fn missing_identity_and_timestamp_do_not_replay_history() {
        let mut tracker = ActivityTracker::new();
        assert!(tracker
            .observe("chat", &[message("", "")], false)
            .is_empty());
        assert!(tracker
            .observe("chat", &[message("opaque", "")], false)
            .is_empty());
    }
    #[test]
    fn distinct_message_ids_with_identical_timestamp_are_not_dropped() {
        let mut tracker = ActivityTracker::new();
        let first = message("1000", "2026-01-01T00:00:00Z");
        assert!(tracker
            .observe("chat", std::slice::from_ref(&first), true)
            .is_empty());
        let second = message("1001", "2026-01-01T00:00:00Z");
        assert_eq!(
            tracker
                .observe("chat", &[first, second.clone()], false)
                .len(),
            1
        );
        assert!(tracker.observe("chat", &[second], false).is_empty());
    }

    #[tokio::test]
    async fn shutdown_cancels_owned_tasks() {
        let (mut backend, _) = Backend::for_test();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (dropped_tx, dropped_rx) = tokio::sync::oneshot::channel();
        struct OnDrop(Option<tokio::sync::oneshot::Sender<()>>);
        impl Drop for OnDrop {
            fn drop(&mut self) {
                if let Some(tx) = self.0.take() {
                    let _ = tx.send(());
                }
            }
        }
        backend.task = Some(tokio::spawn(async move {
            let _guard = OnDrop(Some(dropped_tx));
            let _ = started_tx.send(());
            std::future::pending::<()>().await;
        }));
        started_rx.await.unwrap();
        backend.shutdown().await;
        dropped_rx.await.unwrap();
    }
    #[test]
    fn stale_startup_chat_summary_does_not_emit_recent_history() {
        let mut tracker = ActivityTracker::new();
        tracker.started_at = timestamp_millis("2026-01-01T10:00:00Z").unwrap();
        let chat = api::ChatInfo {
            id: "chat".into(),
            last_message_time: Some("2026-01-01T09:59:00Z".into()),
            last_message_id: Some("1000".into()),
            name: "Chat".into(),
            is_group: false,
            name_source: crate::api::ChatNameSource::Topic,
            last_message_sender_id: None,
            last_message_type: None,
            unread_count: None,
            has_unread: None,
            last_message_sender: None,
            last_message_preview: None,
        };
        tracker.reconcile_chats(&[chat], &mut PendingChats::default());
        assert!(tracker
            .observe("chat", &[message("1001", "2026-01-01T09:59:30Z")], false)
            .is_empty());
        assert_eq!(
            tracker
                .observe("chat", &[message("1002", "2026-01-01T10:00:01Z")], false)
                .len(),
            1
        );
    }
}
