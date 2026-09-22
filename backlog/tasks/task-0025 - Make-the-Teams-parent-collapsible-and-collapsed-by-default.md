---
id: TASK-0025
title: Make the Teams parent collapsible and collapsed by default
status: Done
assignee:
  - '@codex'
created_date: '2026-09-18 15:34'
updated_date: '2026-09-18 15:44'
labels: []
dependencies: []
references:
  - src/tui/sidebar.rs
  - src/tui/app.rs
  - src/tui/mouse_tests.rs
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Keep the sidebar focused on Chats at startup and let users hide or reveal the entire Teams hierarchy with one action on its parent heading.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 Every new TUI session starts with the Teams parent collapsed and Chats visible.
- [x] #2 Clicking the Teams heading or pressing Enter on it toggles the entire Teams hierarchy.
- [x] #3 Parent toggles preserve individual team expansion states, and background refreshes preserve the parent state and valid sidebar selection.
- [x] #4 Selecting a channel through search reveals its Teams ancestors and opens the correct channel.
- [x] #5 Keyboard navigation and mouse hit targets remain correct in both parent states, with regression coverage and updated help where needed.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Add a Teams parent expansion flag defaulting to collapsed and filter the visible hierarchy without changing individual team expansion states.
2. Make the Teams heading selectable and clickable with an expansion indicator; update navigation and identity-based selection restoration.
3. Preserve parent state across refreshes and reveal the parent when opening a channel from search.
4. Add focused keyboard, mouse, refresh, and search regressions; update relevant help; run cargo test --locked, cargo fmt -- --check, cargo clippy --locked --all-targets, and git diff --check. Preserve existing unrelated working-tree changes.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Inspected sidebar rendering, navigation, refresh, mouse targets, and channel search. TEAMS is currently a non-interactive header and newly loaded individual teams default to expanded. Plan awaiting user approval as required by AGENTS.md.

Implemented selectable and clickable Teams parent, collapsed by default, with session-preserved state and search ancestor expansion. Added rendered mouse/keyboard, empty-sidebar, and refresh regressions; adapted existing fixtures to explicitly select chats or expand the parent. Full suite passed: 167 tests, 1 ignored PTY fixture. README and help updated.

Validation complete: cargo build --locked; cargo test --locked (167 passed, 1 ignored); strengthened search regression passed with both ancestors initially collapsed; cargo fmt -- --check; cargo clippy --locked --all-targets (warnings, no errors); git diff --check; Python PTY checks passed normal, error, and panic cleanup. Reviewed the diff and preserved pre-existing message/UI changes.
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
The Teams parent now starts collapsed so Chats is immediately visible. Users can click TEAMS or select it and press Enter to reveal or hide the complete hierarchy; individual team expansion states and the parent state survive refreshes during the session. Channel search reveals both ancestors before opening the selected channel.

Updated keyboard selection, mouse hit targets, README, and help. Added regressions for parent toggling, empty navigation, refresh selection, and collapsed-ancestor search.

Validation: 167 tests passed; real PTY mouse and cleanup checks passed; build, formatting, Clippy, and diff checks completed. Compiler and Clippy warnings remain. The standalone installed executable was not replaced.
<!-- SECTION:FINAL_SUMMARY:END -->
