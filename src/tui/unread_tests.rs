//! Rendered-state tests use an ephemeral reducer and never touch account config.
use super::*;
use crate::api;
use crate::tui::unread::Badge;
use ratatui::{backend::TestBackend, Terminal};

fn incoming(id: &str) -> IncomingMessage {
    IncomingMessage {
        chat_id: "chat".into(),
        id: id.into(),
        sender_id: "8:orgid:other".into(),
        sender: "Other".into(),
        timestamp: "2026-09-18T10:00:00Z".into(),
        content: "new content".into(),
        mentions: vec![],
    }
}

fn message(id: &str, lines: usize) -> api::MessageInfo {
    let event = incoming(id);
    api::MessageInfo {
        id: event.id,
        sender_id: event.sender_id,
        sender: event.sender,
        timestamp: event.timestamp,
        content: "message line\n".repeat(lines),
        mentions: vec![],
    }
}

fn chat(id: &str) -> api::ChatInfo {
    api::ChatInfo {
        id: id.into(),
        name: format!("Chat {id}"),
        is_group: false,
        name_source: crate::api::ChatNameSource::Topic,
        last_message_id: None,
        last_message_sender_id: None,
        last_message_type: None,
        last_message_time: None,
        last_message_sender: None,
        last_message_preview: None,
        unread_count: None,
        has_unread: None,
    }
}

fn fixture() -> (App, Backend, Terminal<TestBackend>) {
    let mut app = App::new(LogBuffer::new());
    app.current_user_id = Some("self".into());
    app.current_chat_id = Some("chat".into());
    app.sidebar.update_chats(vec![chat("chat")], Some("self"));
    app.sidebar.loading = false;
    let (backend, _) = Backend::for_test();
    let terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    (app, backend, terminal)
}

fn load(app: &mut App, backend: &Backend, messages: Vec<api::MessageInfo>) {
    app.handle_backend_response(
        BackendResponse::Messages {
            chat_id: "chat".into(),
            result: Ok(messages),
        },
        backend,
    );
}

#[test]
fn selection_fetch_and_render_without_success_ack_preserve_unread() {
    let (mut app, backend, mut terminal) = fixture();
    app.handle_event(Event::FocusGained, &backend);
    app.observe_incoming(&incoming("1"));
    load(&mut app, &backend, vec![message("1", 1)]);
    assert_eq!(app.unread.badge("chat").count, 1);
    terminal.draw(|frame| app.render(frame)).unwrap();
    // The render closure alone cannot acknowledge: terminal.draw can fail later.
    assert_eq!(app.unread.badge("chat").count, 1);
    assert!(app.acknowledge_rendered());
    assert!(!app.unread.badge("chat").any());
    assert!(!app.acknowledge_rendered());
}

#[test]
fn unfocused_overlaid_and_background_views_preserve_unread() {
    let (mut app, backend, mut terminal) = fixture();
    app.observe_incoming(&incoming("1"));
    load(&mut app, &backend, vec![message("1", 1)]);
    terminal.draw(|frame| app.render(frame)).unwrap();
    app.acknowledge_rendered();
    assert_eq!(app.unread.badge("chat").count, 1);
    app.handle_event(Event::FocusGained, &backend);
    app.show_help = true;
    terminal.draw(|frame| app.render(frame)).unwrap();
    app.acknowledge_rendered();
    assert_eq!(app.unread.badge("chat").count, 1);
    app.show_help = false;
    app.search.activate();
    terminal.draw(|frame| app.render(frame)).unwrap();
    app.acknowledge_rendered();
    assert_eq!(app.unread.badge("chat").count, 1);
    app.search.deactivate();
    app.handle_event(Event::FocusLost, &backend);
    terminal.draw(|frame| app.render(frame)).unwrap();
    app.acknowledge_rendered();
    assert_eq!(app.unread.badge("chat").count, 1);
    app.current_chat_id = Some("other-chat".into());
    app.handle_event(Event::FocusGained, &backend);
    terminal.draw(|frame| app.render(frame)).unwrap();
    app.acknowledge_rendered();
    assert_eq!(app.unread.badge("chat").count, 1);
}

