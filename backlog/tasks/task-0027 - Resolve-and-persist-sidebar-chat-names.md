---
id: TASK-0027
title: Resolve and persist sidebar chat names
status: Done
assignee:
  - '@codex'
created_date: '2026-09-23 14:29'
updated_date: '2026-09-23 14:39'
labels: []
dependencies: []
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Resolve participant labels without opening chats or depending on the last sender; retain names across restarts.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 Untitled chats resolve participant names in the background, excluding self and using group member labels.
- [x] #2 Account-scoped names survive restart and incomplete responses while confirmed changes replace stale labels.
- [x] #3 History is a fallback and unresolved chats show Unknown conversation rather than IDs.
- [x] #4 Regression tests and TUI validation pass.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Verify native member/profile lookup.
2. Add account-scoped persistent names and bounded background resolution.
3. Preserve labels on refresh, use history fallback, and test restart/own-message/group cases.
4. Run formatting, tests, Clippy, release build and PTY checks.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Verified native roster GET and regional middleTier beta/users/fetch with existing Teams AAD token. Added separate bounded resolver, account-scoped atomic cache, explicit chat type parsing, and group-safe fallback. Implementation remains isolated in the requested branch/worktree.

179 offline tests pass. Live resolver check against the UPM account resolved two recent untitled chats without opening them. Clippy passes with existing project warnings; release and PTY validation underway.

Final validation: 179 tests passed (two intentionally ignored), live UPM resolver passed, cargo fmt and diff checks passed, Clippy passed with existing warnings, release build passed, and PTY normal/error/panic cleanup checks passed. Reviewed stale-title handling and response ordering; original dev checkout remains clean.
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Resolve sidebar chat names independently of the latest sender. Untitled conversations use native participant rosters and the regional Teams directory; one-to-one history remains a fallback. A bounded background worker refreshes labels without blocking message commands.

Persist validated names in an atomic account-scoped cache, retain them on failed/incomplete responses, and display Unknown conversation when unresolved. Preserve explicit titles, selection, drafts, search and reading position. Correct one-to-one classification using Teams metadata.

Validation: 179 offline tests passed; live UPM lookup resolved two recent untitled chats; formatting, Clippy, release build and normal/error/panic PTY checks passed. Existing project warnings remain. Native service lookup failures retain cached labels and retry.
<!-- SECTION:FINAL_SUMMARY:END -->
