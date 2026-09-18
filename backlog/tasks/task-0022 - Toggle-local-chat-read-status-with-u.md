---
id: TASK-0022
title: Toggle local chat read status with u
status: Done
assignee:
  - '@codex'
created_date: '2026-09-18 14:09'
updated_date: '2026-09-18 14:14'
labels: []
dependencies: []
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Let users manually keep a chat unread or clear unread activity without opening it.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 Plain u toggles the selected sidebar chat or open message chat without interfering with compose, search, overlays or modified keys.
- [x] #2 Manual unread persists across refresh and restart, survives current-chat redraws, and clears after leaving and reopening with latest content rendered.
- [x] #3 Mark read clears known unread activity without suppressing newer incoming messages; existing saved state remains compatible.
- [x] #4 Document shortcut and validate regression tests, formatting, lint and terminal behavior.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Add backward-compatible persisted manual unread state and explicit read/unread operations.
2. Bind plain u by focused pane and prevent redraw acknowledgment until the manually marked open chat is left and reopened.
3. Update help and documentation; test persistence, refresh, navigation, new arrivals and input isolation.
4. Run Rust tests, formatting, Clippy and terminal checks; commit on the isolated feature branch.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
User approved the previously described implementation with: ok implement it in a new branch. Work is isolated in ost-toggle-unread, based on the activity integration branch.

Implemented persistent manual reminders, explicit local read acknowledgment, focused-pane u routing, and redraw hold until leaving/reopening. Added disk compatibility, refresh, new-arrival, redraw/navigation and input-isolation regressions. Found the existing unread UI tests were not registered in app.rs; registered the module so all its regressions now execute. 161 tests pass.
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Added plain u to toggle the focused conversation read/unread locally. Manual reminders persist per account and survive refreshes and redraws until explicitly cleared or the conversation is left and reopened with its newest content displayed. Explicit read clears known activity while allowing later messages to become unread; compose/search typing and modified shortcuts retain their behavior. Help and README describe the shortcut.

Added persistence, backward compatibility, stale snapshot, navigation and keyboard regressions, and registered the previously disconnected unread UI test module. Validation: 161 tests passed, cargo fmt and diff checks passed, all-target Clippy passed with existing warnings, cargo build passed, and offline normal/error/panic PTY checks passed. No remote read receipts are sent.
<!-- SECTION:FINAL_SUMMARY:END -->
