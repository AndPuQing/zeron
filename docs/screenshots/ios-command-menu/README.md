These are unedited XCTest screenshots from the offline demo workspace on an
**iPhone 17 Pro simulator**, built with **Xcode 26.6**.

Source revision: `fed05921f8b17f55824cd48b2aec5883621f790e`.
CI run: https://github.com/AndPuQing/zeron/actions/runs/39366183714

The iOS run passed 26 native unit tests and 9 UI tests. The mobile core checks
passed 332 tests (1 ignored), and the generated Swift bindings matched.

- `light.png` and `dark.png`: native, scrollable command/skill suggestions on Liquid Glass above the composer and keyboard.
- `filtered-light.png` and `filtered-dark.png`: the list shrinks to one matching skill while typing.

`SessionFlowTests.testCommandSuggestionsStayCompactAndInsertSkills` verifies the
menu stays above the input, limits its height, keeps descriptions to compact
rows, excludes host paths from display, filters results, and inserts the selected
skill without showing its canonical path in the composer. Both appearances run
in the same UI flow. The native unit tests also retain the stale-context selection
guard. Skill invocation links continue to retain the full identity/path internally.
