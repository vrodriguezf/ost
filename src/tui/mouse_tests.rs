//! Offline interaction tests click rendered text rather than internal hit regions.

use super::*;
use crate::api;
use crate::tui::{
    messages::Message,
    sidebar::{Channel, Chat, Team},
};
use crossterm::event::KeyEvent;
use ratatui::{backend::TestBackend, buffer::Buffer, layout::Rect, Terminal};
use tokio::sync::mpsc::UnboundedReceiver;
use unicode_width::UnicodeWidthStr;

struct Harness {
    app: App,
    backend: Backend,
    commands: UnboundedReceiver<BackendCommand>,
    terminal: Terminal<TestBackend>,
}

impl Harness {
    fn new() -> Self {
        let (backend, commands) = Backend::for_test();
        let mut app = App::new(LogBuffer::new());
        app.user_name = "Me".into();
        app.sidebar.loading = false;
        Self {
            app,
            backend,
            commands,
            terminal: Terminal::new(TestBackend::new(100, 24)).unwrap(),
        }
    }

    fn draw(&mut self) {
        self.terminal.draw(|frame| self.app.render(frame)).unwrap();
    }

    fn event(&mut self, event: Event) {
        self.app.handle_event(event, &self.backend);
        self.draw();
    }

    fn mouse(&mut self, kind: MouseEventKind, x: u16, y: u16) {
        self.event(Event::Mouse(MouseEvent {
            kind,
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        }));
    }

    fn click(&mut self, x: u16, y: u16) {
        self.mouse(MouseEventKind::Down(MouseButton::Left), x, y);
    }

    fn key(&mut self, code: KeyCode) {
        self.event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    }

    fn text_position(&self, text: &str) -> (u16, u16) {
        let buffer = self.terminal.backend().buffer();
        for y in 0..buffer.area.height {
            let row = row_text(buffer, y);
            if let Some(start) = row.find(text) {
                return (row[..start].width() as u16, y);
            }
        }
        panic!(
            "text {text:?} missing from screen: {:?}",
            self.terminal.backend().buffer()
        );
    }

    fn click_text(&mut self, text: &str) {
        let (x, y) = self.text_position(text);
        self.click(x, y);
    }

    fn loaded_chat(&mut self) -> String {
        match self.commands.try_recv().expect("a load command") {
            BackendCommand::LoadMessages { chat_id, limit } => {
                assert_eq!(limit, 50);
                chat_id
            }
            _ => panic!("unexpected backend command"),
        }
    }

    fn add_chats(&mut self, count: usize) {
        self.app.sidebar.chats = (0..count)
            .map(|i| Chat {
                name: format!("Chat {i:02}"),
                id: format!("chat-{i}"),
                is_group: false,
                name_source: crate::api::ChatNameSource::Topic,
                unread: Default::default(),
                online: false,
            })
            .collect();
    }

    fn add_team(&mut self) {
        self.app.sidebar.teams.push(Team {
            name: "Team One".into(),
            id: "team-1".into(),
            expanded: true,
            channels: vec![Channel {
                name: "Channel One".into(),
                id: "channel-1".into(),
                unread: Default::default(),
            }],
        });
    }

    fn add_messages(&mut self, count: usize) {
        self.app.messages.messages = (0..count)
            .map(|i| message(&format!("Message {i:02}")))
            .collect();
        self.app.messages.expanded_threads = vec![true; count];
    }
}

fn row_text(buffer: &Buffer, y: u16) -> String {
    // Skip continuation cells belonging to wide characters.
    let mut result = String::new();
    let mut x = 0;
    while x < buffer.area.width {
        let symbol = buffer[(x, y)].symbol();
        result.push_str(symbol);
        x += (symbol.width() as u16).max(1);
    }
    result
}

fn message(content: &str) -> Message {
    Message {
        id: content.into(),
        sender_id: "other".into(),
        sender: "Sender".into(),
        timestamp: "12:34".into(),
        content: content.into(),
        reactions: vec![],
        reply_count: 0,
        replies: vec![],
        attachments: vec![],
    }
}

