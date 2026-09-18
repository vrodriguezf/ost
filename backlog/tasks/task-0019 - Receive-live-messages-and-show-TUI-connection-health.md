---
id: TASK-0019
title: Receive live messages and show TUI connection health
status: In Progress
assignee:
  - '@codex-live'
created_date: '2026-09-18 11:52'
updated_date: '2026-09-18 11:52'
labels:
  - tui
  - feature
dependencies: []
references:
  - src/tui/backend.rs
  - src/tui/app.rs
  - src/trouter/mod.rs
priority: high
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Complete the deferred live reception from TASK-0002 so the TUI refreshes incoming messages and conversations while idle and accurately reports the push connection state.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Incoming messages update the selected conversation and recent-chat list without user input, with reconnection and bounded fallback refresh after missed events.
- [ ] #2 The TUI shows actual push connection, reconnecting, and degraded states independently of user presence.
- [ ] #3 Background updates preserve compose text, sidebar selection, search interaction, and message scroll position; duplicate or stale responses cannot reorder or repeat messages.
- [ ] #4 The receive loop shuts down cleanly, never prints into the alternate screen or activates call handling, and handles network or expired-token failures without silently freezing.
- [ ] #5 Focused offline tests, the existing test suite, formatting, linting, and terminal interaction checks pass; user documentation describes live updates.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Extract a quiet, cancellable push subscription using existing session/WebSocket/registration helpers; publish typed incoming activity and real connection health to the TUI backend.
2. Refresh only affected conversations and coalesce requests; add a bounded periodic catch-up path and recover credentials on renewal/reconnect.
3. Retain message identity and sender identity in API data, keep navigation and reading position stable during background refresh, and avoid call side effects.
4. Add receive/dedup/recovery/viewport tests and document live status; validate Rust checks and existing PTY interaction.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Implementation and three parallel feature branches approved by the user. Shared event contract will be coordinated with unread and notification agents.
<!-- SECTION:NOTES:END -->
