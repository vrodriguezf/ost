---
id: TASK-0019
title: Receive live messages and show TUI connection health
status: In Progress
assignee:
  - '@codex-live'
created_date: '2026-09-18 11:52'
updated_date: '2026-09-18 12:10'
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
- [x] #1 Incoming messages update the selected conversation and recent-chat list without user input, with reconnection and bounded fallback refresh after missed events.
- [x] #2 The TUI shows actual push connection, reconnecting, and degraded states independently of user presence.
- [x] #3 Background updates preserve compose text, sidebar selection, search interaction, and message scroll position; duplicate or stale responses cannot reorder or repeat messages.
- [x] #4 The receive loop shuts down cleanly, never prints into the alternate screen or activates call handling, and handles network or expired-token failures without silently freezing.
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

Shared activity contract committed and compiled. Implemented message-only Trouter registration with setup/heartbeat deadlines, cancellation and bounded reconnect; token renewal now covers Skype expiry and serializes refresh rotation. Backend coalesces chat invalidations, fetches authoritative messages, and suppresses baseline history/replayed events. Message viewport work and offline tests in progress.

Feature validation: 123 offline tests passed, 1 PTY fixture ignored in the ordinary suite; dedicated PTY runner passed normal/error/panic exits. Formatting and Clippy pass (existing repository warnings remain). Root also verified the real account in an isolated config reached Live after WebSocket and message registration and exited cleanly; no real incoming message was sent or tested. Kept AC5 pending combined-branch validation.
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
The TUI now receives message activity through a quiet, message-only Trouter subscription, reconciles affected conversations, and checks for missed updates every 30 seconds. The header reports actual push health separately from Teams presence, with bounded reconnects, credential renewal, and clean task cancellation.

Stable message identities prevent duplicate or stale history replacement. Background refreshes retain draft text, search selection, message selection, and the rendered reading anchor; scrolling at the latest message follows new arrivals. Initial history and reconnect replays do not emit duplicate incoming activity. Incoming events include sender identity and native explicit mention MRIs for unread and notification consumers.

Validation: 123 tests passed; formatting and Clippy pass with inherited warnings. The existing SGR PTY checks pass for normal/error/panic cleanup. Root verified a real read-only push connection reached Live, but delivery of a newly sent message was not exercised. Combined feature validation remains pending in the integration branch.
<!-- SECTION:FINAL_SUMMARY:END -->
