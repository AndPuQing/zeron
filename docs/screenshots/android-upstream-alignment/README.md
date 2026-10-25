# Android upstream flow alignment

Actual Android 16 / API 36 emulator captures of this fork's Android UI using
the offline demo. Devices, accounts, projects and conversation content are
fixtures. All PNGs are original captures, without cropping, stitching or
retouching, and are documentation assets rather than packaged app resources.

Phone captures are 1080 × 2400 at 420 dpi. Tablet captures use the same emulator
with `adb shell wm size 3000x2000`; they do not establish physical-tablet or
foldable-device behavior. Reset the override with `adb shell wm size reset`.

The APK uses workspace version 0.3.20 and the Android source in
`align/android-upstream-flows`. The shell captures were made after its final
navigation/lifecycle fixes, before the thumbnail-click change; their screen
implementations are unchanged by that patch. Welcome, voice, visual and image
captures use the final Debug APK, including the thumbnail-click change.
The writable build copy's Kotlin/XML sources match the working tree. The
existing native core was reused because this change edits no Rust source.

## Reproduction

Cold-start the Debug APK in demo mode, choosing a route below:

```sh
adb shell am force-stop work.puqing.zerun.android
adb shell am start -W \
  -n work.puqing.zerun.android/sh.zeron.android.MainActivity \
  --ez demo true --ez fast true --es route home
```

| File | Route and interaction |
| --- | --- |
| `phone-sessions.png` | `home`: upstream Sessions bottom app bar and session sections. |
| `phone-new-session.png` | `new`: focused composer and visible keyboard. |
| `tablet-settings-dialog.png` | Set tablet size, open `chat:chat-deploy`, then Settings. Providers → Back returns to the settings dialog's own stack. |
| `tablet-inspector.png` | Same tablet chat; open Agents, side chats and terminals. The unsent draft is retained. |
| `onboarding-appearance.png` | `welcome:appearance`: appearance and wallpaper setup. |
| `onboarding-agents.png` | `welcome:agents`: agents on demo computers. The GitHub step is skipped when Cloud is unavailable. |
| `voice-stage.png` | `home`, adding `--ez voice-preview true --ez voice-stage true`: scripted Debug UI, without a live provider call. |
| `interactive-visuals.png` | `chat:chat-visuals`: native dashboard controls and chart, loaded from the demo. |
| `image-thumbnail.png` | `chat:chat-ios-scroll`: scroll to the first user message to reveal its loaded image thumbnail. |
| `image-preview.png` | Tap that thumbnail to open the full-screen viewer. Close returns to the same chat. |

## Verification

- Debug APK and instrumentation APK builds passed. The final thumbnail-click
  change was compiled and packaged successfully with `:app:assembleDebug`.
- JVM unit tests: 31 passed, zero failures/errors/skips, during the flow
  alignment. The subsequent thumbnail patch changes only the Composable's
  click wiring and pending-image guard.
- Emulator regressions passed in targeted batches after integration fixes:
  phone back stack; tablet settings and inspector; resizing with canonical
  skill drafts and staged attachments; onboarding completion and recreation;
  provider/import recreation; side-chat search/forking; skill insertion; and
  native voice preparation with old-call callback isolation.
- Keep the failure history distinct: the initial 42-test run had 36 passes and
  6 failures. A later targeted run passed 14/15 and exposed lost tablet
  selection. After the navigation-state fix, a lifecycle batch passed 7/8;
  the remaining appearance test used a stale accessibility node. After its
  lookup was corrected, that test passed. No final single all-42-green run
  is claimed.
- On the final APK, tapping a loaded message thumbnail opened the full-screen
  viewer with image, close and share controls. Double-tap zoom was visually
  checked; the close control returned to the chat. Unloaded and pending images
  use upstream's disabled-click condition. Pending-upload interaction,
  pinch-to-zoom and sharing were not exercised in this verification.
- `git diff --check` passed.

Real WorkOS/provider authorization, real-provider voice/audio, physical
devices, process-death draft recovery and production signing were not tested.
Draft/attachment retention covers navigation and Activity recreation in the
running process. Cloud provisioning remains disabled by the fork deployment.
Formula rendering still uses the existing Android text fallback. Some command
menu and loading styles remain specific to the fork.

The upstream voice lifecycle replaces the prior explicit hang-up on removal
from Recents; that behavior is no longer promised. End the call using its UI
or notification controls. Old-call media callbacks remain generation-guarded.
