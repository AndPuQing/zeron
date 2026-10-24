# Android provider and chat controls

Unedited `adb shell screencap -p` captures from the offline demo workspace on
an **Android 16 (API 36) emulator**, AVD `zerun-preview-api36`,
1080 × 2400 at 420 dpi. These are emulator captures, not physical-device results.
The computer names, accounts, usage and conversation in the images are demo data.
No images were cropped, stitched or retouched.

The APK uses the Android source in this PR (`feat/android-ios-parity`, workspace
version 0.3.20). Both Android Rust libraries (arm64-v8a and x86_64, `mobile`
profile) and the host UniFFI library were built from the current Rust source;
Kotlin bindings were generated from that host library. Gradle 9.5.1 / JDK 25
built the Debug APK with `-PzeronSkipCore`. Screenshot files are documentation
assets and do not ship in the APK.

## Captures and reproduction

Launch the installed Debug APK in demo mode:

```sh
adb shell am start -n work.puqing.zerun.android/sh.zeron.android.MainActivity \
  --ez demo true --ez fast true --es route settings -f 0x10008000
```

- `providers.png`: Settings → Providers, showing the selected computer, its
  offered agents, enable switches and account summaries.
- `accounts.png`: open Claude Code to see the active account, switch action,
  plans, usage windows, removal actions and the add-account entry.
- `model-picker.png` / `model-picker-dark.png`: light and dark captures of the
  model sheet, following the iOS layout: left-aligned title and computer name,
  circular close control, capsule agent strip and a soft violet current-model
  highlight. Launch with `--es route chat:chat-deploy`, tap the composer and its
  model chip, browse Codex and choose GPT-6.1-Sol, then reopen the sheet. The
  screenshots show that committed selection. The list comes from the shared
  core catalog. Selecting a tab alone does not change the chat's agent; picking
  a model commits the choice. When names do not fit, only the selected agent
  keeps its name; the other icons retain full accessibility labels. The strip
  scrolls horizontally. Change Appearance in Settings for each capture.
- `delete-confirmation.png`: close the picker, open the chat's More menu and
  choose Delete. The capture stops at confirmation; Cancel was used afterward.

## Verification boundary

After the model sheet update, 45 JVM unit tests and all 5
`ProviderParityFlowTest` emulator UI tests passed with no failures or skips;
Debug packaging succeeded. The agent-switch test selects the icon-only Codex
tab by its accessibility label and asserts that browsing leaves the chat
configuration unchanged before choosing a model. At the preceding revision
(`1add963a`), these tests and 3 `ShellFlowTest` close, system-Back and queued-edit
draft regressions passed together (8 tests); the shell implementation did not
change in this visual update.

The suite retains strict draft, queue, skill-identity and attachment assertions.
Navigation waits select the app's Back control and the session row by id after
the outgoing editor disappears. Real provider authorization was not performed:
its cancellation, timeout, polling and paste-code lifecycle use deterministic
JVM tests; account switching in these images and UI tests uses the demo backend.
Physical-device folding, process-death draft recovery and production signing
were not tested in this change. Cloud provisioning remains disabled.
