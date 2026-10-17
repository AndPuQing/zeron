# Switching a session's worktree

On desktop, iOS and Android, open the worktree picker in the session composer.
On mobile, when a pull request replaces the branch chip, use its menu's
**Switch Worktree** action.

The picker lists only existing linked worktrees, identified by their branches.
The current worktree is marked selected. The main checkout and branches without
a worktree are excluded, even when the project itself was opened from a linked
worktree. An empty list displays a message instead of offering a checkout action.

Selecting a worktree moves the current Zerun session into that folder. The chat
id and visible transcript stay intact; subsequent file searches, diffs and agent
runs use the selected directory. No `git checkout` runs and no files, branches,
worktrees or uncommitted changes are created, removed, reset, stashed or merged.
A session currently in the main directory can move into an existing worktree,
but this picker does not offer the main directory as a destination. Existing
editor/terminal tabs retain their own paths. Detached worktrees without branch
names are not listed.

Native agent resume is directory-scoped. Moving worktrees stops the old idle
runtime, and the next turn may start a fresh native conversation. Retaining the
Zerun transcript does not guarantee that earlier context is replayed to the
provider. Menus disclose this. Imported native sessions remain bound to their
native copy's original folder and cannot move through this action.

Running turns and background agent work block switching. Requests carrying a
stale session directory are rejected, including delayed messages that would
otherwise run in the previous directory.

## Implementation and compatibility

`SwitchSessionRef` resolves only a linked worktree on the owning device,
validates project membership and expected current cwd, and serializes against
prompt and queue delivery. It publishes cwd, branch, checkout id and source
context in one registry operation. Cached launch configuration is retargeted
for steer/input fallbacks while preserving approval and sandbox settings;
native-session provenance remains bound to its original directory.

The session catalog exposes `SessionWorktree { name, is_current }`; main
checkout and unoccupied branches are excluded before presentation. Desktop and
mobile share this conversion. Session APIs and menus use worktree naming; the
existing wire method remains `SwitchSessionRef`. Clients filter the worktree
catalog for mouse and keyboard navigation. The host
also rejects main-checkout, unoccupied and missing refs. The legacy `SwitchRef`
RPC rejects session-specific requests, preventing an older branch-switch client
from falling back to an in-place checkout. Existing non-session Git operations
remain available elsewhere in the application. Update execution hosts with the
clients; older hosts without `SwitchSessionRef` reject the new request.

Regression coverage uses real temporary Git worktrees and an offline persistent
agent probe. It covers worktree-to-worktree round trips, dirty-file preservation,
checkout identity, rejection of main/free/missing refs, stale requests, busy
turns, post-switch steer delivery and preservation of approval settings. Shared
client and GPUI tests cover catalog filtering and host routing.
