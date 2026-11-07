# Workspace PDF preview

Files opens `.pdf` files (extension matched case-insensitively) in a read-only
page view on desktop. Pages are stacked top to bottom and initially fit the
widest page to the panel. The floating controls at the bottom show the current
page and zoom from 50% to 400% of that fit; clicking the percentage returns to
fit. A zoom wider than the panel scrolls horizontally. PDFs never open in an
editor buffer, so they get no highlighting or autosave work.

Loading, errors and limits are shown in the panel: password-protected or
unsupported encryption, files that are not readable PDFs, documents with no
pages, files over 8 MiB, a 30-second load deadline, and the safety limits
below. A page that fails to render says so in its place while the rest of the
document draws, and is not retried until the document reloads.

## Routing

`files/pdf_preview.rs` reads the bytes exactly like an image: through
`WorkspaceFilesClient::read_image` (`ReadWorkspaceImage`), routed to the device
that owns the workspace, with the same checkout and content-hash checks, 8 MiB
cap and 384 KiB chunks. The host answers `application/pdf` for `.pdf` files.
A host from before PDF previews refuses the format, and the panel asks the user
to update Zerun on that device.

## Rendering and memory

`files/pdf_render.rs` parses and rasterizes with
[hayro](https://github.com/LaurenzV/hayro), a pure-Rust renderer, on one
dedicated thread per open PDF. The page list is virtualized: only the rows the
list draws (the viewport, plus rows it is still measuring) ask for rasters, at
the width they are drawn at in device pixels, rounded up to 64-pixel steps so
small resizes reuse the raster. Once per frame, after layout, the view hands
the worker the rasters those rows still lack; requests for rows no longer drawn
are withdrawn before they render, a page already rendering is not asked for
twice, and a raster that arrives for a width the page no longer wants (after a
zoom, say) is dropped. A raster of the wrong width keeps drawing until the new
one arrives. A renderer panic only fails that page.

Each PDF view retains about 96 MiB of rasters, counting both the CPU frame and
the GPU texture. The drawn rows' widths are scaled down together, to no less
than 256 pixels, so their rasters fit that budget; a single raster is capped at
4096 pixels per side and 8 megapixels (64 MiB counted that way). Drawn pages are
never evicted; other pages are, farthest first. While a page renders the worker
briefly holds its pixmap as well, handed over without a copy. With very many
rows drawn at once (a narrow zoom on tiny pages) the 256-pixel floor can exceed
the budget.

Collapsing the panel or switching files, tabs, chats or targets suspends the
view: the render thread stops and all rasters are released, and the reading
position is kept. Showing it again reloads from the owning device and returns
to that position. Modification, rename and deletion follow the image preview
lifecycle.

## Untrusted files

A PDF can declare far more work than its size suggests, and hayro sets no
limits of its own. The preview renders in-process, so it checks and bounds the
file first (`files/pdf_scan.rs`):

- Before parsing, every stream object is found by its `N G obj` header, which
  covers the compressed object and cross-reference streams the parser reads
  first. After parsing, the same streams are checked again through the
  document, which resolves indirect values and decrypts encrypted data.
- A stream with more than one expanding filter (Flate, LZW, run-length) in
  `/Filter`, or with anything but ASCII encodings ahead of one, is refused.
- Each compressed stream is decoded without keeping the output to measure it:
  past 256 MiB for one stream, or 1 GiB for the document, the file is refused.
- An image whose declared size exceeds 64 megapixels, or 256 MiB of samples,
  is refused, as is a stream whose `/Length` is longer than the file.
- Scanning and parsing must finish within 15 seconds, and each page within 20.
  A worker that overruns is abandoned: the view stops sending it work, shows
  "This PDF took too long to render", and drops anything it later sends. The
  thread cannot be stopped, so it runs until it finishes on its own; the file
  (by content hash) is not opened again in that session, so one hostile file
  costs at most one stuck thread.

What remains: the worker shares the app's process, so an allocation hayro
makes that the checks do not foresee can still abort the app. Not checked
beforehand are inline images in page content, sizes that JPEG, JPEG 2000 and
JBIG2 data declare inside the codestream, damaged Flate data that hayro's
fallback decoder recovers more from than zlib does, and the object streams an
encrypted file's parser decrypts before the second pass. The render thread has
a 32 MiB stack; hayro caps object and form nesting, but a stack overflow would
still abort rather than fail the page.

## Fonts

hayro embeds substitute fonts for the PDF standard-14 fonts and Adobe's CMaps
for CJK text, so documents that do not embed their fonts still render. Their
licenses are listed in `THIRD_PARTY_NOTICES.md`.