#[test]
fn tall_newest_message_and_scrolled_history_wait_until_bottom_is_visible() {
    let (mut app, backend, mut terminal) = fixture();
    app.handle_event(Event::FocusGained, &backend);
    app.observe_incoming(&incoming("2"));
    load(&mut app, &backend, vec![message("1", 20), message("2", 40)]);
    terminal.draw(|frame| app.render(frame)).unwrap();
    app.acknowledge_rendered();
    assert!(!app.messages.rendered_latest);
    assert_eq!(app.unread.badge("chat").count, 1);
    app.messages.viewport.follow_selection = false;
    app.messages.viewport.offset = 0;
    terminal.draw(|frame| app.render(frame)).unwrap();
    app.acknowledge_rendered();
    assert_eq!(app.unread.badge("chat").count, 1);
    app.messages.viewport.offset = usize::MAX;
    terminal.draw(|frame| app.render(frame)).unwrap();
    assert!(app.messages.rendered_latest);
    assert!(app.acknowledge_rendered());
    assert!(!app.unread.badge("chat").any());
}

#[test]
fn failed_load_and_zero_sized_pane_never_acknowledge_old_content() {
    let (mut app, backend, mut terminal) = fixture();
    app.handle_event(Event::FocusGained, &backend);
    app.observe_incoming(&incoming("1"));
    load(&mut app, &backend, vec![message("1", 1)]);
    app.handle_backend_response(
        BackendResponse::Messages {
            chat_id: "chat".into(),
            result: Err(anyhow::anyhow!("offline")),
        },
        &backend,
    );
    terminal.draw(|frame| app.render(frame)).unwrap();
    app.acknowledge_rendered();
    assert_eq!(app.unread.badge("chat").count, 1);
    load(&mut app, &backend, vec![message("1", 1)]);
    terminal.backend_mut().resize(15, 3);
    terminal.draw(|frame| app.render(frame)).unwrap();
    app.acknowledge_rendered();
    assert!(!app.messages.rendered_latest);
    assert_eq!(app.unread.badge("chat").count, 1);
}

