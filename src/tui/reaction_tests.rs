//! Offline tests for reaction targets, modal interaction, and state preservation.

use super::*;
use crossterm::event::KeyEvent;
use ratatui::{backend::TestBackend, Terminal};

fn message(id: &str) -> api::MessageInfo {
    api::MessageInfo {
        id: id.into(),
        sender_id: "other".into(),
        sender: "Other".into(),
        timestamp: "2026-09-18T10:00:00Z".into(),
        content: format!("Message {id}"),
        mentions: vec![],
        content_blocks: Vec::new(),
        reactions: vec![],
    }
}

fn reaction(key: &str, users: &[&str]) -> api::Reaction {
    api::Reaction {
        key: key.into(),
        count: users.len() as u32,
        users: users.iter().map(|s| (*s).into()).collect(),
    }
}

fn app() -> App {
    let mut app = App::new(LogBuffer::new());
    app.current_user_id = Some("me".into());
    app.current_chat_id = Some("chat".into());
    app.active_pane = Pane::Messages;
    app.messages
        .update_messages("Chat", (1..60).map(|id| message(&id.to_string())).collect());
    app.messages.selected = 2;
    app.messages.viewport.follow_bottom = false;
    app.messages.viewport.follow_selection = false;
    app.messages.viewport.offset = 5;
    app.compose.input = "unfinished draft".into();
    app
}

fn key(app: &mut App, backend: &Backend, code: KeyCode) {
    app.handle_event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)), backend);
}

#[test]
fn reaction_picker_add_remove_cancel_and_custom_ownership() {
    let (backend, mut commands) = Backend::for_test();
    let mut app = app();
    key(&mut app, &backend, KeyCode::Char('r'));
    key(&mut app, &backend, KeyCode::Down);
    key(&mut app, &backend, KeyCode::Esc);
    assert!(app.reaction_picker.is_none());
    assert!(commands.try_recv().is_err());
    app.messages.messages[2].reactions = vec![
        reaction("like", &["other"]),
        reaction("custom", &["8:orgid:ME"]),
    ];
    key(&mut app, &backend, KeyCode::Char('r'));
    assert_eq!(
        app.reaction_picker
            .as_ref()
            .unwrap()
            .choices
            .last()
            .unwrap(),
        "custom"
    );
    key(&mut app, &backend, KeyCode::Enter);
    assert!(
        matches!(commands.try_recv().unwrap(), BackendCommand::ChangeReaction { chat_id, message_id, key, remove: false } if chat_id == "chat" && message_id == "3" && key == "like")
    );
    key(&mut app, &backend, KeyCode::Enter);
    assert!(commands.try_recv().is_err());
    app.handle_backend_response(
        BackendResponse::ReactionChanged {
            chat_id: "chat".into(),
            message_id: "3".into(),
            result: Ok(Some(vec![
                reaction("like", &["other", "me"]),
                reaction("custom", &["me"]),
            ])),
        },
        &backend,
    );
    key(&mut app, &backend, KeyCode::Char('r'));
    key(&mut app, &backend, KeyCode::Enter);
    assert!(
        matches!(commands.try_recv().unwrap(), BackendCommand::ChangeReaction { key, remove: true, .. } if key == "like")
    );
    assert_eq!(app.messages.messages[2].reactions[1].key, "custom");
}

#[test]
fn reaction_picker_rejects_invalid_stale_and_modified_targets() {
    let (backend, mut commands) = Backend::for_test();
    for scenario in 0..6 {
        let mut app = app();
        match scenario {
            0 => app.current_chat_id = None,
            1 => app.messages.loading = true,
            2 => app.messages.selected = 999,
            3 => app.messages.messages[2].native_id = None,
            4 => app.current_user_id = None,
            _ => app.active_pane = Pane::Sidebar,
        }
        key(&mut app, &backend, KeyCode::Char('r'));
        assert!(app.reaction_picker.is_none());
    }
    let mut app = app();
    for modifiers in [
        KeyModifiers::CONTROL,
        KeyModifiers::ALT,
        KeyModifiers::SHIFT,
    ] {
        app.handle_event(
            Event::Key(KeyEvent::new(KeyCode::Char('r'), modifiers)),
            &backend,
        );
        assert!(app.reaction_picker.is_none());
    }
    app.handle_event(
        Event::Key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE)),
        &backend,
    );
    assert!(app.reaction_picker.is_some());
    app.current_chat_id = Some("different".into());
    key(&mut app, &backend, KeyCode::Enter);
    assert!(app.reaction_picker.is_none());
    assert!(commands.try_recv().is_err());
}

