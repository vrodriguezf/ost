//! TUI Application state and main event loop

use anyhow::Result;
use crossterm::{
    event::{
        DisableFocusChange, DisableMouseCapture, EnableFocusChange, EnableMouseCapture, Event,
        EventStream, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    },
    execute,
};
use ratatui::DefaultTerminal;
use tokio_stream::StreamExt;

use super::activity::{ConnectionState, IncomingMessage};
use super::backend::{Backend, BackendCommand, BackendResponse};
use super::compose::ComposeState;
use super::debug_log::DebugLogState;
use super::log_capture::LogBuffer;
use super::messages::MessagesState;
use super::mouse::{is_actionable, HitMap, Target, WHEEL_LINES};
use super::notifications::{NotificationContext, NotificationPolicy, NotificationService};
use super::search::{SearchResultKind, SearchState};
use super::sidebar::SidebarState;
use super::ui;
use super::unread::{configured_account, MessageStamp, UnreadState};

/// Active pane in the TUI
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    #[default]
    Sidebar,
    Messages,
    Compose,
}

impl Pane {
    pub fn as_str(&self) -> &'static str {
        match self {
            Pane::Sidebar => "sidebar",
            Pane::Messages => "messages",
            Pane::Compose => "compose",
        }
    }
}

/// Application state
pub struct App {
    /// Whether the app should exit
    pub should_exit: bool,
    /// Online status (for display)
    pub is_online: bool,
    /// Teams presence is independent of push connection health.
    pub presence: String,
    /// Current user name
    pub user_name: String,
    /// Stable Graph/MRI identity, used to suppress notifications for our messages.
    pub current_user_id: Option<String>,
    /// Starts conservatively until a focus event or direct interaction is received.
    pub terminal_focused: bool,
    /// Current channel name
    pub channel_name: String,
    /// Member count
    #[allow(dead_code)]
    pub member_count: u32,
    /// Connection state description
    pub connection_state: String,
    /// Active pane
    pub active_pane: Pane,
    /// Sidebar state (teams/channels/chats + navigation)
    pub sidebar: SidebarState,
    /// Messages pane state
    pub messages: MessagesState,
    /// Compose box state
    pub compose: ComposeState,
    /// Whether the help popup is visible
    pub show_help: bool,
    /// Global search overlay state
    pub search: SearchState,
    /// The chat/channel ID currently being viewed.
    pub current_chat_id: Option<String>,
    /// Status message shown in the status bar (errors, info).
    pub status_message: Option<String>,
    /// Whether the status message is an error.
    pub status_is_error: bool,
    /// Debug log pane state.
    pub debug_log: DebugLogState,
    /// Hit targets from the most recently rendered frame.
    pub mouse: HitMap,
    notification_policy: NotificationPolicy,
    notification_service: Option<NotificationService>,
    pub unread: UnreadState,
    /// Newest loaded identity; selection or loading alone never acknowledges it.
    pub loaded_last_stamp: Option<MessageStamp>,
    rendered_chat_id: Option<String>,
    /// Hold a manual unread reminder until this conversation is left.
    manual_unread_hold: Option<String>,
    deferred_incoming: Vec<IncomingMessage>,
}

impl App {
    /// Create a new App with the given log buffer for debug log capture.
    pub fn new(log_buffer: LogBuffer) -> Self {
        Self {
            should_exit: false,
            is_online: false,
            presence: "unknown".into(),
            user_name: "Loading...".to_string(),
            current_user_id: None,
            terminal_focused: false,
            channel_name: "".to_string(),
            member_count: 0,
            connection_state: "Connecting...".to_string(),
            active_pane: Pane::default(),
            sidebar: SidebarState::default(),
            messages: MessagesState::default(),
            compose: ComposeState::default(),
            show_help: false,
            search: SearchState::default(),
            current_chat_id: None,
            status_message: None,
            status_is_error: false,
            debug_log: DebugLogState::new(log_buffer),
            mouse: HitMap::default(),
            notification_policy: NotificationPolicy::from_env(),
            notification_service: None,
            unread: UnreadState::default(),
            loaded_last_stamp: None,
            rendered_chat_id: None,
            manual_unread_hold: None,
            deferred_incoming: Vec::new(),
        }
    }
}

impl App {
    /// Cycle to the next pane.
    fn next_pane(&mut self) {
        self.active_pane = match self.active_pane {
            Pane::Sidebar => Pane::Messages,
            Pane::Messages => Pane::Compose,
            Pane::Compose => Pane::Sidebar,
        };
    }

