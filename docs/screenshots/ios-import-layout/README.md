These are unedited XCTest screenshots from the offline demo workspace on an
**iPhone 17 Pro simulator**, built with **Xcode 26.6**.

Source revision: `45fe7c98f6a563157b9f52d542be3ad066b6ff3e`.
CI run: https://github.com/AndPuQing/zeron/actions/runs/39202461986

- `light-selected.png`: one selected session and the compact import action.
- `dark.png`: dark appearance with unavailable sessions collapsed.
- `unavailable.png`: expanded unavailable histories and their reasons.
- `agents.png`: native single-selection agent menu with Claude selected.
- `agent-selected.png`: the selected agent's brand mark and system pop-up indicator.
- `hosts.png`: native computer menu, including an unavailable computer.

The two import UI regressions and six existing UI flows passed, alongside
18 native unit tests. The import flow also checks selecting an agent, returning
to all agents, and switching computers. Core tests and generated Swift binding
checks also passed.
