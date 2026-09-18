---
id: TASK-0020
title: Track and display unread chat and channel activity
status: Done
assignee:
  - '@codex-unread'
created_date: '2026-09-18 11:52'
updated_date: '2026-09-18 12:16'
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
- [x] #1 Unread or new activity is visibly distinguished in chats and channels, including collapsed team summaries, and survives sidebar refresh.
- [x] #2 Unread tracking is keyed by conversation and message identity, excludes own and duplicate messages, and uses server unread metadata when available without inventing historical counts.
- [x] #3 Per-account local read state survives restarts and does not modify Teams read receipts; unavailable historical state is described accurately.
- [x] #4 Focused unread, rendering, and persistence tests plus applicable suite, formatting, and lint checks pass; documentation explains unread behavior.
- [x] #5 Unread activity clears after successful display of the newest content in a focused terminal or an authoritative server read marker covering that activity; absent metadata, failed loads, background or covered panes, and scrolling preserve newer unread activity.
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

Implemented account-scoped local unread reducer, native consumption-horizon dots, stable sidebar selection and badge preservation, collapsed-team summaries, and successful-render/focus gating. Incoming activity waits for user identity; no message text is persisted and no Teams read receipts are sent. Authoritative server read watermarks clear only covered activity. Added reducer, persistence, native metadata, and rendered-state regressions; combined live/notification wiring remains for root integration.

Validation: cargo test --locked --no-fail-fast passed 124 tests; 1 existing PTY fixture ignored for root combined verification. cargo clippy --locked --all-targets completed with existing repository warnings; cargo fmt --all -- --check and git diff --check passed. New tests cover native horizon parsing, identity/self/duplicate exclusion, absent metadata, external read coverage, local persistence and corruption, no-op write avoidance, failed/tiny/scrolled/tall/unfocused/overlaid rendering, and sidebar reorder/collapsed summaries. AC5 left open pending root combined live/notification integration and PTY run.

Integration review clarified that authoritative server read markers may acknowledge older activity without an OST render. The local client still never sends Teams read receipts. Combined regression tests cover focus, duplicate suppression, and delayed identity shared with notifications.

Combined integration verification complete: 147 tests passed; formatting, default Clippy, release build, and PTY mouse/focus cleanup checks passed. Release executable installed with a backup; existing user session left running until restart.
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Added unread badges for chats, channels, and collapsed teams, preserving state and selection across refresh and reorder. Distinct locally observed incoming messages get counts; native read markers seed dots when historical counts are unknown. Own messages and duplicate events do not increase counts.

Read state is saved atomically per tenant/account without message content or outbound Teams read receipts. Activity clears only after successful rendering of the latest content in a focused, unobscured pane, or an authoritative server read marker that covers it. Newer activity survives stale server snapshots, failed loads, and scrolling. Badge changes redraw immediately without repeatedly saving unchanged state.

Validated persistence, metadata, identity, focus, tall-message, reload, and cross-feature behavior in 147 passing combined Rust tests. Formatting, default all-targets Clippy, release build, and normal/error/panic terminal checks passed; real API metadata loaded successfully in the release smoke test.
<!-- SECTION:FINAL_SUMMARY:END -->