#[test]
fn sidebar_clicks_expand_teams_and_open_channels_and_chats() {
    let mut h = Harness::new();
    h.add_team();
    h.add_chats(2);
    h.draw();
    h.click_text("Team One");
    assert!(!h.app.sidebar.teams[0].expanded);
    assert!(h.commands.try_recv().is_err());
    h.click_text("Team One");
    assert!(h.app.sidebar.teams[0].expanded);
    h.click_text("Channel One");
    assert_eq!(h.loaded_chat(), "channel-1");
    h.click_text("Chat 01");
    assert_eq!(h.loaded_chat(), "chat-1");
    assert_eq!(h.app.current_chat_id.as_deref(), Some("chat-1"));
    assert!(h.app.messages.loading);
}

#[test]
fn sidebar_wheel_does_not_activate_or_snap_back_and_click_uses_visible_row() {
    let mut h = Harness::new();
    h.add_chats(50);
    h.app.active_pane = Pane::Compose;
    h.draw();
    h.mouse(MouseEventKind::ScrollDown, 3, 6);
    assert_eq!(h.app.sidebar.viewport.offset, 3);
    assert_eq!(h.app.active_pane, Pane::Compose);
    h.draw();
    assert_eq!(h.app.sidebar.viewport.offset, 3);
    assert!(h.commands.try_recv().is_err());
    h.click_text("Chat 10");
    assert_eq!(h.loaded_chat(), "chat-10");
    for _ in 0..30 {
        h.mouse(MouseEventKind::ScrollDown, 3, 6);
    }
    h.click_text("Chat 49");
    assert_eq!(h.loaded_chat(), "chat-49");
    for _ in 0..30 {
        h.mouse(MouseEventKind::ScrollUp, 3, 6);
    }
    assert_eq!(h.app.sidebar.viewport.offset, 0);
    h.key(KeyCode::Up);
    h.text_position("Chat 48"); // Keyboard navigation reveals the selection again.
}

#[test]
fn clicks_after_backend_reorder_open_the_displayed_chat() {
    let mut h = Harness::new();
    h.add_chats(40);
    h.draw();
    h.mouse(MouseEventKind::ScrollDown, 3, 6);
    let chats = (0..40)
        .rev()
        .map(|i| api::ChatInfo {
            id: format!("chat-{i}"),
            name: format!("Chat {i:02}"),
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
        })
        .collect();
    h.app
        .handle_backend_response(BackendResponse::Chats(Ok(chats)), &h.backend);
    // Until the updated list is drawn, old click targets are invalid.
    assert_eq!(h.app.mouse.at(3, 6), None);
    h.draw();
    h.click_text("Chat 30");
    assert_eq!(h.loaded_chat(), "chat-30");
}

#[test]
fn headers_borders_blank_space_and_unsupported_mouse_events_do_not_open_chats() {
    let mut h = Harness::new();
    h.add_chats(1);
    h.draw();
    h.click_text("TEAMS");
    h.click_text("CHATS");
    h.click(0, 5);
    h.click(3, 15);
    h.click(u16::MAX, u16::MAX);
    let (x, y) = h.text_position("Chat 00");
    for kind in [
        MouseEventKind::Down(MouseButton::Right),
        MouseEventKind::Down(MouseButton::Middle),
        MouseEventKind::Up(MouseButton::Left),
        MouseEventKind::Drag(MouseButton::Left),
        MouseEventKind::Moved,
    ] {
        h.mouse(kind, x, y);
    }
    h.event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x,
        row: y,
        modifiers: KeyModifiers::SHIFT,
    }));
    assert!(h.commands.try_recv().is_err());
    h.app.sidebar.chats.clear();
    h.app.sidebar.loading = true;
    h.draw();
    h.click_text("Loading...");
    assert!(h.commands.try_recv().is_err());
}

