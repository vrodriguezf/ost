---
id: TASK-0020
title: Track and display unread chat and channel activity
status: In Progress
assignee:
  - '@codex-unread'
created_date: '2026-09-18 11:52'
updated_date: '2026-09-18 11:52'
labels:
  - tui
  - feature
dependencies: []
references:
  - src/tui/sidebar.rs
  - src/tui/app.rs
  - src/api/chat.rs
priority: high
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Complete the deferred unread behavior from TASK-0002. Preserve real unread metadata where available and track unseen incoming activity, with visible sidebar badges and read state that survives refresh and restart.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Unread or new activity is visibly distinguished in chats and channels, including collapsed team summaries, and survives sidebar refresh.
- [ ] #2 Unread tracking is keyed by conversation and message identity, excludes own and duplicate messages, and uses server unread metadata when available without inventing historical counts.
- [ ] #3 Messages clear unread activity only after successful display at the newest message in the focused terminal; background, unfocused, scrolled-up, and failed loads preserve unread state.
- [ ] #4 Per-account local read state survives restarts and does not modify Teams read receipts; unavailable historical state is described accurately.
- [ ] #5 Focused unread, rendering, and persistence tests plus applicable suite, formatting, and lint checks pass; documentation explains unread behavior.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Add per-account unread/read tracking keyed by conversation and message identity, preserving authoritative unread metadata where supplied by the native API.
2. Wire sidebar badges and collapsed-team summaries to state that survives reload, reordering, and restart.
3. Consume incoming-activity events and acknowledge locally only after the newest content has actually been rendered in a focused terminal, preserving scrolled-up/background unread activity.
4. Add state, persistence, and rendering tests plus documentation; verify integration with live updates and desktop notifications.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Implementation and three parallel feature branches approved by the user. No server read-receipt mutation is required.
<!-- SECTION:NOTES:END -->
