# Android cross-chat attribution screenshots

Unedited `adb exec-out screencap -p` captures from the offline demo workspace on
an **Android 16 (API 36) emulator** — `sdk_gphone64_x86_64`, AVD
`zerun-preview-api36`, 1080 × 2400 at 420 dpi — not a physical device.

Source revision: the `fix/mobile-cross-chat-attribution` branch this directory
lands on. The APK was built from that branch's Rust mobile core
(`scripts/android/build-core.sh`, `mobile` profile, arm64-v8a + x86_64) and
`./gradlew :app:assembleDebug -PzeronSkipCore` (Gradle 9.5.1, JDK 25).

The demo chat (`chat-attribution`) holds two messages from another chat in the
verbatim stored form, with fictitious titles and ids.

- `collapsed.png`: the folded bubble shows `Message from 示例会话标题 A · 本机 dsh
  (a1b2c3d4)` — the sender in the semibold face — and then the body's first
  lines. `Show more` follows. The reply instruction and the full routing UUID
  are absent, and the body is visible in the collapsed preview.
- `expanded.png`: the same bubble after tapping `Show more`. The rest of the
  body, ending in 「也就是整段正文的最后一句。」, is visible and the control reads
  `Show less`.

  **Cropping note:** in this capture the first line of the sender attribution is
  clipped at the top edge of the viewport. `collapsed.png` is the capture that
  shows the complete sender line; `expanded.png` is kept as the expanded
  evidence for the body tail and `Show less`. No screenshot was stitched, and no
  product layout or demo data was changed for this.

Screenshots are captured from the app the demo profile launches; no real chat
titles, ids, versions or build numbers appear in them.
