These are unedited XCTest screenshots from the offline demo workspace on an
**iPhone 17 Pro simulator**, built with **Xcode 26.6**.

Source revision: `beef6b410da0a0d2c4431267c164486ee69cf2fa`.
CI run: https://github.com/AndPuQing/zeron/actions/runs/39199169719

- `light-selected.png`: one selected session and the compact import action.
- `dark.png`: dark appearance with unavailable sessions collapsed.
- `unavailable.png`: expanded unavailable histories and their reasons.
- `agents.png`: native agent menu with brand icons after loading the second page.

The two import UI regressions and six existing UI flows passed, alongside
18 native unit tests. Core tests and generated Swift binding checks also passed.
