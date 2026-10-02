---
id: TASK-0030
title: Support message reactions in the TUI
status: Done
assignee:
  - '@codex'
created_date: '2026-09-30 09:37'
updated_date: '2026-10-02 09:58'
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
- [x] #2 The r key opens a keyboard reaction picker for a valid selected message, with add and remove actions that preserve unrelated reactions.
- [x] #3 Native reaction requests use the existing regional Teams API and report loading and actionable errors.
- [x] #4 Successful reactions and background message updates refresh reactions while preserving conversation selection, draft, history position, and unread counts.
- [x] #5 Focused tests cover parsing, HTTP request construction, picker interactions, cancellation, invalid or stale targets, and refresh behavior.
- [x] #6 Reactions and wrapped hyperlinks coexist after integration into dev, and the reaction popup hides underlying hyperlink targets.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Add native reaction parsing and narrowly scoped add/remove API calls.
2. Integrate a keyboard reaction picker and state-preserving refresh through the TUI backend.
3. Add parser, HTTP, interaction, and background refresh regression tests; document controls and run focused checks.

4. Replace the reaction shortcut with plain r and synchronize help, docs, and existing interaction tests.

5. Merge into dev, preserve both hyperlink and reaction rendering, verify popup hyperlink isolation, and run integrated checks before committing the merge.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
- Added native reaction parsing with per-emoji/user deduplication, preserved ownership identities, and scoped PUT/DELETE requests. Exact-message follow-up reads update reactions outside the newest history page.
- Added a keyboard picker with add/remove ownership, loading/error/cancel behavior, valid target guards, mouse isolation, and silent reaction refresh. Reaction strips wrap custom Unicode labels and mark own reactions.
- cargo test --locked reaction: 12 passed. cargo fmt --all and git diff --check passed. Parent review, full suite, Clippy, release build, and PTY smoke checks remain; no live account writes performed.

Parent review completed. Full validation passed: cargo test --locked (198 passed, 2 ignored); cargo fmt -- --check; cargo clippy --locked --all-targets (existing repository warnings); cargo build --locked --release; normal/error/panic PTY smoke checks; git diff --check. No live account mutations were used for validation.

Changed the reaction shortcut from + to plain r at user request. Removed the unimplemented r reply hint and synchronized README, help, design reference, and existing interaction tests. Validation: all 12 reaction tests, formatting, whitespace checks, and Clippy passed (existing repository warnings).

Integrated with dev hyperlink changes. Resolved README, message imports, and Aider ignore conflicts while preserving both features. Reaction overlays clear terminal hyperlink targets; added an open/close regression test and updated the hyperlink fixture for reaction metadata. Integrated validation passed: 204 tests, 2 ignored; formatting, diff checks, Clippy (existing warnings), release build, and normal/error/panic PTY checks.
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Added message reactions to the TUI through the existing native Teams chat API. Messages show deduplicated counts and current-user ownership. Pressing r on a selected message opens a picker for adding common reactions or removing an existing own reaction, with keyboard navigation, cancellation, and loading/error feedback.

Successful changes refresh the exact message while preserving the conversation, draft, selection, history position, and unread state. Existing push and polling updates refresh reaction counts. The picker rejects unavailable or synthetic targets, blocks duplicate submissions and mouse clicks through the overlay, and supports removing existing custom reactions. Reaction strips wrap within the message width. README and help describe the controls.

Validation: 198 tests passed, 2 ignored, including 12 new parser, localhost HTTP, interaction, rendering, and refresh tests. Formatting, Clippy, release build, diff checks, and normal/error/panic PTY smoke checks passed. Clippy reports existing repository warnings. Live reaction writes have not been tested against an account.

Integrated into dev with the wrapped-link feature preserved. Reaction popups suppress underlying terminal hyperlink targets and restore them on close. The integrated suite passes 204 tests (2 ignored), plus formatting, Clippy, release build, and normal/error/panic terminal smoke checks. No live reaction writes were exercised.
<!-- SECTION:FINAL_SUMMARY:END -->
