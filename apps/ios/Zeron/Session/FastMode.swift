import UIKit

/// The model catalog owns the wire values; Fast is not a Codex-only setting.
struct FastModeOption: Equatable {
    let id: String
    let on: String
    let off: String

    init?(_ option: ModelOption) {
        let choices = option.choices.map(\.id)
        let on: String
        if ["fastMode", "fast_mode"].contains(option.id), choices.contains("on") {
            on = "on"
        } else if option.id == "fast", choices.contains("true") {
            on = "true"
        } else if choices.contains("fast") {
            on = "fast"
        } else { return nil }
        guard let other = choices.first(where: { $0 != on }) else { return nil }
        id = option.id
        self.on = on
        off = option.defaultChoice != on && choices.contains(option.defaultChoice) ? option.defaultChoice : other
    }

    static func find(in options: [ModelOption]) -> Self? { options.compactMap(Self.init).first }

    func enabled(in values: [String: String]) -> Bool? {
        guard let value = values[id] else { return nil }
        // FFI also reads native booleans and legacy Codex tier values as strings.
        return value == on || (on == "on" && value == "true") || (id == "serviceTier" && value == "priority")
    }

    func title(in values: [String: String]) -> String {
        enabled(in: values).map { $0 ? "Fast on" : "Fast off" } ?? "Fast: Default"
    }

    func applying(_ enabled: Bool?, to values: [String: String]) -> [String: String] {
        var next = values
        next[id] = enabled.map { $0 ? on : off }
        return next
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