#[test]
fn reaction_result_preserves_old_history_draft_selection_and_unread() {
    let (backend, mut commands) = Backend::for_test();
    let mut app = app();
    let unread = app.unread.badge("chat");
    key(&mut app, &backend, KeyCode::Char('r'));
    key(&mut app, &backend, KeyCode::Enter);
    commands.try_recv().unwrap();
    app.handle_backend_response(
        BackendResponse::ReactionChanged {
            chat_id: "chat".into(),
            message_id: "3".into(),
            result: Ok(Some(vec![reaction("heart", &["me", "other"])])),
        },
        &backend,
    );
    assert_eq!(app.messages.messages.len(), 59);
    assert_eq!(app.messages.messages[2].reactions[0].count, 2);
    assert_eq!(app.messages.selected, 2);
    assert_eq!(app.messages.viewport.offset, 5);
    assert_eq!(app.compose.input, "unfinished draft");
    assert_eq!(app.current_chat_id.as_deref(), Some("chat"));
    assert_eq!(app.unread.badge("chat"), unread);
    assert!(commands.try_recv().is_err());
    app.current_chat_id = Some("elsewhere".into());
    app.handle_backend_response(
        BackendResponse::ReactionChanged {
            chat_id: "chat".into(),
            message_id: "3".into(),
            result: Ok(Some(vec![])),
        },
        &backend,
    );
    assert_eq!(app.current_chat_id.as_deref(), Some("elsewhere"));
    assert_eq!(app.messages.messages[2].reactions[0].count, 2);
}

#[test]
fn reaction_errors_unlock_picker_and_pending_escape_does_not_resend() {
    let (backend, mut commands) = Backend::for_test();
    let mut app = app();
    key(&mut app, &backend, KeyCode::Char('r'));
    key(&mut app, &backend, KeyCode::Enter);
    commands.try_recv().unwrap();
    app.handle_backend_response(
        BackendResponse::ReactionChanged {
            chat_id: "chat".into(),
            message_id: "3".into(),
            result: Err(anyhow::anyhow!("403 Forbidden")),
        },
        &backend,
    );
    assert!(!app.reaction_picker.as_ref().unwrap().pending);
    assert!(app
        .reaction_picker
        .as_ref()
        .unwrap()
        .error
        .as_ref()
        .unwrap()
        .contains("403"));
    key(&mut app, &backend, KeyCode::Enter);
    commands.try_recv().unwrap();
    key(&mut app, &backend, KeyCode::Esc);
    key(&mut app, &backend, KeyCode::Char('r'));
    assert!(app.reaction_picker.is_none());
    assert!(commands.try_recv().is_err());
    app.handle_backend_response(
        BackendResponse::ReactionChanged {
            chat_id: "chat".into(),
            message_id: "3".into(),
            result: Ok(None),
        },
        &backend,
    );
    assert!(app
        .status_message
        .as_ref()
        .unwrap()
        .contains("updated, but refresh failed"));
    key(&mut app, &backend, KeyCode::Char('r'));
    key(&mut app, &backend, KeyCode::Enter);
    commands.try_recv().unwrap();
    app.handle_backend_response(BackendResponse::ClientError("expired".into()), &backend);
    assert!(!app.reaction_picker.as_ref().unwrap().pending);
    assert!(app.pending_reaction.is_none());
}

#[test]
fn reaction_popup_clears_underlying_hyperlinks_and_restores_them_on_close() {
    let (backend, _) = Backend::for_test();
    let mut app = app();
    for message in &mut app.messages.messages {
        message.content = "https://example.com/message".into();
    }
    let mut terminal = Terminal::new(TestBackend::new(90, 24)).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
    assert!(!app.messages.links.is_empty());

    key(&mut app, &backend, KeyCode::Char('r'));
    terminal.draw(|frame| app.render(frame)).unwrap();
    assert!(app.reaction_picker.is_some());
    assert!(app.messages.links.is_empty());

    key(&mut app, &backend, KeyCode::Esc);
    terminal.draw(|frame| app.render(frame)).unwrap();
    assert!(!app.messages.links.is_empty());
}

#[test]
fn reaction_modal_blocks_mouse_and_read_acknowledgement_and_renders_ownership() {
    let (backend, mut commands) = Backend::for_test();
    let mut app = app();
    app.messages.messages[2].reactions = vec![reaction("like", &["me"])];
    app.messages.rendered_latest = true;
    app.loaded_last_stamp = Some(MessageStamp::new("59", "2026-09-18T10:00:00Z"));
    app.rendered_chat_id = Some("chat".into());
    app.terminal_focused = true;
    assert!(app.is_reading_latest("chat"));
    key(&mut app, &backend, KeyCode::Char('r'));
    assert!(!app.is_reading_latest("chat"));
    let mut terminal = Terminal::new(TestBackend::new(90, 24)).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(text.contains("Remove like"));
    assert!(text.contains("Enter: apply"));
    app.handle_event(
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 1,
            row: 4,
            modifiers: KeyModifiers::NONE,
        }),
        &backend,
    );
    assert_eq!(app.current_chat_id.as_deref(), Some("chat"));
    assert_eq!(app.active_pane, Pane::Messages);
    assert!(commands.try_recv().is_err());
}
