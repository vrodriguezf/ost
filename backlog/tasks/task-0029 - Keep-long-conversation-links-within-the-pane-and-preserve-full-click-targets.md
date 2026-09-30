---
id: TASK-0029
title: Keep long conversation links within the pane and preserve full click targets
status: In Progress
assignee:
  - '@codex'
created_date: '2026-09-28 10:56'
updated_date: '2026-09-28 11:24'
labels: []
dependencies: []
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Long URLs are clipped by the message pane because wrapping only breaks between words. Terminal URL detection then opens an incomplete address. Preserve the full destination independently of visible link text.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 Long URLs and unbroken text remain within the message pane, including replies and narrow layouts.
- [x] #2 Terminal hyperlink activation opens the complete original URL even when its visible text is wrapped or shortened, on supported terminals.
- [x] #3 Scrolling and resizing preserve correct link destinations without leaving stale clickable regions.
- [x] #4 Regression coverage verifies long URLs, Unicode text, and link rendering across scrolling and resizing.
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Fix message wrapping to use terminal display widths and split overlong tokens safely, preserving pane bounds for messages and replies.
2. Preserve full URL targets through layout and emit terminal hyperlinks for visible link cells, with correct updates on scroll and resize.
3. Add regression tests for long URLs, Unicode, narrow panes, and link target lifecycle; run locked tests, formatting, Clippy, and a terminal rendering smoke check.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Confirmed wrap_text only wraps between whitespace-separated words and measures byte lengths. Message content is rendered as plain spans without hyperlink targets. Awaiting implementation-plan approval required by AGENTS.md.

User approved the plan and requested a new branch in the existing checkout. Created fix/conversation-link-wrapping. Implementing display-width wrapping and OSC 8 links after layout, with stale-link clearing and overlay handling.

Implemented grapheme/display-width wrapping with correct indentation and full OSC 8 targets on each visible URL segment. URL metadata follows scrolling/resizing and is cleared for loading and overlays. Added unit, layout, and real-PTY hyperlink coverage.
Validation: 190 tests passed, 2 ignored; the unrelated UDP media bind test fails under this sandbox (Operation not permitted) and was excluded from the successful run. Clippy completed with existing warnings; formatting and diff checks passed. PTY normal/error/panic modes passed. Keeping status In Progress because the unrestricted full suite cannot pass in this environment.

Integration review: no blocking findings. Independently reran full locked suite: 190 passed, 2 ignored, only calling::media::tests::test_media_session_binds failed because sandbox forbids UDP binding. Formatting, diff check, Clippy (existing warnings), release build, CLI help, and normal/error/panic PTY checks passed. Installed teams-cli already matches the reviewed release binary byte-for-byte (SHA256 f30864258b135b5a65e039a23480ff9796f33bf74aeda355b75bd15bb71fb507). Reinstallation was attempted but the sandbox denied access to /home/victor/.local/.crates.toml as read-only. Leaving task In Progress until unrestricted full-suite verification is available.

Commit and dev integration could not proceed: Git failed to create .git/index.lock because .git is read-only in this session. No commit, merge, or push completed.
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Long conversation URLs wrap within the message pane and retain full OSC 8 destinations across wrapping, scrolling, resizing, and overlays. Unicode wrapping respects grapheme boundaries and terminal column widths.

Review found no blocking issues. Validation: 190 tests passed, 2 ignored; the unrelated UDP bind test is blocked by the sandbox. Formatting, Clippy (existing warnings), release build, CLI help, and all three PTY modes passed.

The installed executable already matches the reviewed release artifact by SHA256. A fresh Cargo installation is blocked by the read-only installation directory. Full unrestricted test validation remains pending. Requires an OSC 8-capable terminal or multiplexer.
<!-- SECTION:FINAL_SUMMARY:END -->
