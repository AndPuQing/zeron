# Branch checkout and new-session worktrees

Existing desktop, iOS and Android sessions keep their working directory. Use
**Switch Branch** in the composer (or the pull request chip menu on phones) to
check out a branch in that directory. The session may be in the main checkout
or a linked worktree. Choosing a branch never moves the chat to another folder.

Git rejects branches checked out elsewhere and local edits that would be
overwritten. Non-conflicting local edits remain in the checkout. Zerun does not
force checkout, reset files, stash changes or remove worktrees.

A running turn, pending live question or background work blocks checkout.
The host also refuses checkout when another live Zerun session is working in
the same directory. Native session ids, import provenance and cwd stay intact;
imported sessions can switch branches in their original directory. Checkout
does not terminate an idle agent or create a replacement native conversation.

## New sessions

The existing Local / Worktree controls remain available when creating a chat.
Choose the local checkout, select an existing linked worktree, or create a new
worktree from a base branch. Once the chat starts, its directory stays fixed.
For complex multi-worktree work, instruct the agent to use its Git tools, or
start another chat in the other worktree. There is no automatic cross-worktree
conversation migration, context copying, or Handoff operation.

## Host contract

`CheckoutSessionBranch` validates ownership, expected cwd and project membership,
serializes against this chat's prompt/queue delivery, runs checkout in that cwd,
and publishes branch, checkout identity and source context together. The
`SessionBranch { name, is_current }` catalog uses the session directory rather
than assuming that the project's root is the current checkout.

Legacy `SwitchRef` rejects session-specific requests, and the previous
`SwitchSessionRef` migration method is rejected. Older hosts that lack
`CheckoutSessionBranch` fail explicitly, so a new client cannot accidentally
invoke directory migration on an older execution host.

An orphaned question answer which must start a new run waits for the same
checkout/queue lock and rereads state after waiting. Normal live answers bypass
that lock. Failed recovery leaves the question unanswered.

## Verification

Real temporary Git repositories cover checkout in the main directory and a
linked worktree, occupied and missing refs, dirty-file conflicts, stale cwd,
busy sessions sharing a checkout, and preservation of native/import metadata.
Shared-client and native GPUI tests cover branch selection and host routing.
The orphaned-answer tests cover serialization, live answers and recovery failure.