    /// Cycle to the previous pane (reverse of next_pane).
    fn prev_pane(&mut self) {
        self.active_pane = match self.active_pane {
            Pane::Sidebar => Pane::Compose,
            Pane::Messages => Pane::Sidebar,
            Pane::Compose => Pane::Messages,
        };
    }

    /// Handle a crossterm event.
    pub fn handle_event(&mut self, event: Event, backend: &Backend) {
        match event {
            Event::FocusGained => {
                self.terminal_focused = true;
                return;
            }
            Event::FocusLost => {
                self.terminal_focused = false;
                return;
            }
            Event::Mouse(mouse) => {
                self.terminal_focused = true;
                self.handle_mouse(mouse, backend);
                return;
            }
            Event::Resize(_, _) => {
                self.mouse.clear();
                return;
            }
            _ => {}
        }
        if let Event::Key(key_event) = event {
            if key_event.kind != KeyEventKind::Press {
                return;
            }
            // Direct input confirms focus on terminals without focus reports.
            self.terminal_focused = true;
            // When help popup is visible, any key closes it.
            if self.show_help {
                self.show_help = false;
                return;
            }

            // Clear status message on any keypress.
            self.status_message = None;

            // When search overlay is active, route all keys to search handler.
            if self.search.active {
                self.handle_search_key(key_event, backend);
                return;
            }

            // Ctrl+K activates global search from any mode.
            if key_event.code == KeyCode::Char('k')
                && key_event.modifiers.contains(KeyModifiers::CONTROL)
            {
                self.search.activate();
                return;
            }

            // Ctrl+D toggles debug log pane from any mode.
            if key_event.code == KeyCode::Char('d')
                && key_event.modifiers.contains(KeyModifiers::CONTROL)
            {
                self.debug_log.toggle();
                return;
            }

            // When debug log is visible, handle scroll keys.
            if self.debug_log.visible {
                match key_event.code {
                    KeyCode::PageUp => {
                        self.debug_log.scroll_up(10);
                        return;
                    }
                    KeyCode::PageDown => {
                        self.debug_log.scroll_down(10);
                        return;
                    }
                    _ => {}
                }
            }

            // When the compose pane is focused, most keys are text input.
            if self.active_pane == Pane::Compose {
                self.handle_compose_key(key_event, backend);
            } else {
                self.handle_navigation_key(key_event, backend);
            }
        }
    }