#[test]
fn messages_scroll_by_lines_and_tall_messages_do_not_snap_back() {
    let mut h = Harness::new();
    let content = (0..70)
        .map(|i| format!("Line {i:02}\n"))
        .collect::<String>();
    h.app.messages.messages = vec![message(&content)];
    h.app.messages.expanded_threads = vec![true];
    h.draw();
    h.mouse(MouseEventKind::ScrollDown, 35, 8);
    assert_eq!(h.app.messages.viewport.offset, 3);
    h.text_position("Line 02");
    h.click_text("Line 05");
    h.draw();
    assert_eq!(h.app.messages.viewport.offset, 3);
    h.mouse(MouseEventKind::ScrollDown, 35, 8);
    assert_eq!(h.app.messages.viewport.offset, 6);
    for _ in 0..40 {
        h.mouse(MouseEventKind::ScrollDown, 35, 8);
    }
    h.text_position("Line 69");
    let at_bottom = h.app.messages.viewport.offset;
    h.mouse(MouseEventKind::ScrollDown, 35, 8);
    assert_eq!(h.app.messages.viewport.offset, at_bottom);
    h.key(KeyCode::Up);
    assert_eq!(h.app.messages.viewport.offset, 0);
    h.text_position("Line 00");
}

#[test]
fn wrapped_messages_replies_and_blank_separator_have_correct_click_targets() {
    let mut h = Harness::new();
    h.add_messages(3);
    h.app.messages.messages[0].content = "wrapped content ".repeat(14);
    h.app.messages.messages[0].replies = vec![message("Reply body")];
    h.app.messages.messages[0].reply_count = 1;
    h.draw();
    h.click_text("Reply body");
    assert_eq!(h.app.messages.selected, 0);
    assert!(h.app.messages.expanded_threads[0]);
    h.click_text("Reply body");
    assert!(!h.app.messages.expanded_threads[0]);
    h.click_text("replies (Enter to expand)");
    assert!(h.app.messages.expanded_threads[0]);
    h.click_text("Message 01");
    assert_eq!(h.app.messages.selected, 1);
    let (x, y) = h.text_position("Reply body");
    h.click(x, y + 1); // Blank separator after the first thread.
    assert_eq!(h.app.messages.selected, 1);
}

#[test]
fn compose_click_places_cursor_in_unicode_text_and_preserves_keyboard_editing() {
    let mut h = Harness::new();
    h.app.compose.input = "a界e\u{301}🙂z".into();
    h.app.compose.move_end();
    h.draw();
    let (x, y) = h.text_position("a界e\u{301}🙂z");
    h.click(x + 2, y); // Second terminal cell of the wide CJK character.
    assert_eq!(h.app.active_pane, Pane::Compose);
    assert_eq!(h.app.compose.cursor_pos, 1);
    h.key(KeyCode::Char('X'));
    assert_eq!(h.app.compose.input, "aX界e\u{301}🙂z");
    let (x, y) = h.text_position("aX界e\u{301}🙂z");
    h.click(x + 5, y); // After e + combining accent, before emoji.
    assert_eq!(h.app.compose.cursor_pos, 5);
    h.key(KeyCode::Char('Y'));
    assert_eq!(h.app.compose.input, "aX界e\u{301}Y🙂z");
    h.key(KeyCode::Tab);
    assert_eq!(h.app.active_pane, Pane::Sidebar);
}

#[test]
fn compose_click_uses_horizontal_scroll_and_newline_display_columns() {
    let mut h = Harness::new();
    h.app.compose.input = format!("{}\nEND", "a".repeat(100));
    h.app.compose.move_end();
    h.draw();
    h.click_text("END");
    assert_eq!(h.app.compose.cursor_pos, 101);
    h.key(KeyCode::Char('X'));
    assert!(h.app.compose.input.ends_with("\nXEND"));
    h.click_text(" | ");
    assert_eq!(h.app.compose.cursor_pos, 100);
}