#[test]
fn sidebar_reorder_preserves_identity_badges_collapsed_teams_and_selection() {
    let (mut app, _backend, mut terminal) = fixture();
    let teams = || {
        vec![api::TeamInfo {
            id: "team".into(),
            name: "Team One".into(),
            channels: vec![api::ChannelInfo {
                id: "channel".into(),
                name: "General".into(),
            }],
        }]
    };
    app.sidebar.update_teams(teams());
    app.sidebar.teams_expanded = true;
    app.sidebar.teams[0].expanded = false;
    app.sidebar
        .update_chats(vec![chat("other"), chat("chat")], Some("self"));
    app.sidebar.selected = app.sidebar.item_count() - 1;
    app.observe_incoming(&incoming("1"));
    let mut event = incoming("2");
    event.chat_id = "channel".into();
    app.observe_incoming(&event);
    app.sidebar
        .update_chats(vec![chat("chat"), chat("other")], Some("self"));
    app.sidebar.update_teams(teams());
    assert_eq!(app.sidebar.selected_item_id().as_deref(), Some("chat"));
    assert_eq!(app.sidebar.chats[0].unread.count, 1);
    assert_eq!(app.sidebar.teams[0].channels[0].unread.count, 1);
    assert!(!app.sidebar.teams[0].expanded);
    terminal.draw(|frame| app.render(frame)).unwrap();
    let buffer = terminal.backend().buffer();
    let row = (0..buffer.area.height)
        .map(|y| (0..22).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .find(|row| row.contains("Team One"))
        .unwrap();
    assert!(
        row.ends_with("1║"),
        "Collapsed team must show its child unread count: {row}"
    );
    assert_eq!(
        Badge {
            count: 2,
            unknown: true
        }
        .label(),
        "2+"
    );
}

#[test]
fn incoming_before_identity_load_is_deferred_and_self_is_filtered() {
    let mut app = App::new(LogBuffer::new());
    let mut own = incoming("1");
    own.sender_id = "8:orgid:self".into();
    app.observe_incoming(&own);
    app.observe_incoming(&incoming("2"));
    assert!(!app.unread.badge("chat").any());
    app.load_unread_account("unused-test", "self");
    assert_eq!(app.unread.badge("chat").count, 1);
}

fn press_u(app: &mut App, backend: &Backend) {
    app.handle_event(
        Event::Key(crossterm::event::KeyEvent::new(
            KeyCode::Char('u'),
            KeyModifiers::NONE,
        )),
        backend,
    );
}

fn select_chat(app: &mut App, id: &str) {
    app.sidebar.selected = app.sidebar.flat_items().iter().position(|item| {
        matches!(item, super::super::sidebar::SidebarItem::Chat(index) if app.sidebar.chats[*index].id == id)
    }).unwrap();
}

#[test]
fn manual_unread_waits_for_leave_reopen_and_successful_latest_render() {
    let (mut app, backend, mut terminal) = fixture();
    app.active_pane = Pane::Messages;
    load(&mut app, &backend, vec![message("1", 1)]);
    press_u(&mut app, &backend);
    for _ in 0..2 {
        terminal.draw(|frame| app.render(frame)).unwrap();
        app.acknowledge_rendered();
        assert_eq!(app.unread.badge("chat").label(), "●");
    }
    app.sidebar
        .update_chats(vec![chat("chat"), chat("other")], Some("self"));
    select_chat(&mut app, "chat");
    app.handle_sidebar_enter(&backend); // Reopening the same chat is not leaving it.
    load(&mut app, &backend, vec![message("1", 1)]);
    terminal.draw(|frame| app.render(frame)).unwrap();
    app.acknowledge_rendered();
    assert!(app.unread.badge("chat").any());
    select_chat(&mut app, "other");
    app.handle_sidebar_enter(&backend);
    select_chat(&mut app, "chat");
    app.handle_sidebar_enter(&backend);
    terminal.draw(|frame| app.render(frame)).unwrap();
    app.acknowledge_rendered();
    assert!(app.unread.badge("chat").any()); // Loading is not reading.
    load(&mut app, &backend, vec![message("1", 1)]);
    terminal.draw(|frame| app.render(frame)).unwrap();
    app.acknowledge_rendered();
    assert!(!app.unread.badge("chat").any());
    press_u(&mut app, &backend);
    press_u(&mut app, &backend);
    assert!(!app.unread.badge("chat").any());
    assert_eq!(app.status_message.as_deref(), Some("Marked read"));
}

#[test]
fn toggle_targets_focus_and_preserves_text_input_and_overlays() {
    let (mut app, backend, _) = fixture();
    app.sidebar
        .update_chats(vec![chat("chat"), chat("other")], Some("self"));
    select_chat(&mut app, "other");
    press_u(&mut app, &backend);
    assert!(app.unread.badge("other").any());
    assert!(!app.unread.badge("chat").any());
    app.active_pane = Pane::Messages;
    press_u(&mut app, &backend);
    assert!(app.unread.badge("chat").any());
    app.active_pane = Pane::Compose;
    press_u(&mut app, &backend);
    assert_eq!(app.compose.input, "u");
    app.active_pane = Pane::Messages;
    app.search.activate();
    press_u(&mut app, &backend);
    assert_eq!(app.search.query, "u");
    app.search.deactivate();
    app.show_help = true;
    press_u(&mut app, &backend);
    assert!(!app.show_help);
    for modifiers in [KeyModifiers::CONTROL, KeyModifiers::ALT] {
        app.handle_event(
            Event::Key(crossterm::event::KeyEvent::new(
                KeyCode::Char('u'),
                modifiers,
            )),
            &backend,
        );
    }
    assert!(app.unread.badge("chat").any());
    assert!(app.unread.badge("other").any());
    app.active_pane = Pane::Sidebar;
    select_chat(&mut app, "other");
    press_u(&mut app, &backend);
    assert!(!app.unread.badge("other").any());
    assert_eq!(app.current_chat_id.as_deref(), Some("chat"));
}
