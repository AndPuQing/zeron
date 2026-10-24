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
- `model-picker.png`: launch with `--es route chat:chat-deploy`, tap the composer,
  then its model chip, and select the Codex tab. The list comes from the shared
  core catalog. Selecting a tab alone does not change the chat's agent; picking
  a model commits the choice. The tab strip scrolls horizontally.
- `delete-confirmation.png`: close the picker, open the chat's More menu and
  choose Delete. The capture stops at confirmation; Cancel was used afterward.

## Verification boundary

Local verification: 45 JVM unit tests and 8 selected emulator UI tests passed,
with no failures or skips, and Debug packaging succeeded. The UI selection was
`ProviderParityFlowTest` (5 tests), plus `ShellFlowTest`'s
`newSessionDraftKeepsItsTargetAfterClose`,
`newSessionDraftKeepsItsTargetAfterSystemBack`, and
`leavingQueuedEditRestoresTheUnsentDraft`.

The suite retains strict draft, queue, skill-identity and attachment assertions.
Navigation waits select the app's Back control and the session row by id after
the outgoing editor disappears. Real provider authorization was not performed:
its cancellation, timeout, polling and paste-code lifecycle use deterministic
JVM tests; account switching in these images and UI tests uses the demo backend.
Physical-device folding, process-death draft recovery and production signing
were not tested in this change. Cloud provisioning remains disabled.
