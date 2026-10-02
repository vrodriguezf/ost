//! Readable message content, with native Teams quote attribution kept separate.

use scraper::{ElementRef, Html, Node};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MessageBlock {
    Text(String),
    Quote {
        author: Option<String>,
        text: String,
    },
}

pub(super) struct ParsedContent {
    pub plain_text: String,
    pub blocks: Vec<MessageBlock>,
}

/// Parse only rich-text messages as HTML: literal `<value>` and entities in a
/// plain-text message belong to the sender, not to a markup parser.
pub(super) fn parse_message_content(content: &str, is_html: bool) -> ParsedContent {
    let blocks = if is_html {
        let html = Html::parse_fragment(content);
        let mut output = BlockBuilder::default();
        visit_children(html.root_element(), &mut output, None, false, 0);
        output.finish()
    } else {
        let text = content.trim();
        if text.is_empty() {
            Vec::new()
        } else {
            vec![MessageBlock::Text(text.to_owned())]
        }
    };
    ParsedContent {
        plain_text: plain_text(&blocks),
        blocks,
    }
}

/// Retain boundaries and attribution in CLI output, notifications and previews.
fn plain_text(blocks: &[MessageBlock]) -> String {
    blocks
        .iter()
        .map(|block| match block {
            MessageBlock::Text(text) => text.clone(),
            MessageBlock::Quote { author, text } => {
                let mut lines = vec![format!(
                    "> {}",
                    author.as_deref().unwrap_or("Quoted message")
                )];
                lines.extend(text.lines().map(|line| format!("> {line}")));
                lines.join("\n")
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[derive(Default)]
struct BlockBuilder {
    blocks: Vec<MessageBlock>,
    text: String,
}

impl BlockBuilder {
    fn boundary(&mut self) {
        if !self.text.is_empty() && !self.text.ends_with('\n') {
            self.text.push('\n');
        }
    }

    fn append(&mut self, text: &str) {
        // Whitespace between source tags is indentation, not empty paragraphs.
        // Mixed text still retains literal newlines (our send API uses them).
        if text.chars().all(char::is_whitespace) && text.contains('\n') {
            if !self.text.is_empty() && !self.text.ends_with(char::is_whitespace) {
                self.text.push(' ');
            }
        } else {
            self.text.push_str(text);
        }
    }

    fn flush(&mut self) {
        let text = normalize_text(&self.text);
        if !text.is_empty() {
            self.blocks.push(MessageBlock::Text(text));
        }
        self.text.clear();
    }

    fn finish(mut self) -> Vec<MessageBlock> {
        self.flush();
        self.blocks
    }
}

fn normalize_text(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\u{a0}', " ")
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

fn is_quote(element: ElementRef<'_>) -> bool {
    matches!(element.value().name(), "blockquote" | "quote")
}

fn has_property(element: ElementRef<'_>, property: &str) -> bool {
    element
        .attr("itemprop")
        .is_some_and(|value| value.split_whitespace().any(|part| part == property))
}

/// Read explicit author metadata only. A first line or a bold phrase can be
/// ordinary quoted prose and must never silently disappear into an attribution.
fn author_element(element: ElementRef<'_>, depth: usize) -> Option<ElementRef<'_>> {
    if depth > 64 {
        return None;
    }
    for child in element.child_elements() {
        if is_quote(child) || has_property(child, "preview") {
            continue;
        }
        if ["mri", "name", "author"]
            .iter()
            .any(|property| has_property(child, property))
            && child.text().any(|text| !text.trim().is_empty())
        {
            return Some(child);
        }
        if let Some(author) = author_element(child, depth + 1) {
            return Some(author);
        }
    }
    None
}

fn parse_quote(element: ElementRef<'_>, depth: usize) -> MessageBlock {
    let author_node = author_element(element, 0);
    let author = element
        .attr("authorname")
        .or_else(|| element.attr("data-author-name"))
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.split_whitespace().collect::<Vec<_>>().join(" "))
        .or_else(|| {
            author_node.map(|node| {
                node.text()
                    .collect::<String>()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            })
        });
    let mut output = BlockBuilder::default();
    visit_children(element, &mut output, author_node, true, depth + 1);
    // Nested quotes retain their own attribution and text inside the outer
    // quote. Flattening their readable form avoids discarding nested context.
    let text = plain_text(&output.finish());
    MessageBlock::Quote { author, text }
}

fn visit_children(
    element: ElementRef<'_>,
    output: &mut BlockBuilder,
    excluded_author: Option<ElementRef<'_>>,
    in_quote: bool,
    depth: usize,
) {
    for child in element.children() {
        if let Some(element) = ElementRef::wrap(child) {
            visit_element(element, output, excluded_author, in_quote, depth + 1);
        } else if let Node::Text(text) = child.value() {
            output.append(text);
        }
    }
}

fn visit_element(
    element: ElementRef<'_>,
    output: &mut BlockBuilder,
    excluded_author: Option<ElementRef<'_>>,
    in_quote: bool,
    depth: usize,
) {
    let name = element.value().name();
    if Some(element) == excluded_author
        || matches!(name, "script" | "style")
        || (in_quote && (has_property(element, "time") || name == "legacyquote"))
    {
        return;
    }
    // Bound recursion for malformed/deep service fragments, retaining their
    // text even when we can no longer retain their detailed layout.
    if depth > 64 {
        output.boundary();
        for text in element.text() {
            output.append(text);
        }
        output.boundary();
        return;
    }
    if is_quote(element) {
        output.flush();
        output.blocks.push(parse_quote(element, depth));
        return;
    }
    if name == "br" {
        output.text.push('\n');
        return;
    }
    if name == "img" {
        if let Some(alt) = element.attr("alt") {
            output.append(alt);
        }
        return;
    }
    let block = matches!(
        name,
        "p" | "div"
            | "section"
            | "article"
            | "header"
            | "footer"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "ul"
            | "ol"
            | "li"
            | "pre"
            | "table"
            | "tr"
            | "hr"
    );
    if block {
        output.boundary();
    }
    visit_children(element, output, excluded_author, in_quote, depth);
    if name == "a" {
        if let Some(href) = element.attr("href").map(str::trim) {
            // Keep friendly-label links usable by the TUI's existing URL/OSC 8
            // renderer without changing the display model for ordinary text.
            let label: String = element.text().collect();
            if (href.starts_with("https://") || href.starts_with("http://")) && label.trim() != href
            {
                output.append(&format!(" ({href})"));
            }
        }
    }
    if matches!(name, "td" | "th") {
        output.append(" ");
    }
    if block {
        output.boundary();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> MessageBlock {
        MessageBlock::Text(value.to_owned())
    }

    fn quote(author: Option<&str>, value: &str) -> MessageBlock {
        MessageBlock::Quote {
            author: author.map(str::to_owned),
            text: value.to_owned(),
        }
    }

    #[test]
    fn native_teams_reply_keeps_attribution_and_response_separate() {
        // Microsoft's documented Skype/Teams Reply markup:
        // https://learn.microsoft.com/en-us/graph/api/chat-getallretainedmessages
        let parsed = parse_message_content(
            r#"<div>
<blockquote itemscope="" itemtype="http://schema.skype.com/Reply" itemid="42">
<strong itemprop="mri" itemid="8:orgid:alice">Alice &amp; Bob</strong><span itemprop="time" itemid="42">12:00</span>
<p itemprop="preview">han mejorado la infra</p>
</blockquote>
<p>En el devday presumieron de que habían mejorado aún más</p>
</div>"#,
            true,
        );
        assert_eq!(
            parsed.blocks,
            vec![
                quote(Some("Alice & Bob"), "han mejorado la infra"),
                text("En el devday presumieron de que habían mejorado aún más")
            ]
        );
        assert_eq!(
            parsed.plain_text,
            "> Alice & Bob\n> han mejorado la infra\n\nEn el devday presumieron de que habían mejorado aún más"
        );
    }

    #[test]
    fn named_quote_and_legacy_quote_attributes_are_explicit_authors() {
        assert_eq!(
            parse_message_content(
                "<blockquote><span itemprop='name'>Display Name</span><p itemprop='preview'>quoted text</p></blockquote>response",
                true
            ).blocks,
            vec![quote(Some("Display Name"), "quoted text"), text("response")]
        );
        assert_eq!(
            parse_message_content(
                "<quote authorname='  Mar&#237;a\n  García  '><legacyquote>[Maria, 12:00]</legacyquote>first<br>second<legacyquote>&lt;&lt;&lt;</legacyquote></quote>reply",
                true
            ).blocks,
            vec![quote(Some("María García"), "first\nsecond"), text("reply")]
        );
    }

    #[test]
    fn generic_quote_never_guesses_an_author_from_body_text() {
        let parsed = parse_message_content(
            "<blockquote><b>Not an author</b><p>The quoted body</p></blockquote><p>Reply</p>",
            true,
        );
        assert_eq!(
            parsed.blocks,
            vec![quote(None, "Not an author\nThe quoted body"), text("Reply")]
        );
        assert!(parsed
            .plain_text
            .starts_with("> Quoted message\n> Not an author"));
    }

    #[test]
    fn blocks_preserve_text_on_both_sides_and_multiple_quotes() {
        assert_eq!(
            parse_message_content(
                "<p>Before</p><blockquote>One</blockquote><div>Between</div><blockquote>Two</blockquote><p>After</p>",
                true
            ).blocks,
            vec![text("Before"), quote(None, "One"), text("Between"), quote(None, "Two"), text("After")]
        );
    }

    #[test]
    fn html_entities_boundaries_and_full_hyperlinks_survive() {
        let url = "https://example.com/very/long/path?first=1&second=2";
        let html = format!(
            "<p>A&nbsp;&amp;&nbsp;B &lt; 10 &#x1F44D; &eacute; &amp;lt;</p><div>next<br/>line</div><blockquote><a href='{}'>the docs</a></blockquote>",
            url.replace('&', "&amp;")
        );
        assert_eq!(
            parse_message_content(&html, true).blocks,
            vec![
                text("A & B < 10 👍 é &lt;\nnext\nline"),
                quote(None, &format!("the docs ({url})"))
            ]
        );
        assert_eq!(
            parse_message_content(&format!("<p><a href='{url}'>{url}</a></p>"), true).blocks,
            vec![text(url)]
        );
    }

    #[test]
    fn plain_text_preserves_comparisons_literals_entities_and_newlines() {
        let value = "Use <value> when 1 < 2 > 0\nKeep &amp; and **markup** literal";
        let parsed = parse_message_content(value, false);
        assert_eq!(parsed.blocks, vec![text(value)]);
        assert_eq!(parsed.plain_text, value);
        assert_eq!(
            parse_message_content("<p>1 < 2 > 0</p>", true).blocks,
            vec![text("1 < 2 > 0")]
        );
    }

    #[test]
    fn nested_quotes_keep_each_author_and_all_context() {
        assert_eq!(
            parse_message_content(
                "<blockquote authorname='Alice'>Outer before<blockquote><strong itemprop='mri'>Bob</strong><p>Inner</p></blockquote>Outer after</blockquote>Response",
                true
            ).blocks,
            vec![quote(Some("Alice"), "Outer before\n\n> Bob\n> Inner\n\nOuter after"), text("Response")]
        );
    }

    #[test]
    fn malformed_and_empty_quotes_remain_readable() {
        let parsed = parse_message_content("<p>Before<blockquote>Open quote<p>Still here", true);
        assert_eq!(
            parsed.blocks,
            vec![text("Before"), quote(None, "Open quote\nStill here")]
        );
        assert_eq!(
            parse_message_content("<blockquote></blockquote><p>Reply</p>", true).blocks,
            vec![quote(None, ""), text("Reply")]
        );
    }

    #[test]
    fn preview_mentions_do_not_become_quote_authors_and_emoji_alt_is_kept() {
        assert_eq!(
            parse_message_content(
                "<blockquote><p itemprop='preview'>Hello <span itemprop='name'>Alice</span><img alt='🙂'></p></blockquote>",
                true
            ).blocks,
            vec![quote(None, "Hello Alice🙂")]
        );
    }
}
