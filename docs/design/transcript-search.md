# Message search

The search index is per execution computer and derived from persisted session
snapshots available to that computer. It does not download every remote chat or
claim to search history that has not synced. Android and iOS explicitly select
one computer; desktop searches its connected engine. Hosts without the RPC
cannot provide message search; existing title search remains available.

Index user and assistant text only, excluding reasoning/tool logs. Preserve the
stable message ID with each result so selecting the second hit in a conversation
opens that message, not its first match. Desktop opens find-in-thread; phones
jump after the corresponding message's layout arrives.

The derived FTS index separates Chinese/Japanese/Korean characters before
indexing and querying, retaining original text for snippets. This supports
queries within unspaced sentences without enabling raw FTS operators. Latin
terms retain prefix matching; all whitespace-separated query terms must match.
Result counts are bounded at the host. A bounded refresh reports `indexing` until
all pending snapshots have been considered; clients retry while the query stays
active. Unindexed chats precede already-indexed changing chats so live output
cannot starve older history. Index schema changes rebuild only the
search cache; conversation data remains unchanged.

Validation covers Chinese substrings, mixed-language queries, message identity,
operator escaping, hidden reasoning, deletion, restart and cache rebuild. Mobile
builds, generated bindings and UI checks run only in CI.

Review regressions:

- Continuation segments join onto the same root message ID used by desktop and
  mobile layouts. Index version 4 rebuilds older derived entries; the regression
  restores a version 3 cache and checks root IDs, deduplication and queries whose
  terms span two segments. The transcript-search RPC integration test runs in
  the Linux core CI suite.
- Palette message identities use the chat and stable message ID, so repeated or
  changing snippets cannot move keyboard selection to another result.
- A delayed desktop result scrolls to the matching block inside its exact
  message. A token-prefix query with no literal find match still lands on that
  message's first block; input echoes do not reset an exact result selection.
- Desktop matching builds one original-byte-offset character index and uses
  prefix fallback (linear scanning). It retains per-character lowercase and
  non-overlapping matching semantics. Tests compare an independent naive oracle
  on mixed Unicode input and cover dense matches and long near-miss prefixes.
- Refresh must commit at least one pending chat even if metadata collection
  already exhausted the budget, then yield normally. A zero-budget SQLite test
  verifies progress across requests, stable message IDs, and eventual completion.
- Returning to the iOS message-search page restarts the unchanged query on the
  same selected computer. Hidden pages do not start requests, and lifecycle/query
  generations reject late results from before navigation or query/host changes.
  The lifecycle regressions are part of the iOS CI unit suite.
