//! Sidebar widget: Teams hierarchy with collapsible teams/channels and Chats list.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Widget},
};

use super::mouse::{HitMap, Target, Viewport};
use super::unread::{Badge, UnreadState};
use crate::api;

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

/// A channel inside a team.
#[derive(Clone)]
pub struct Channel {
    pub name: String,
    /// The real channel/thread ID from the API.
    pub id: String,
    /// Known incoming count and an optional unknown historical backlog.
    pub unread: Badge,
}

/// A team containing channels.
#[derive(Clone)]
#[allow(dead_code)]
pub struct Team {
    pub name: String,
    /// The real team ID from the API.
    pub id: String,
    pub expanded: bool,
    pub channels: Vec<Channel>,
}

/// A direct-message contact.
#[derive(Clone)]
pub struct Chat {
    pub name: String,
    pub name_source: api::ChatNameSource,
    /// The real chat/conversation thread ID from the API.
    pub id: String,
    /// true = group chat (shows a different icon)
    pub is_group: bool,
    /// Known incoming count and an optional unknown historical backlog.
    pub unread: Badge,
    /// Whether this contact is online (show presence dot)
    pub online: bool,
}

/// Sidebar state: owns the data and tracks navigation.
pub struct SidebarState {
    pub teams: Vec<Team>,
    /// Whether the Teams parent reveals its hierarchy during this session.
    pub teams_expanded: bool,
    pub chats: Vec<Chat>,
    /// Index into the flat item list (0-based)
    pub selected: usize,
    pub viewport: Viewport,
    /// Whether data is still loading.
    pub loading: bool,
}

impl Default for SidebarState {
    fn default() -> Self {
        Self {
            teams: Vec::new(),
            teams_expanded: false,
            chats: Vec::new(),
            selected: 0,
            viewport: Viewport::default(),
            loading: true,
        }
    }
}

impl SidebarState {
    /// Update teams data from API response.
    pub fn update_teams(&mut self, teams: Vec<api::TeamInfo>) {
        let selected = self.selection_key();
        let old = std::mem::take(&mut self.teams);
        self.teams = teams
            .into_iter()
            .map(|t| Team {
                name: t.name,
                expanded: old
                    .iter()
                    .find(|old| old.id == t.id)
                    .is_none_or(|old| old.expanded),
                id: t.id,
                channels: t
                    .channels
                    .into_iter()
                    .map(|c| Channel {
                        name: c.name,
                        unread: old
                            .iter()
                            .flat_map(|t| &t.channels)
                            .find(|old| old.id == c.id)
                            .map_or(Badge::default(), |old| old.unread),
                        id: c.id,
                    })
                    .collect(),
            })
            .collect();
        self.restore_selection(selected);
    }

    /// Update chats data from API response.
    pub fn update_chats(&mut self, chats: Vec<api::ChatInfo>, current_user: Option<&str>) {
        let selected = self.selection_key();
        let old = std::mem::take(&mut self.chats);
        self.chats = chats
            .into_iter()
            .map(|c| {
                let previous = old.iter().find(|old| old.id == c.id);
                let known = previous.filter(|old| {
                    old.name_source != api::ChatNameSource::Identifier
                        && !old.name.trim().is_empty()
                });
                let peer_name = c.name_source == api::ChatNameSource::LastSender
                    && current_user
                        .zip(c.last_message_sender_id.as_deref())
                        .is_some_and(|(user, sender)| {
                            !sender.is_empty() && !super::unread::same_user(user, sender)
                        });
                let (name, name_source) =
                    if c.name_source == api::ChatNameSource::Topic && !c.name.trim().is_empty() {
                        (c.name, c.name_source)
                    } else if let Some(known) = known {
                        (known.name.clone(), known.name_source)
                    } else if peer_name && !c.name.trim().is_empty() {
                        (c.name, c.name_source)
                    } else {
                        (c.id.clone(), api::ChatNameSource::Identifier)
                    };
                Chat {
                    name,
                    name_source,
                    unread: previous.map_or(Badge::default(), |old| old.unread),
                    online: previous.is_some_and(|old| old.online),
                    id: c.id,
                    is_group: c.is_group,
                }
            })
            .collect();
        self.restore_selection(selected);
    }