#[test]
fn search_scroll_and_click_open_the_visible_result_and_block_underlying_panes() {
    let mut h = Harness::new();
    h.add_chats(35);
    h.draw();
    h.click_text("C-k: search");
    assert!(h.app.search.active);
    for c in "Chat".chars() {
        h.key(KeyCode::Char(c));
    }
    h.mouse(MouseEventKind::ScrollDown, 4, 7);
    assert_eq!(h.app.search.viewport.offset, 3);
    assert_eq!(h.app.sidebar.viewport.offset, 0);
    h.click_text("Chat 10");
    assert_eq!(h.loaded_chat(), "chat-10");
    assert!(!h.app.search.active);
    h.event(Event::Key(KeyEvent::new(
        KeyCode::Char('k'),
        KeyModifiers::CONTROL,
    )));
    h.mouse(MouseEventKind::ScrollDown, 4, 20); // Outside overlay, over sidebar.
    assert_eq!(h.app.sidebar.viewport.offset, 0);
    h.click(4, 20);
    assert!(!h.app.search.active);
    assert!(h.commands.try_recv().is_err());
}

#[test]
fn search_keyboard_enter_and_message_results_use_existing_navigation() {
    let mut h = Harness::new();
    h.add_team();
    h.add_messages(30);
    h.draw();
    h.app.search.activate();
    h.app.search.query = "Channel One".into();
    h.app.search.update_results(&h.app.sidebar, &h.app.messages);
    h.draw();
    h.key(KeyCode::Enter);
    assert_eq!(h.loaded_chat(), "channel-1");
    h.add_messages(30);
    h.app.messages.loading = false;
    h.app.search.activate();
    h.app.search.query = "Message 25".into();
    h.app.search.update_results(&h.app.sidebar, &h.app.messages);
    h.draw();
    // The result row contains the content snippet after its sender label.
    let (_, y) = h.text_position(" - Message 25");
    h.click(5, y);
    assert_eq!(h.app.messages.selected, 25);
    assert_eq!(h.app.active_pane, Pane::Messages);
    h.text_position("Message 25");
    assert!(h.commands.try_recv().is_err());
}

#[test]
fn help_clicks_dismiss_without_activating_the_chat_beneath() {
    let mut h = Harness::new();
    h.add_chats(30);
    h.draw();
    let chat_position = h.text_position("Chat 00");
    h.click_text("[?] Help");
    assert!(h.app.show_help);
    h.text_position("Wheel");
    h.mouse(MouseEventKind::ScrollDown, 3, 6);
    assert_eq!(h.app.sidebar.viewport.offset, 0);
    h.click(chat_position.0, chat_position.1);
    assert!(!h.app.show_help);
    assert!(h.commands.try_recv().is_err());
}

#[test]
fn debug_wheel_scrolls_its_own_viewport_and_stops_at_oldest_logs() {
    let mut h = Harness::new();
    let logs = LogBuffer::new();
    for i in 0..30 {
        logs.push(format!("Log {i:02}"));
    }
    h.app.debug_log = DebugLogState::new(logs);
    h.app.debug_log.toggle();
    h.app.debug_log.refresh();
    h.app.active_pane = Pane::Compose;
    h.draw();
    let (x, y) = h.text_position("Log 29");
    h.mouse(MouseEventKind::ScrollUp, x, y);
    h.text_position("Log 26");
    assert_eq!(h.app.active_pane, Pane::Compose);
    assert_eq!(h.app.sidebar.viewport.offset, 0);
    for _ in 0..30 {
        h.mouse(MouseEventKind::ScrollUp, x, y);
    }
    h.text_position("Log 00");
    h.text_position("Log 03");
    for _ in 0..30 {
        h.mouse(MouseEventKind::ScrollDown, x, y);
    }
    h.text_position("Log 29");
}

#[test]
fn resize_and_debug_toggle_rebuild_targets_for_the_rendered_layout() {
    let mut h = Harness::new();
    h.add_chats(40);
    h.add_messages(20);
    h.draw();
    h.mouse(MouseEventKind::ScrollDown, 3, 6);
    h.app.handle_event(Event::Resize(70, 16), &h.backend);
    assert_eq!(h.app.mouse.at(3, 6), None);
    h.terminal.backend_mut().resize(70, 16);
    h.terminal.resize(Rect::new(0, 0, 70, 16)).unwrap();
    h.draw();
    h.click_text("Chat 05");
    assert_eq!(h.loaded_chat(), "chat-5");
    h.app.debug_log.toggle();
    h.draw();
    h.click_text("Chat 03");
    assert_eq!(h.loaded_chat(), "chat-3");
    h.app.compose.input = "Draft".into();
    h.draw();
    h.click_text("Draft");
    assert_eq!(h.app.active_pane, Pane::Compose);
}

