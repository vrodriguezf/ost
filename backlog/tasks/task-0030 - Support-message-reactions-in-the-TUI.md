---
id: TASK-0030
title: Support message reactions in the TUI
status: Done
assignee:
  - '@codex'
created_date: '2026-09-30 09:37'
updated_date: '2026-09-30 09:46'
labels: []
dependencies: []
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Read and manage native Teams message reactions from the selected TUI message so users can react and remove their own reactions without losing conversation state.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 Existing message reactions display accurate deduplicated counts and current-user ownership from supported native payload forms.
- [x] #2 The plus key opens a keyboard reaction picker for a valid selected message, with add and remove actions that preserve unrelated reactions.
- [x] #3 Native reaction requests use the existing regional Teams API and report loading and actionable errors.
- [x] #4 Successful reactions and background message updates refresh reactions while preserving conversation selection, draft, history position, and unread counts.
- [x] #5 Focused tests cover parsing, HTTP request construction, picker interactions, cancellation, invalid or stale targets, and refresh behavior.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Add native reaction parsing and narrowly scoped add/remove API calls.
2. Integrate a keyboard reaction picker and state-preserving refresh through the TUI backend.
3. Add parser, HTTP, interaction, and background refresh regression tests; document controls and run focused checks.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
- Added native reaction parsing with per-emoji/user deduplication, preserved ownership identities, and scoped PUT/DELETE requests. Exact-message follow-up reads update reactions outside the newest history page.
- Added a keyboard picker with add/remove ownership, loading/error/cancel behavior, valid target guards, mouse isolation, and silent reaction refresh. Reaction strips wrap custom Unicode labels and mark own reactions.
- cargo test --locked reaction: 12 passed. cargo fmt --all and git diff --check passed. Parent review, full suite, Clippy, release build, and PTY smoke checks remain; no live account writes performed.

Parent review completed. Full validation passed: cargo test --locked (198 passed, 2 ignored); cargo fmt -- --check; cargo clippy --locked --all-targets (existing repository warnings); cargo build --locked --release; normal/error/panic PTY smoke checks; git diff --check. No live account mutations were used for validation.
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Added message reactions to the TUI through the existing native Teams chat API. Messages show deduplicated counts and current-user ownership. Pressing + on a selected message opens a picker for adding common reactions or removing an existing own reaction, with keyboard navigation, cancellation, and loading/error feedback.

Successful changes refresh the exact message while preserving the conversation, draft, selection, history position, and unread state. Existing push and polling updates refresh reaction counts. The picker rejects unavailable or synthetic targets, blocks duplicate submissions and mouse clicks through the overlay, and supports removing existing custom reactions. Reaction strips wrap within the message width. README and help describe the controls.

Validation: 198 tests passed, 2 ignored, including 12 new parser, localhost HTTP, interaction, rendering, and refresh tests. Formatting, Clippy, release build, diff checks, and normal/error/panic PTY smoke checks passed. Clippy reports existing repository warnings. Live reaction writes have not been tested against an account.
<!-- SECTION:FINAL_SUMMARY:END -->
