These are unedited XCTest screenshots from the offline demo workspace on an
**iPhone 17 Pro simulator (iOS 26.5)**, built with **Xcode 26.6**.

Source revision: `0d36449d05e4061761597850fcb96f723abf5101`.
CI run: https://github.com/AndPuQing/zeron/actions/runs/39370288596

The iOS job passed **27 native unit tests and 9 selected UI tests**. The mobile
core checks passed **332 tests (1 ignored)**, and the generated Swift bindings
matched. The macOS native suite also passed.

- `light.png`, `dark.png`: the desktop completion-row layout on Liquid Glass:
  the desktop command/magic-wand icons at 16 pt, a Geist Medium 13 name and a
  muted 12.5 detail on one line, with no row separators.
- `command-light.png`, `command-dark.png`: the same row for the `/compact`
  command, using the desktop command glyph and keeping the canonical slash name.
- `filtered-light.png`, `filtered-dark.png`: filtering `/` to `imag` leaves the
  single `Imagegen` skill row.

Skills show the desktop human-readable name (`Imagegen`, `Openai Docs`, …) while
insertion keeps the canonical `$imagegen` identity. Descriptions, unavailable
reasons and input hints share the muted detail; host paths are not shown. The
card is the existing glass surface with the desktop 12 pt radius, 4 pt inset and
7 pt row highlight. Mobile-only adaptations are a 44 pt minimum touch row, a
280 pt height cap with native scrolling above the keyboard, Dynamic Type and
VoiceOver labels.

`SessionFlowTests.testCommandSuggestionsStayCompactAndInsertSkills` verifies the
menu stays above the input and keyboard, caps its height, keeps light/dark rows
one line and 44–50 pt tall, omits paths, filters to the matching skill, checks the
desktop skill name, inserts the canonical `$imagegen` invocation, and renders the
`/compact` command row. The native unit tests cover the desktop display-name
mapping, including `$review` / `$review:2` disambiguation with encoded path
identity, and retain the stale-context selection guard.