    /// An unresolved label may be recovered from a named peer, never our own reply.
    pub fn recover_chat_name(
        &mut self,
        chat_id: &str,
        sender_id: &str,
        sender: &str,
        current_user: Option<&str>,
    ) -> bool {
        let Some(user) = current_user else {
            return false;
        };
        if sender_id.is_empty() || super::unread::same_user(user, sender_id) {
            return false;
        }
        let name = sender.trim();
        if name.is_empty() || name == "?" || name == "[unknown]" || name == sender_id {
            return false;
        }
        let Some(chat) = self.chats.iter_mut().find(|chat| chat.id == chat_id) else {
            return false;
        };
        if chat.name_source != api::ChatNameSource::Identifier {
            return false;
        }
        chat.name = name.to_owned();
        chat.name_source = api::ChatNameSource::LastSender;
        true
    }

    pub fn apply_unread(&mut self, state: &UnreadState) {
        for chat in &mut self.chats {
            chat.unread = state.badge(&chat.id);
        }
        for channel in self.teams.iter_mut().flat_map(|team| &mut team.channels) {
            channel.unread = state.badge(&channel.id);
        }
    }

    fn selection_key(&self) -> Option<(bool, String)> {
        match self.flat_items().get(self.selected)? {
            SidebarItem::Team(index) => Some((true, self.teams[*index].id.clone())),
            _ => self.selected_item_id().map(|id| (false, id)),
        }
    }

    fn restore_selection(&mut self, selected: Option<(bool, String)>) {
        if let Some((is_team, id)) = selected {
            if let Some(index) = self.flat_items().iter().position(|item| match item {
                SidebarItem::Team(index) => is_team && self.teams[*index].id == id,
                SidebarItem::Channel(team, channel) => {
                    !is_team && self.teams[*team].channels[*channel].id == id
                }
                SidebarItem::Chat(index) => !is_team && self.chats[*index].id == id,
                _ => false,
            }) {
                self.selected = index;
            }
        }
        self.clamp_selection();
    }

    /// Get the chat/channel ID of the currently selected item.
    pub fn selected_item_id(&self) -> Option<String> {
        let items = self.flat_items();
        match items.get(self.selected)? {
            SidebarItem::Channel(ti, ci) => Some(self.teams[*ti].channels[*ci].id.clone()),
            SidebarItem::Chat(ci) => Some(self.chats[*ci].id.clone()),
            _ => None,
        }
    }

