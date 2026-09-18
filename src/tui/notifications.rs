//! Desktop alerts for new incoming activity. History responses never enter this module.

use std::collections::{HashSet, VecDeque};
use std::ffi::OsStr;
use std::process::Stdio;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::activity::IncomingMessage;

const RECENT_MESSAGES: usize = 4096;
const QUEUE_CAPACITY: usize = 16;
const DELIVERY_TIMEOUT: Duration = Duration::from_secs(2);
const QUEUE_MAX_AGE: Duration = Duration::from_secs(5);

#[derive(Debug, PartialEq, Eq)]
pub struct Notification {
    pub summary: String,
    pub body: String,
}

pub struct NotificationContext<'a> {
    pub current_user_id: Option<&'a str>,
    pub current_chat_id: Option<&'a str>,
    pub terminal_focused: bool,
    pub conversation_name: &'a str,
}

/// Policy is separate from delivery so tests and the render loop never start processes.
pub struct NotificationPolicy {
    enabled: bool,
    seen: HashSet<(String, String)>,
    recent: VecDeque<(String, String)>,
}

impl NotificationPolicy {
    pub fn from_env() -> Self {
        Self::new(notifications_enabled(
            std::env::var("OST_NOTIFICATIONS").ok().as_deref(),
        ))
    }

