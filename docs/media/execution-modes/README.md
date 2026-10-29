# Execution controls screenshots

Desktop controls are the existing dev implementation; see
[`../../screenshots/access-modes/`](../../screenshots/access-modes/README.md).
This PR no longer introduces or captures a second desktop picker.

The six Android/iOS images in this directory were captured by the PR's native
UI tests before integration with dev (`e1cb7d4d`), using Android 35 and the
alternate Xcode 27 simulator. They document the retained mobile UI; they are
historical evidence, not a claim that the integrated revision has passed CI.
New runs upload Android screenshots and iOS test attachments for verification.

The demo screens exercise actual native controls with synthetic conversations.
They do not prove provider permission enforcement or cross-device sync ordering.
Those behaviors require the accompanying core/protocol regression tests.
