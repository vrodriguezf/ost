---
id: TASK-0024
title: Follow new messages at the bottom and reveal successful sends
status: Done
assignee:
  - '@codex'
created_date: '2026-09-18 15:28'
updated_date: '2026-09-18 15:36'
labels:
  - tui
dependencies: []
references:
  - src/tui/messages.rs
  - src/tui/mouse.rs
  - src/tui/app.rs
  - src/tui/backend.rs
  - src/tui/mouse_tests.rs
priority: high
type: bug
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
The message viewport can remain anchored after sending, hiding the new reply while compose retains focus. Wheel scrolling disables selection following even after returning to the bottom, which also prevents incoming messages from staying visible. Distinguish following the latest messages from preserving a position in older history.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 A successful send reveals the newest message in the originating conversation without requiring message-pane focus or manual scrolling.
- [x] #2 Incoming messages keep the view at the bottom when it was already at the bottom, including after scrolling back down with the mouse.
- [x] #3 Incoming updates preserve the reading anchor when the user is above the bottom; compose focus and drafts remain unchanged.
- [x] #4 Failed sends and send completions for a different conversation do not move the active conversation viewport.
- [x] #5 Regression tests cover outgoing and incoming refreshes, mouse return to bottom, scrolled-up history, long messages, failed sends, and conversation switching; existing checks pass.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Separate following the bottom from following keyboard selection in the message pane. Detect the viewport bottom from rendered geometry, retain history anchors above the bottom, and keep bottom-following effective for wrapped or tall messages.
2. Include the originating chat ID in send completions; on success, request a refresh for that chat and reveal its latest messages if still active, retaining compose focus. Failed sends and completions for another chat must not move the active viewport.
3. Add offline render/event regressions for successful sends from a scrolled-up view, incoming messages at bottom after mouse scrolling, anchored reading above bottom, tall messages, failures, and switching chats during a send.
4. Run the complete Rust suite, formatting, default-feature Clippy, PTY interaction checks, and diff review; finalize Backlog metadata.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Investigation confirmed that wheel scrolling and message clicks disable follow_selection; update_messages follows new arrivals only when this flag is true and the last message is selected. Returning to the bottom with the wheel does not re-enable it. MessageSent currently reloads history without changing scroll intent and carries no chat ID. Existing TASK-0019 tests deliberately preserve the anchor above the bottom. Prior identity fix remains unchanged in the current working tree. Awaiting the required approval for this additional implementation plan.

User approved the plan and requested implementation in a subagent on a new branch. Created fix/tui-message-autoscroll in isolated worktree /home/victor/Github/ost-message-autoscroll from e0c4d6b; preserving the uncommitted identity fix in the original checkout.

Implemented persistent bottom-following separately from selection navigation and rendered unread visibility. Send completions now carry the originating chat ID; successful sends reveal the active originating chat and queue backend reconciliation without changing subscriptions for inactive chats. Added offline regressions for stale/queued refreshes, tall outgoing/incoming messages, wheel return to bottom, reading anchors, failures, and chat switching.

Validation passed in isolated scrolling branch: cargo test --locked (167 passed, 1 ignored PTY fixture), cargo build --locked, cargo fmt -- --check, default-feature cargo clippy --locked --all-targets, git diff --check, and python3 tests/tui_mouse_pty.py (normal/error/panic terminal cleanup). Compiler and Clippy report existing warnings. Self-review completed; root is validating compatibility with the separate identity fix.

Root compatibility review: applied committed scrolling change 16e7873 alongside the existing uncommitted TASK-0023 identity fix in an isolated temporary worktree. Patches combined without conflicts; combined suite passed 170 tests (1 intentionally ignored fixture), formatting and diff checks. Root source and installed executable remain unchanged.
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Successful sends now reveal the latest messages in their originating conversation while retaining compose focus. Incoming refreshes follow the actual rendered bottom, including after mouse scrolling back down; reading older history keeps its anchor. Following the bottom remains stable for tall messages and queued or stale history responses. Keyboard, mouse, and search navigation can stop following.

Send results carry their chat ID so failures and completions for another conversation cannot move the active viewport. Backend reconciliation refreshes the sent-to conversation without switching the active subscription. README live-update guidance was updated.

Validation: 167 Rust tests passed; build, formatting, default-feature Clippy, diff checks, and normal/error/panic PTY interaction checks passed. Six new render/event regressions cover successful/failed sends, incoming updates, tall messages, wheel return to bottom, retained drafts and reading anchors, stale refreshes, and conversation switching. Existing compiler/Clippy warnings remain.

Compatibility validation with the separate own-message identity fix also passed: 170 tests, formatting, and diff checks; patches apply together without conflicts.
<!-- SECTION:FINAL_SUMMARY:END -->