    pub(super) fn new(enabled: bool) -> Self {
        Self {
            enabled,
            seen: HashSet::new(),
            recent: VecDeque::new(),
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn prepare(
        &mut self,
        message: &IncomingMessage,
        context: NotificationContext<'_>,
    ) -> Option<Notification> {
        if !self.enabled || message.id.is_empty() || message.chat_id.is_empty() {
            return None;
        }
        let key = (message.chat_id.clone(), message.id.clone());
        if !self.seen.insert(key.clone()) {
            return None;
        }
        self.recent.push_back(key);
        if self.recent.len() > RECENT_MESSAGES {
            if let Some(oldest) = self.recent.pop_front() {
                self.seen.remove(&oldest);
            }
        }

        // Remember suppressed activity too: a later focus change must not replay it.
        let current_user = identity(context.current_user_id?);
        let sender = identity(&message.sender_id);
        if current_user.is_empty() || sender.is_empty() || same_identity(current_user, sender) {
            return None;
        }

        // The backend distinguishes initial history from new activity. This additional
        // guard avoids stale catch-up alerts after a long network interruption.
        if let Ok(sent) = DateTime::parse_from_rfc3339(&message.timestamp) {
            if Utc::now().signed_duration_since(sent).num_seconds() > 120 {
                return None;
            }
        }

        let mentioned = message
            .mentions
            .iter()
            .any(|mentioned| same_identity(current_user, identity(mentioned)));
        if !mentioned
            && context.terminal_focused
            && context.current_chat_id == Some(message.chat_id.as_str())
        {
            return None;
        }

        let sender = plain_snippet(&message.sender, 80);
        let sender = if sender.is_empty() {
            "Someone"
        } else {
            &sender
        };
        let conversation = plain_snippet(context.conversation_name, 100);
        let conversation = if conversation.is_empty() {
            "Teams conversation"
        } else {
            &conversation
        };
        let summary = if mentioned {
            format!("{sender} mentioned you in {conversation}")
        } else {
            format!("{sender} — {conversation}")
        };
        let snippet = plain_snippet(&message.content, 240);
        let snippet = if snippet.is_empty() {
            "New message"
        } else {
            &snippet
        };
        Some(Notification {
            summary,
            body: escape_markup(snippet),
        })
    }
}

fn notifications_enabled(value: Option<&str>) -> bool {
    !matches!(
        value.map(str::trim).map(str::to_ascii_lowercase).as_deref(),
        Some("0" | "off" | "false" | "no")
    )
}

/// Service sender IDs may be URL paths, MRIs, or Graph object IDs.
/// Strip only known person namespaces, retaining unknown namespaces verbatim.
fn identity(value: &str) -> &str {
    let value = value.trim().rsplit('/').next().unwrap_or_default();
    value
        .strip_prefix("8:orgid:")
        .or_else(|| value.strip_prefix("8:live:"))
        .or_else(|| value.strip_prefix("8:"))
        .unwrap_or(value)
}

fn same_identity(left: &str, right: &str) -> bool {
    !left.is_empty() && !right.is_empty() && left.eq_ignore_ascii_case(right)
}

fn plain_snippet(text: &str, max_chars: usize) -> String {
    let mut result = String::new();
    let mut count = 0;
    let mut space = false;
    for character in text.chars() {
        if character.is_whitespace() {
            space = !result.is_empty();
            continue;
        }
        if character.is_control() {
            continue;
        }
        if count >= max_chars {
            result.push('…');
            break;
        }
        if space {
            result.push(' ');
            count += 1;
            space = false;
        }
        if count >= max_chars {
            result.push('…');
            break;
        }
        result.push(character);
        count += 1;
    }
    result
}

fn escape_markup(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub(super) struct QueuedNotification {
    pub notification: Notification,
    queued_at: Instant,
}

/// One worker and a bounded queue keep subprocesses out of the TUI event loop.
pub struct NotificationService {
    tx: mpsc::Sender<QueuedNotification>,
    worker: Option<JoinHandle<()>>,
}

impl NotificationService {
    pub fn start() -> Self {
        let (tx, mut rx) = mpsc::channel::<QueuedNotification>(QUEUE_CAPACITY);
        let worker = tokio::spawn(async move {
            let mut warned = false;
            while let Some(queued) = rx.recv().await {
                // Never replay a backlog of alerts when a notification daemon stalls.
                if queued.queued_at.elapsed() > QUEUE_MAX_AGE {
                    continue;
                }
                if let Err(error) = deliver(
                    OsStr::new("notify-send"),
                    &queued.notification,
                    DELIVERY_TIMEOUT,
                )
                .await
                {
                    if !warned {
                        tracing::warn!("Desktop notifications unavailable: {error}");
                        warned = true;
                    }
                }
            }
        });
        Self {
            tx,
            worker: Some(worker),
        }
    }

    /// A captured queue for app integration tests; cannot run notify-send.
    #[cfg(test)]
    pub fn for_test() -> (Self, mpsc::Receiver<QueuedNotification>) {
        let (tx, rx) = mpsc::channel(QUEUE_CAPACITY);
        (Self { tx, worker: None }, rx)
    }

    pub fn enqueue(&self, notification: Notification) {
        if self
            .tx
            .try_send(QueuedNotification {
                notification,
                queued_at: Instant::now(),
            })
            .is_err()
        {
            tracing::debug!("Desktop notification queue full or closed; alert dropped");
        }
    }
}

impl Drop for NotificationService {
    fn drop(&mut self) {
        if let Some(worker) = &self.worker {
            worker.abort();
        }
    }
}

async fn deliver(
    executable: &OsStr,
    notification: &Notification,
    timeout: Duration,
) -> std::io::Result<()> {
    let mut child = Command::new(executable)
        .args([
            "--app-name",
            "OST",
            "--icon",
            "mail-unread",
            "--expire-time",
            "5000",
            "--",
        ])
        .arg(&notification.summary)
        .arg(&notification.body)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    match tokio::time::timeout(timeout, child.wait()).await {
        Ok(Ok(status)) if status.success() => Ok(()),
        Ok(Ok(status)) => Err(std::io::Error::other(format!(
            "notify-send exited with {status}"
        ))),
        Ok(Err(error)) => Err(error),
        Err(_) => {
            let _ = child.kill().await;
            Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "notify-send exceeded its time limit",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn incoming() -> IncomingMessage {
        IncomingMessage {
            chat_id: "chat".into(),
            id: "message".into(),
            sender_id: "8:orgid:other".into(),
            sender: "Alice".into(),
            timestamp: Utc::now().to_rfc3339(),
            content: "A new message".into(),
            mentions: Vec::new(),
        }
    }

    fn context(focused: bool) -> NotificationContext<'static> {
        NotificationContext {
            current_user_id: Some("ME"),
            current_chat_id: Some("chat"),
            terminal_focused: focused,
            conversation_name: "Research",
        }
    }

    #[test]
    fn foreground_background_and_mentions() {
        let mut policy = NotificationPolicy::new(true);
        let mut message = incoming();
        assert!(policy.prepare(&message, context(true)).is_none());
        // A duplicate initially suppressed while reading does not alert after focus loss.
        assert!(policy.prepare(&message, context(false)).is_none());
        message.id = "background".into();
        assert!(policy.prepare(&message, context(false)).is_some());
        message.id = "other-chat".into();
        message.chat_id = "another-chat".into();
        assert!(policy.prepare(&message, context(true)).is_some());
        message.id = "mention".into();
        message.chat_id = "chat".into();
        message.mentions = vec!["8:orgid:me".into()];
        let notification = policy.prepare(&message, context(true)).unwrap();
        assert_eq!(notification.summary, "Alice mentioned you in Research");
        assert_eq!(notification.body, "A new message");
    }

    #[test]
    fn own_messages_and_unknown_identity_never_alert() {
        for sender_id in [
            "ME",
            "8:orgid:me",
            "https://example.test/users/8:orgid:me",
            "",
        ] {
            let mut message = incoming();
            message.sender_id = sender_id.into();
            message.mentions = vec!["me".into()];
            assert!(NotificationPolicy::new(true)
                .prepare(&message, context(false))
                .is_none());
        }
        let mut missing_identity = context(false);
        missing_identity.current_user_id = None;
        assert!(NotificationPolicy::new(true)
            .prepare(&incoming(), missing_identity)
            .is_none());
    }

    #[test]
    fn mention_requires_exact_metadata_identity() {
        for mentions in [vec![], vec!["someone-me".into()], vec!["0".into()]] {
            let mut message = incoming();
            message.content = "@ME, can you check this?".into();
            message.mentions = mentions;
            assert!(NotificationPolicy::new(true)
                .prepare(&message, context(true))
                .is_none());
        }
    }

    #[test]
    fn repeated_deliveries_old_catchup_and_disabled_policy_are_suppressed() {
        let mut policy = NotificationPolicy::new(true);
        let mut message = incoming();
        assert!(policy.prepare(&message, context(false)).is_some());
        assert!(policy.prepare(&message, context(false)).is_none());
        message.id = "stale".into();
        message.timestamp = (Utc::now() - chrono::Duration::minutes(10)).to_rfc3339();
        assert!(policy.prepare(&message, context(false)).is_none());
        assert!(NotificationPolicy::new(false)
            .prepare(&incoming(), context(false))
            .is_none());
    }

    #[test]
    fn opt_out_accepts_common_values() {
        for value in ["0", "OFF", "false", " no "] {
            assert!(!notifications_enabled(Some(value)));
        }
        for value in [None, Some("on"), Some("1"), Some("true")] {
            assert!(notifications_enabled(value));
        }
    }

    #[test]
    fn snippets_are_bounded_unicode_text_and_markup_is_escaped() {
        let mut message = incoming();
        message.sender = "--urgency=critical".into();
        message.content = "<b>Hello</b> & \n世界\0\u{1b}".into();
        let notification = NotificationPolicy::new(true)
            .prepare(&message, context(false))
            .unwrap();
        assert_eq!(notification.summary, "--urgency=critical — Research");
        assert_eq!(notification.body, "&lt;b&gt;Hello&lt;/b&gt; &amp; 世界");
        assert_eq!(plain_snippet("é世界abc", 3), "é世界…");
        assert_eq!(plain_snippet("ab cdef", 3), "ab …");
    }

    #[test]
    fn burst_queue_is_bounded_and_enqueue_never_waits() {
        let (service, mut captured) = NotificationService::for_test();
        for index in 0..QUEUE_CAPACITY + 5 {
            service.enqueue(Notification {
                summary: format!("{index}"),
                body: String::new(),
            });
        }
        for index in 0..QUEUE_CAPACITY {
            assert_eq!(
                captured.try_recv().unwrap().notification.summary,
                format!("{index}")
            );
        }
        assert!(captured.try_recv().is_err());
    }

    #[cfg(unix)]
    struct FakeExecutable {
        directory: std::path::PathBuf,
        executable: std::path::PathBuf,
    }

    #[cfg(unix)]
    impl FakeExecutable {
        fn new(body: &str) -> Self {
            use std::os::unix::fs::PermissionsExt;
            let directory =
                std::env::temp_dir().join(format!("ost-notify-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&directory).unwrap();
            let executable = directory.join("notify-send");
            std::fs::write(&executable, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
            Self {
                directory,
                executable,
            }
        }
    }

    #[cfg(unix)]
    impl Drop for FakeExecutable {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.directory);
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn executable_receives_literal_positional_arguments() {
        let fake = FakeExecutable::new("printf '%s\\000' \"$@\" > \"$0.args\"");
        let notification = Notification {
            summary: "--help $(touch /tmp/never-execute) `false` \"sender\"".into(),
            body: "&lt;b&gt;hello &amp; goodbye&lt;/b&gt;\nsecond line".into(),
        };
        deliver(fake.executable.as_os_str(), &notification, DELIVERY_TIMEOUT)
            .await
            .unwrap();
        let output = std::fs::read(fake.directory.join("notify-send.args")).unwrap();
        let arguments: Vec<&[u8]> = output.split(|byte| *byte == 0).collect();
        assert_eq!(arguments.len(), 10);
        assert_eq!(arguments[0], b"--app-name");
        assert_eq!(arguments[1], b"OST");
        assert_eq!(arguments[6], b"--");
        assert_eq!(arguments[7], notification.summary.as_bytes());
        assert_eq!(arguments[8], notification.body.as_bytes());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn absent_failing_and_hung_helpers_are_bounded_errors() {
        let notification = Notification {
            summary: "test".into(),
            body: "test".into(),
        };
        let absent = std::env::temp_dir().join(format!("no-ost-notify-{}", uuid::Uuid::new_v4()));
        let error = deliver(absent.as_os_str(), &notification, DELIVERY_TIMEOUT)
            .await
            .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
        let failing = FakeExecutable::new("exit 1");
        assert!(deliver(
            failing.executable.as_os_str(),
            &notification,
            DELIVERY_TIMEOUT
        )
        .await
        .is_err());
        let hung = FakeExecutable::new("exec sleep 30");
        let began = Instant::now();
        let error = deliver(
            hung.executable.as_os_str(),
            &notification,
            Duration::from_millis(50),
        )
        .await
        .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        assert!(began.elapsed() < Duration::from_secs(2));
    }
}
