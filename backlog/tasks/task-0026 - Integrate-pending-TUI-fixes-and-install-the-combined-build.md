---
id: TASK-0026
title: Integrate pending TUI fixes and install the combined build
status: Done
assignee:
  - '@codex'
created_date: '2026-09-22 15:09'
updated_date: '2026-09-22 15:12'
labels: []
dependencies: []
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Make the local activity integration branch the reproducible source of the installed TUI, preserving independent feature branches and existing worktrees.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 Identity and collapsible Teams changes are committed on separate feature branches.
- [x] #2 Activity integration includes the identity, collapsible Teams, and autoscroll fixes and passes combined validation.
- [x] #3 Installed teams-cli matches the validated integration release build and the main checkout is clean.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Preserve a recovery copy of the dirty checkout and installed binary; commit own-message identity and Teams collapse separately on feature branches.
2. Merge those branches and the existing autoscroll branch into feat/tui-activity-integration; resolve overlaps while preserving all feature behavior.
3. Run the combined Rust suite, formatting, Clippy, offline PTY checks, and diff review.
4. Install the validated release build, verify its hash and CLI startup, complete task metadata, and leave the main checkout clean on integration.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
User approved the proposed separation, integration, validation, and installation plan. Existing feature branches and worktrees will be preserved.

Committed identity as 52dd0c1 and Teams collapse as 2dbb5cd on independent branches based on e0c4d6b. Recorded the existing autoscroll task on its feature branch. Merged all three into integration; resolved the appended-test conflict by preserving both groups. Recovery copy: /tmp/ost-integration-recovery-ivwtghqv; safety stash retained until completion.

Combined validation passed: cargo test --locked (173 passed, 1 intentionally ignored fixture), cargo fmt -- --check, cargo clippy --locked --all-targets (existing warnings, no errors), git diff --check, and offline Python PTY mouse/terminal cleanup in normal, error, and panic modes. Reviewed the combined source diff; all three feature behaviors and test groups are preserved.

Installed with cargo install --path . --locked --root /home/victor/.local. Installed executable and target/release/teams-cli have identical SHA256 f400eeb86e3dc363b82ee06661d173af614b008b5d01c8cb11f91303bd1342af; CLI --help succeeds with the ost-arpada configuration environment. Source commit: abfe2d0. Every local branch tip is now an ancestor of integration. Original feature worktrees are preserved. Final commit contains task metadata only; release source remains unchanged.
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
The local activity integration branch now contains all existing TUI features plus own-message identity repair, a collapsible Teams parent closed by default, and message autoscroll fixes. Identity and Teams collapse are preserved on independent feature branches, and the autoscroll branch is merged with both sets of appended regression tests retained.

Installed the combined release build at /home/victor/.local/bin/teams-cli and verified it matches target/release/teams-cli byte for byte. The original checkout is left on feat/tui-activity-integration; existing feature branches and worktrees are preserved. Changes remain local.

Validation: 173 Rust tests passed, 1 intentionally ignored PTY fixture; formatting, default-feature Clippy, diff checks, CLI help, and offline PTY normal/error/panic checks passed. Existing compiler/Clippy warnings remain. No live Teams messages were sent. Recovery files and the prior binary are saved under /tmp/ost-integration-recovery-ivwtghqv.
<!-- SECTION:FINAL_SUMMARY:END -->
