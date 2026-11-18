import UIKit

extension FastModeControl {
    func title(in values: [String: String]) -> String {
        enabled(in: values).map { $0 ? "Fast on" : "Fast off" } ?? "Fast: Default"
    }

    func chip(values: [String: String]) -> ComposerChip {
        ComposerChip(id: "fast", title: "", symbol: "bolt.fill",
                     tint: enabled(in: values) == true ? Palette.accent : nil,
                     accessibilityLabel: title(in: values))
    }

    func enabled(in values: [String: String]) -> Bool? {
        fastModeEnabled(control: self, value: values[id])
    }

    func applying(_ enabled: Bool?, to values: [String: String]) -> [String: String] {
        setFastMode(control: self, enabled: enabled, values: values)
    }

    func menu(values: [String: String], onPick: @escaping (Bool?) -> Void) -> UIMenu {
        let selected = enabled(in: values)
        return UIMenu(title: "Fast mode", children: [
            UIAction(title: "Use default", state: selected == nil ? .on : .off) { _ in onPick(nil) },
            UIAction(title: "Off", state: selected == false ? .on : .off) { _ in onPick(false) },
            UIAction(title: "On", state: selected == true ? .on : .off) { _ in onPick(true) },
        ])
    }
}
