# Task 09: Remove unavailable products and mock results from search

**Status:** Backlog, not started
**Priority:** High
**Type:** Cleanup
**Dependencies:** None

## Outcome

Ensure the command palette only advertises usable features and real data.

## Scope

- Remove mock Docs, Mail, Chat, DM, and Inbox results and navigation entries while those products remain disabled.
- Use current product availability and authorized workspace data for task/navigation results.
- Remove the command palette dependency on the mock store where it is no longer needed.
- Keep keyboard navigation, dismissal, task search, and mobile layout intact.

## Acceptance criteria

- [ ] Disabled-product and seeded mock names never appear in command-palette results.
- [ ] Real task results open the correct task in the active workspace.
- [ ] Empty search, no results, keyboard selection, Escape, and workspace switching are covered by tests.
- [ ] Enabled settings/task navigation remains available.

## Starting points

- [apps/web/src/components/shell/CommandPalette.tsx](../../../apps/web/src/components/shell/CommandPalette.tsx)
- [apps/web/src/components/shell/productNavigation.ts](../../../apps/web/src/components/shell/productNavigation.ts)
- [apps/web/src/mock/store.ts](../../../apps/web/src/mock/store.ts)

## Limits

No new global search backend or implementation of disabled products.

## Execution handoff

This is a backlog task, not an approved step-by-step implementation plan. Recheck current code and the [backlog constraints](README.md#execution-rules) before starting.

- [ ] Confirm task-specific decisions and write the bounded design in `docs/superpowers/specs/`.
- [ ] For code changes, use the writing-plans skill to create a test-first implementation plan in `docs/superpowers/plans/`.
- [ ] Implement only this task, or perform its authorized operational verification; record commands, outcomes, and unresolved failures.
- [ ] Review against every acceptance criterion and link the implementation plan or verification report below before marking complete.

[Back to team-readiness backlog](README.md)
