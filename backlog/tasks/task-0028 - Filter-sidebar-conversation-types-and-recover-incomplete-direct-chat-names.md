---
id: TASK-0028
title: Filter sidebar conversation types and recover incomplete direct-chat names
status: Done
assignee:
  - '@codex'
created_date: '2026-09-24 10:52'
updated_date: '2026-09-24 11:02'
labels: []
dependencies: []
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Internal activity streams and Teams channel threads appear as unknown chats; direct chats with incomplete rosters cannot recover participant names.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 Internal activity feeds are excluded from Chats and from background name resolution.
- [x] #2 Team and channel entries use native titles under Teams instead of appearing as unnamed chats, preserving navigation and existing hierarchy.
- [x] #3 Direct-chat names recover from validated peer identity or history when the roster only contains self, without naming groups from a single sender.
- [x] #4 Regression, live read-only account checks and TUI validation pass.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Classify native conversation types and keep internal streams out of chat resolution and activity polling.
2. Merge native team/channel metadata into the Teams hierarchy without losing Graph entries or expansion state.
3. Recover incomplete direct-chat roster peers from validated conversation identity and history, preserving group-safe naming.
4. Add regression tests, run live UPM read-only verification, formatting, tests, Clippy, release build and PTY checks.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Removed the clean secondary worktree after confirming its commit was already merged. The fix branch is now checked out in the primary directory and includes the dev README commit. Added type filtering, native Teams/channel hierarchy supplementation, validated direct-chat peer recovery and preserved channel polling. Initial regression suite: 185 passed.

Live UPM read-only verification: 32 real chats, 14 channel summaries retained under 18 teams, all 16 untitled chats resolved, zero unresolved in the tested 50-entry snapshot.
Final validation: 186 offline tests passed; formatting, diff check, Clippy, release build and PTY normal/error/panic checks passed. Existing compiler/lint warnings remain. Regression coverage includes feed filtering, native parent metadata and Graph merge deduplication, direct-ID validation, group-safe history fallback, and retained channel unread/selection/drafts.
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Keep internal activity feeds and channel threads out of Chats. Team/channel entries use native titles and parent metadata to supplement the Graph Teams hierarchy; channel activity polling and unread summaries remain enabled. Internal feed invalidations are excluded from message polling.
Recover direct-chat peer names when the roster is missing or self-only by validating native conversation UUIDs against the signed-in account, then using directory lookup or unambiguous peer history. Group chats never use single-sender history as their title.
Validation: 186 automated tests passed, live UPM snapshot resolved all 16 untitled chats and retained 14 channels under Teams, and formatting, Clippy, release build and PTY cleanup checks passed. Native enumeration remains bounded to recent entries and existing Graph coverage; service failures retain the established unknown-name fallback.
<!-- SECTION:FINAL_SUMMARY:END -->
