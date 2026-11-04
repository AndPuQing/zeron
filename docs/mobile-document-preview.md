# Documents on mobile

In iOS and Android conversations, tap a Markdown file link to open a read-only
preview inside the app. The file is fetched from the conversation's execution
device, including a remote Linux host. The host must be online and support the
workspace file-read API.

`.md` and `.markdown` files open as formatted Markdown. **Source** shows the
original text; **Preview** returns to the formatted view. Other supported text
files open as source. **Copy** copies the displayed document text. Files are
never edited or saved by this viewer.

Conversation links follow the desktop transcript's file-link rules, and relative
paths resolve from the conversation's folder. Links inside a document follow the
desktop Markdown preview's rules: they resolve beside that document and cannot
climb above the folder it was opened from or name an absolute path. Only files
inside the conversation's workspace open; anything else shows an error. A link
to the open document, such as `#usage`, keeps it open; the viewer does not
scroll to headings or lines. Back returns to the previous document, with its
position and view, or to the conversation. Local raster images use the existing
chat image reader and remain limited to the chat's folder. Network images stay
links; HTML is not executed. Formatting follows the mobile transcript renderer,
so desktop-only rendering features are not added by this viewer.

Previews are limited to the first 2 MiB of text, with a visible truncation notice;
the text arrives in bounded chunks from hosts that support it. The host can reject
oversized files, unsupported encodings, binary files or inaccessible paths. Empty
documents have an explicit empty state; failed reads show an error and **Retry**.
On Android, **Copy** and text selection take at most the first 100,000 characters
and say so. PDF and Office document rendering are not supported.