#[test]
fn small_terminals_and_empty_views_handle_mouse_without_panics_or_commands() {
    let mut h = Harness::new();
    for (width, height) in [(1, 1), (2, 2), (10, 4), (23, 8), (25, 10), (40, 12)] {
        h.terminal.backend_mut().resize(width, height);
        h.terminal.resize(Rect::new(0, 0, width, height)).unwrap();
        for overlay in 0..3 {
            h.app.search.active = overlay == 1;
            h.app.search.query = "none".into();
            h.app.show_help = overlay == 2;
            h.draw();
            h.mouse(MouseEventKind::ScrollDown, width - 1, height - 1);
            h.click(width - 1, height - 1);
        }
    }
    assert!(h.commands.try_recv().is_err());
}

#[test]
fn joined_emoji_clicks_use_grapheme_width_in_compose_and_search() {
    let mut h = Harness::new();
    h.app.compose.input = "👩‍💻👍🏽Z".into();
    h.app.compose.move_end();
    h.draw();
    let (x, y) = h.text_position("👩‍💻👍🏽Z");
    h.click(x + 4, y);
    assert_eq!(h.app.compose.cursor_pos, 5);
    h.key(KeyCode::Char('!'));
    assert_eq!(h.app.compose.input, "👩‍💻👍🏽!Z");

    h.app.search.activate();
    h.app.search.query = format!("{}👩‍💻Z", "a".repeat(120));
    h.app.search.move_end();
    h.draw();
    h.click_text("Z");
    assert_eq!(h.app.search.cursor_pos, 123);
    h.key(KeyCode::Char('!'));
    assert!(h.app.search.query.ends_with("👩‍💻!Z"));
}

#[test]
fn compose_send_still_dispatches_once_after_mouse_focus() {
    let mut h = Harness::new();
    h.add_chats(1);
    h.draw();
    h.click_text("Chat 00");
    assert_eq!(h.loaded_chat(), "chat-0");
    h.click_text("Type a message");
    for c in "Draft".chars() {
        h.key(KeyCode::Char(c));
    }
    h.key(KeyCode::Enter);
    match h.commands.try_recv().unwrap() {
        BackendCommand::SendMessage { chat_id, message } => {
            assert_eq!(chat_id, "chat-0");
            assert_eq!(message, "Draft");
        }
        _ => panic!("expected an offline send command"),
    }
    assert!(h.app.compose.input.is_empty());
    assert!(h.commands.try_recv().is_err());
}

#[test]
fn empty_or_loading_messages_discard_old_scroll_limits_and_click_targets() {
    let mut h = Harness::new();
    h.add_messages(50);
    h.draw();
    h.mouse(MouseEventKind::ScrollDown, 35, 8);
    assert_eq!(h.app.messages.viewport.offset, 3);
    h.app.messages.loading = true;
    h.app.messages.messages.clear();
    h.draw();
    h.mouse(MouseEventKind::ScrollDown, 35, 8);
    h.click_text("Loading messages...");
    assert_eq!(h.app.messages.viewport.offset, 0);
    assert!(h.commands.try_recv().is_err());
}

/// Invoked by tests/tui_mouse_pty.py inside a real pseudo-terminal.
/// Uses production initialization/cleanup and Crossterm input with fixture data.
#[test]
#[ignore = "requires a PTY; run python3 tests/tui_mouse_pty.py"]
fn terminal_session_fixture() {
    fn session(mode: &str) -> Result<()> {
        let (mut terminal, _session) = init_terminal()?;
        let mut h = Harness::new();
        h.add_chats(40);
        h.add_messages(20);
        while !h.app.should_exit {
            terminal.draw(|frame| h.app.render(frame))?;
            h.app.handle_event(crossterm::event::read()?, &h.backend);
        }
        assert_eq!(h.app.compose.input, "mouse draft");
        assert_eq!(h.app.sidebar.viewport.offset, 3);
        match mode {
            "error" => anyhow::bail!("fixture error"),
            "panic" => panic!("fixture panic"),
            "normal" => Ok(()),
            _ => panic!("unknown fixture mode"),
        }
    }
    let mode = std::env::var("OST_MOUSE_FIXTURE_MODE").expect("run using the PTY script");
    let result = session(&mode);
    if mode == "error" {
        assert!(result.is_err());
    } else {
        result.unwrap();
    }
}