    /// Get the display name of the currently selected item.
    pub fn selected_item_name(&self) -> Option<String> {
        let items = self.flat_items();
        match items.get(self.selected)? {
            SidebarItem::Channel(ti, ci) => {
                let team = &self.teams[*ti];
                let channel = &team.channels[*ci];
                Some(format!("{} > #{}", team.name, channel.name))
            }
            SidebarItem::Chat(ci) => Some(self.chats[*ci].name.clone()),
            SidebarItem::Team(ti) => Some(self.teams[*ti].name.clone()),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Flat item enumeration
// ---------------------------------------------------------------------------

/// One row in the sidebar's flat list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarItem {
    /// Selectable "TEAMS" parent toggle
    TeamsHeader,
    /// A team row (index into SidebarState.teams)
    Team(usize),
    /// A channel row (team_idx, channel_idx)
    Channel(usize, usize),
    /// "CHATS" separator
    ChatsHeader,
    /// A chat row (index into SidebarState.chats)
    Chat(usize),
}

impl SidebarState {
    /// Build a flat list of items in display order.
    pub fn flat_items(&self) -> Vec<SidebarItem> {
        let mut items = Vec::new();

        // Teams header
        items.push(SidebarItem::TeamsHeader);

        if self.teams_expanded {
            for (ti, team) in self.teams.iter().enumerate() {
                items.push(SidebarItem::Team(ti));
                if team.expanded {
                    for (ci, _ch) in team.channels.iter().enumerate() {
                        items.push(SidebarItem::Channel(ti, ci));
                    }
                }
            }
        }

        // Chats separator
        items.push(SidebarItem::ChatsHeader);

        for (ci, _chat) in self.chats.iter().enumerate() {
            items.push(SidebarItem::Chat(ci));
        }

        items
    }

    /// Total number of flat items.
    pub fn item_count(&self) -> usize {
        self.flat_items().len()
    }

    /// Move selection up.
    pub fn move_up(&mut self) {
        self.viewport.follow_selection = true;
        if self.selected > 0 {
            self.selected -= 1;
            self.skip_headers_up();
        }
    }

    /// Move selection down.
    pub fn move_down(&mut self) {
        self.viewport.follow_selection = true;
        let count = self.item_count();
        if count == 0 {
            return;
        }
        if self.selected < count - 1 {
            self.selected += 1;
            self.clamp_selection();
        }
    }

    /// Toggle the Teams parent or an individual team.
    pub fn toggle_expand(&mut self) {
        let items = self.flat_items();
        match items.get(self.selected) {
            Some(SidebarItem::TeamsHeader) => self.teams_expanded = !self.teams_expanded,
            Some(SidebarItem::Team(ti)) => {
                self.teams[*ti].expanded = !self.teams[*ti].expanded;
            }
            _ => return,
        }
        self.viewport.follow_selection = true;
    }

    /// Skip non-selectable headers when moving up.
    fn skip_headers_up(&mut self) {
        let items = self.flat_items();
        while self.selected > 0 {
            match items.get(self.selected) {
                Some(SidebarItem::ChatsHeader) => {
                    self.selected -= 1;
                }
                _ => break,
            }
        }
        // If no selectable row was found, move down instead
        if let Some(SidebarItem::ChatsHeader) = items.get(self.selected) {
            self.skip_headers_down();
        }
    }

    /// Skip non-selectable headers when moving down.
    fn skip_headers_down(&mut self) {
        let items = self.flat_items();
        let count = items.len();
        while self.selected < count - 1 {
            match items.get(self.selected) {
                Some(SidebarItem::ChatsHeader) => {
                    self.selected += 1;
                }
                _ => break,
            }
        }
    }

    /// Clamp selected index to valid range after structural changes.
    pub fn clamp_selection(&mut self) {
        let count = self.item_count();
        if count == 0 {
            self.selected = 0;
            return;
        }
        if self.selected >= count {
            self.selected = count - 1;
        }
        // After clamping, skip headers
        self.skip_headers_down();
        if matches!(
            self.flat_items().get(self.selected),
            Some(SidebarItem::ChatsHeader)
        ) {
            self.skip_headers_up();
        }
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// Render the sidebar into the given area.
pub fn render(
    area: Rect,
    buf: &mut Buffer,
    state: &mut SidebarState,
    focused: bool,
    hits: &mut HitMap,
) {
    hits.add(area, Target::Sidebar);
    let border_style = if focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let border_type = if focused {
        BorderType::Double
    } else {
        BorderType::Plain
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type)
        .border_style(border_style);

    let inner = block.inner(area);
    block.render(area, buf);

    // Show loading indicator if data hasn't arrived yet.
    if state.loading && state.teams.is_empty() && state.chats.is_empty() {
        state.viewport.prepare(0, inner.height as usize, 0..0);
        if inner.height > 0 && inner.width > 0 {
            let loading_area = Rect::new(inner.x, inner.y, inner.width, 1);
            let line = Line::from(Span::styled(
                " Loading...",
                Style::default().fg(Color::DarkGray),
            ));
            Paragraph::new(line).render(loading_area, buf);
        }
        return;
    }

    let items = state.flat_items();
    let available_height = inner.height as usize;

    if available_height == 0 || items.is_empty() {
        return;
    }

    // Compute scroll offset so selected item is visible.
    state.viewport.prepare(
        items.len(),
        available_height,
        state.selected..state.selected + 1,
    );
    let scroll_offset = state.viewport.offset;

    for (row_idx, item_idx) in (scroll_offset..items.len())
        .take(available_height)
        .enumerate()
    {
        let item = &items[item_idx];
        let ctx = RowCtx {
            area: Rect::new(inner.x, inner.y + row_idx as u16, inner.width, 1),
            selected: item_idx == state.selected,
            pane_focused: focused,
        };

        render_item(buf, &ctx, item, state);
        if !matches!(item, SidebarItem::ChatsHeader) {
            hits.add(ctx.area, Target::SidebarItem(item_idx));
        }
    }
}

/// Rendering context for a single sidebar row.
struct RowCtx {
    area: Rect,
    selected: bool,
    pane_focused: bool,
}

/// Style for a list item (channel or chat) based on selection and unread state.
fn item_style(selected: bool, has_unread: bool) -> Style {
    if selected {
        Style::default()
            .fg(Color::White)
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD)
    } else if has_unread {
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    }
}

/// Style for a badge (unread count) based on selection state.
fn badge_style(selected: bool) -> Style {
    if selected {
        Style::default()
            .fg(Color::Yellow)
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    }
}

/// Render a single sidebar item into the buffer.
fn render_item(buf: &mut Buffer, ctx: &RowCtx, item: &SidebarItem, state: &SidebarState) {
    let w = ctx.area.width as usize;
    match item {
        SidebarItem::TeamsHeader => {
            let label = if state.teams_expanded {
                "▼ TEAMS"
            } else {
                "▶ TEAMS"
            };
            let style = item_style(ctx.selected && ctx.pane_focused, false)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD);
            render_row(buf, ctx.area, label, "", style, style);
        }

        SidebarItem::Team(ti) => {
            let team = &state.teams[*ti];
            let indicator = if team.expanded {
                "\u{25BC}"
            } else {
                "\u{25B6}"
            };
            let cursor = if ctx.selected { "\u{25BA}" } else { " " };
            let label = format!("{}{} {}", cursor, indicator, team.name);

            let style = if ctx.selected {
                Style::default()
                    .fg(Color::White)
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let unread = team
                .channels
                .iter()
                .fold(Badge::default(), |sum, channel| sum.combine(channel.unread));
            let badge = if team.expanded {
                String::new()
            } else {
                unread.label()
            };
            let style = if unread.any() {
                style.add_modifier(Modifier::BOLD)
            } else {
                style
            };
            render_row(
                buf,
                ctx.area,
                &label,
                &badge,
                style,
                badge_style(ctx.selected),
            );
        }

        SidebarItem::Channel(ti, ci) => {
            let channel = &state.teams[*ti].channels[*ci];
            let cursor = if ctx.selected { "\u{25BA}" } else { " " };
            let label = format!("  {}# {}", cursor, channel.name);
            let badge = channel.unread.label();

            let style = item_style(ctx.selected, channel.unread.any());
            let bstyle = if channel.unread.any() {
                badge_style(ctx.selected)
            } else {
                style
            };

            render_row(buf, ctx.area, &label, &badge, style, bstyle);
        }

        SidebarItem::ChatsHeader => {
            // Render a separator line: " -- CHATS --------"
            let prefix = " -- CHATS ";
            let dashes = w.saturating_sub(prefix.len());
            let label = format!("{}{}", prefix, "-".repeat(dashes));
            let style = Style::default().fg(Color::DarkGray);
            render_row(buf, ctx.area, &label, "", style, style);
        }

        SidebarItem::Chat(ci) => {
            let chat = &state.chats[*ci];
            let icon = if chat.is_group {
                "\u{1F465}"
            } else {
                "\u{1F464}"
            };
            let cursor = if ctx.selected { "\u{25BA}" } else { " " };
            let label = format!("{}{} {}", cursor, icon, chat.name);
            let badge = if chat.unread.any() {
                chat.unread.label()
            } else if chat.online {
                "*".to_string()
            } else {
                String::new()
            };

            let style = item_style(ctx.selected, chat.unread.any());
            let bstyle = if chat.unread.any() {
                badge_style(ctx.selected)
            } else if chat.online {
                Style::default().fg(Color::Green)
            } else {
                style
            };

            render_row(buf, ctx.area, &label, &badge, style, bstyle);
        }
    }
}

/// Render a row with left-aligned text and an optional right-aligned badge.
fn render_row(
    buf: &mut Buffer,
    area: Rect,
    left: &str,
    badge: &str,
    text_style: Style,
    badge_style: Style,
) {
    let width = area.width as usize;
    if width == 0 {
        return;
    }

    // Truncate left text if needed, leaving room for badge + 1 space
    let badge_w = unicode_width::UnicodeWidthStr::width(badge);
    let max_left = if badge_w > 0 {
        width.saturating_sub(badge_w + 1)
    } else {
        width
    };

    // Truncate by display width, not char count
    let mut left_truncated = String::new();
    let mut left_w = 0;
    for ch in left.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if left_w + cw > max_left {
            break;
        }
        left_truncated.push(ch);
        left_w += cw;
    }

    // Padding between left text and badge
    let pad = if badge_w > 0 {
        width.saturating_sub(left_w + badge_w)
    } else {
        width.saturating_sub(left_w)
    };

    // Build the line
    let line = Line::from(vec![
        Span::styled(left_truncated, text_style),
        Span::styled(" ".repeat(pad), text_style),
        Span::styled(badge.to_string(), badge_style),
    ]);

    let row_area = Rect::new(area.x, area.y, area.width, 1);
    Paragraph::new(line).render(row_area, buf);
}
