import Foundation

/// A leading slash token only; preserve arguments when completing in mid-token.
struct SlashQuery: Equatable {
    let range: NSRange
    let query: String

    static func active(in text: String, selection: NSRange) -> SlashQuery? {
        let ns = text as NSString
        guard selection.length == 0, selection.location <= ns.length else { return nil }
        let prefix = ns.substring(to: selection.location)
        let trimmed = prefix.trimmingCharacters(in: .whitespacesAndNewlines)
        guard trimmed.hasPrefix("/"), !trimmed.dropFirst().contains(where: \.isWhitespace),
              prefix.last?.isWhitespace != true else { return nil }
        let start = selection.location - (trimmed as NSString).length
        let indent = ns.substring(to: start).split(separator: "\n", omittingEmptySubsequences: false).last ?? ""
        guard indent.count < 4, !indent.contains("\t") else { return nil }
        let suffix = ns.substring(from: start)
        let token = suffix.prefix(while: { !$0.isWhitespace })
        return SlashQuery(range: NSRange(location: start, length: (String(token) as NSString).length), query: String(trimmed.dropFirst()))
    }
}