fn live_message(id: u64, content: &str) -> api::MessageInfo {
    api::MessageInfo {
        id: id.to_string(),
        sender_id: "8:orgid:other".into(),
        sender: "Sender".into(),
        timestamp: "2026-09-18T10:00:00Z".into(),
        content: content.into(),
        mentions: vec![],
    }
}

#[test]
fn background_history_keeps_draft_search_and_message_identity() {
    let mut h = Harness::new();
    h.app.current_chat_id = Some("chat".into());
    h.app.messages.update_messages(
        "Chat",
        vec![
            live_message(1, "first match"),
            live_message(2, "second match"),
        ],
    );
    h.app.messages.selected = 0;
    h.app.compose.input = "unfinished draft".into();
    h.app.search.activate();
    h.app.search.query = "match".into();
    h.app.search.cursor_pos = 3;
    h.app.search.update_results(&h.app.sidebar, &h.app.messages);
    h.app.search.selected = 1;
    let apply = |h: &mut Harness, messages| {
        h.app.handle_backend_response(
            BackendResponse::Messages {
                chat_id: "chat".into(),
                result: Ok(messages),
            },
            &h.backend,
        );
    };
    apply(
        &mut h,
        vec![
            live_message(2, "second match"),
            live_message(3, "new match"),
        ],
    );
    assert_eq!(h.app.messages.messages.len(), 3);
    assert_eq!(h.app.messages.messages[h.app.messages.selected].id, "1");
    assert_eq!(h.app.compose.input, "unfinished draft");
    assert!(h.app.search.active);
    assert_eq!(h.app.search.query, "match");
    assert_eq!(h.app.search.cursor_pos, 3);
    let SearchResultKind::Message(index) = h.app.search.selected_result().unwrap().kind else {
        panic!("message result")
    };
    assert_eq!(h.app.messages.messages[index].id, "2");
    // A stale snapshot cannot erase or duplicate the newer message.
    apply(
        &mut h,
        vec![
            live_message(1, "first match"),
            live_message(2, "second match"),
        ],
    );
    assert_eq!(h.app.messages.messages.len(), 3);
    assert_eq!(h.app.messages.messages.last().unwrap().id, "3");
}

#[test]
fn background_history_preserves_wheel_anchor_and_follows_latest_when_selected() {
    let mut h = Harness::new();
    let messages = (1..30)
        .map(|i| live_message(i, &format!("Message {i}")))
        .collect();
    h.app.messages.update_messages("Chat", messages);
    h.draw();
    h.app.messages.viewport.scroll(true);
    h.draw();
    let offset = h.app.messages.viewport.offset;
    let selected = h.app.messages.messages[h.app.messages.selected].id.clone();
    h.app
        .messages
        .update_messages("Chat", vec![live_message(30, "new message")]);
    h.draw();
    assert_eq!(h.app.messages.viewport.offset, offset);
    assert_eq!(
        h.app.messages.messages[h.app.messages.selected].id,
        selected
    );
    // Older backfilled messages preserve the first visible text via its ID anchor.
    let first_visible = row_text(h.terminal.backend().buffer(), 3);
    h.app
        .messages
        .update_messages("Chat", vec![live_message(0, "old backfill")]);
    h.draw();
    assert_eq!(row_text(h.terminal.backend().buffer(), 3), first_visible);
    h.app.messages.selected = h.app.messages.messages.len() - 1;
    h.app.messages.viewport.follow_selection = true;
    h.draw();
    h.app
        .messages
        .update_messages("Chat", vec![live_message(31, "latest message")]);
    h.draw();
    assert_eq!(h.app.messages.messages[h.app.messages.selected].id, "31");
    h.text_position("latest message");
}

