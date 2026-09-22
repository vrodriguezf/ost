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
        name_source: crate::api::ChatNameSource::Topic,
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
fn own_message_rendering_recovers_after_delayed_identity_and_history_refresh() {
    let (mut app, backend, _) = setup();
    app.current_user_id = None;
    app.user_name = "Loading...".into();
    let mut own = message("active", "1");
    own.sender_id = "https://chat/contacts/8:orgid:ME".into();
    own.sender.clear();
    own.content = "Unique reply body".into();
    app.handle_backend_response(
        BackendResponse::Messages {
            chat_id: "active".into(),
            result: Ok(vec![history(&own)]),
        },
        &backend,
    );
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    let body_position = |terminal: &Terminal<TestBackend>| {
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .find_map(|y| {
                let row: String = (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect();
                row.find("Unique reply body").map(|x| (x, y))
            })
            .expect("reply body is visible")
    };
    draw(&mut app, &mut terminal);
    let before = body_position(&terminal);
    app.handle_backend_response(
        BackendResponse::UserInfo(Ok(crate::api::UserInfo {
            id: "me".into(),
            display_name: "My Name".into(),
            mail: None,
        })),
        &backend,
    );
    draw(&mut app, &mut terminal);
    let after = body_position(&terminal);
    assert!(
        after.0 > before.0,
        "own message should now be right-aligned"
    );
    let buffer = terminal.backend().buffer();
    let header: String = (0..buffer.area.width)
        .map(|x| buffer[(x, after.1 - 1)].symbol())
        .collect();
    assert!(header.contains("My Name"));
    let rendered = buffer.clone();
    let selected = app.messages.selected;
    let offset = app.messages.viewport.offset;

    // The native history can continue to omit the display name after sending.
    own.sender = "?".into();
    app.handle_backend_response(
        BackendResponse::Messages {
            chat_id: "active".into(),
            result: Ok(vec![history(&own)]),
        },
        &backend,
    );
    draw(&mut app, &mut terminal);
    assert_eq!(*terminal.backend().buffer(), rendered);
    assert_eq!(app.messages.selected, selected);
    assert_eq!(app.messages.viewport.offset, offset);
}

#[test]
fn chat_refresh_after_a_reply_never_replaces_a_known_name_with_an_id() {
    let (mut app, backend, _) = setup();
    let mut named = chat("active");
    named.name = "Colleague".into();
    app.handle_backend_response(BackendResponse::Chats(Ok(vec![named])), &backend);

    // Native replies can lack both a topic and lastMessage.imdisplayname.
    let mut reply_snapshot = chat("active");
    reply_snapshot.name = reply_snapshot.id.clone();
    reply_snapshot.name_source = crate::api::ChatNameSource::Identifier;
    reply_snapshot.last_message_sender_id = Some("8:orgid:me".into());
    app.handle_backend_response(BackendResponse::Chats(Ok(vec![reply_snapshot])), &backend);
    assert_eq!(app.sidebar.chats[0].name, "Colleague");
}

#[test]
fn sender_hints_keep_names_stable_and_real_renames_update_every_label() {
    let (mut app, backend, mut alerts) = setup();
    app.compose.input = "Keep this draft".into();
    app.messages.viewport.follow_selection = false;
    app.messages.viewport.offset = 3;
    app.search.activate();
    app.search.query = "Conversation".into();

    let mut sender_hint = chat("active");
    sender_hint.name = "My own name".into();
    sender_hint.name_source = crate::api::ChatNameSource::LastSender;
    sender_hint.last_message_sender_id = Some("8:orgid:me".into());
    app.handle_backend_response(
        BackendResponse::Chats(Ok(vec![chat("background"), sender_hint])),
        &backend,
    );
    assert_eq!(app.sidebar.selected_item_id().as_deref(), Some("active"));
    assert_eq!(app.sidebar.chats[1].name, "Conversation active");
    assert_eq!(app.channel_name, "Conversation active");
    assert_eq!(app.messages.channel_header, "Conversation active");
    assert!(app.search.active);
    assert_eq!(app.compose.input, "Keep this draft");
    assert_eq!(app.messages.viewport.offset, 3);

    let mut renamed = chat("active");
    renamed.name = "Project room".into();
    app.handle_backend_response(BackendResponse::Chats(Ok(vec![renamed])), &backend);
    assert_eq!(app.sidebar.chats[0].name, "Project room");
    assert_eq!(app.channel_name, "Project room");
    assert_eq!(app.messages.channel_header, "Project room");
    assert_eq!(app.messages.viewport.offset, 3);
    app.terminal_focused = false;
    app.handle_backend_response(
        BackendResponse::IncomingMessage(message("active", "after-rename")),
        &backend,
    );
    assert!(alerts
        .try_recv()
        .unwrap()
        .notification
        .summary
        .contains("Project room"));
    assert_eq!(app.unread.badge("active").count, 1);
}

#[test]
fn history_recovers_an_unresolved_name_from_a_peer_without_renaming_known_chats() {
    let (mut app, backend, mut alerts) = setup();
    let mut unresolved = chat("active");
    unresolved.name = "My own name".into();
    unresolved.name_source = crate::api::ChatNameSource::LastSender;
    unresolved.last_message_sender_id = Some("8:orgid:me".into());
    app.sidebar.chats.clear();
    app.handle_backend_response(BackendResponse::Chats(Ok(vec![unresolved])), &backend);
    assert_eq!(app.sidebar.chats[0].name, "active");

    let peer = message("active", "1");
    let mut own = message("active", "2");
    own.sender = "Me".into();
    own.sender_id = "8:orgid:me".into();
    app.handle_backend_response(
        BackendResponse::Messages {
            chat_id: "active".into(),
            result: Ok(vec![history(&peer), history(&own)]),
        },
        &backend,
    );
    assert_eq!(app.sidebar.chats[0].name, "Colleague");
    assert_eq!(app.channel_name, "Colleague");
    assert_eq!(app.messages.channel_header, "Colleague");
    assert!(
        alerts.try_recv().is_err(),
        "History must not trigger an alert"
    );

    let mut another = message("active", "3");
    another.sender = "Another participant".into();
    app.handle_backend_response(
        BackendResponse::Messages {
            chat_id: "active".into(),
            result: Ok(vec![history(&another)]),
        },
        &backend,
    );
    assert_eq!(app.sidebar.chats[0].name, "Colleague");
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
