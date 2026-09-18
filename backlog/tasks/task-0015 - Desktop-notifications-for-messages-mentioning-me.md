---
id: TASK-0015
title: Desktop notifications for messages mentioning me
status: In Progress
assignee:
  - '@codex-notifications'
created_date: '2026-02-06 00:13'
updated_date: '2026-09-18 11:52'
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
- [ ] #2 Notification shows sender name, channel/chat name, and message snippet
- [ ] #3 Notifications only fire for messages from others, not the user's own messages
- [ ] #4 Works with notify-send or equivalent Linux notification mechanism
- [ ] #5 New incoming messages in background conversations, and in the selected conversation when terminal focus is lost, can show desktop alerts; foreground non-mentions are suppressed.
- [ ] #6 Historical loads, repeated push deliveries, and fallback refreshes do not replay desktop notifications.
- [ ] #7 Notification execution is asynchronous, safely passes sender and message text as literal arguments, supports an opt-out, and tolerates an unavailable notification daemon.
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
<!-- SECTION:NOTES:END -->
