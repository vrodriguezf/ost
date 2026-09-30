//! Keep URL destinations separate from visible text and terminal layout.
use std::collections::BTreeSet;

use ratatui::buffer::{Buffer, Cell};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub url: String,
}

pub struct WrappedLine {
    pub text: String,
    pub links: Vec<Link>,
}

/// Recognize explicit web URLs only. Never pass terminal control characters
/// through to an OSC command. Keep escapes and query strings unchanged.
fn urls(text: &str) -> Vec<(usize, usize)> {
    let mut result = Vec::new();
    let mut offset = 0;
    while let Some(relative) = text[offset..].find("http") {
        let start = offset + relative;
        let rest = &text[start..];
        if !rest.starts_with("https://") && !rest.starts_with("http://") {
            offset = start + 4;
            continue;
        }
        let end = rest
            .find(|c: char| {
                c.is_whitespace() || c.is_control() || matches!(c, '<' | '>' | '"' | '\'')
            })
            .unwrap_or(rest.len());
        let mut candidate = rest[..end].trim_end_matches(['.', ',', ';', '!', '?']);
        for (open, close) in [('(', ')'), ('[', ']'), ('{', '}')] {
            while candidate.ends_with(close)
                && candidate.matches(close).count() > candidate.matches(open).count()
            {
                candidate = &candidate[..candidate.len() - 1];
            }
        }
        if url::Url::parse(candidate).is_ok_and(|url| url.host_str().is_some()) {
            result.push((start, start + candidate.len()));
        }
        offset = start + end.max(4);
    }
    result
}

/// Wrap at word boundaries when possible, otherwise at grapheme boundaries.
/// Link positions use terminal columns; destinations always retain the full URL.
pub fn wrap(text: &str, width: usize) -> Vec<WrappedLine> {
    if width == 0 {
        return Vec::new();
    }
    let mut output = Vec::new();
    for raw in text.lines() {
        // Do not let message-provided controls become terminal escape sequences.
        let clean: String = raw
            .chars()
            .map(|c| if c == '\t' { ' ' } else { c })
            .filter(|c| !c.is_control())
            .collect();
        let targets = urls(&clean);
        let mut start = 0;
        if clean.is_empty() {
            output.push(WrappedLine {
                text: String::new(),
                links: Vec::new(),
            });
        }
        while start < clean.len() {
            let remaining = &clean[start..];
            let mut used = 0;
            let mut end = 0;
            let mut space = None;
            for (index, grapheme) in remaining.grapheme_indices(true) {
                let columns = grapheme.width();
                if used + columns > width {
                    break;
                }
                used += columns;
                end = index + grapheme.len();
                if grapheme.chars().all(char::is_whitespace) {
                    space = Some(end);
                }
            }
            if end == 0 {
                // A two-column grapheme cannot fit in a one-column viewport.
                start += remaining.graphemes(true).next().unwrap().len();
                output.push(WrappedLine {
                    text: "�".into(),
                    links: Vec::new(),
                });
                continue;
            }
            if end < remaining.len() {
                if let Some(boundary) = space {
                    end = boundary;
                }
            }
            let finish = start + end;
            let visible = &clean[start..finish];
            let links = targets
                .iter()
                .filter_map(|&(a, b)| {
                    let first = a.max(start);
                    let last = b.min(finish);
                    (first < last).then(|| Link {
                        x: clean[start..first].width(),
                        y: 0,
                        width: clean[first..last].width(),
                        url: clean[a..b].to_string(),
                    })
                })
                .collect();
            output.push(WrappedLine {
                text: visible.to_string(),
                links,
            });
            start = finish;
        }
    }
    output
}

/// Repaint link rows after Ratatui's normal diff. OSC metadata is deliberately
/// kept out of its buffer so escape bytes cannot corrupt its width calculations.
/// Previous rows are repainted too: identical text may now have a different URL,
/// or a popup may have replaced a link without changing every character.
#[derive(Default)]
pub struct Painter {
    rows: BTreeSet<u16>,
}

