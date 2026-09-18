---
id: TASK-0015
title: Desktop notifications for messages mentioning me
status: In Progress
assignee:
  - '@codex-notifications'
created_date: '2026-02-06 00:13'
updated_date: '2026-09-18 12:02'
labels:
  - tui
  - feature
dependencies: []
references:
  - src/tui/backend.rs
  - src/tui/app.rs
priority: medium
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
When a new message arrives that mentions the current user (by @name or @mention), show a desktop notification via the system notification daemon (e.g. notify-send on Linux). This lets the user keep the TUI in the background and still be alerted when someone needs their attention.

Should detect mentions in incoming messages from the trouter/websocket event stream and trigger a desktop notification with the sender name, channel/chat name, and a snippet of the message content.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 A desktop notification appears when a new message mentions the current user
- [x] #2 Notification shows sender name, channel/chat name, and message snippet
- [x] #3 Notifications only fire for messages from others, not the user's own messages
- [x] #4 Works with notify-send or equivalent Linux notification mechanism
- [x] #5 New incoming messages in background conversations, and in the selected conversation when terminal focus is lost, can show desktop alerts; foreground non-mentions are suppressed.
- [ ] #6 Historical loads, repeated push deliveries, and fallback refreshes do not replay desktop notifications.
- [x] #7 Notification execution is asynchronous, safely passes sender and message text as literal arguments, supports an opt-out, and tolerates an unavailable notification daemon.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Implement an independently testable notification policy for incoming activity, current-user identity, terminal focus, mentions, and duplicate suppression.
2. Deliver notifications asynchronously via notify-send with literal arguments, bounded execution, and a documented opt-out.
3. Integrate with the shared incoming-activity event contract from the live-updates branch and verify the combined behavior on the integration branch.
4. Run focused policy/argument tests, normal Rust checks, and an offline terminal smoke test; document any live-delivery verification limits.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
User explicitly approved implementation in three subagents and three feature branches. Plan review authorized by that instruction; implementation can proceed.

Implementing a bounded asynchronous notify-send worker and identity-based policy. Incoming event baseline/dedup contract shared with TASK-0019; no historical message responses will invoke notifications.

Implemented notification policy, bounded nonblocking queue (16), notify-send timeout (2 seconds), stale catch-up/queue dropping, metadata identity checks, and focus-aware app hooks. Captured-queue app regression verifies historical loads never enqueue, focused non-mentions stay quiet, new unfocused messages and true mentions enqueue, and duplicates stay quiet across focus changes. Fake helper tests verify literal -- positional arguments, markup escaping, missing/failing helpers, and killing a hung helper. Initial full default suite passed 118 tests (1 offline PTY fixture ignored); final lint/build/PTY checks underway. Actual live reception is provided by the separately developed live-updates branch, so combined event-flow acceptance remains with integration.

Final validation passed: cargo test --locked (118 passed, 1 ignored); notification focused tests after final snippet fix (9 passed); cargo clippy --locked --all-targets (baseline warnings only); cargo build --locked; cargo fmt -- --check; git diff --check; offline tests/tui_mouse_pty.py normal/error/panic sessions. No actual Teams messages or real desktop alerts were sent. App service capture is available for root combined integration tests. Task remains In Progress pending integration AC 1 and 6.
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Added desktop notification policy and app hooks for incoming activity: background conversations, unfocused terminal messages, and explicit account-ID mentions enqueue sender/conversation/message previews. Own messages, foreground non-mentions, duplicate deliveries, and old catch-up messages are suppressed.

Delivery uses a bounded asynchronous notify-send worker with literal arguments, escaped body markup, subprocess timeout, graceful service failure, and OST_NOTIFICATIONS=off opt-out. Tests cover policy, captured app event flow, bounded queue, argument safety, and absent/failing/hung helpers. Default suite: 118 passed; Clippy/build/format/diff checks and offline terminal normal/error/panic checks passed.

The independent branch consumes the shared incoming-activity contract; live delivery and reconnect/fallback acceptance are completed on the integration branch. No live message was sent or desktop alert displayed during validation.
<!-- SECTION:FINAL_SUMMARY:END -->
