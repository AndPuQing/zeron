# Selection chat actions

Add desktop actions for selected transcript text and workspace code. Both actions
stage an editable, source-labelled quote; neither sends a prompt automatically.
Adding context appends one undoable edit and preserves existing text, attachments,
review comments and the selected provider. IME composition and read-only inputs
refuse an insertion instead of damaging the draft.
While a question or queue edit borrows the composer input, Add to chat is
rejected with a message asking the user to finish that interaction first.
The ordinary draft, question answer and queued text are left untouched.

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

No engine, harness or wire-format change is needed: all agents receive ordinary
quoted text through the established composer/send path. Mobile is outside the
initial desktop scope. Regression tests cover ownership, draft preservation and
undo, source/range formatting, fork success/failure/retry and UI action routing.
