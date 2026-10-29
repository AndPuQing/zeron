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
Result counts are bounded at the host. Index schema changes rebuild only the
search cache; conversation data remains unchanged.

Validation covers Chinese substrings, mixed-language queries, message identity,
operator escaping, hidden reasoning, deletion, restart and cache rebuild. Mobile
builds, generated bindings and UI checks run only in CI.
