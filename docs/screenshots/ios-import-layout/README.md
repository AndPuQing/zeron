These are unedited XCTest screenshots from the offline demo workspace on an
**iPhone 17 Pro simulator**, built with **Xcode 26.6**.

Source revision: `74094dec9e9b01becefdafa826f517689a8003fa`.
CI run: https://github.com/AndPuQing/zeron/actions/runs/39207343088

- `light-selected.png`: native row selection and a mixed group checkbox.
- `dark.png`: empty group checkbox, native search and collapsed unavailable histories.
- `unavailable.png`: expanded histories with their unavailability reasons.
- `agents.png`: native single-selection agent menu with Claude selected.
- `agent-selected.png`: selected agent brand mark and native pop-up indicator.
- `hosts.png`: native computer menu, including an unavailable computer.

18 native unit tests and 8 UI tests passed. The two import flows cover
select-all and clear states, preserving hidden selections through filtering,
restoring native row selection by source identity, keyboard bounds, unavailable
history disclosure, scroll pagination, agent selection and switching computers.
The standalone Select visible and Load more sessions rows are removed. VoiceOver
can request another page through a custom action on list items.

Mobile core checks passed 326 tests (1 ignored), and generated Swift bindings
matched.
