# Android onboarding, provider and chat controls

Unedited `adb screencap -p` captures of signed-out setup and the offline demo
workspace on an **Android 16 (API 36) emulator**, AVD `zerun-preview-api36`,
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
- `welcome.png`, `onboarding-appearance.png`, `onboarding-sync.png`,
  `onboarding-agents.png`, `onboarding-done.png`: the welcome page and setup
  flow. Welcome, appearance and sync were captured signed out: cold-launch
  with `--ez signedout true --es onboarding welcome`, use Get started and
  Continue, and stop at Log in. No authorization was performed. Agents and
  completion use `--ez demo true --es onboarding agents` and Continue;
  their devices/accounts are fictitious. These previews do not set a real
  account's completion/progress markers. Use signedout only with test data:
  that launch flag clears stored login credentials. `--es onboarding <step>`
  selects a preview step (appearance, sync, agents or done).

## Scope of the iOS adaptation

This is not a complete port of every iOS screen or interaction.

| Flow | Android behavior in this PR |
| --- | --- |
| Welcome and setup | Welcome → appearance/wallpaper → sync/login → device agents → completion. Back, Activity recreation, account/org completion and pending-step storage are supported. |
| Device Providers | Reused inside setup and Settings, including device selection, enable switches, accounts and phone-initiated login. |
| Model selection | Agent tabs, current-model highlight and guarded selection; Android uses an opaque tonal material. iOS's horizontal page swipes and long-list search remain outside this change. |
| Cloud/GitHub/billing | Withheld; this fork has not enabled Cloud provisioning. No Cloud promises or setup steps are shown. |
| Chat notifications | Not implemented on Android. The iOS chat-push setup step is omitted; existing voice-call notifications and their permission flow are unchanged. |

## Verification boundary

After onboarding was added, 52 JVM unit tests and 10 selected emulator UI
tests passed, with no failures/skips, and Debug packaging succeeded. The UI
selection includes `OnboardingFlowTest` (2), `ProviderParityFlowTest` (5) and
the 3 `ShellFlowTest` close, system-Back and queued-edit draft regressions.
The 7 new state tests cover authentication gating, account/org isolation,
pending-step restoration and preview modes not writing completion markers.
These simulate storage/controller recreation, not a real OS process kill.

Earlier attempts intermittently missed controls: a failure capture showed the
restored draft on screen, and a later hierarchy dump included its editor,
but the preceding lookup had returned no editor. That single lookup took
about 9 seconds, exceeding its 5-second polling window. The return-to-chat
lookup now allows 15 seconds so another snapshot can be queried. The skill
test focuses and expands the composer before typing. All original draft,
queue and canonical-identity assertions remain. These observations do not
establish a product defect or its root cause. Navigation selects the app's
Back control and the session row by id after the outgoing editor disappears.

Real provider authorization was not performed:
its cancellation, timeout, polling and paste-code lifecycle use deterministic
JVM tests; account switching in these images and UI tests uses the demo backend.
Physical-device folding, process-death draft recovery and production signing
were not tested in this change. Cloud provisioning remains disabled.
