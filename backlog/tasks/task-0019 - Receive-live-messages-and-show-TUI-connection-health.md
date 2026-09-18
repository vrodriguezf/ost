---
id: TASK-0019
title: Receive live messages and show TUI connection health
status: Done
assignee:
  - '@codex-live'
created_date: '2026-09-18 11:52'
updated_date: '2026-09-18 12:16'
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
- [x] #5 Focused offline tests, the existing test suite, formatting, linting, and terminal interaction checks pass; user documentation describes live updates.
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

Root integration read-only PTY smoke using an isolated copy of the existing account config reached Live after WebSocket and message registration, remained healthy, and exited cleanly. No deliberate Teams message sends occurred. Final combined suite pending.

Combined integration verification complete: 147 tests passed; formatting, default Clippy, release build, and PTY mouse/focus cleanup checks passed. Release executable installed with a backup; existing user session left running until restart.
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
The TUI now subscribes to messaging push events, refreshes affected conversations without input, and checks every 30 seconds for missed activity. The status reflects real messaging connection health separately from Teams presence. Bounded reconnection and credential renewal retain failed background updates for retry.

Message and sender identities support duplicate suppression, reliable mentions, and a silent history baseline. Background refresh preserves drafts, selection, search, and message scroll position. The quiet subscription neither prints into the terminal display nor dispatches calls.

Validated 147 combined Rust tests, formatting, default all-targets Clippy, release build, and normal/error/panic terminal checks including focus-capture cleanup. A real account reached Live and loaded 46 conversations through a 34-second release smoke test, then exited cleanly. Message arrival/retry paths were exercised with offline fixtures; no live test message was sent.
<!-- SECTION:FINAL_SUMMARY:END -->
