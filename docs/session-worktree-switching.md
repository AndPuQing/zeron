# Switching a session's branch or worktree

On desktop, iOS and Android, open the branch menu in the session composer.
On mobile, when a pull request replaces the branch chip, use its menu's
**Switch Branch or Worktree** action.

- The current branch is selected.
- Selecting a branch already checked out elsewhere moves this Zerun session
  to that existing worktree (including the project's original checkout).
  Neither checkout's branch or uncommitted files are changed.
- Selecting an unoccupied branch checks it out in the session's current folder.
  Git refuses operations that would overwrite local changes.

The chat id and visible transcript are preserved. Subsequent file searches,
diffs and agent runs use the selected directory. Existing terminal/editor tabs
retain their own paths. The picker lists branches, so detached worktrees without
a branch are not selectable through this menu.

Native agent resume is directory-scoped. After moving to another worktree,
the old idle runtime is stopped and the next turn may start a fresh native
conversation; retaining the Zerun transcript does not guarantee the provider
receives all earlier context. Menus disclose this before switching. Imported
native sessions remain bound to their copied native session's original folder
and cannot move through this action.

A running turn or background agent work blocks switching. Stop it first.
An in-place branch checkout also checks for active work in other sessions
sharing the same folder; selecting an existing worktree leaves that folder alone.
Requests from stale clients with the old directory are rejected, including
messages that would otherwise run in the old directory. Switching does not
create, delete, reset, stash or merge worktrees.

## Implementation and compatibility

`SwitchSessionRef` resolves the branch on the owning device, validates project
membership and the expected current cwd, and serializes against prompt and queue
delivery. It publishes cwd, branch, checkout id and conversation source context
in one registry operation. Clients do not independently patch these fields.
Cached launch configuration is retargeted for steer/input fallbacks while
preserving approval and sandbox settings; native-session provenance remains
bound to its original directory.

The separate RPC is intentional: older hosts report an unsupported method
instead of executing legacy `SwitchRef` checkout behavior. Update the execution
host together with the client. `SwitchRef` remains available for ordinary
checkout operations.

Regression coverage uses real temporary Git worktrees, including dirty files and
paths with spaces, plus an offline persistent agent probe. It checks main/worktree
round trips, checkout identity, stale requests, free branches, running-turn
rejection and the next native run's cwd/resume identity. Shared-client demo and
GPUI tests cover mobile menu plumbing and desktop host routing.