    fn handle_mouse(&mut self, event: MouseEvent, backend: &Backend) {
        if !is_actionable(event) {
            return;
        }
        let Some(target) = self.mouse.at(event.column, event.row) else {
            return;
        };
        if self.show_help {
            if event.kind == MouseEventKind::Down(MouseButton::Left) {
                self.show_help = false;
            }
            return;
        }
        if self.search.active {
            match (event.kind, target) {
                (MouseEventKind::Down(MouseButton::Left), Target::SearchResult(idx)) => {
                    self.search.selected = idx;
                    self.apply_search_selection(backend);
                }
                (MouseEventKind::Down(MouseButton::Left), Target::SearchCursor(position)) => {
                    self.search.cursor_pos = position;
                }
                (MouseEventKind::Down(MouseButton::Left), Target::DismissSearch) => {
                    self.search.deactivate();
                }
                (
                    MouseEventKind::ScrollUp | MouseEventKind::ScrollDown,
                    Target::Search | Target::SearchCursor(_) | Target::SearchResult(_),
                ) => {
                    self.search
                        .viewport
                        .scroll(event.kind == MouseEventKind::ScrollUp);
                }
                _ => {}
            }
            return;
        }
        match event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.status_message = None;
                match target {
                    Target::Sidebar | Target::SidebarItem(_) => {
                        self.active_pane = Pane::Sidebar;
                        if let Target::SidebarItem(idx) = target {
                            self.sidebar.selected = idx;
                            self.sidebar.viewport.follow_selection = false;
                            self.handle_sidebar_enter(backend);
                        }
                    }
                    Target::Messages | Target::Message(_) => {
                        if let Target::Message(idx) = target {
                            // A second click on the selected message toggles its thread.
                            if self.active_pane == Pane::Messages && self.messages.selected == idx {
                                self.messages.toggle_thread();
                            }
                            self.messages.selected = idx;
                            self.messages.viewport.follow_selection = false;
                        }
                        self.active_pane = Pane::Messages;
                    }
                    Target::Compose | Target::ComposeCursor(_) => {
                        self.active_pane = Pane::Compose;
                        if let Target::ComposeCursor(position) = target {
                            self.compose.cursor_pos = position;
                        }
                    }
                    Target::Help => self.show_help = true,
                    Target::Search => self.search.activate(),
                    _ => {}
                }
            }
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let up = event.kind == MouseEventKind::ScrollUp;
                match target {
                    Target::Sidebar | Target::SidebarItem(_) => self.sidebar.viewport.scroll(up),
                    Target::Messages | Target::Message(_) => self.messages.viewport.scroll(up),
                    Target::DebugLog if up => self.debug_log.scroll_up(WHEEL_LINES),
                    Target::DebugLog => self.debug_log.scroll_down(WHEEL_LINES),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    /// Handle key events when a non-compose pane is focused.
    fn handle_navigation_key(&mut self, key_event: crossterm::event::KeyEvent, backend: &Backend) {
        match key_event.code {
            KeyCode::Char('u') if key_event.modifiers.is_empty() => {
                self.toggle_chat_unread();
            }
            KeyCode::Char('q') => {
                self.should_exit = true;
            }
            KeyCode::Tab => {
                self.next_pane();
            }
            KeyCode::BackTab => {
                self.prev_pane();
            }
            KeyCode::Right => {
                self.next_pane();
            }
            KeyCode::Left => {
                self.prev_pane();
            }
            // Direct pane jump with number keys
            KeyCode::Char('1') => {
                self.active_pane = Pane::Sidebar;
            }
            KeyCode::Char('2') => {
                self.active_pane = Pane::Messages;
            }
            KeyCode::Char('3') => {
                self.active_pane = Pane::Compose;
            }
            // Sidebar-specific keys (only when sidebar is focused)
            KeyCode::Up | KeyCode::Char('k') if self.active_pane == Pane::Sidebar => {
                self.sidebar.move_up();
            }
            KeyCode::Down | KeyCode::Char('j') if self.active_pane == Pane::Sidebar => {
                self.sidebar.move_down();
            }
            KeyCode::Enter if self.active_pane == Pane::Sidebar => {
                self.handle_sidebar_enter(backend);
            }
            // Messages pane keys
            KeyCode::Up | KeyCode::Char('k') if self.active_pane == Pane::Messages => {
                self.messages.select_previous();
            }
            KeyCode::Down | KeyCode::Char('j') if self.active_pane == Pane::Messages => {
                self.messages.select_next();
            }
            KeyCode::Enter if self.active_pane == Pane::Messages => {
                self.messages.toggle_thread();
            }
            // Help popup toggle (available from any non-compose pane)
            KeyCode::Char('?') => {
                self.show_help = !self.show_help;
            }
            _ => {}
        }
    }

    fn toggle_chat_unread(&mut self) {
        let chat_id = match self.active_pane {
            Pane::Sidebar => self.sidebar.selected_item_id(),
            Pane::Messages => self.current_chat_id.clone(),
            Pane::Compose => None,
        };
        let Some(chat_id) = chat_id else { return };
        let marked_unread = self.unread.toggle(&chat_id);
        if self.current_chat_id.as_deref() == Some(&chat_id) {
            self.manual_unread_hold = marked_unread.then_some(chat_id);
        }
        self.sidebar.apply_unread(&self.unread);
        self.status_is_error = false;
        self.status_message = Some(
            if marked_unread {
                "Marked unread"
            } else {
                "Marked read"
            }
            .into(),
        );
        if let Err(error) = self.unread.save() {
            self.set_error(format!("Unread state not saved: {error:#}"));
        }
    }

    /// Handle Enter key on a sidebar item.
    ///
    /// If the selected item is a team, toggle expand/collapse.
    /// If it's a channel or chat, load its messages.
    fn handle_sidebar_enter(&mut self, backend: &Backend) {
        let items = self.sidebar.flat_items();
        let item = match items.get(self.sidebar.selected) {
            Some(item) => *item,
            None => return,
        };

        match item {
            super::sidebar::SidebarItem::TeamsHeader | super::sidebar::SidebarItem::Team(_) => {
                self.sidebar.toggle_expand();
                self.sidebar.clamp_selection();
            }
            super::sidebar::SidebarItem::Channel(_, _) | super::sidebar::SidebarItem::Chat(_) => {
                if let Some(id) = self.sidebar.selected_item_id() {
                    let name = self.sidebar.selected_item_name().unwrap_or_default();
                    if self.current_chat_id.as_deref() != Some(&id) {
                        self.manual_unread_hold = None;
                    }
                    self.current_chat_id = Some(id.clone());
                    self.channel_name = name.clone();
                    self.messages.loading = true;
                    self.messages.channel_header = name;
                    self.messages.messages.clear();
                    backend.send(BackendCommand::LoadMessages {
                        chat_id: id,
                        limit: 50,
                    });
                }
            }
            _ => {}
        }
    }

    /// Handle key events when the compose pane is focused.
    fn handle_compose_key(&mut self, key_event: crossterm::event::KeyEvent, backend: &Backend) {
        let modifiers = key_event.modifiers;
        let code = key_event.code;

        match (code, modifiers) {
            // Tab always cycles pane focus.
            (KeyCode::Tab, _) => {
                self.next_pane();
            }
            // Shift+Tab cycles backward.
            (KeyCode::BackTab, _) => {
                self.prev_pane();
            }
            // Esc leaves compose and goes to Messages pane.
            (KeyCode::Esc, _) => {
                self.active_pane = Pane::Messages;
            }
            // Ctrl+Enter inserts a newline.
            (KeyCode::Enter, m) if m.contains(KeyModifiers::CONTROL) => {
                self.compose.insert_newline();
            }
            // Enter sends the message.
            (KeyCode::Enter, _) => {
                if let Some(text) = self.compose.send() {
                    if let Some(ref chat_id) = self.current_chat_id {
                        backend.send(BackendCommand::SendMessage {
                            chat_id: chat_id.clone(),
                            message: text,
                        });
                    } else {
                        self.status_message =
                            Some("No chat selected. Select a channel or chat first.".to_string());
                        self.status_is_error = true;
                    }
                }
            }
            // Ctrl+U clears the compose box.
            (KeyCode::Char('u'), m) if m.contains(KeyModifiers::CONTROL) => {
                self.compose.clear();
            }
            // Backspace deletes character before cursor.
            (KeyCode::Backspace, _) => {
                self.compose.backspace();
            }
            // Delete removes character at cursor.
            (KeyCode::Delete, _) => {
                self.compose.delete();
            }
            // Arrow keys for cursor movement.
            (KeyCode::Left, _) => {
                self.compose.move_left();
            }
            (KeyCode::Right, _) => {
                self.compose.move_right();
            }
            (KeyCode::Home, _) => {
                self.compose.move_home();
            }
            (KeyCode::End, _) => {
                self.compose.move_end();
            }
            // Regular character input.
            (KeyCode::Char(c), m) => {
                // Only insert if no modifiers or just shift (for uppercase).
                if m.is_empty() || m == KeyModifiers::SHIFT {
                    self.compose.insert_char(c);
                }
            }
            _ => {}
        }
    }

    /// Handle key events when the search overlay is active.
    fn handle_search_key(&mut self, key_event: crossterm::event::KeyEvent, backend: &Backend) {
        let code = key_event.code;
        let modifiers = key_event.modifiers;

        match (code, modifiers) {
            // Esc closes the search overlay.
            (KeyCode::Esc, _) => {
                self.search.deactivate();
            }
            // Up arrow navigates results.
            (KeyCode::Up, _) => {
                self.search.select_previous();
            }
            // Down arrow navigates results.
            (KeyCode::Down, _) => {
                self.search.select_next();
            }
            // Enter selects the current result.
            (KeyCode::Enter, _) => {
                self.apply_search_selection(backend);
            }
            // Backspace deletes character before cursor.
            (KeyCode::Backspace, _) => {
                self.search.backspace();
                self.search.update_results(&self.sidebar, &self.messages);
            }
            // Delete removes character at cursor.
            (KeyCode::Delete, _) => {
                self.search.delete_at_cursor();
                self.search.update_results(&self.sidebar, &self.messages);
            }
            // Left/Right move cursor.
            (KeyCode::Left, _) => {
                self.search.move_left();
            }
            (KeyCode::Right, _) => {
                self.search.move_right();
            }
            (KeyCode::Home, _) => {
                self.search.move_home();
            }
            (KeyCode::End, _) => {
                self.search.move_end();
            }
            // Regular character input.
            (KeyCode::Char(c), m) => {
                // Only insert if no modifiers or just shift (for uppercase).
                if m.is_empty() || m == KeyModifiers::SHIFT {
                    self.search.insert_char(c);
                    self.search.update_results(&self.sidebar, &self.messages);
                }
            }
            _ => {}
        }
    }

    /// Apply the currently selected search result: navigate to the matching item.
    fn apply_search_selection(&mut self, backend: &Backend) {
        use super::search::SearchResultKind;

        let result = match self.search.selected_result() {
            Some(r) => r.kind.clone(),
            None => {
                self.search.deactivate();
                return;
            }
        };

        match result {
            SearchResultKind::Channel(team_idx, channel_idx) => {
                // Reveal both ancestors before selecting the channel.
                self.sidebar.teams_expanded = true;
                if !self.sidebar.teams[team_idx].expanded {
                    self.sidebar.teams[team_idx].expanded = true;
                }
                // Find the flat index of this channel in the sidebar.
                let items = self.sidebar.flat_items();
                for (idx, item) in items.iter().enumerate() {
                    if let super::sidebar::SidebarItem::Channel(ti, ci) = item {
                        if *ti == team_idx && *ci == channel_idx {
                            self.sidebar.selected = idx;
                            break;
                        }
                    }
                }
                self.active_pane = Pane::Sidebar;
                self.sidebar.viewport.follow_selection = true;
                self.handle_sidebar_enter(backend);
            }
            SearchResultKind::Chat(chat_idx) => {
                // Select the chat in the sidebar.
                let items = self.sidebar.flat_items();
                for (idx, item) in items.iter().enumerate() {
                    if let super::sidebar::SidebarItem::Chat(ci) = item {
                        if *ci == chat_idx {
                            self.sidebar.selected = idx;
                            break;
                        }
                    }
                }
                self.active_pane = Pane::Sidebar;
                self.sidebar.viewport.follow_selection = true;
                self.handle_sidebar_enter(backend);
            }
            SearchResultKind::Message(msg_idx) => {
                // Select the message in the messages pane.
                if msg_idx < self.messages.messages.len() {
                    self.messages.selected = msg_idx;
                    self.messages.viewport.follow_selection = true;
                }
                self.active_pane = Pane::Messages;
            }
        }

        self.search.deactivate();
    }

    /// Handle a response from the async backend.
    fn handle_backend_response(&mut self, response: BackendResponse, backend: &Backend) {
        self.mouse.clear();
        let selected_search_key = self.search_selected_key();
        let selected_search = self.search.selected;
        let search_offset = self.search.viewport.offset;
        match response {
            // Both consumers see the activity before the authoritative history
            // response can acknowledge it as displayed.
            BackendResponse::IncomingMessage(message) => self.dispatch_incoming(&message),
            BackendResponse::ConnectionState(state) => {
                self.is_online = state == ConnectionState::Connected;
                self.connection_state = match state {
                    ConnectionState::Connecting => "Connecting push...".into(),
                    ConnectionState::Connected => "Live".into(),
                    ConnectionState::Reconnecting { retry_in_secs } => {
                        format!("Reconnecting in {retry_in_secs}s; polling")
                    }
                    ConnectionState::Degraded(reason) => format!("Degraded: {reason}"),
                };
            }
            BackendResponse::Teams(Ok(teams)) => {
                self.sidebar.update_teams(teams);
                self.sidebar.apply_unread(&self.unread);
                self.sidebar.loading = false;
                // Keep selection valid after the hierarchy changes.
                if self.sidebar.selected == 0 {
                    self.sidebar.clamp_selection();
                }
            }
            BackendResponse::Teams(Err(e)) => {
                self.set_error(format!("Failed to load teams: {:#}", e));
                self.sidebar.loading = false;
            }
            BackendResponse::Chats(Ok(chats)) => {
                for chat in &chats {
                    self.unread
                        .observe_chat(chat, self.current_user_id.as_deref());
                }
                self.sidebar
                    .update_chats(chats, self.current_user_id.as_deref());
                self.sync_chat_header();
                self.sidebar.apply_unread(&self.unread);
                self.sidebar.loading = false;
            }
            BackendResponse::Chats(Err(e)) => {
                self.set_error(format!("Failed to load chats: {:#}", e));
                self.sidebar.loading = false;
            }
            BackendResponse::Messages { chat_id, result } => {
                // Only apply if this is still the chat we're looking at.
                if self.current_chat_id.as_deref() == Some(&chat_id) {
                    match result {
                        Ok(msgs) => {
                            for message in msgs.iter().rev() {
                                if self.sidebar.recover_chat_name(
                                    &chat_id,
                                    &message.sender_id,
                                    &message.sender,
                                    self.current_user_id.as_deref(),
                                ) {
                                    break;
                                }
                            }
                            self.sync_chat_header();
                            self.unread.observe_history(
                                &chat_id,
                                &msgs,
                                self.current_user_id.as_deref(),
                            );
                            self.loaded_last_stamp = msgs
                                .last()
                                .filter(|m| !m.id.is_empty())
                                .map(|m| MessageStamp::new(&m.id, &m.timestamp));
                            self.sidebar.apply_unread(&self.unread);
                            let header = self.messages.channel_header.clone();
                            self.messages.update_messages(&header, msgs);
                        }
                        Err(e) => {
                            self.messages.loading = false;
                            self.loaded_last_stamp = None;
                            self.set_error(format!("Failed to load messages: {:#}", e));
                        }
                    }
                }
            }
            BackendResponse::MessageSent(Ok(())) => {
                self.status_message = Some("Message sent".to_string());
                self.status_is_error = false;
                // Reload messages for the current chat.
                if let Some(ref chat_id) = self.current_chat_id {
                    backend.send(BackendCommand::LoadMessages {
                        chat_id: chat_id.clone(),
                        limit: 50,
                    });
                }
            }
            BackendResponse::MessageSent(Err(e)) => {
                self.set_error(format!("Failed to send message: {:#}", e));
            }
            BackendResponse::UserInfo(Ok(info)) => {
                self.user_name = info.display_name;
                if self.current_user_id.is_none() {
                    self.load_unread_account("", &info.id);
                }
                self.current_user_id = Some(info.id);
            }
            BackendResponse::UserInfo(Err(e)) => {
                self.set_error(format!("Failed to load user info: {:#}", e));
            }
            BackendResponse::Presence(Ok(presence)) => {
                self.presence = presence.availability;
            }
            BackendResponse::Presence(Err(e)) => {
                tracing::debug!("Failed to load presence: {:#}", e);
                self.presence = "unknown".into();
            }
            BackendResponse::ClientError(msg) => {
                self.connection_state = "Not authenticated".to_string();
                self.is_online = false;
                self.sidebar.loading = false;
                self.messages.loading = false;
                self.set_error(format!("Auth: {}", msg));
            }
        }
        if self.search.active {
            self.search.update_results(&self.sidebar, &self.messages);
            self.search.selected = selected_search_key
                .and_then(|key| {
                    self.search.results.iter().position(|result| {
                        self.search_result_key(&result.kind).as_deref() == Some(&key)
                    })
                })
                .unwrap_or(selected_search.min(self.search.results.len().saturating_sub(1)));
            self.search.viewport.offset = search_offset;
        }
    }

    /// Only live incoming events call this; fetching history never shows an alert.
    fn notify_incoming(&mut self, message: &super::activity::IncomingMessage) {
        let conversation_name = self
            .sidebar
            .chats
            .iter()
            .find(|chat| chat.id == message.chat_id)
            .map(|chat| chat.name.as_str())
            .or_else(|| {
                self.sidebar
                    .teams
                    .iter()
                    .flat_map(|team| &team.channels)
                    .find(|channel| channel.id == message.chat_id)
                    .map(|channel| channel.name.as_str())
            })
            .unwrap_or("Teams conversation");
        let context = NotificationContext {
            current_user_id: self.current_user_id.as_deref(),
            current_chat_id: self.current_chat_id.as_deref(),
            terminal_focused: self.terminal_focused,
            conversation_name,
        };
        if let Some(notification) = self.notification_policy.prepare(message, context) {
            if let Some(service) = &self.notification_service {
                service.enqueue(notification);
            }
        }
    }

    fn load_unread_account(&mut self, tenant: &str, user: &str) {
        #[cfg(test)]
        let _ = tenant;
        #[cfg(not(test))]
        match UnreadState::load_for_account(tenant, user) {
            Ok(state) => self.unread = state,
            Err(error) => self.set_error(format!("Unread state unavailable: {error:#}")),
        }
        self.current_user_id = Some(user.to_owned());
        for message in std::mem::take(&mut self.deferred_incoming) {
            self.dispatch_incoming(&message);
        }
        self.sidebar.apply_unread(&self.unread);
    }

    fn dispatch_incoming(&mut self, message: &IncomingMessage) {
        if self.current_user_id.is_some() {
            if self.sidebar.recover_chat_name(
                &message.chat_id,
                &message.sender_id,
                &message.sender,
                self.current_user_id.as_deref(),
            ) {
                self.sync_chat_header();
            }
            self.notify_incoming(message);
        }
        self.observe_incoming(message);
    }

    fn sync_chat_header(&mut self) {
        if let Some(chat) = self
            .sidebar
            .chats
            .iter()
            .find(|chat| self.current_chat_id.as_deref() == Some(chat.id.as_str()))
        {
            self.channel_name = chat.name.clone();
            // A label change must not be mistaken for opening a different chat.
            self.messages.channel_header = chat.name.clone();
        }
    }

    pub fn observe_incoming(&mut self, message: &IncomingMessage) {
        if self.current_user_id.is_none() {
            // Do not classify our own activity as unread before identity loads.
            if self.deferred_incoming.len() < 1024 {
                self.deferred_incoming.push(message.clone());
            }
            return;
        }
        self.unread
            .incoming(message, self.current_user_id.as_deref());
        self.sidebar.apply_unread(&self.unread);
    }

    /// Whether the previous successful frame showed the latest loaded content.
    /// Notifications can use this before applying an incoming message.
    pub fn is_reading_latest(&self, chat_id: &str) -> bool {
        self.terminal_focused
            && !self.show_help
            && !self.search.active
            && !self.messages.loading
            && self.loaded_last_stamp.is_some()
            && self.messages.rendered_latest
            && self.current_chat_id.as_deref() == Some(chat_id)
            && self.rendered_chat_id.as_deref() == Some(chat_id)
    }

    /// Called after terminal.draw succeeds, never merely after fetch or selection.
    pub fn acknowledge_rendered(&mut self) -> bool {
        let before = self
            .current_chat_id
            .as_deref()
            .map(|id| self.unread.badge(id));
        self.rendered_chat_id = self.current_chat_id.clone();
        if let (Some(chat_id), Some(stamp)) = (&self.current_chat_id, &self.loaded_last_stamp) {
            if self.is_reading_latest(chat_id)
                && self.manual_unread_hold.as_deref() != Some(chat_id)
            {
                self.unread.acknowledge(chat_id, stamp);
                self.sidebar.apply_unread(&self.unread);
            }
        }
        let changed = before
            != self
                .current_chat_id
                .as_deref()
                .map(|id| self.unread.badge(id));
        if let Err(error) = self.unread.save() {
            self.set_error(format!("Unread state not saved: {error:#}"));
            return true;
        }
        changed
    }

    /// Stable result identity, captured before background data changes indices.
    fn search_selected_key(&self) -> Option<String> {
        self.search
            .selected_result()
            .and_then(|result| self.search_result_key(&result.kind))
    }

    fn search_result_key(&self, kind: &SearchResultKind) -> Option<String> {
        match *kind {
            SearchResultKind::Chat(index) => self
                .sidebar
                .chats
                .get(index)
                .map(|chat| format!("chat:{}", chat.id)),
            SearchResultKind::Channel(team, channel) => self
                .sidebar
                .teams
                .get(team)
                .and_then(|team| team.channels.get(channel))
                .map(|channel| format!("channel:{}", channel.id)),
            SearchResultKind::Message(index) => self
                .messages
                .messages
                .get(index)
                .map(|message| format!("message:{}", message.id)),
        }
    }

    /// Set an error status message.
    fn set_error(&mut self, msg: String) {
        self.status_message = Some(msg);
        self.status_is_error = true;
    }

    /// Render the UI
    pub fn render(&mut self, frame: &mut ratatui::Frame) {
        ui::render(frame, self);
    }
}

/// Run the TUI application with terminal restore on exit.
///
/// Sets up a panic hook so the terminal is always restored even on panic.
/// Requires a LogBuffer for capturing tracing output into the debug log pane.
pub async fn run(log_buffer: LogBuffer) -> Result<()> {
    let (mut terminal, _session) = init_terminal()?;
    run_app(&mut terminal, log_buffer).await
}

fn init_terminal() -> Result<(DefaultTerminal, TerminalSession)> {
    // Install a panic hook that restores the terminal before printing the panic.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        default_hook(info);
    }));

    let terminal = ratatui::init();
    let session = TerminalSession;
    execute!(std::io::stdout(), EnableMouseCapture, EnableFocusChange)?;
    Ok((terminal, session))
}