impl Painter {
    pub fn cells(&mut self, buffer: &Buffer, links: &[Link]) -> Vec<(u16, u16, Cell)> {
        let current: BTreeSet<_> = links.iter().map(|link| link.y as u16).collect();
        let rows: BTreeSet<_> = self.rows.union(&current).copied().collect();
        self.rows = current;
        let mut result = Vec::new();
        for y in rows {
            if y < buffer.area.y || y >= buffer.area.bottom() {
                continue;
            }
            let mut x = buffer.area.x;
            let mut previous_url = None;
            while x < buffer.area.right() {
                let mut cell = buffer[(x, y)].clone();
                let width = cell.symbol().width().max(1) as u16;
                let target = links.iter().find(|link| {
                    link.y == y as usize
                        && (x as usize) >= link.x
                        && (x as usize) < link.x + link.width
                });
                let url = target.map_or("", |link| link.url.as_str());
                let mut symbol = String::new();
                if previous_url != Some(url) {
                    symbol.push_str(&format!("\x1b]8;;{url}\x1b\\"));
                }
                symbol.push_str(cell.symbol());
                if x.saturating_add(width) >= buffer.area.right() {
                    symbol.push_str("\x1b]8;;\x1b\\");
                }
                cell.set_symbol(&symbol);
                previous_url = Some(url);
                result.push((x, y, cell));
                x = x.saturating_add(width);
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{
        backend::{Backend, CrosstermBackend},
        layout::Rect,
    };

    #[test]
    fn wrapped_url_keeps_destination_on_every_segment() {
        let url = format!(
            "https://example.com/{}?q=one%20two&x=3#anchor",
            "long-path/".repeat(20)
        );
        let text = format!("Read {url} then reply");
        for width in [1, 7, 20, 80] {
            let lines = wrap(&text, width);
            assert_eq!(
                lines
                    .iter()
                    .map(|line| line.text.as_str())
                    .collect::<String>(),
                text
            );
            assert!(lines.iter().all(|line| line.text.width() <= width));
            let linked_columns: usize = lines
                .iter()
                .flat_map(|line| &line.links)
                .map(|link| {
                    assert_eq!(link.url, url);
                    assert!(link.x + link.width <= width);
                    link.width
                })
                .sum();
            assert_eq!(linked_columns, url.width());
        }
    }

    #[test]
    fn unicode_graphemes_and_multiple_links_survive_wrapping() {
        let text = "界 e\u{301} 👩‍💻 https://example.com/a_(b), https://example.org/?a=1&b=2";
        for width in [2, 5, 17] {
            let lines = wrap(text, width);
            assert_eq!(
                lines
                    .iter()
                    .map(|line| line.text.as_str())
                    .collect::<String>(),
                text
            );
            assert!(lines.iter().all(|line| line.text.width() <= width));
            assert!(lines.iter().flat_map(|line| &line.links).all(|link| {
                link.url == "https://example.com/a_(b)"
                    || link.url == "https://example.org/?a=1&b=2"
            }));
        }
        assert!(wrap(text, 0).is_empty());
        assert!(wrap("界", 1).iter().all(|line| line.text.width() <= 1));
    }

    #[test]
    fn terminal_bytes_keep_full_url_and_clear_old_links() {
        let mut painter = Painter::default();
        let mut buffer = Buffer::empty(Rect::new(0, 0, 8, 2));
        buffer.set_string(0, 0, "界 click", ratatui::style::Style::default());
        let url = "https://example.com/very/long/path?x=1&y=2";
        let mut links = vec![Link {
            x: 3,
            y: 0,
            width: 5,
            url: url.into(),
        }];
        let cells = painter.cells(&buffer, &links);
        assert!(
            !cells.iter().any(|(x, _, _)| *x == 1),
            "wide-cell continuation must not be repainted"
        );
        let mut bytes = Vec::new();
        let mut backend = CrosstermBackend::new(&mut bytes);
        backend
            .draw(cells.iter().map(|(x, y, cell)| (*x, *y, cell)))
            .unwrap();
        let emitted = String::from_utf8(bytes).unwrap();
        assert!(emitted.contains(&format!("\x1b]8;;{url}\x1b\\c")));
        // Same visible text, different destination: repaint despite no text diff.
        links[0].url = "https://example.org/new".into();
        assert!(painter
            .cells(&buffer, &links)
            .iter()
            .any(|(_, _, cell)| cell.symbol().contains("https://example.org/new")));
        let cleared = painter.cells(&buffer, &[]);
        assert!(!cleared.is_empty());
        assert!(cleared
            .iter()
            .all(|(_, _, cell)| !cell.symbol().contains("https://")));
        assert!(painter.cells(&buffer, &[]).is_empty());
        painter.cells(&buffer, &links);
        assert!(painter
            .cells(&Buffer::empty(Rect::new(0, 0, 0, 0)), &[])
            .is_empty());
    }

    #[test]
    fn controls_cannot_escape_into_link_targets() {
        let lines = wrap("https://example.com/a\x1b]evil\x07", 80);
        for link in lines.iter().flat_map(|line| &line.links) {
            assert!(!link.url.chars().any(char::is_control));
        }
    }
}
