//! Shared activity contract for live reception, unread state, and notifications.

/// A new text message observed after the backend's initial history baseline.
/// Stable service IDs allow consumers to reject duplicate deliveries and self messages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncomingMessage {
    pub chat_id: String,
    pub id: String,
    pub sender_id: String,
    pub sender: String,
    pub timestamp: String,
    pub content: String,
    /// Explicitly mentioned service identities, when supplied by Teams.
    pub mentions: Vec<String>,
}

/// Health of the push subscription, independent of Teams user presence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnectionState {
    Connecting,
    Connected,
    Reconnecting { retry_in_secs: u64 },
    Degraded(String),
}
