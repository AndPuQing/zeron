# Workspace PDF preview

Files opens `.pdf` files (extension matched case-insensitively) in a read-only
page view on desktop. Pages are stacked top to bottom and initially fit the
widest page to the panel. The floating controls at the bottom show the current
page and zoom from 50% to 400% of that fit; clicking the percentage returns to
fit. A zoom wider than the panel scrolls horizontally. PDFs never open in an
editor buffer, so they get no highlighting or autosave work.

Loading, errors and limits are shown in the panel: password-protected or
unsupported encryption, files that are not readable PDFs, documents with no
pages, files over 8 MiB, and a 30-second load deadline. A page that fails to
render stays blank while the rest of the document draws.

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
dedicated thread per open PDF. The page list is virtualized: only rows near the
viewport ask for rasters, and only at the width they are drawn at in device
pixels, rounded up to 64-pixel steps so small resizes reuse the raster. A
raster of the wrong width keeps drawing until the new one arrives. Each raster
is capped at 4096 pixels per side and 16 megapixels. Requests the user has
scrolled past are dropped before they render, and a renderer panic only fails
that page.

Each PDF view retains at most 96 MiB of rasters, counting both the CPU frame
and the GPU texture; pages away from the viewport are evicted first and
visible pages never are. Collapsing the panel or switching files, tabs, chats
or targets suspends the view: the render thread stops and all rasters are
released. Showing it again reloads from the owning device. Modification,
rename and deletion follow the image preview lifecycle.

## Fonts

hayro embeds substitute fonts for the PDF standard-14 fonts and Adobe's CMaps
for CJK text, so documents that do not embed their fonts still render. Their
licenses are listed in `THIRD_PARTY_NOTICES.md`.