fn restore_terminal() {
    let _ = execute!(std::io::stdout(), DisableMouseCapture, DisableFocusChange);
    ratatui::restore();
}

/// Also restores the terminal if the async session is cancelled or returns an error.
struct TerminalSession;

impl Drop for TerminalSession {
    fn drop(&mut self) {
        restore_terminal();
    }
}

async fn run_app(terminal: &mut DefaultTerminal, log_buffer: LogBuffer) -> Result<()> {
    let mut app = App::new(log_buffer);
    if let Some((tenant, user)) = configured_account() {
        app.load_unread_account(&tenant, &user);
    }
    if app.notification_policy.enabled() {
        app.notification_service = Some(NotificationService::start());
    }
    let mut backend = Backend::start();
    let mut events = EventStream::new().filter(|event| match event {
        Ok(Event::Mouse(mouse)) => is_actionable(*mouse),
        _ => true,
    });

    // Fire initial data loads.
    backend.send(BackendCommand::LoadTeams);
    backend.send(BackendCommand::LoadChats { limit: 50 });
    backend.send(BackendCommand::LoadUserInfo);
    backend.send(BackendCommand::LoadPresence);

    while !app.should_exit {
        // Drain log buffer before rendering to keep it from growing unbounded.
        app.debug_log.refresh();
        terminal.draw(|frame| app.render(frame))?;
        if app.acknowledge_rendered() {
            terminal.draw(|frame| app.render(frame))?;
        }

        tokio::select! {
            maybe_event = events.next() => {
                match maybe_event {
                    Some(Ok(event)) => {
                        app.handle_event(event, &backend);
                    }
                    Some(Err(e)) => {
                        tracing::error!("Event stream error: {:#}", e);
                    }
                    None => {
                        // Event stream ended.
                        break;
                    }
                }
            }
            maybe_response = backend.recv() => {
                match maybe_response {
                    Some(response) => {
                        app.handle_backend_response(response, &backend);
                    }
                    None => {
                        // Backend channel closed.
                        break;
                    }
                }
            }
        }
    }

    backend.shutdown().await;
    Ok(())
}

