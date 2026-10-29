# Execution controls screenshots

Desktop controls are the existing dev implementation; see
[`../../screenshots/access-modes/`](../../screenshots/access-modes/README.md).
This PR no longer introduces or captures a second desktop picker.

Android images were captured from the integrated implementation (`785076cc`)
on Android 35 by [CI run 40838739738](https://github.com/AndPuQing/zeron/actions/runs/40838739738).
Both the build and real emulator UI test passed, including selecting and retaining
Supervised + Plan in the composer/model picker.

iOS images were captured from the same integrated revision (`785076cc`), on
an iPhone 17 Pro simulator using the default macOS/Xcode pool:
[CI run 40838739709](https://github.com/AndPuQing/zeron/actions/runs/40838739709).
The entire macOS workflow passed, including mobile core/binding checks, all 31
iOS unit tests and all 13 selected UI tests. The permission/Plan regression
also verifies that switching models/providers preserves Supervised + Plan.

The subsequent desktop-only layout fix does not change the mobile sources.

The demo screens exercise actual native controls with synthetic conversations.
They do not prove provider permission enforcement or cross-device sync ordering.
Those behaviors require the accompanying core/protocol regression tests.