#[test]
fn presence_never_claims_that_the_push_connection_is_healthy() {
    let mut h = Harness::new();
    h.app.handle_backend_response(
        BackendResponse::ConnectionState(ConnectionState::Reconnecting { retry_in_secs: 8 }),
        &h.backend,
    );
    h.app.handle_backend_response(
        BackendResponse::Presence(Err(anyhow::anyhow!("offline"))),
        &h.backend,
    );
    assert!(!h.app.is_online);
    assert!(h.app.connection_state.contains("Reconnecting"));
    h.app.handle_backend_response(
        BackendResponse::ConnectionState(ConnectionState::Connected),
        &h.backend,
    );
    assert!(h.app.is_online);
    assert_eq!(h.app.connection_state, "Live");
}

fn live_history(h: &mut Harness, chat_id: &str, messages: Vec<api::MessageInfo>) {
    h.app.handle_backend_response(
        BackendResponse::Messages {
            chat_id: chat_id.into(),
            result: Ok(messages),
        },
        &h.backend,
    );
}

fn chat_with_history() -> Harness {
    let mut h = Harness::new();
    h.app.current_chat_id = Some("chat".into());
    live_history(
        &mut h,
        "chat",
        (1..30)
            .map(|i| live_message(i, &format!("Message {i}")))
            .collect(),
    );
    h.draw();
    h
}

#[test]
fn successful_send_reveals_latest_without_leaving_compose_even_after_stale_refresh() {
    let mut h = chat_with_history();
    h.mouse(MouseEventKind::ScrollUp, 35, 8);
    assert!(!h.app.messages.rendered_latest);
    h.click_text("Type a message");
    h.app.compose.input = "outgoing reply".into();
    h.key(KeyCode::Enter);
    assert!(matches!(
        h.commands.try_recv().unwrap(),
        BackendCommand::SendMessage { chat_id, message }
            if chat_id == "chat" && message == "outgoing reply"
    ));
    h.app.compose.input = "next draft".into();
    h.app.handle_backend_response(
        BackendResponse::MessageSent {
            chat_id: "chat".into(),
            result: Ok(()),
        },
        &h.backend,
    );
    assert_eq!(h.loaded_chat(), "chat");
    // An already queued history response may precede visibility of our send.
    live_history(&mut h, "chat", vec![live_message(29, "Message 29")]);
    h.draw();
    assert!(h.app.messages.rendered_latest);
    let content = format!(
        "{}\nEND OF SENT MESSAGE",
        "wrapped outgoing body ".repeat(300)
    );
    live_history(&mut h, "chat", vec![live_message(30, &content)]);
    h.draw();
    h.text_position("END OF SENT MESSAGE");
    assert!(h.app.messages.rendered_latest);
    assert_eq!(h.app.messages.messages[h.app.messages.selected].id, "30");
    assert_eq!(h.app.active_pane, Pane::Compose);
    assert_eq!(h.app.compose.input, "next draft");
    // Following the bottom must survive subsequent draws of a tall message.
    h.draw();
    h.text_position("END OF SENT MESSAGE");
    h.mouse(MouseEventKind::ScrollUp, 35, 8);
    let offset = h.app.messages.viewport.offset;
    live_history(&mut h, "chat", vec![live_message(31, "later reply")]);
    h.draw();
    assert_eq!(h.app.messages.viewport.offset, offset);
    assert!(!h.app.messages.rendered_latest);
}

