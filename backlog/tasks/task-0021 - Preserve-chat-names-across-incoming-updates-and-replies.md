---
id: TASK-0021
title: Preserve chat names across incoming updates and replies
status: Done
assignee:
  - '@codex'
created_date: '2026-09-18 12:30'
updated_date: '2026-09-18 12:37'
labels:
  - tui
  - bug
dependencies: []
references:
  - src/api/chat.rs
  - src/tui/sidebar.rs
  - src/tui/app.rs
priority: high
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Fix the live-refresh regression where a missing latest-sender display name replaces a known conversation label with its long thread ID after incoming messages are addressed.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 A known chat name survives refreshes with missing naming metadata, a thread-ID fallback, or a different latest sender; explicit conversation-title changes still take effect.
- [x] #2 Opening an unresolved chat can recover a friendly label from a named message sender other than the current user, without replacing a known title.
- [x] #3 Sidebar, open-chat header, search, and notification labels stay consistent while unread badges, selected conversation, drafts, and scroll position are preserved.
- [x] #4 Regression tests reproduce the reply/refresh failure before the fix and pass afterward; the relevant Rust suite, formatting, lint, release build, and terminal checks pass.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Distinguish authoritative conversation titles from sender-derived and identifier fallback labels in API metadata.
2. Reproduce the name-to-ID refresh regression, then preserve known names by stable conversation ID while allowing authoritative title changes.
3. Recover unresolved selected-chat labels from named non-self history and keep header/search/notifications synchronized without resetting reading position.
4. Run regression and full default tests, format/lint/release and terminal validation, then merge into the activity integration and update the installed executable.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
This repairs a regression in the previously authorized live-message refresh implementation. The user has reported the failure while continuing that work; no additional feature or permission scope is needed.

Reproduced the original failure before editing production code: reply refresh changed the label from Colleague to the conversation ID.
Added explicit title/sender/identifier provenance and stable-ID name preservation, plus non-self history recovery and synchronized open-chat headers. The regression and full default suite now pass: 151 tests.
Remaining validation: lint, terminal cleanup, release build and executable installation.

Validation complete: 151 tests passed, formatting and diff checks passed, all-target Clippy passed with existing warnings, release build passed, and PTY normal/error/panic terminal restoration checks passed. No Teams messages were sent.
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Preserve readable chat names when live refreshes omit naming metadata or report a different latest sender. Explicit conversation titles remain authoritative, and opening an unresolved chat can recover a name from a named non-self participant in history. Sidebar, headers, search and notifications share the stable label without resetting selection, drafts or scrolling.

Verified the name-to-ID regression failed before the fix and passes afterward. All 151 tests, formatting, all-target Clippy, release build and normal/error/panic PTY checks pass; existing compiler and lint warnings remain.
<!-- SECTION:FINAL_SUMMARY:END -->