#[cfg(test)]
#[path = "mouse_tests.rs"]
mod mouse_tests;

#[cfg(test)]
mod notification_tests {
    use super::*;
    use crate::api;
    use crate::tui::activity::IncomingMessage;
    use crate::tui::sidebar::Chat;

    #[test]
    fn only_live_events_enqueue_alerts_and_focus_changes_take_effect() {
        let mut app = App::new(LogBuffer::new());
        app.notification_policy = NotificationPolicy::new(true);
        let (service, mut captured) = NotificationService::for_test();
        app.notification_service = Some(service);
        let (backend, _) = Backend::for_test();
        app.handle_backend_response(
            BackendResponse::UserInfo(Ok(api::UserInfo {
                id: "me".into(),
                display_name: "Me".into(),
                mail: None,
            })),
            &backend,
        );
        app.current_chat_id = Some("chat".into());
        app.sidebar.chats.push(Chat {
            id: "chat".into(),
            name: "Research".into(),
            is_group: true,
            name_source: crate::api::ChatNameSource::Topic,
            unread: Default::default(),
            online: false,
        });
        let message = IncomingMessage {
            chat_id: "chat".into(),
            id: "new".into(),
            sender_id: "https://example.test/contacts/8:orgid:other".into(),
            sender: "Alice".into(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            content: "Hello".into(),
            mentions: Vec::new(),
        };
        app.handle_backend_response(
            BackendResponse::Messages {
                chat_id: "chat".into(),
                result: Ok(vec![api::MessageInfo {
                    id: message.id.clone(),
                    sender_id: message.sender_id.clone(),
                    sender: message.sender.clone(),
                    timestamp: message.timestamp.clone(),
                    content: message.content.clone(),
                    mentions: vec!["me".into()],
                }]),
            },
            &backend,
        );
        assert!(captured.try_recv().is_err(), "history cannot show alerts");
        app.handle_event(Event::FocusGained, &backend);
        app.handle_backend_response(BackendResponse::IncomingMessage(message.clone()), &backend);
        assert!(
            captured.try_recv().is_err(),
            "foreground non-mention is quiet"
        );
        app.handle_event(Event::FocusLost, &backend);
        app.handle_backend_response(BackendResponse::IncomingMessage(message.clone()), &backend);
        assert!(
            captured.try_recv().is_err(),
            "focus loss cannot replay messages"
        );
        let mut next = message.clone();
        next.id = "next".into();
        app.handle_backend_response(BackendResponse::IncomingMessage(next), &backend);
        let queued = captured.try_recv().unwrap();
        assert_eq!(queued.notification.summary, "Alice — Research");
        assert_eq!(queued.notification.body, "Hello");
        app.handle_event(Event::FocusGained, &backend);
        let mut mention = message;
        mention.id = "mention".into();
        mention.mentions = vec!["8:orgid:me".into()];
        app.handle_backend_response(BackendResponse::IncomingMessage(mention), &backend);
        assert!(captured
            .try_recv()
            .unwrap()
            .notification
            .summary
            .contains("mentioned you"));
    }
}

#[cfg(test)]
#[path = "activity_tests.rs"]
mod activity_tests;

#[cfg(test)]
#[path = "unread_tests.rs"]
mod unread_tests;
