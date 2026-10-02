---
id: TASK-0031
title: Render quoted replies with clear author and content hierarchy
status: Done
assignee:
  - '@codex'
created_date: '2026-10-02 10:10'
updated_date: '2026-10-02 10:31'
labels:
  - tui
  - ux
dependencies: []
references:
  - src/api/chat.rs
  - src/tui/messages.rs
  - src/tui/hyperlinks.rs
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Quoted replies currently lose their HTML structure before reaching the TUI, causing the quoted author and text to run together and making the quoted message indistinguishable from the new reply. Preserve the reply context and present it as a compact, readable quote inside the existing message card.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 Quoted replies distinguish the quoted author, original text, and new response; author and text never run together.
- [x] #2 Reply context uses a subtle vertical accent, a clear author or fallback label, muted quoted text, and normal emphasis for the new response within the existing message cards.
- [x] #3 Multiline text, HTML entities, Unicode, and narrow panes remain readable without losing message text; unquoted messages retain their expected presentation.
- [x] #4 Quoted reply rendering preserves complete URL targets, own-message alignment, selection, scrolling, and live refresh behavior.
<!-- AC:END -->

## Definition of Done
<!-- DOD:BEGIN -->
- [x] #1 Parser and rendering regression tests pass, including quote structure, missing author, wrapping, and links.
- [x] #2 Formatting, full test suite, linting, and diff checks complete; changes self-reviewed.
- [x] #3 Relevant documentation updated and a rendered terminal preview visually checked.
<!-- DOD:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Preserve Teams quoted-reply structure in the message parsing pipeline, including author metadata, quote text, and response text; use a clear fallback when author metadata is unavailable. Preserve paragraph boundaries and decode entities without concatenating text.
2. Carry the structured quote into the TUI and render a compact inset with a vertical accent, author label, muted wrapped quote text, and a small gap before the normal reply body. Retain current card backgrounds, selection, and own-message alignment.
3. Reuse display-width-aware wrapping and full hyperlink targets for quote content. Verify message ranges and refresh merging preserve selection and reading position.
4. Add focused parser and rendering regression tests for quoted replies, missing authors, multiline and Unicode content, narrow panes, links, and normal messages. Update relevant docs, inspect a rendered terminal preview, and run cargo test --locked, cargo fmt --check, cargo clippy --locked --all-targets, and git diff --check.
5. Complete Backlog acceptance criteria, Definition of Done, and final summary after validation.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Investigated the supplied screenshot. src/api/chat.rs::strip_html removes all tags without retaining quote or paragraph boundaries, and src/tui/messages.rs renders the resulting content as a single undifferentiated body. Existing hyperlink wrapping and scroll ranges should be reused. Plan recorded; awaiting the review required by AGENTS.md before coding.

User approved the layout and requested subagents. Implementing on fix/quoted-reply-rendering in the same checkout, with parser, integration coverage, and review delegated; keeping existing card colors, alignment, and hyperlink behavior.

Implemented structured Teams reply parsing with explicit author metadata and readable plain text for CLI/search. Quotes render as an inset with the existing author palette, a slim rail, muted text, and separation from the response; current card selection and own-message alignment are retained.
Validation: cargo test --locked (220 passed, 3 ignored); cargo fmt --check; cargo clippy --locked --all-targets (existing warnings); git diff --check; PTY normal/error/panic checks passed. Parser, rendering, mouse selection, search, draft/history preservation, Unicode wrapping, full hyperlinks, and native HTTP-response fixtures are covered.
Visually inspected /tmp/ost-reply-preview.png, generated from a 120x38 production Ratatui buffer. Independent code review found no blockers. The parser dependency graph was added without upgrading existing lockfile versions.
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Quoted replies now show the original author and quoted text in a muted inset, followed by the response at normal emphasis. This fixes author/text concatenation and makes reply context readable while preserving existing card colors, selection, sent-message alignment, and reactions.
The native message parser preserves ordered text/quote blocks, paragraph boundaries, decoded HTML entities, fallback labels, and full link destinations. CLI output and search retain readable content. Sender headers also fit narrow panes using Unicode-aware truncation.
Validation: 220 tests passed, 3 ignored; formatting, Clippy, diff checks, and normal/error/panic terminal smoke tests passed. Added parser/API and rendering regressions, quote selection/search/history tests, and a full-screen visual fixture; updated README and TUI specification. No authenticated live account requests or writes were needed.
<!-- SECTION:FINAL_SUMMARY:END -->