#[test]
fn incoming_messages_follow_mouse_return_to_bottom_and_tall_queued_updates() {
    let mut h = chat_with_history();
    h.mouse(MouseEventKind::ScrollUp, 35, 8);
    h.mouse(MouseEventKind::ScrollUp, 35, 8);
    h.mouse(MouseEventKind::ScrollDown, 35, 8);
    h.mouse(MouseEventKind::ScrollDown, 35, 8);
    assert!(h.app.messages.rendered_latest);
    assert!(!h.app.messages.viewport.follow_selection);
    h.click_text("Type a message");
    h.app.compose.input = "draft remains".into();
    let content = format!(
        "{}\nEND OF INCOMING MESSAGE",
        "wrapped incoming body ".repeat(300)
    );
    live_history(&mut h, "chat", vec![live_message(30, &content)]);
    live_history(&mut h, "chat", vec![live_message(30, &content)]);
    h.draw();
    h.text_position("END OF INCOMING MESSAGE");
    assert!(h.app.messages.rendered_latest);
    h.draw();
    assert!(h.app.messages.rendered_latest);
    assert_eq!(h.app.active_pane, Pane::Compose);
    assert_eq!(h.app.compose.input, "draft remains");
}

#[test]
fn incoming_messages_preserve_scrolled_history_and_compose_draft() {
    let mut h = chat_with_history();
    h.mouse(MouseEventKind::ScrollUp, 35, 8);
    h.click_text("Type a message");
    h.app.compose.input = "unfinished".into();
    let offset = h.app.messages.viewport.offset;
    let first_visible = row_text(h.terminal.backend().buffer(), 3);
    live_history(&mut h, "chat", vec![live_message(30, "new arrival")]);
    h.draw();
    assert_eq!(h.app.messages.viewport.offset, offset);
    assert_eq!(row_text(h.terminal.backend().buffer(), 3), first_visible);
    assert!(!h.app.messages.rendered_latest);
    assert_eq!(h.app.active_pane, Pane::Compose);
    assert_eq!(h.app.compose.input, "unfinished");
}

#[test]
fn failed_send_preserves_viewport_and_does_not_refresh() {
    let mut h = chat_with_history();
    h.mouse(MouseEventKind::ScrollUp, 35, 8);
    h.click_text("Type a message");
    let offset = h.app.messages.viewport.offset;
    h.app.handle_backend_response(
        BackendResponse::MessageSent {
            chat_id: "chat".into(),
            result: Err(anyhow::anyhow!("offline")),
        },
        &h.backend,
    );
    h.draw();
    assert!(h.app.status_is_error);
    assert_eq!(h.app.messages.viewport.offset, offset);
    assert!(!h.app.messages.rendered_latest);
    assert_eq!(h.app.active_pane, Pane::Compose);
    assert!(h.commands.try_recv().is_err());
}

#[test]
fn completion_after_switching_chats_does_not_scroll_or_reload_the_active_chat() {
    let mut h = chat_with_history();
    h.add_chats(2);
    h.draw();
    h.click_text("Chat 01");
    assert_eq!(h.loaded_chat(), "chat-1");
    live_history(
        &mut h,
        "chat-1",
        (1..30)
            .map(|i| live_message(i, &format!("Other {i}")))
            .collect(),
    );
    h.draw();
    h.mouse(MouseEventKind::ScrollUp, 35, 8);
    h.click_text("Type a message");
    h.app.compose.input = "other conversation draft".into();
    let offset = h.app.messages.viewport.offset;
    h.app.handle_backend_response(
        BackendResponse::MessageSent {
            chat_id: "chat".into(),
            result: Ok(()),
        },
        &h.backend,
    );
    live_history(&mut h, "chat", vec![live_message(30, "old chat send")]);
    h.draw();
    assert_eq!(h.app.messages.viewport.offset, offset);
    assert!(!h.app.messages.rendered_latest);
    assert_eq!(h.app.current_chat_id.as_deref(), Some("chat-1"));
    assert_eq!(h.app.active_pane, Pane::Compose);
    assert_eq!(h.app.compose.input, "other conversation draft");
    assert!(h.commands.try_recv().is_err());
}

#[test]
fn history_updates_before_first_render_keep_the_latest_message_selected() {
    let mut h = Harness::new();
    h.app.current_chat_id = Some("chat".into());
    live_history(&mut h, "chat", vec![live_message(1, "first")]);
    live_history(&mut h, "chat", vec![live_message(2, "latest")]);
    h.draw();
    assert_eq!(h.app.messages.messages[h.app.messages.selected].id, "2");
    h.text_position("latest");
}
