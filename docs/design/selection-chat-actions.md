# Selection chat actions

Add desktop actions for selected transcript text and workspace code. Both actions
stage source-labelled context cards above the composer; neither sends a prompt
automatically. Hovering a card previews the source and full selection in a
scrollable panel. Each card can be removed independently. Adding or removing
context preserves the draft text, selection, IME composition, attachments,
review comments and selected provider. Read-only inputs reject new selections.
While a question or queue edit borrows the composer input, Add to chat is
rejected with a message asking the user to finish that interaction first.
The ordinary draft, question answer and queued text are left untouched. Existing
selection cards remain attached to the ordinary draft and are hidden while its
input is borrowed.

`Add to chat` targets the conversation owning the selected surface. `Ask in side
chat` uses the existing fork workflow (latest completed response), opens the side
pane and stages the quote after the fork succeeds. The main draft stays intact.
A failed fork keeps its quote for retry; closing or navigating away must not insert
it into another conversation. Unsaved/no-completed-response conversations keep the
existing explicit fork error rather than silently switching semantics.

Selection menus snapshot their text and origin before taking focus. File quotes
include the workspace-relative path and line range from the in-memory editor,
including unsaved edits. Diff quotes identify the file and old/new side. Stale
menus are rejected after changing the source conversation/workspace.
Diff selection scopes isolate files and before/after sides, but do not retain
layout records across frames. Repaints replace layouts, and closed surfaces are
retired by the shared frame registry, just like transcript surfaces.

Selection cards are stored per conversation alongside the in-memory draft. At
send time their full source-labelled quotes are combined with the user's text;
selection-only messages can also be sent or queued. A successful send consumes
the cards. A failed send restores them without dropping context added while the
request was in flight. Navigating between conversations preserves each draft's
cards, and deleting a conversation removes its staged context.

No engine, harness or wire-format change is needed: all agents receive ordinary
quoted text through the established composer/send path. Mobile is outside the
initial desktop scope. Regression tests cover ownership, draft and IME
preservation, removal, navigation, send restoration, source/range formatting,
fork success/failure/retry and UI action routing.
