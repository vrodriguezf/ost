---
id: TASK-0023
title: Identify own messages by account ID and restore missing sender labels
status: Done
assignee:
  - '@codex'
created_date: '2026-09-18 14:51'
updated_date: '2026-09-18 15:17'
labels:
  - tui
dependencies: []
references:
  - src/tui/messages.rs
  - src/tui/ui.rs
  - src/tui/unread.rs
  - src/api/chat.rs
priority: high
type: bug
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Newly sent messages can appear without the current user name and lose right alignment because ownership is determined by display-name equality. Make own-message presentation consistent with loaded history even when native message display names are absent or blank.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 Own top-level messages remain right-aligned with own-message styling when sender names are missing, blank, placeholders, or different from the account display name.
- [x] #2 Own messages without a usable sender name display the current account name, including after account identity loads and history refreshes.
- [x] #3 Messages from another account remain left-aligned even when their display name matches the current user; unavailable identities do not create false ownership.
- [x] #4 Regression tests cover normalized sender IDs, missing names, delayed account identity, and unchanged message selection and scrolling.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Pass current account ID into message rendering and match ownership with the existing normalized identity helper, guarding unavailable identities.
2. Resolve a display label for own messages with missing, blank, or placeholder sender names using the current account name at render time so delayed identity and refreshed history work consistently.
3. Add rendering and app-state regression coverage for own/other sender IDs, duplicate display names, delayed identity, and history refresh; retain current card geometry and scrolling behavior.
4. Run cargo test --locked, cargo fmt -- --check, cargo clippy --locked --all-targets, the existing TUI PTY smoke test where applicable, and git diff --check; review the diff and complete task metadata.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Investigation: current branch is feat/tui-activity-integration at e0c4d6b, with initially clean working tree. Local main and current branch both determine own-message alignment using sender display-name equality. Native history parser preserves blank imdisplayname and substitutes ? for missing names; send helper omits imdisplayname. Existing sender_id and same_user helper support identity-based rendering. bunx --yes backlog.md and cargo are available. Awaiting required implementation-plan approval; no source changes made.

User approved implementation and requested a new branch. Created fix/tui-own-message-identity from the current integration branch.

Implemented account-ID ownership in message rendering and render-time fallback labels for missing native display names. Focused regressions passed for MRI/resource URL IDs, blank and placeholder names, and delayed account identity plus history refresh. Full validation is underway.

Validation completed: cargo test --locked passed 164 tests (1 ignored terminal fixture); cargo fmt -- --check, cargo clippy --locked --all-targets (warnings reported), and git diff --check passed. Python PTY smoke test passed normal, error, and panic cleanup modes. Reviewed focused diff; no API or send behavior changed and no live message was sent. Installed teams-cli binary has not been replaced.
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Own messages now keep their right alignment and account name when native Teams history omits the sender display name. Ownership uses normalized account IDs instead of display-name equality, preventing other users with the same name from appearing as the current user.

Missing sender labels are resolved during rendering, so messages also recover when account identity loads after history. Existing card geometry, selection, scrolling, and thread behavior are retained.

Validation: 164 tests passed, including new rendering and delayed-identity/history-refresh regressions; formatting, default-feature Clippy, diff checks, and all three PTY smoke-test modes passed. Clippy reports repository warnings. No live Teams messages were sent; the installed binary was not replaced.
<!-- SECTION:FINAL_SUMMARY:END -->
