# Documents on mobile

In iOS and Android conversations, tap a Markdown file link to open a read-only
preview inside the app. The file is fetched from the conversation's execution
device, including a remote Linux host. The host must be online and support the
workspace file-read API.

`.md` and `.markdown` files open as formatted Markdown. **Source** shows the
original text; **Preview** returns to the formatted view. Other supported text
files open as source. The copy icon copies the displayed document text. The
options menu opens **File details**, including the full path and host, or
**Copy path**. Files are never edited or saved by this viewer. On wider screens,
the reading column stays centered with a bounded line width.

Conversation links follow the desktop transcript's file-link rules, and relative
paths resolve from the conversation's folder. Links inside a document follow the
desktop Markdown preview's rules: they resolve beside that document, never name
an absolute path, and cannot climb above the conversation's folder for a document
opened by a relative path. A link to the open document, such as `#usage`, keeps
it open; the viewer does not scroll to headings or lines. Back returns to the
previous document, with its position and view, or to the conversation.

Files inside the conversation's workspace open directly. A file outside it, and
any file in a conversation whose folder is the host's home folder, requires
a confirmation that names the host device and the file's full path. With a host
that supports chunked reads, **Open anyway** reads it and **Cancel** leaves it
unread. This applies to links in the conversation and inside documents alike.

Credential locations are never previewed, even after confirmation. The host
refuses them after resolving symbolic links: `~/.ssh`, `~/.aws`, `~/.gnupg`,
`~/.config/gcloud`, `~/.kube`, `~/.docker/config.json`, `~/.netrc`,
`~/.git-credentials`, `~/.npmrc`, `~/.pypirc`, `~/Library/Keychains`, and
Zerun's own data folder (`~/.zerun`, apart from the conversation folders under
its `scratch` and `worktrees`).

Older hosts fall back to the existing single-response file-read API. That API
returns the contents before the phone can determine whether confirmation is
needed: the phone withholds the preview until **Open anyway**, but **Cancel**
cannot undo the read or transfer. Such hosts do not enforce the credential list,
and large files can exceed the relay's message limit. Update the execution host
to get chunked transfer and confirmation before file contents are read.

Local raster images use the existing chat image reader and remain limited to the
chat's folder. Network images stay links; HTML is not executed. Formatting
follows the mobile transcript renderer, so desktop-only rendering features are
not added by this viewer.

Previews are limited to the first 2 MiB of text, with a visible truncation notice;
the text arrives in bounded chunks from hosts that support it. The host can reject
oversized files, unsupported encodings, binary files or inaccessible paths. Empty
documents have an explicit empty state; failed reads show an error and **Retry**.
On Android, **Copy** and text selection take at most the first 100,000 characters
and say so. PDF and Office document rendering are not supported.

Large code blocks are split into bounded rows in both Source and Preview. Very
large compound Markdown sections are shown as source with a notice; other
sections keep their formatting. Long source lines continue in another block.
The document toolbar's Copy action keeps the original text, subject to the
platform's documented copy limit.
