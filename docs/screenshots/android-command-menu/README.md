# Android command menu screenshots

These are unedited `adb exec-out screencap -p` captures from the offline demo
workspace on an **Android 16 (API 36) emulator** — `sdk_gphone64_x86_64`,
AVD `zerun-preview-api36`, 1080 × 2400 at 420 dpi — not a physical device.

Source revision: `7c58f80671e84d1aaa8df6d199f7bfee3313ab6f`.
The APK was built from that revision with the current Rust mobile core:
`./gradlew :app:testDebugUnitTest :app:assembleDebug` (Gradle 9.5.1, JDK 25 in
the local preview container).

Local device regression on the same AVD: `CommandMenuFlowTest` **1/1 passed**
with the software keyboard visible. It covers 48 dp touch-row height, the
selected first row's left/right edges and inset in both composer entries, the
menu's position above the composer and visible IME, filtering, and canonical
`$imagegen` / `/compact` insertion. The single-line layout is confirmed by the
unedited screenshots below, not by a text-layout assertion.

- `light.png`, `dark.png`: skill rows in the desktop single-line layout, with the
  shared magic-wand drawable, a medium name and a muted inline description. The
  menu is bounded and scrolls when more rows exist.
- `command-light.png`, `command-dark.png`: the `/compact` row with the shared
  desktop command glyph. Commands keep their canonical slash name.
- `new-session-light.png`: the new-session composer entry uses the same 12 dp
  horizontal inset and menu width as the in-session entry; it is not full width.

Skills use the desktop human-readable name, including `$review:2` → `Review · 2`
for same-name catalog entries; insertion still uses the encoded canonical path.
Host paths are not shown.

An early working capture contained a floating three-line Gboard toolbar over the
first row. That overlay belongs to the system IME, not the app chrome; Gboard was
reset to its docked keyboard for these captures. No product layer was changed for
that overlay. The keyboard is genuinely expanded in every screenshot above.
