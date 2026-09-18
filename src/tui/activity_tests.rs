//! Cross-feature regressions for live reception, unread state, and desktop alerts.

use super::*;
use crate::api::{ChatInfo, MessageInfo};
use crate::tui::activity::{ConnectionState, IncomingMessage};
use crate::tui::notifications::{NotificationService, QueuedNotification};
use ratatui::{backend::TestBackend, Terminal};
use tokio::sync::mpsc::Receiver;

fn message(chat_id: &str, id: &str) -> IncomingMessage {
    IncomingMessage {
        chat_id: chat_id.into(),
        id: id.into(),
        sender_id: "https://chat/v1/users/ME/contacts/8:orgid:other".into(),
        sender: "Colleague".into(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        content: format!("New message {id}"),
        mentions: Vec::new(),
    }
}

fn history(message: &IncomingMessage) -> MessageInfo {
    MessageInfo {
        id: message.id.clone(),
        sender_id: message.sender_id.clone(),
        sender: message.sender.clone(),
        timestamp: message.timestamp.clone(),
        content: message.content.clone(),
        mentions: message.mentions.clone(),
    }
}

fn chat(id: &str) -> ChatInfo {
    ChatInfo {
        id: id.into(),
        name: format!("Conversation {id}"),
        is_group: false,
        last_message_id: None,
        last_message_sender_id: None,
        last_message_type: None,
        unread_count: None,
        has_unread: None,
        last_message_time: None,
        last_message_sender: None,
        last_message_preview: None,
    }
}

fn setup() -> (App, Backend, Receiver<QueuedNotification>) {
    let (backend, _) = Backend::for_test();
    let mut app = App::new(LogBuffer::new());
    app.current_user_id = Some("me".into());
    app.user_name = "Me".into();
    app.terminal_focused = true;
    let (service, capture) = NotificationService::for_test();
    app.notification_service = Some(service);
    app.handle_backend_response(
        BackendResponse::Chats(Ok(vec![chat("active"), chat("background")])),
        &backend,
    );
    app.current_chat_id = Some("active".into());
    app.messages.channel_header = "Conversation active".into();
    (app, backend, capture)
}

fn draw(app: &mut App, terminal: &mut Terminal<TestBackend>) {
    terminal.draw(|frame| app.render(frame)).unwrap();
    if app.acknowledge_rendered() {
        terminal.draw(|frame| app.render(frame)).unwrap();
    }
}

#[test]
fn delayed_identity_replays_incoming_activity_once_and_suppresses_own_messages() {
    let (mut app, backend, mut alerts) = setup();
    app.current_user_id = None;
    let incoming = message("background", "deferred-1");
    let mut own = message("background", "deferred-own");
    own.sender_id = "8:orgid:me".into();
    for event in [incoming.clone(), incoming, own] {
        app.handle_backend_response(BackendResponse::IncomingMessage(event), &backend);
    }
    assert!(alerts.try_recv().is_err());
    assert!(!app.unread.badge("background").any());
    app.handle_backend_response(
        BackendResponse::UserInfo(Ok(crate::api::UserInfo {
            display_name: "Me".into(),
            id: "me".into(),
            mail: None,
        })),
        &backend,
    );
    assert_eq!(app.unread.badge("background").count, 1);
    assert!(alerts.try_recv().is_ok());
    assert!(alerts.try_recv().is_err());
    assert!(app.deferred_incoming.is_empty());
}

#[test]
fn background_activity_sets_badge_and_alert_without_disturbing_a_draft() {
    let (mut app, backend, mut alerts) = setup();
    app.active_pane = Pane::Compose;
    app.compose.input = "An unfinished reply".into();
    app.compose.cursor_pos = 4;
    let selected = app.sidebar.selected;
    let incoming = message("background", "new-1");
    app.handle_backend_response(BackendResponse::IncomingMessage(incoming.clone()), &backend);

    assert_eq!(app.unread.badge("background").count, 1);
    assert!(alerts
        .try_recv()
        .unwrap()
        .notification
        .summary
        .contains("Conversation background"));
    app.handle_backend_response(BackendResponse::IncomingMessage(incoming), &backend);
    assert_eq!(app.unread.badge("background").count, 1);
    assert!(alerts.try_recv().is_err());
    assert_eq!(app.compose.input, "An unfinished reply");
    assert_eq!(app.compose.cursor_pos, 4);
    assert_eq!(app.active_pane, Pane::Compose);
    assert_eq!(app.sidebar.selected, selected);

    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    draw(&mut app, &mut terminal);
    assert!(app.unread.badge("background").any());
}

#[test]
fn unfocused_arrival_stays_unread_until_a_focused_successful_draw() {
    let (mut app, backend, mut alerts) = setup();
    app.handle_event(Event::FocusLost, &backend);
    let incoming = message("active", "new-2");
    app.handle_backend_response(BackendResponse::IncomingMessage(incoming.clone()), &backend);
    app.handle_backend_response(
        BackendResponse::Messages {
            chat_id: "active".into(),
            result: Ok(vec![history(&incoming)]),
        },
        &backend,
    );
    assert!(alerts.try_recv().is_ok());
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    draw(&mut app, &mut terminal);
    assert!(app.unread.badge("active").any());
    app.handle_event(Event::FocusGained, &backend);
    assert!(app.unread.badge("active").any());
    draw(&mut app, &mut terminal);
    assert!(!app.unread.badge("active").any());

    app.handle_backend_response(BackendResponse::IncomingMessage(incoming), &backend);
    assert!(!app.unread.badge("active").any());
    assert!(alerts.try_recv().is_err());
}

#[test]
fn presence_cannot_hide_degraded_reception_and_own_activity_is_silent() {
    let (mut app, backend, mut alerts) = setup();
    app.handle_backend_response(
        BackendResponse::ConnectionState(ConnectionState::Reconnecting { retry_in_secs: 5 }),
        &backend,
    );
    let connection_status = app.connection_state.clone();
    app.handle_backend_response(
        BackendResponse::Presence(Err(anyhow::anyhow!("presence unavailable"))),
        &backend,
    );
    assert_eq!(app.connection_state, connection_status);
    let mut own = message("background", "own-1");
    own.sender_id = "https://chat/v1/users/ME/contacts/8:orgid:me".into();
    app.handle_backend_response(BackendResponse::IncomingMessage(own), &backend);
    assert!(!app.unread.badge("background").any());
    assert!(alerts.try_recv().is_err());
}
