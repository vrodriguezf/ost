//! Keyboard reaction picker, bound to a service message identity.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::api::Reaction;

pub struct ReactionPicker {
    pub chat_id: String,
    pub message_id: String,
    pub choices: Vec<String>,
    pub selected: usize,
    pub pending: bool,
    pub error: Option<String>,
}

impl ReactionPicker {
    pub fn new(
        chat_id: String,
        message_id: String,
        reactions: &[Reaction],
        user: Option<&str>,
    ) -> Self {
        let mut choices: Vec<String> = ["like", "heart", "laugh", "surprised", "sad", "angry"]
            .into_iter()
            .map(String::from)
            .collect();
        for reaction in reactions.iter().filter(|r| r.is_own(user)) {
            if !choices.contains(&reaction.key) {
                choices.push(reaction.key.clone());
            }
        }
        Self {
            chat_id,
            message_id,
            choices,
            selected: 0,
            pending: false,
            error: None,
        }
    }
}

pub fn render(
    frame: &mut Frame,
    picker: &ReactionPicker,
    reactions: &[Reaction],
    user: Option<&str>,
) {
    let screen = frame.area();
    let width = screen.width.min(60);
    let height = screen.height.min(picker.choices.len().min(12) as u16 + 5);
    let area = Rect::new(
        screen.x + (screen.width - width) / 2,
        screen.y + (screen.height - height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Message reactions ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let list_area = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(2),
    );
    let items: Vec<_> = picker
        .choices
        .iter()
        .map(|key| {
            let own = reactions.iter().any(|r| r.key == *key && r.is_own(user));
            ListItem::new(format!("{} {}", if own { "Remove" } else { "Add" }, key))
        })
        .collect();
    let list = List::new(items).highlight_symbol("> ").highlight_style(
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    );
    let mut state = ListState::default().with_selected(Some(picker.selected));
    frame.render_stateful_widget(list, list_area, &mut state);
    let footer = if picker.pending {
        "Updating reaction... Esc: close"
    } else {
        "Up/Down: choose  Enter: apply  Esc: cancel"
    };
    let lines = vec![
        Line::styled(
            picker.error.as_deref().unwrap_or(""),
            Style::default().fg(Color::Red),
        ),
        Line::from(footer),
    ];
    frame.render_widget(
        Paragraph::new(lines),
        Rect::new(
            inner.x,
            inner.y + inner.height.saturating_sub(2),
            inner.width,
            inner.height.min(2),
        ),
    );
}
