# Mobile session import screenshots

Native Android screenshots captured on 2026-10-18 from the mobile session-import
feature, running a locally built Debug APK in an Android 16 / API 36 emulator.
Both ARM64 and x86_64 Rust libraries were cross-compiled from the feature source;
the emulator uses x86_64.

The application runs the offline DemoHost workspace. Session titles, paths,
device names and the temporary import failure are demo fixtures; no personal
history or real desktop sessions are included.

- `android-chooser.png`: the import page reached from Sessions → Import, with
  floating glass controls for host selection, search and the agent dropdown,
  native agent marks, and only unimported sessions. Running
  sessions and missing project directories show their unavailable reasons.
- `android-agents.png`: the compact agent dropdown with native Codex and Claude
  Code brand marks and a checkmark on the current choice.
- `android-results.png`: a three-session batch with two successful imports and
  one simulated temporary failure. Successful sessions remain imported in
  Archived, while Retry remaining retries only the failed item.

Images were captured directly with `adb exec-out screencap -p`, without visual
editing. They document the feature and are not bundled into the application.
The iOS picker is implemented, but these images do not represent iOS validation.
