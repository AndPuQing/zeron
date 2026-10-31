# Usage insights

**Settings → Usage** shows how each agent has done on the sessions stored on
this computer:

- **headline figures**: sessions, turns, success rate, rework rate and total
  agent time for the selected range;
- **Agents**: per harness, its sessions and turns, success rate, rework rate
  and typical (median) turn time, plus failed and stopped counts;
- **Task types**: per inferred task type, its sessions, success and rework
  rates, and the best agent once there is enough to compare;
- **Plan limits**: the usage windows (session, week, month) of every signed-in
  account that reports them, with the soonest reset.

The range is 7, 30 or 90 days, or all time; it defaults to 30 days.

## Where the numbers come from

Everything is worked out by the local engine from its own transcripts. No
transcript text, statistic or derived value leaves the device, and no model
is called.

- `zeron_doc::usage::chat_usage` reads a chat's transcript into turn records:
  the harness, start time, recorded duration, outcome, and whether the next
  message asked for rework.
- `zeron_engine::usage_insights` caches one record set per chat in
  `usage-insights.sqlite3` in the profile store, keyed by the chat snapshot's
  `saved_at` and the chat's harness attribution. Each request re-reads only
  the chats where either changed, within a two-second budget, and drops rows
  for deleted chats. A chat whose snapshot can't be loaded is skipped and
  tried again after the others on a later request. Deleting the file only
  costs a re-read, so a cache that won't open is deleted and rebuilt (or, if
  the folder can't be written, kept in memory for that run).
- `zeron_proto::usage_insights::aggregate` builds the report. The engine
  answers `GetUsageInsights` (`{ sinceMs? }` → `UsageInsightsReport`) and
  advertises `usage-insights-v1` (part of the current capability set, so the
  deferred desktop engine advertises it before its profile opens). The method
  is not relay-forwarded.
- While older chats are still unread the report carries `indexing: true`, and
  the page asks again until it is complete.
- Plan limits are the Accounts list's last known usage, read without probing
  the provider. Settings → Accounts and the composer ring probe as before.

In synced mode the page covers chats whose documents are stored on this
computer. Other devices' history appears once it has been replicated here.

## Definitions

These are heuristics over what transcripts record. They describe your own
history and are not a benchmark.

| Term | Meaning |
| --- | --- |
| Turn | One finished assistant reply, with continuation segments joined. Replies still streaming are not counted until they end. Messages holding only a seam or context compaction are not turns, even when stopped; a reply stopped before it produced anything is a stopped turn. |
| Succeeded | The turn finished without an error part. **Success rate** = succeeded turns / all turns. |
| Failed | The turn ended on an error part. |
| Stopped | The turn was stopped by the person or cut off by a crash or shutdown (`aborted`). |
| Needed rework | A succeeded turn whose next message from the person rejects it or reports that the result is still wrong, broken or failing. Rejections are openings like "no, …", "不对" or a pasted "Traceback …"; broad words ("broken", "failed", "撤销", "有问题") count only as openings. Reports are specific phrases anywhere in the message, like "still fails", "undo that", "还是不行", "撤销刚才的修改" or a pasted "error: …". Openings that wave something off ("no need", "no worries") don't reject the reply, though a report after them still counts ("no problem, but it still crashes"). Ordinary next requests and questions ("add undo support", "加一个撤销按钮", "如果有问题告诉我", "这样对不对？", a pasted "0 failed" test summary) don't count. **Rework rate** = reworked turns / succeeded turns. |
| Typical time | Median recorded duration of succeeded turns. Agent time sums every recorded duration. |
| Task type | Keyword classification of the session's first message, in English and Chinese. Types are bug fixes, tests, refactoring, performance, docs, reviews, build and release, features, questions and research, and other. The type with the most matches wins; ties go to the type listed first. |
| Best agent | Highest success × (1 − rework) among harnesses with at least five turns in that task type. It is shown only when two or more harnesses qualify. |

Attribution and exclusions:

- Each turn counts for the harness that ran it. Provider-switch seams split a
  chat's turns by harness. Turns after the last seam, and all turns without
  one, belong to the harness that last held the chat's native session, else
  its configured harness.
- History copied into a fork counts for its source chat only.
- Messages that another chat's agent sends through `send_message` are not the
  person's. They neither set a task type nor mark rework.
- Archived sessions and side chats are included. Deleted chats and the hidden
  voice orchestrator chats are not.

## Verification

The table below lists what the tests cover:

| Area | Covered by |
| --- | --- |
| Classification | Unit tests in `zeron-proto` for English and Chinese task types, whole-word matching, and correction detection. |
| Aggregation | Rates, medians, the time filter, the best-agent threshold and the report's wire shape. |
| Transcript reading | Unit tests in `zeron-doc` for outcomes, durations, rework, provider-switch attribution (including an unreadable last seam), continuation joining, stopped compactions, fork history and agent-sent messages. |
| Engine | Engine unit tests for harness attribution, snapshot reading and rebuilding an unreadable cache. |
| End to end | `crates/engine/tests/usage_insights.rs`: mock-harness turns queued over the RPC transport, the advertised capability, null params, the time filter, chat deletion, and a restart that provably serves the cached row. |
| Desktop page | Unit tests for range bounds, figure formatting, the best-agent line and the plan-limits list. |
